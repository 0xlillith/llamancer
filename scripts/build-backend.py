#!/usr/bin/env python3
"""Build an explicitly selected, pinned llama.cpp revision without sudo.

The result is a separate external backend, not a reviewed binary-archive install.
Build native artifacts for this host; no bit-for-bit reproducibility is promised.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile

UPSTREAM = "https://github.com/ggml-org/llama.cpp.git"
RELEASE = "b12726"
REVISION = "81ff0cab06cf97b445c647d278594c7f97b39740"


def run(args, env, cwd, timeout, capture=False):
    """Wait for the owned process group; cancel it on failure/interruption."""
    process = subprocess.Popen(args, cwd=cwd, env=env, start_new_session=True,
                               stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE if capture else sys.stderr,
                               stderr=subprocess.STDOUT if capture else sys.stderr,
                               text=True)
    try:
        output, _ = process.communicate(timeout=timeout)
    except BaseException:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
        raise
    if process.returncode:
        raise RuntimeError(f"{Path(args[0]).name} exited {process.returncode}")
    return output or ""


def private_root():
    data = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")
    if not data.is_absolute():
        raise ValueError("XDG_DATA_HOME must be absolute")
    root = data / "llamancer/source-builds"
    root.mkdir(parents=True, mode=0o700, exist_ok=True)
    info = root.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o022:
        raise ValueError("Unsafe source-build storage")
    return root


def build(args):
    root = private_root()
    tag = RELEASE + "-" + args.kind + ("-" + args.gpu_target if args.gpu_target else "")
    target = root / tag
    # A private lock serializes publication; never overwrite an existing build.
    lock_fd = os.open(root / "build.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    with os.fdopen(lock_fd, "r+b") as lock:
        info = os.fstat(lock.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
            raise ValueError("Unsafe source-build lock")
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if target.exists() or target.is_symlink():
            raise FileExistsError(f"Build already exists: {target}")
        if shutil.disk_usage(root).free < 8 * 1024**3:
            raise RuntimeError("Need at least 8 GiB free for a source build")
        with tempfile.TemporaryDirectory(prefix=".building-", dir=root) as scratch:
            stage = Path(scratch)
            home = stage / "home"
            home.mkdir(mode=0o700)
            env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                   "HOME": str(home), "LANG": "C", "LC_ALL": "C",
                   "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
                   "GIT_TERMINAL_PROMPT": "0"}
            src, out, publish = stage / "src", stage / "build", stage / "publish"
            src.mkdir()
            run(["git", "init", "--quiet"], env, src, 20)
            run(["git", "-c", "protocol.file.allow=never", "fetch", "--depth=1",
                 "--no-tags", UPSTREAM, REVISION], env, src, 180)
            actual = run(["git", "rev-parse", "FETCH_HEAD"], env, src, 20, True).strip()
            if actual != REVISION:
                raise RuntimeError("Source commit differs from reviewed revision")
            run(["git", "-c", "core.hooksPath=/dev/null", "checkout", "--detach", REVISION],
                env, src, 30)
            flags = ["-DCMAKE_BUILD_TYPE=Release", "-DGGML_NATIVE=ON",
                     "-DGGML_CCACHE=OFF", "-DLLAMA_BUILD_TESTS=OFF",
                     "-DLLAMA_BUILD_EXAMPLES=OFF", "-DLLAMA_BUILD_TOOLS=ON",
                     "-DLLAMA_BUILD_SERVER=ON", "-DLLAMA_CURL=OFF",
                     "-DFETCHCONTENT_FULLY_DISCONNECTED=ON",
                     "-DCMAKE_BUILD_RPATH_USE_ORIGIN=ON",
                     "-DCMAKE_BUILD_RPATH=$ORIGIN",
                     "-DGGML_VULKAN=" + ("ON" if args.kind == "vulkan" else "OFF"),
                     "-DGGML_HIP=" + ("ON" if args.kind == "rocm" else "OFF"),
                     "-DGGML_CUDA=OFF"]
            if args.kind == "rocm":
                rocm = Path("/opt/rocm")
                compiler = rocm / "llvm/bin/clang++"
                if not compiler.is_file():
                    raise RuntimeError("ROCm compiler not found at /opt/rocm/llvm/bin/clang++")
                flags += ["-DCMAKE_C_COMPILER=" + str(compiler.with_name("clang")),
                          "-DCMAKE_CXX_COMPILER=" + str(compiler),
                          "-DCMAKE_PREFIX_PATH=" + str(rocm),
                          "-DGPU_TARGETS=" + args.gpu_target]
            run(["cmake", "-S", str(src), "-B", str(out), "-G", "Ninja", *flags], env, stage, 120)
            run(["cmake", "--build", str(out), "--parallel", str(args.jobs),
                 "--target", "llama-server", "llama-bench"], env, stage, 1800)
            shutil.copytree(out / "bin", publish / "bin", symlinks=True)
            shutil.copyfile(src / "LICENSE", publish / "LICENSE")
            for path in (publish / "bin").rglob("*"):
                if path.is_symlink() and not path.resolve(strict=True).is_relative_to(publish):
                    raise RuntimeError("Build produced an external symlink")
            # Remove the build tree before probing, so missing RPATHs or libs
            # cannot accidentally pass using files that will not be published.
            shutil.rmtree(out)
            server = publish / "bin/llama-server"
            version = run([str(server), "--version"], env, publish, 15, True)
            if REVISION[:7] not in version:
                raise RuntimeError("Executable version does not match source revision")
            devices = run([str(server), "--list-devices"], env, publish, 15, True)
            expected = {"vulkan": "Vulkan", "rocm": "ROCm"}.get(args.kind)
            if expected and not re.search(r"(?m)^\s*" + expected + r"\d+:", devices):
                raise RuntimeError("Compiled GPU backend exposes no compatible device")
            toolchain = {name: run([name, "--version"], env, stage, 15, True).splitlines()[0]
                         for name in ("cmake", "ninja", "git")}
            (publish / "build.json").write_text(json.dumps(
                {"upstream": UPSTREAM, "release": RELEASE, "commit": REVISION,
                 "kind": args.kind, "gpu_target": args.gpu_target, "flags": flags,
                 "version": version, "devices": devices, "tools": toolchain}, indent=2) + "\n")
            if target.exists() or target.is_symlink():
                raise FileExistsError("Destination appeared during build")
            publish.rename(target)
            print(target / "bin/llama-server")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=["cpu", "vulkan", "rocm"])
    parser.add_argument("--gpu-target", help="Explicit AMD ISA, e.g. gfx1201, for ROCm")
    parser.add_argument("--jobs", type=int, default=min(os.cpu_count() or 1, 8))
    args = parser.parse_args()
    if not 1 <= args.jobs <= 32:
        parser.error("--jobs must be between 1 and 32")
    if args.kind == "rocm":
        if not args.gpu_target or not re.fullmatch(r"gfx[0-9a-z]{3,8}", args.gpu_target):
            parser.error("ROCm requires an explicit valid --gpu-target")
    elif args.gpu_target:
        parser.error("--gpu-target is only for ROCm")
    for program in ("git", "cmake", "ninja"):
        if not shutil.which(program):
            parser.error(f"Missing build prerequisite: {program}")
    # SIGTERM triggers the same cleanup path as Ctrl-C.
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    try:
        build(args)
    except KeyboardInterrupt:
        print("Source build cancelled", file=sys.stderr)
        return 130
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"Source build failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
