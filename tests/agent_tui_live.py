#!/usr/bin/env python3
"""Opt-in real foreground agent launch from Llamancer through a controlling PTY.

Uses a private home, local router, synthetic API key and temporary project.
Checks actual launched process arguments/environment, not a simulated agent.
"""
import argparse
import fcntl
import json
import os
import pty
import secrets
import select
import signal
import socket
import struct
import subprocess
import tempfile
import termios
import time
import urllib.request
from pathlib import Path
from backend_ui_live import Screen
from terminal_smoke import read_until


def children(pid):
    path = Path(f"/proc/{pid}/task/{pid}/children")
    try:
        return [int(p) for p in path.read_text().split()]
    except FileNotFoundError:
        return []


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("server", type=Path)
    parser.add_argument("model", type=Path)
    parser.add_argument("agent", choices=["omp", "pi"])
    args = parser.parse_args()
    binary, server, model = (p.resolve(strict=True) for p in (args.binary, args.server, args.model))
    with tempfile.TemporaryDirectory(prefix="llamancer-agent-tui-") as scratch:
        root = Path(scratch)
        config = root / "config/llamancer"
        config.mkdir(parents=True)
        home = root / "home"
        home.mkdir()
        project = root / "project"
        project.mkdir()
        token = secrets.token_hex(24)
        key = root / "api-key"
        key.write_text(token + "\n")
        key.chmod(0o600)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        endpoint = f"http://127.0.0.1:{port}/"
        (config / "config.toml").write_text(
            "model_imports = [" + json.dumps(str(model)) + "]\n"
            + "server_binary = " + json.dumps(str(server)) + "\n"
            + "endpoint = " + json.dumps(endpoint) + "\ndevice = \"none\"\n"
            + "default_agent = " + json.dumps(args.agent) + "\n"
            + "project_dir = " + json.dumps(str(project)) + "\n"
            + "api_key_file = " + json.dumps(str(key)) + "\n"
            + "[profiles." + json.dumps(str(model)) + ".engine]\ncontext_size = 4096\ngpu_layers = 0\nthreads = 2\n"
        )
        env = {"PATH": os.environ["PATH"], "HOME": str(home),
               "XDG_CONFIG_HOME": str(config.parent), "TERM": "xterm-256color",
               "NO_PROXY": "127.0.0.1,localhost",
               "LLAMA_CPP_API_KEY": "stale-fixture",
               "LLAMANCER_PI_KEY": "stale-fixture"}
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def request(path, body=None):
            req = urllib.request.Request(endpoint + path,
                data=None if body is None else json.dumps(body).encode(),
                headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
            with opener.open(req, timeout=2) as res:
                return json.load(res)

        master, slave = pty.openpty()
        # A bare PTY is not a terminal emulator. Answer cursor-position
        # reports and primary device attributes requested by interactive TUIs.
        query_tail=b""
        def receive():
            nonlocal query_tail
            data=os.read(master,65536)
            combined=query_tail+data
            for query,reply in ((b"\x1b[6n",b"\x1b[1;1R"),(b"\x1b[c",b"\x1b[?1;2c"),(b"\x1b[0c",b"\x1b[?1;2c")):
                start=0
                while True:
                    index=combined.find(query,start)
                    if index<0:break
                    if index+len(query)>len(query_tail):os.write(master,reply)
                    start=index+len(query)
            query_tail=combined[-8:]
            return data
        app = None
        def control_terminal():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
            original = termios.tcgetattr(slave)
            app = subprocess.Popen([binary], cwd=project, stdin=slave, stdout=slave,
                                   stderr=slave, env=env, preexec_fn=control_terminal)
            screen = Screen()
            screen.wait(master, app, b"Scan complete", receive=receive)
            os.write(master, b"4")
            time.sleep(.2)
            os.write(master, b"s")
            deadline = time.monotonic() + 45
            model_id = None
            while time.monotonic() < deadline:
                if select.select([master], [], [], .1)[0]:
                    screen.feed(receive())
                assert app.poll() is None
                try:
                    models = request("models")["data"]
                    if len(models) == 1:
                        model_id = models[0]["id"]
                        break
                except (OSError, ValueError, KeyError):
                    pass
            assert model_id, "Router unavailable"
            assert request("models/load", {"model": model_id})["success"]
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                if select.select([master], [], [], .1)[0]:
                    screen.feed(receive())
                try:
                    if any(m["id"] == model_id and m.get("status", {}).get("value") == "loaded" for m in request("models")["data"]):
                        break
                except (OSError, ValueError, KeyError):
                    pass
            else:
                raise TimeoutError("Model not loaded")
            # Wait for the TUI's own snapshot, not just the direct HTTP check.
            screen.wait(master, app, b"loaded", seconds=15, receive=receive)
            time.sleep(2.3)
            prior = set(children(app.pid))
            os.write(master, b"A")
            deadline = time.monotonic() + 20
            agent = None
            while time.monotonic() < deadline:
                if select.select([master], [], [], .1)[0]:
                    receive()
                for pid in set(children(app.pid)) - prior:
                    try:
                        argv = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
                        if b"--model" in argv:
                            agent = pid
                            break
                    except FileNotFoundError:
                        pass
                if agent is not None:
                    break
            assert agent is not None, "No real agent launched"
            argv = Path(f"/proc/{agent}/cmdline").read_bytes().split(b"\0")
            actual_env = dict(item.split(b"=", 1) for item in Path(f"/proc/{agent}/environ").read_bytes().split(b"\0") if b"=" in item)
            assert token.encode() not in b" ".join(argv), "Credential exposed in arguments"
            assert b"stale-fixture" not in actual_env.values(), "Inherited session credential retained"
            assert model_id.encode() in argv[argv.index(b"--model") + 1]
            key_name = b"LLAMA_CPP_API_KEY" if args.agent == "omp" else b"LLAMANCER_PI_KEY"
            assert actual_env.get(key_name) == token.encode(), "Wrong session credential"
            assert Path(f"/proc/{agent}/cwd").resolve() == project
            print(f"{args.agent}: actual launch and scoped credential verified", flush=True)
            # Exercise recovery from an independently terminated foreground
            # agent. This does not certify the agent's own quit keybindings.
            os.kill(agent, signal.SIGTERM)
            returned = Screen()
            deadline = time.monotonic() + 15
            while agent in children(app.pid) and time.monotonic() < deadline:
                if select.select([master], [], [], .1)[0]:
                    returned.feed(receive())
            if agent in children(app.pid):
                os.kill(agent, signal.SIGKILL)
            returned.wait(master, app, b"LLAMANCER", seconds=20, receive=receive)
            assert app.poll() is None and agent not in children(app.pid)
            os.write(master, b"q")
            read_until(master, app, b"\x1b[?1049l", seconds=20)
            assert app.wait(timeout=30) == 0
            assert termios.tcgetattr(slave) == original
            with socket.socket() as sock:
                sock.settimeout(2)
                assert sock.connect_ex(("127.0.0.1", port)) != 0
            print(f"{args.agent}: real interactive launch, model/cwd, scoped credential, return to TUI and shutdown passed.")
        finally:
            if app is not None and app.poll() is None:
                app.terminate()
                app.wait(timeout=30)
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    main()
