#!/usr/bin/env python3
"""Opt-in Pi executable smoke test using an authenticated local SSE fixture."""
import argparse
import hashlib
import http.server
import json
import os
import subprocess
import tempfile
import threading
from pathlib import Path

MODEL = "test-model"
KEY = "fixture-key"
TEXT = "Llamancer streaming test passed."
FILE_CONTENT = "llamancer-read-fixture-7f91"


class Fixture(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.send_error(404)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.requests.append((self.path, body, self.headers.get("Authorization")))
        if self.path != "/v1/chat/completions" or body.get("model") != MODEL:
            self.send_error(400)
            return
        if self.headers.get("Authorization") != f"Bearer {KEY}":
            self.send_error(401)
            return
        if not body.get("stream"):
            self.send_error(400)
            return
        if self.server.read_tool and len(self.server.requests) == 1:
            deltas = [
                ({"role": "assistant", "content": ""}, None),
                ({"tool_calls": [{"index": 0, "id": "call_fixture",
                                 "type": "function", "function": {
                                     "name": "read", "arguments": '{"path":"fixture.txt"}'}}]}, None),
                ({}, "tool_calls"),
            ]
        else:
            deltas = [({"role": "assistant", "content": ""}, None),
                      ({"content": TEXT}, None), ({}, "stop")]
        chunks = ["data: " + json.dumps({
            "id": "chatcmpl-fixture", "object": "chat.completion.chunk", "created": 1,
            "model": MODEL, "choices": [{"index": 0, "delta": delta, "finish_reason": stop}]
        }) for delta, stop in deltas]
        chunks.append("data: [DONE]")
        payload = ("\n\n".join(chunks) + "\n\n").encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pi", type=Path, help="Path to installed Pi executable")
    parser.add_argument("--read-tool", action="store_true")
    args = parser.parse_args()
    executable = args.pi.absolute()
    extension = Path(__file__).resolve().parent.parent / "src/agents/pi.js"
    with http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture) as server, \
            tempfile.TemporaryDirectory(prefix="llamancer-pi-test-") as work:
        server.requests = []
        server.read_tool = args.read_tool
        (Path(work) / "fixture.txt").write_text(FILE_CONTENT)
        endpoint = f"http://127.0.0.1:{server.server_port}/"
        provider = "llamancer-local-" + hashlib.sha256(
            f"{endpoint}\n{MODEL}".encode()).hexdigest()[:16]
        home = Path(work) / "home"
        home.mkdir()
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"),
               "HOME": str(home), "TERM": "xterm-256color",
               "LLAMANCER_ENDPOINT": endpoint, "LLAMANCER_MODEL": MODEL,
               "LLAMANCER_PROVIDER": provider, "LLAMANCER_CONTEXT": "8192",
               "LLAMANCER_PI_KEY": KEY}
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            result = subprocess.run(
                [str(executable), "--extension", str(extension),
                 "--model", f"{provider}/{MODEL}",
                 *(["--tools", "read"] if args.read_tool else ["--no-tools"]),
                 "--no-mcp", "--no-session", "--no-context-files", "--offline",
                 "--mode", "json", "-p", "Reply briefly."],
                env=env, cwd=work, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT, text=True, timeout=25)
            assert result.returncode == 0, f"Pi exited with {result.returncode}"
            expected = 2 if args.read_tool else 1
            assert len(server.requests) == expected, f"Expected {expected} requests, got {len(server.requests)}"
            for path, body, auth in server.requests:
                assert path == "/v1/chat/completions"
                assert body["model"] == MODEL and body["stream"] is True
                assert auth == f"Bearer {KEY}"
            if args.read_tool:
                second = server.requests[1][1]
                assert any(m.get("role") == "tool" and FILE_CONTENT in json.dumps(m)
                           for m in second["messages"]), "Read result did not return to the model"
            events = [json.loads(line) for line in result.stdout.splitlines()
                      if line.startswith("{")]
            assert any(event.get("type") == "message_end"
                       and event.get("message", {}).get("role") == "assistant"
                       and TEXT in json.dumps(event["message"])
                       for event in events), "Missing streamed final assistant message"
            print("Pi authenticated endpoint, model selection, SSE and final assistant text passed."
                  + (" Read-tool round trip passed." if args.read_tool else ""))
        except subprocess.TimeoutExpired:
            print("Timed out; request count",len(server.requests))
            raise
        finally:
            server.shutdown()
            thread.join()


if __name__ == "__main__":
    main()
