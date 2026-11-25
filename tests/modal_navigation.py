#!/usr/bin/env python3
"""Real PTY mouse/keyboard modal regression, no external models or backends."""
import fcntl
import json
import os
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path
from backend_ui_live import Screen
from terminal_smoke import read_until


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="llamancer-modal-") as scratch:
        root = Path(scratch)
        config = root / "config/llamancer"
        config.mkdir(parents=True)
        models = root / "models"
        models.mkdir()
        for name in ("bad-a.gguf", "bad-b.gguf"):
            (models / name).write_bytes(b"broken")
        (config / "config.toml").write_text("model_roots = [" + json.dumps(str(models)) + "]\n")
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
            original = termios.tcgetattr(slave)
            env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=str(config.parent),
                       XDG_DATA_HOME=str(root / "data"))
            process = subprocess.Popen([str(binary)], stdin=slave, stdout=slave, stderr=slave, env=env)
            screen = Screen()
            def wait(marker):
                screen.wait(master, process, marker.encode())
            def key(data):
                os.write(master, data)
                time.sleep(.15)
            def click(x, y, code=0):
                key(f"\x1b[<{code};{x};{y}M".encode())
                if code==0:
                    key(f"\x1b[<0;{x};{y}m".encode())
            def settle():
                deadline=time.monotonic()+.3
                while time.monotonic()<deadline:
                    if select.select([master],[],[],.05)[0]:
                        screen.feed(os.read(master,65536))
            def current():
                return "\n".join("".join(row) for row in screen.rows)
            wait("Scan complete")
            key(b"i")
            wait("Invalid files")
            # Second invalid row: body starts at 0-based y=5, two header rows.
            click(5,9)
            wait("> "+str(models/"bad-b.gguf"))
            key(b"5")
            click(110,4)
            settle()
            assert "Invalid files" in current(), "Underlying tab activated from modal"
            key(b"\x1b")
            wait("Library")
            key(b"5")
            key(b"B")
            wait("Backend Manager")
            # Third backend row, 1-based y=11.
            click(5,11)
            wait("> rocm")
            key(b"d")
            wait("y confirm")
            click(5,11,65)
            settle()
            assert "y confirm" in current(), "Mouse wheel dismissed confirmation"
            key(b"n")
            wait("Cancelled")
            key(b"\x1b")
            key(b"q")
            read_until(master, process, b"\x1b[?1049l")
            assert process.wait(timeout=5)==0
            assert termios.tcgetattr(slave)==original
            print("Actual modal keyboard isolation, mouse selection, scroll-safe confirmation and restoration passed.")
        finally:
            if process is not None and process.poll() is None:
                process.terminate()
                process.wait(timeout=10)
            os.close(master)
            os.close(slave)


if __name__=="__main__":
    main()
