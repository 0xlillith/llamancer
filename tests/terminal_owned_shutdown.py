#!/usr/bin/env python3
"""Opt-in actual TUI-owned server SIGTERM cleanup and PTY restoration."""
import fcntl
import json
import os
import pty
import select
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import termios
import time
import urllib.request
from pathlib import Path
from terminal_smoke import read_until
from backend_ui_live import Screen


def main():
    binary, server, model = (Path(p).resolve(strict=True) for p in sys.argv[1:4])
    with tempfile.TemporaryDirectory(prefix="llamancer-tui-owned-") as scratch:
        root = Path(scratch)
        config = root / "llamancer"
        config.mkdir()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        endpoint = f"http://127.0.0.1:{port}"
        (config / "config.toml").write_text(
            "model_imports = [" + json.dumps(str(model)) + "]\n"
            + "server_binary = " + json.dumps(str(server)) + "\n"
            + "endpoint = " + json.dumps(endpoint) + "\ndevice = \"none\"\n"
        )
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
            original = termios.tcgetattr(slave)
            env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=str(root))
            process = subprocess.Popen([binary], stdin=slave, stdout=slave, stderr=slave, env=env)
            Screen().wait(master, process, b"Scan complete")
            os.write(master, b"4")
            time.sleep(.2)
            os.write(master, b"s")
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                if select.select([master], [], [], .1)[0]:
                    os.read(master, 65536)
                assert process.poll() is None
                try:
                    with urllib.request.urlopen(endpoint + "/props", timeout=1) as reply:
                        if json.load(reply).get("role") == "router":
                            break
                except OSError:
                    pass
            else:
                raise TimeoutError("Owned router not ready")
            process.send_signal(signal.SIGTERM)
            read_until(master, process, b"\x1b[?1049l", seconds=20)
            assert process.wait(timeout=30) == 0
            assert termios.tcgetattr(slave) == original
            with socket.socket() as sock:
                sock.settimeout(2)
                assert sock.connect_ex(("127.0.0.1", port)) != 0
            print("Actual TUI-owned router stopped and terminal restored on SIGTERM.")
        finally:
            if process is not None and process.poll() is None:
                process.terminate()
                process.wait(timeout=30)
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    main()
