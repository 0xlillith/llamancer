#!/usr/bin/env python3
"""Opt-in real KDE/Kitty foreground agent acceptance with existing synthetic settings.

No personal agent configuration or actual tokens are read or changed. The
private Kitty instance uses a socket-only remote-control channel. It is visible
on the current desktop and closes automatically on successful completion.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import tempfile
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "server", "model"):
        parser.add_argument(name, type=Path)
    parser.add_argument("agent", choices=("omp", "pi"))
    args = parser.parse_args()
    binary, server, model = (p.resolve(strict=True) for p in
                             (args.binary, args.server, args.model))
    gui = dict(os.environ)
    # Pick up the graphical session's display coordinates when invoked by SSH.
    for line in subprocess.check_output(
            ["systemctl", "--user", "show-environment"], text=True).splitlines():
        key, _, value = line.partition("=")
        if key in ("DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR",
                   "DBUS_SESSION_BUS_ADDRESS", "XDG_SESSION_TYPE"):
            gui[key] = value
    with tempfile.TemporaryDirectory(prefix="llamancer-kitty-agent-") as scratch:
        root = Path(scratch)
        home, project, config = root / "home", root / "project", root / "config/llamancer"
        for directory in (home, project, config):
            directory.mkdir(parents=True)
        token = secrets.token_hex(24)
        key_file = root / "key"
        key_file.write_text(token + "\n")
        key_file.chmod(0o600)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        endpoint = f"http://127.0.0.1:{port}/"
        (config / "config.toml").write_text(
            f"model_imports = [{json.dumps(str(model))}]\n"
            f"server_binary = {json.dumps(str(server))}\n"
            f"endpoint = {json.dumps(endpoint)}\ndevice = \"none\"\n"
            f"default_agent = {json.dumps(args.agent)}\n"
            f"project_dir = {json.dumps(str(project))}\n"
            f"api_key_file = {json.dumps(str(key_file))}\n"
            f"[profiles.{json.dumps(str(model))}.engine]\n"
            "context_size = 4096\ngpu_layers = 0\nthreads = 2\n")
        env = {k: v for k, v in gui.items() if k in (
            "PATH", "DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS", "XDG_SESSION_TYPE", "LANG", "LC_ALL")}
        env.update(HOME=str(home), XDG_CONFIG_HOME=str(config.parent),
                   NO_PROXY="127.0.0.1,localhost", LLAMA_CPP_API_KEY="stale-fixture",
                   LLAMANCER_PI_KEY="stale-fixture")
        if args.agent == "omp":
            for setting, value in (
                ("startup.setupWizard", "false"),
                ("input.bareExitOnEmptySession", "true"),
                ("theme.dark", "titanium"),
                ("modelRoles", '{"default":"llama.cpp/previous-fixture"}'),
            ):
                subprocess.run(["omp", "config", "set", setting, value],
                               cwd=project, env=env, check=True,
                               stdout=subprocess.DEVNULL, timeout=15)
            settings = home / ".omp/agent/config.yml"
        else:
            settings = home / ".pi/agent/settings.json"
            env["PI_CODING_AGENT_DIR"] = str(settings.parent)
            settings.parent.mkdir(parents=True)
            settings.write_text(json.dumps(
                {"defaultProvider": "llamancer-previous-fixture",
                 "defaultModel": "previous-fixture", "theme": "dark"}))
        original_settings = settings.read_bytes()
        remote = "unix:" + str(root / "kitty.sock")
        kitty = subprocess.Popen(
            ["kitty", "--config", "NONE", "--override",
             "allow_remote_control=socket-only", "--listen-on", remote,
             "--class", "llamancer-agent-acceptance", str(binary)],
            cwd=project, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        def rc(*arguments):
            return subprocess.check_output(
                ["kitten", "@", "--to", remote, *arguments],
                env=gui, text=True, stderr=subprocess.DEVNULL, timeout=5)

        def screen():
            return rc("get-text")

        def wait(predicate, description, timeout=20):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if kitty.poll() is not None:
                    raise RuntimeError("Isolated Kitty window exited prematurely")
                try:
                    if predicate():
                        return
                except (OSError, subprocess.SubprocessError, ValueError, KeyError):
                    pass
                time.sleep(.25)
            raise TimeoutError(description)

        def request(path, body=None):
            req = urllib.request.Request(
                endpoint + path, data=None if body is None else json.dumps(body).encode(),
                headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
            with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(
                    req, timeout=2) as response:
                return json.load(response)

        def owned_processes():
            # Restrict recovery to exact private test cwd, even if Kitty exits early.
            for proc in Path("/proc").iterdir():
                if not proc.name.isdigit():
                    continue
                try:
                    if os.readlink(proc / "cwd") != str(project):
                        continue
                    name = (proc / "comm").read_text().strip()
                    if name in ("llamancer", "llama-server"):
                        yield int(proc.name), name
                except OSError:
                    continue

        try:
            wait(lambda: "Scan complete" in screen(), "TUI failed to open")
            rc("send-text", "4")
            time.sleep(.3)
            rc("send-text", "s")
            wait(lambda: len(request("models")["data"]) == 1, "Router unavailable", 45)
            model_id = request("models")["data"][0]["id"]
            assert request("models/load", {"model": model_id})["success"]
            wait(lambda: request("models")["data"][0]["status"]["value"] == "loaded",
                 "Model load failed", 45)
            wait(lambda: "loaded" in screen(), "TUI did not show loaded model")
            time.sleep(2.5)
            assert settings.read_bytes() == original_settings, "Pre-launch agent settings changed"
            rc("send-text", "A")
            # Wait for actual agent's composer and chosen model, not just its PID.
            wait(lambda: "LLAMANCER" not in screen() and model_id in screen(),
                 "Agent did not select the explicit local model", 30)
            if args.agent == "omp":
                rc("send-text", "quit")
                time.sleep(.3)
                rc("send-key", "enter")
            else:
                rc("send-key", "ctrl+d")
            wait(lambda: "LLAMANCER" in screen(), "Ordinary agent exit did not return to TUI")
            if args.agent == "omp":
                assert "bareExitOnEmptySession: true" in settings.read_text()
            else:
                assert json.loads(settings.read_text()).get("theme") == "dark"
            # Native agents may update their own defaults. No settings were
            # rewritten by Llamancer before launch, and unrelated preferences survive.
            print(f"{args.agent}: actual Kitty selected model overrides stored fixture; "
                  f"ordinary exit and preference preservation passed", flush=True)
            rc("send-text", "q")
            assert kitty.wait(timeout=30) == 0
            with socket.socket() as sock:
                assert sock.connect_ex(("127.0.0.1", port)) != 0, "Router port still open"
        finally:
            for pid, name in owned_processes():
                if name == "llamancer":
                    try:
                        os.kill(pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
            if kitty.poll() is None:
                try:
                    kitty.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    kitty.terminate()
                    kitty.wait(timeout=10)
            # Recovery for an unexpectedly disconnected graphical terminal.
            for pid, _ in owned_processes():
                try:
                    os.kill(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass


if __name__ == "__main__":
    main()
