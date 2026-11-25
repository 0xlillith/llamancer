#!/usr/bin/env python3
"""Opt-in OMP executable smoke test using an authenticated local SSE fixture."""
import argparse
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
        from urllib.parse import urlsplit
        path = urlsplit(self.path).path
        model = {"id": MODEL, "object": "model", "status": {"value": "loaded"},
                 "meta": {"n_ctx": 8192, "n_ctx_train": 8192}, "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}}
        responses = {"/health": {"status": "ok"}, "/props": {"role": "router"},
                     "/models": {"data": [model]}, "/v1/models": {"object": "list", "data": [model]}}
        if path not in responses:
            self.send_error(404)
            return
        payload = json.dumps(responses[path]).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        auth = self.headers.get("Authorization")
        if self.path != "/v1/responses" or body.get("model") != MODEL:
            self.send_error(400)
            return
        if auth != f"Bearer {KEY}":
            self.send_error(401)
            return
        if not body.get("stream"):
            self.send_error(400)
            return
        # OMP performs capability probes without instructions before the actual turn.
        actual = bool(body.get("instructions"))
        if actual:
            self.server.requests.append((self.path, body, auth))
        response = {"id": "resp_fixture", "object": "response", "status": "in_progress",
                    "model": MODEL}
        events = []
        def emit(kind, **fields):
            events.append({"type": kind, **fields})
        emit("response.created", response=response.copy())
        emit("response.in_progress", response=response.copy())
        if actual and self.server.read_tool and len(self.server.requests) == 1:
            arguments = '{"path":"fixture.txt"}'
            item = {"id": "fc_fixture", "type": "function_call", "call_id": "call_fixture",
                    "name": "read", "arguments": "", "status": "in_progress"}
            emit("response.output_item.added", output_index=0, item=item.copy())
            emit("response.function_call_arguments.delta", item_id=item["id"], output_index=0, delta=arguments)
            emit("response.function_call_arguments.done", item_id=item["id"], output_index=0, arguments=arguments)
            item.update(arguments=arguments, status="completed")
        else:
            item = {"id": "msg_fixture", "type": "message", "role": "assistant",
                    "status": "in_progress", "content": []}
            emit("response.output_item.added", output_index=0, item=item.copy())
            part = {"type": "output_text", "text": "", "annotations": [], "logprobs": []}
            emit("response.content_part.added", item_id=item["id"], output_index=0, content_index=0, part=part.copy())
            emit("response.output_text.delta", item_id=item["id"], output_index=0, content_index=0, delta=TEXT)
            emit("response.output_text.done", item_id=item["id"], output_index=0, content_index=0, text=TEXT)
            part["text"] = TEXT
            emit("response.content_part.done", item_id=item["id"], output_index=0, content_index=0, part=part)
            item.update(status="completed", content=[part])
        emit("response.output_item.done", output_index=0, item=item)
        response.update(status="completed", output=[item], created_at=1,
                        usage={"input_tokens": 20, "output_tokens": 10, "total_tokens": 30,
                               "input_tokens_details": {"cached_tokens": 0}})
        emit("response.completed", response=response)
        payload = "".join(f"event: {e['type']}\ndata: {json.dumps(e)}\n\n" for e in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("omp", type=Path, help="Path to installed OMP executable")
    parser.add_argument("--read-tool", action="store_true")
    args = parser.parse_args()
    executable = args.omp.absolute()
    with http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture) as server, \
            tempfile.TemporaryDirectory(prefix="llamancer-omp-test-") as work:
        server.requests = []
        server.read_tool = args.read_tool
        (Path(work) / "fixture.txt").write_text(FILE_CONTENT)
        endpoint = f"http://127.0.0.1:{server.server_port}/"
        home = Path(work) / "home"
        home.mkdir()
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"),
               "HOME": str(home), "TERM": "xterm-256color",
               "LLAMA_CPP_BASE_URL": endpoint, "LLAMA_CPP_API_KEY": KEY}
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            result = subprocess.run(
                [str(executable), "--model", f"llama.cpp/{MODEL}",
                 "--no-tools", *(["--tools", "read"] if args.read_tool else []),
                 "--no-extensions", "--no-skills", "--no-session", "--no-context-files",
                 "--offline", "--no-judge", "--no-spotlight", "--no-title",
                 "--max-time", "35s", "--mode", "json", "-p", "Reply briefly."],
                env=env, cwd=work, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT, text=True, timeout=45)
            print("OMP status",result.returncode,"requests",len(server.requests),flush=True)
            if result.returncode!=0:print("OMP failed",flush=True)
            assert result.returncode == 0, f"OMP exited with {result.returncode}"
            expected = 2 if args.read_tool else 1
            assert len(server.requests) == expected, f"Expected {expected} requests, got {len(server.requests)}"
            for path, body, auth in server.requests:
                assert path == "/v1/responses"
                assert body["model"] == MODEL and body["stream"] is True
                assert auth == f"Bearer {KEY}"
            if args.read_tool:
                second = server.requests[1][1]
                assert any(m.get("type") == "function_call_output" and FILE_CONTENT in json.dumps(m)
                           for m in second["input"]), "Read result did not return to the model"
            events = [json.loads(line) for line in result.stdout.splitlines()
                      if line.startswith("{")]
            assert any(event.get("type") == "message_end"
                       and event.get("message", {}).get("role") == "assistant"
                       and TEXT in json.dumps(event["message"])
                       for event in events), "Missing streamed final assistant message"
            print("OMP authenticated endpoint, model selection, SSE and final assistant text passed."
                  + (" Read-tool round trip passed." if args.read_tool else ""))
        except subprocess.TimeoutExpired:
            print("Timed out; request count",len(server.requests))
            raise
        finally:
            server.shutdown()
            thread.join()


if __name__ == "__main__":
    main()
