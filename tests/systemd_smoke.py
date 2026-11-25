#!/usr/bin/env python3
"""Opt-in real systemd user-unit smoke test with an isolated model and configuration."""
import argparse
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request
import uuid
from pathlib import Path


def systemctl(*args, check=True):
    return subprocess.run(["systemctl", "--user", *args], check=check,
                          text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          timeout=40)


def quote(value):
    # systemd quoting, including percent specifiers and environment expansion.
    value = str(value)
    if any(ord(char) < 32 or ord(char) == 127 for char in value):
        raise ValueError("Control character in service path")
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("%", "%%").replace("$", "$$") + '"'


def ready(endpoint, unit):
    deadline = time.monotonic() + 50
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(endpoint + "/props", timeout=2) as response:
                if json.load(response).get("role") == "router":
                    return
        except (OSError, ValueError):
            pass
        state = systemctl("is-active", unit, check=False).stdout.strip()
        if state in ("failed", "inactive"):
            raise AssertionError(f"Service became {state}")
        time.sleep(0.2)
    raise TimeoutError("Router readiness timeout")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("llamancer", type=Path)
    parser.add_argument("server", type=Path)
    parser.add_argument("model", type=Path)
    args = parser.parse_args()
    binary = args.llamancer.resolve(strict=True)
    server = args.server.resolve(strict=True)
    model = args.model.resolve(strict=True)
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    unit_dir = runtime / "systemd/user"
    unit_dir.mkdir(parents=True, exist_ok=True)
    unit = "llamancer-smoke-" + uuid.uuid4().hex + ".service"
    unit_path = unit_dir / unit
    systemctl("show-environment")
    with tempfile.TemporaryDirectory(prefix="llamancer-systemd-") as directory:
        root = Path(directory)
        config = root / "config/llamancer"
        config.mkdir(parents=True)
        models = root / "models"
        models.mkdir()
        shutil.copyfile(model, models / "fixture.gguf")
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        endpoint = f"http://127.0.0.1:{port}"
        (config / "config.toml").write_text(
            "schema_version = 1\n"
            + "model_roots = [" + json.dumps(str(models)) + "]\n"
            + "endpoint = " + json.dumps(endpoint) + "\n"
            + "server_binary = " + json.dumps(str(server)) + "\n"
            + 'device = "none"\n'
        )
        template = (Path(__file__).resolve().parent.parent /
                    "packaging/systemd/llamancer.service").read_text()
        template = template.replace("ExecStart=%h/.cargo/bin/llamancer serve",
                                    "ExecStart=" + quote(binary) + " serve\n"
                                    + "Environment=" + quote("XDG_CONFIG_HOME=" + str(root / "config")))
        assert "ExecStart=%h" not in template
        try:
            with unit_path.open("x") as handle:
                handle.write(template)
            systemctl("daemon-reload")
            systemctl("start", unit)
            ready(endpoint, unit)
            pid = systemctl("show", "-p", "MainPID", "--value", unit).stdout.strip()
            assert int(pid) > 0
            systemctl("restart", unit)
            ready(endpoint, unit)
            second_pid = systemctl("show", "-p", "MainPID", "--value", unit).stdout.strip()
            assert int(second_pid) > 0 and pid != second_pid
            systemctl("stop", unit)
            assert systemctl("is-active", unit, check=False).stdout.strip() == "inactive"
            # A recently closed HTTP connection may be in TIME_WAIT even though
            # no server remains. Check the listener first, then use SO_REUSEADDR.
            with socket.socket() as listener:
                listener.settimeout(2)
                assert listener.connect_ex(("127.0.0.1", port)) != 0, "Server still accepts connections"
            with socket.socket() as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", port))
            print("Real systemd user unit start, restart, stop and port release passed.")
        except Exception:
            subprocess.run(["journalctl", "--user", "-u", unit, "-n", "30",
                            "--no-pager"], check=False, timeout=10)
            raise
        finally:
            systemctl("stop", unit, check=False)
            systemctl("reset-failed", unit, check=False)
            unit_path.unlink(missing_ok=True)
            systemctl("daemon-reload")


if __name__ == "__main__":
    main()
