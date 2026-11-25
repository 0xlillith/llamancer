#!/usr/bin/env python3
"""Report release startup, idle resident memory and CPU ticks on Linux."""
import argparse
import fcntl
import json
import os
import platform
import pty
import statistics
import struct
import subprocess
import tempfile
import termios
import time
from pathlib import Path

from terminal_smoke import read_until


def process_sample(pid):
    status = Path(f"/proc/{pid}/status").read_text()
    rss = int(next(line.split()[1] for line in status.splitlines()
                   if line.startswith("VmRSS:")))
    # /proc stat field 2 is parenthesized and may contain spaces.
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    ticks = int(fields[11]) + int(fields[12])
    return rss, ticks


def measure(binary):
    with tempfile.TemporaryDirectory(prefix="llamancer-benchmark-") as root:
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
            env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=root)
            start = time.perf_counter()
            process = subprocess.Popen([str(binary)], stdin=slave, stdout=slave,
                                       stderr=slave, env=env)
            read_until(master, process, b"LLAMANCER")
            startup_ms = (time.perf_counter() - start) * 1000
            time.sleep(0.25)
            rss_before, ticks_before = process_sample(process.pid)
            idle_start = time.monotonic()
            time.sleep(2)
            rss_after, ticks_after = process_sample(process.pid)
            idle_seconds = time.monotonic() - idle_start
            cpu_percent = (ticks_after - ticks_before) / os.sysconf("SC_CLK_TCK") / idle_seconds * 100
            os.write(master, b"q")
            read_until(master, process, b"\x1b[?1049l")
            assert process.wait(timeout=5) == 0
            return {"startup_ms": round(startup_ms, 3),
                    "idle_rss_kib": max(rss_before, rss_after),
                    "idle_cpu_percent": round(cpu_percent, 4)}
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            os.close(master)
            os.close(slave)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--runs", type=int, default=3)
    args = parser.parse_args()
    if not 1 <= args.runs <= 10:
        parser.error("--runs must be between 1 and 10")
    binary = args.binary.resolve(strict=True)
    samples = [measure(binary) for _ in range(args.runs)]
    report = {"architecture": platform.machine(), "kernel": platform.release(),
              "binary_bytes": binary.stat().st_size,
              "samples": samples,
              "median": {key: statistics.median(sample[key] for sample in samples)
                         for key in samples[0]}}
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
