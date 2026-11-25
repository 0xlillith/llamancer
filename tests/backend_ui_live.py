#!/usr/bin/env python3
"""Opt-in PTY smoke for already installed official backends and a small local GGUF."""
import fcntl
import json
import codecs
import re
import os
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
import tomllib
from pathlib import Path
from terminal_smoke import read_until


class Screen:
    """Interpret the cursor-positioned diffs emitted by Ratatui at a fixed size."""
    def __init__(self):
        self.rows = [[" "] * 120 for _ in range(36)]
        self.row = self.col = 0
        self.buffer = ""
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")

    def feed(self, data):
        self.buffer += self.decoder.decode(data)
        while self.buffer:
            if self.buffer.startswith("\x1b"):
                if len(self.buffer) < 2:
                    return
                if self.buffer[1] == "[":
                    match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", self.buffer)
                    if not match:
                        return
                    params, _, command = match.groups()
                    self.buffer = self.buffer[match.end():]
                    if command in ("H", "f"):
                        parts = params.split(";")
                        self.row = int(parts[0] or "1") - 1
                        self.col = int(parts[1] or "1") - 1 if len(parts) > 1 else 0
                    elif command == "J" and params == "2":
                        self.rows = [[" "] * 120 for _ in range(36)]
                    continue
                self.buffer = self.buffer[2:]
                continue
            char, self.buffer = self.buffer[0], self.buffer[1:]
            if char == "\r":
                self.col = 0
            elif char == "\n":
                self.row += 1
            elif ord(char) >= 32:
                if 0 <= self.row < 36 and 0 <= self.col < 120:
                    self.rows[self.row][self.col] = char
                self.col += 1

    def wait(self, master, process, marker, seconds=10):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.1)[0]:
                self.feed(os.read(master, 65536))
            screen = "\n".join("".join(row) for row in self.rows)
            if marker.decode() in screen:
                return
            if process.poll() is not None:
                break
        raise AssertionError(f"Missing {marker!r}: {screen}")


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    model = Path(sys.argv[2]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="llamancer-backend-ui-") as root:
        config = Path(root) / "llamancer"
        config.mkdir()
        (config / "config.toml").write_text("model_imports = [" + json.dumps(str(model)) + "]\n")
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
            original = termios.tcgetattr(slave)
            env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=root)
            process = subprocess.Popen([str(binary)], stdin=slave, stdout=slave, stderr=slave, env=env)
            screen = Screen()
            screen.wait(master, process, b"LLAMANCER")
            time.sleep(0.3)
            os.write(master, b"4")
            time.sleep(0.2)
            os.write(master, b"B")
            screen.wait(master, process, b"Backend Manager")
            # Probe and use CPU, then benchmark the selected small local GGUF.
            os.write(master, b"p")
            screen.wait(master, process, b"(none)", seconds=15)
            os.write(master, b"\r")
            screen.wait(master, process, b"Backend selected", seconds=15)
            settings = tomllib.loads((config / "config.toml").read_text())
            assert settings["device"] == "none"
            assert "b12721-x64" in settings["server_binary"] or "b12721-arm64" in settings["server_binary"]
            os.write(master, b"b")
            time.sleep(0.2)
            os.write(master, b"y")
            screen.wait(master, process, b"Working", seconds=5)
            screen.wait(master, process, b"pp 0 / tg 32:", seconds=90)
            # A selected installation must not be removable.
            os.write(master, b"dy")
            screen.wait(master, process, b"Select another backend", seconds=15)
            os.write(master, b"\x1b[B")
            time.sleep(0.2)
            os.write(master, b"\r")
            screen.wait(master, process, b"Working", seconds=5)
            screen.wait(master, process, b"Backend selected", seconds=15)
            settings = tomllib.loads((config / "config.toml").read_text())
            assert settings["device"].startswith("Vulkan")
            assert "vulkan-" in settings["server_binary"]
            os.write(master, b"b")
            time.sleep(0.2)
            os.write(master, b"y")
            screen.wait(master, process, b"Working", seconds=5)
            screen.wait(master, process, b"pp 0 / tg 32:", seconds=90)
            os.write(master, b"\x1b")
            time.sleep(0.3)
            os.write(master, b"q")
            read_until(master, process, b"\x1b[?1049l")
            assert process.wait(timeout=5) == 0
            assert termios.tcgetattr(slave) == original
            print("Backend TUI probe, selection, CPU/Vulkan benchmarks, selected-removal refusal and terminal restoration passed.")
        finally:
            if process is not None and process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    main()
