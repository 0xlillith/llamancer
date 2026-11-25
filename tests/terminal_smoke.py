#!/usr/bin/env python3
"""Headless Linux terminal smoke test. No third-party Python dependencies."""
import fcntl
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path


def read_until(master, process, marker, seconds=10):
    deadline = time.monotonic() + seconds
    data = bytearray()
    while time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            data.extend(os.read(master, 65536))
            if marker in data:
                return bytes(data)
        if process.poll() is not None and not select.select([master], [], [], 0)[0]:
            break
    raise AssertionError(f"Missing terminal marker {marker!r}")


def check(binary, terminate=False):
    with tempfile.TemporaryDirectory(prefix="llamancer-terminal-") as root:
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
            original = termios.tcgetattr(slave)
            env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=root)
            process = subprocess.Popen(
                [binary], stdin=slave, stdout=slave, stderr=slave, env=env
            )
            start = read_until(master, process, b"LLAMANCER")
            assert b"\x1b[?1049h" in start, "Alternate screen was not entered"
            for key in b"2\t3451":
                os.write(master, bytes([key]))
                time.sleep(0.08)
                if select.select([master], [], [], 0.5)[0]:
                    os.read(master, 65536)
                assert process.poll() is None, "TUI exited during navigation"
            if terminate:
                process.send_signal(signal.SIGTERM)
            else:
                os.write(master, b"q")
            read_until(master, process, b"\x1b[?1049l")
            assert process.wait(timeout=5) == 0
            assert termios.tcgetattr(slave) == original, "Terminal attributes not restored"
            print("Terminal startup, navigation, " + ("SIGTERM" if terminate else "quit") + " and restoration passed.")
        finally:
            if process is not None and process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    binary = str(Path(sys.argv[1]).resolve())
    check(binary)
    check(binary, terminate=True)
