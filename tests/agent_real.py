#!/usr/bin/env python3
"""Run installed OMP/Pi against an isolated real Llamancer CPU/GPU router.

No personal agent configuration, model modification or actual credentials are used.
A capable model is needed for the optional independent read-tool test.
"""
import argparse
import hashlib
import json
import os
import secrets
import signal
import socket
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("llamancer", type=Path)
    parser.add_argument("server", type=Path)
    parser.add_argument("model", type=Path)
    parser.add_argument("agent", choices=["omp", "pi"])
    parser.add_argument("executable", type=Path)
    parser.add_argument("--device", default="none", help="Exact device ID, or none for CPU")
    parser.add_argument("--layers", type=int, default=0)
    parser.add_argument("--context", type=int, default=8192)
    parser.add_argument("--read-tool", action="store_true")
    parser.add_argument("--hex-marker", action="store_true", help="Optional harder random identifier copying challenge")
    args = parser.parse_args()
    binary, server, model, executable = (
        p.resolve(strict=True) for p in (args.llamancer, args.server, args.model, args.executable)
    )
    if not 128 <= args.context <= 32768 or not 0 <= args.layers <= 999:
        parser.error("Use a bounded context and nonnegative layer count")
    if (args.device == "none") != (args.layers == 0) or args.device == "auto":
        parser.error("Choose none/0 for CPU or an explicit GPU device and positive layer count")
    if model.suffix.lower() != ".gguf":
        parser.error("Choose an explicit GGUF model")
    # Match Llamancer's root endpoint normalization and use only temporary credentials.
    with tempfile.TemporaryDirectory(prefix="llamancer-real-agent-") as scratch:
        root = Path(scratch)
        config = root / "config/llamancer"
        config.mkdir(parents=True)
        project = root / "project"
        project.mkdir()
        # Seven independent randomly selected words carry 28 bits of entropy.
        # This tests an ordinary file-reading task without conflating the
        # tool transport with the model's ability to copy fragmented hex tokens.
        words = ("amber", "river", "cedar", "silver", "violet", "cloud", "maple",
                 "ocean", "winter", "forest", "pearl", "stone", "garden", "moon",
                 "sunrise", "harbor")
        marker = ("llamancer-fixture-" + secrets.token_hex(6) if args.hex_marker
                  else " ".join(secrets.choice(words) for _ in range(7)))
        (project / "fixture.txt").write_text(marker + "\n")
        key_file = root / "api-key"
        token = secrets.token_hex(24)
        with open(key_file, "x", opener=lambda path, flags: os.open(path, flags, 0o600)) as key:
            key.write(token + "\n")
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        endpoint = f"http://127.0.0.1:{port}/"
        (config / "config.toml").write_text(
            "schema_version = 1\n"
            + "model_imports = [" + json.dumps(str(model), ensure_ascii=False) + "]\n"
            + "endpoint = " + json.dumps(endpoint) + "\n"
            + "server_binary = " + json.dumps(str(server), ensure_ascii=False) + "\n"
            + "api_key_file = " + json.dumps(str(key_file), ensure_ascii=False) + "\n"
            + "device = " + json.dumps(args.device) + "\n"
            + "[profiles." + json.dumps(str(model), ensure_ascii=False) + ".engine]\n"
            + f"context_size = {args.context}\ngpu_layers = {args.layers}\nthreads = 2\n"
        )
        home = root / "home"
        home.mkdir()
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(home),
               "XDG_CONFIG_HOME": str(root / "config"), "TERM": "xterm-256color",
               "NO_PROXY": "127.0.0.1,localhost"}
        # Preserve only explicit backend runtime search paths where needed.
        for name in ("LD_LIBRARY_PATH", "VK_ICD_FILENAMES", "VK_DRIVER_FILES"):
            if name in os.environ:
                env[name] = os.environ[name]

        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def request(path, data=None):
            req = urllib.request.Request(endpoint + path,
                data=None if data is None else json.dumps(data).encode(),
                headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"})
            with opener.open(req, timeout=10) as response:
                return json.load(response)

        def wait_for(condition, router):
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline:
                if router.poll() is not None:
                    raise RuntimeError("Llamancer supervisor exited before readiness")
                try:
                    models = request("models")["data"]
                    found = condition(models)
                    if found:
                        return found
                except (OSError, ValueError, KeyError):
                    pass
                time.sleep(0.25)
            raise TimeoutError("Router readiness or model load timeout")

        with (root / "server.log").open("w") as log:
            router = subprocess.Popen([str(binary), "serve"], cwd=project, env=env,
                                      stdin=subprocess.DEVNULL, stdout=log, stderr=log)
            try:
                model_id = wait_for(lambda models: models[0]["id"] if len(models) == 1 else None, router)
                assert request("models/load", {"model": model_id})["success"] is True
                wait_for(lambda models: any(m["id"] == model_id and m.get("status", {}).get("value") == "loaded" for m in models), router)
                if args.agent == "omp":
                    env.update(LLAMA_CPP_BASE_URL=endpoint, LLAMA_CPP_API_KEY=token)
                    flags = ["--model", f"llama.cpp/{model_id}", "--no-tools",
                             *(["--tools", "read"] if args.read_tool else []),
                             "--no-extensions", "--no-skills", "--no-session", "--no-context-files",
                             "--offline", "--no-judge", "--no-spotlight", "--no-title", "--max-time", "90s"]
                else:
                    extension = Path(__file__).resolve().parent.parent / "src/agents/pi.js"
                    provider = "llamancer-local-" + hashlib.sha256(f"{endpoint}\n{model_id}".encode()).hexdigest()[:16]
                    env.update(LLAMANCER_ENDPOINT=endpoint, LLAMANCER_MODEL=model_id,
                               LLAMANCER_PROVIDER=provider, LLAMANCER_CONTEXT=str(args.context),
                               LLAMANCER_PI_KEY=token)
                    flags = ["--extension", str(extension), "--model", f"{provider}/{model_id}",
                             *(["--tools", "read"] if args.read_tool else ["--no-tools"]),
                             "--no-mcp", "--no-session", "--no-context-files", "--offline"]
                prompt = ("Read fixture.txt using the read tool. In your final answer, copy the line of text from that file verbatim, preserving the words and spaces. Do not summarize."
                          if args.read_tool else "Reply briefly with a friendly greeting.")
                result = subprocess.run([str(executable), *flags, "--mode", "json", "-p", prompt],
                                        cwd=project, env=env, stdin=subprocess.DEVNULL,
                                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                        text=True, timeout=120)
                events = []
                for line in result.stdout.splitlines():
                    if line.startswith("{"):
                        try:
                            events.append(json.loads(line))
                        except ValueError:
                            continue
                assistants = [e.get("message", {}) for e in events if e.get("type") == "message_end"
                              and e.get("message", {}).get("role") == "assistant"]
                if result.returncode:
                    # Report the exit and stop reason without printing arbitrary model
                    # output, user paths, command lines or credentials.
                    reason = assistants[-1].get("stopReason") if assistants else None
                    raise RuntimeError(f"Agent exited {result.returncode}; stop reason: {reason!r}. Check the agent version, model chat template and context; rerun on a suitable model.")
                final = assistants[-1] if assistants else {}
                assert final.get("stopReason") not in (None, "error", "aborted"), "No successful assistant completion"
                text = "".join(c.get("text", "") for c in final.get("content", []) if c.get("type") == "text")
                assert text.strip(), "No real-model assistant text"
                if args.read_tool:
                    assert any(e.get("type") == "tool_execution_end" and marker in json.dumps(e)
                               for e in events), "Model did not execute the read tool successfully"
                    assert marker in text, "Final answer did not contain the fixture marker"
                print(f"{args.agent}: real authenticated model completion passed"
                      + ("; independent read-tool round trip passed" if args.read_tool else ""))
            finally:
                if router.poll() is None:
                    router.send_signal(signal.SIGTERM)
                    router.wait(timeout=40)
                assert router.returncode == 0, "Supervisor failed to stop cleanly"
        with socket.socket() as sock:
            sock.settimeout(2)
            assert sock.connect_ex(("127.0.0.1", port)) != 0, "Server still accepts connections"


if __name__ == "__main__":
    main()
