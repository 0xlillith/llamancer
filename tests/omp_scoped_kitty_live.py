#!/usr/bin/env python3
"""Opt-in actual OMP foreground session with a conflicting stored provider.

Requires a graphical systemd user session, Kitty, OMP, an official llama-server
and a suitable local GGUF. The user home and test credentials are synthetic.
"""
import argparse
import http.server
import json
import os
import secrets
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
from pathlib import Path

from agent_tui_live import children


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("server", type=Path)
    parser.add_argument("model", type=Path)
    parser.add_argument("--device", default="none")
    parser.add_argument("--layers", type=int, default=0)
    parser.add_argument("--context", type=int, default=16384)
    args = parser.parse_args()
    binary, server, model = (p.resolve(strict=True) for p in (args.binary, args.server, args.model))
    if not 128 <= args.context <= 32768 or not 0 <= args.layers <= 999:
        parser.error("Use a bounded context and GPU layer count")
    if (args.device == "none") != (args.layers == 0) or args.device == "auto":
        parser.error("Use none/0 or an explicit GPU device and positive layer count")
    gui = dict(line.split("=", 1) for line in subprocess.check_output(
        ["systemctl", "--user", "show-environment"], text=True).splitlines() if "=" in line)
    with tempfile.TemporaryDirectory(prefix="llamancer-kitty-agent-") as scratch:
        root = Path(scratch)
        home = root / "home"
        project = root / "project"
        config = root / "config/llamancer"
        stored = home / ".omp/agent"
        for path in (home, project, config, stored):
            path.mkdir(parents=True, exist_ok=True)
        token = secrets.token_hex(24)
        key = root / "key"
        key.write_text(token + "\n")
        key.chmod(0o600)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        endpoint = f"http://127.0.0.1:{port}/"
        (config / "config.toml").write_text(
            "model_imports = [" + json.dumps(str(model)) + "]\n"
            + "server_binary = " + json.dumps(str(server)) + "\n"
            + "endpoint = " + json.dumps(endpoint) + "\n"
            + "device = " + json.dumps(args.device) + "\n"
            + "default_agent = \"omp\"\nproject_dir = " + json.dumps(str(project)) + "\n"
            + "api_key_file = " + json.dumps(str(key)) + "\n"
            + "[profiles." + json.dumps(str(model)) + ".engine]\n"
            + f"context_size = {args.context}\ngpu_layers = {args.layers}\nthreads = 2\n"
        )
        hits = []
        class StaleHandler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                hits.append(True)
                self.send_error(404)
            do_POST = do_GET
            def log_message(self, *unused):
                pass
        with http.server.ThreadingHTTPServer(("127.0.0.1", 0), StaleHandler) as stale:
            stale_thread = threading.Thread(target=stale.serve_forever, daemon=True)
            stale_thread.start()
            stale_endpoint = f"http://127.0.0.1:{stale.server_port}/"
            stored_models = stored / "models.yml"
            stored_models.write_text(json.dumps({"providers": {"llama.cpp": {
                "baseUrl": stale_endpoint, "api": "openai-responses",
                "apiKey": "stale-fixture", "discovery": {"type": "llama.cpp"}}}}))
            original = stored_models.read_bytes()
            env = {"PATH": os.environ["PATH"], "HOME": str(home),
                   "XDG_CONFIG_HOME": str(config.parent),
                   "XDG_DATA_HOME": str(root / "data"), "NO_PROXY": "127.0.0.1,localhost",
                   "LLAMA_CPP_API_KEY": "stale-fixture", "PI_CODING_AGENT_DIR": str(stored),
                   "OMP_PROFILE": "old-profile", "OMP_AUTH_BROKER_URL": stale_endpoint,
                   "OMP_AUTH_BROKER_TOKEN": "stale-fixture"}
            for name in ("DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR",
                         "DBUS_SESSION_BUS_ADDRESS", "XDG_SESSION_TYPE", "LANG", "LC_ALL"):
                if name in gui:
                    env[name] = gui[name]
            rc = "unix:" + str(root / "kitty.sock")
            def remote(*arguments):
                return subprocess.check_output(["kitten", "@", "--to", rc, *arguments],
                    stderr=subprocess.DEVNULL, text=True, timeout=5)
            def wait(condition, seconds=35):
                deadline = time.monotonic() + seconds
                while time.monotonic() < deadline:
                    if kitty.poll() is not None:
                        raise RuntimeError("Kitty exited early")
                    try:
                        value = condition()
                        if value:
                            return value
                    except (OSError, subprocess.CalledProcessError, ValueError, KeyError):
                        pass
                    time.sleep(.3)
                raise TimeoutError("Graphical acceptance condition not met")
            def screen():
                return remote("get-text", "--extent", "screen")
            def send(text):
                remote("send-text", text)
            def press(key):
                remote("send-key", key)
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            def request(path, body=None):
                req = urllib.request.Request(endpoint + path,
                    data=None if body is None else json.dumps(body).encode(),
                    headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
                with opener.open(req, timeout=2) as response:
                    return json.load(response)
            with (root / "kitty.log").open("w") as log:
                kitty = subprocess.Popen(["kitty", "--config", "NONE", "--override",
                    "allow_remote_control=socket-only", "--listen-on", rc,
                    "--class", "llamancer-isolated-acceptance", "--directory",
                    str(project), str(binary)], env=env, stdout=log, stderr=log)
                pid = None
                try:
                    wait(lambda: "Scan complete" in screen())
                    remote("resize-os-window", "--width", "140", "--height", "38", "--unit", "cells")
                    send("4")
                    time.sleep(.4)
                    send("s")
                    models = wait(lambda: request("models")["data"])
                    model_id = models[0]["id"]
                    assert request("models/load", {"model": model_id})["success"]
                    wait(lambda: any(m["id"] == model_id and m.get("status", {}).get("value") == "loaded"
                                     for m in request("models")["data"]))
                    wait(lambda: model_id in screen() and "| loaded" in screen())
                    time.sleep(3)
                    pid = json.loads(remote("ls"))[0]["tabs"][0]["windows"][0]["pid"]
                    before = set(children(pid))
                    send("A")
                    def agent_pid():
                        for child in set(children(pid)) - before:
                            try:
                                argv = Path(f"/proc/{child}/cmdline").read_bytes().split(b"\0")
                                if b"--model" in argv:
                                    return child
                            except FileNotFoundError:
                                pass
                    agent = wait(agent_pid)
                    argv = Path(f"/proc/{agent}/cmdline").read_bytes()
                    actual = dict(item.split(b"=", 1) for item in
                                  Path(f"/proc/{agent}/environ").read_bytes().split(b"\0") if b"=" in item)
                    assert token.encode() not in argv
                    assert (b"llama.cpp/"+model_id.encode()) in argv.split(b"\0")
                    assert actual[b"LLAMA_CPP_API_KEY"] == token.encode()
                    assert b"OMP_PROFILE" not in actual and b"OMP_AUTH_BROKER_URL" not in actual
                    profile = Path(os.fsdecode(actual[b"PI_CODING_AGENT_DIR"]))
                    assert profile != stored and profile.stat().st_mode & 0o777 == 0o700
                    assert token not in (profile / "models.yml").read_text()
                    daemon_marker = b"OMP_DAEMON_DIR=" + os.fsencode(profile / "daemon")
                    assert actual[b"OMP_DAEMON_DIR"] == os.fsencode(profile / "daemon")
                    assert Path(f"/proc/{agent}/cwd").resolve() == project
                    # The model ID must appear in the actual OMP editor, not a
                    # process argv or the previous Llamancer server screen.
                    wait(lambda: "π >" in screen() and "LLAMANCER" not in screen())
                    assert not hits, "Stale configured endpoint contacted"
                    print("OMP graphical editor opened using a private scoped profile.", flush=True)
                    send("Reply with exactly the word hello.")
                    press("enter")
                    # Require a separate answer line, rather than matching the prompt.
                    wait(lambda: any(line.strip().lower() == "hello" for line in screen().splitlines()), 60)
                    print("Real selected model generated the requested response.", flush=True)
                    press("ctrl+d")
                    wait(lambda: agent not in children(pid), 20)
                    wait(lambda: "LLAMANCER" in screen())
                    assert not profile.exists(), "Temporary profile not removed"
                    for proc in Path("/proc").iterdir():
                        if not proc.name.isdecimal():
                            continue
                        try:
                            if (proc / "comm").read_text().strip() == "omp daemon":
                                assert daemon_marker not in (proc / "environ").read_bytes().split(b"\0"), "Scoped OMP daemon survived"
                        except (FileNotFoundError, PermissionError, ProcessLookupError):
                            pass
                    assert stored_models.read_bytes() == original
                    assert not hits, "Stale configured endpoint contacted"
                    print("Normal exit, profile cleanup and untouched stored provider passed.", flush=True)
                    send("q")
                    assert kitty.wait(timeout=25) == 0
                    with socket.socket() as sock:
                        sock.settimeout(2)
                        assert sock.connect_ex(("127.0.0.1", port)) != 0
                finally:
                    if kitty.poll() is None:
                        if pid is not None:
                            try:
                                os.kill(pid, 15)
                            except ProcessLookupError:
                                pass
                        try:
                            kitty.wait(timeout=20)
                        except subprocess.TimeoutExpired:
                            kitty.terminate()
                            kitty.wait(timeout=10)
                    stale.shutdown()
                    stale_thread.join(timeout=5)


if __name__ == "__main__":
    main()
