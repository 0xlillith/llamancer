#!/usr/bin/env python3
"""Opt-in real two-version install, selection, rollback and in-use removal refusal."""
import fcntl
import json
import os
import pty
import socket
import struct
import subprocess
import sys
import termios
import tempfile
import time
import tomllib
import urllib.request
from pathlib import Path
from backend_ui_live import Screen
from terminal_smoke import read_until


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    model = Path(sys.argv[2]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="llamancer-versions-") as scratch:
        root = Path(scratch)
        data = root / "data"
        cfg = root / "config/llamancer"
        cfg.mkdir(parents=True)
        env = dict(os.environ, TERM="xterm-256color", XDG_DATA_HOME=str(data),
                   XDG_CONFIG_HOME=str(cfg.parent))
        old, new = [Path(subprocess.check_output(
            [binary, "backend", "install", "cpu", version], env=env, text=True,
            timeout=90).strip()) for version in ("b12721", "b12726")]
        assert old.is_file() and new.is_file()
        (cfg / "config.toml").write_text("model_imports = [" + json.dumps(str(model)) + "]\n")
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
        app = server = None
        try:
            app = subprocess.Popen([binary], env=env, stdin=slave, stdout=slave, stderr=slave)
            screen = Screen()
            screen.wait(master, app, b"LLAMANCER")
            def key(value):
                os.write(master, value)
                time.sleep(.15)
            def select_version(version):
                key(b"\r")
                screen.wait(master, app, b"Backend selected", 15)
                settings = tomllib.loads((cfg / "config.toml").read_text())
                assert version in settings["server_binary"], settings["server_binary"]
            key(b"5")
            key(b"B")
            screen.wait(master, app, b"Backend Manager")
            select_version("b12721")
            key(b"v")
            screen.wait(master, app, b"b12726")
            select_version("b12726")
            key(b"v")
            screen.wait(master, app, b"b12721")
            select_version("b12721")
            # A separate supervisor with its own config uses the newer version.
            service = root / "service/llamancer"
            service.mkdir(parents=True)
            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                port = sock.getsockname()[1]
            endpoint = f"http://127.0.0.1:{port}"
            (service / "config.toml").write_text(
                "model_imports = [" + json.dumps(str(model)) + "]\n"
                + "server_binary = " + json.dumps(str(new)) + "\n"
                + "endpoint = " + json.dumps(endpoint) + "\ndevice = \"none\"\n")
            server = subprocess.Popen([binary, "serve"], env=dict(env, XDG_CONFIG_HOME=str(service.parent)),
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                assert server.poll() is None
                try:
                    with urllib.request.urlopen(endpoint + "/props", timeout=1) as res:
                        if json.load(res).get("role") == "router":
                            break
                except OSError:
                    time.sleep(.2)
            else:
                raise TimeoutError("Isolated router not ready")
            key(b"v")
            screen.wait(master, app, b"b12726")
            key(b"d")
            screen.wait(master, app, b"y confirm")
            key(b"y")
            screen.wait(master, app, b"Backend storage is in use", 15)
            assert new.is_file()
            server.terminate()
            assert server.wait(timeout=30) == 0
            key(b"d")
            screen.wait(master, app, b"y confirm")
            key(b"y")
            screen.wait(master, app, b"Managed backend removed", 15)
            assert old.is_file() and not new.exists()
            key(b"\x1b")
            key(b"q")
            read_until(master, app, b"\x1b[?1049l")
            assert app.wait(timeout=5) == 0
            print("Real side-by-side CPU installs, upgrade, rollback, in-use refusal and unused removal passed.")
        finally:
            for proc in (app, server):
                if proc is not None and proc.poll() is None:
                    proc.terminate()
                    proc.wait(timeout=30)
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    main()
