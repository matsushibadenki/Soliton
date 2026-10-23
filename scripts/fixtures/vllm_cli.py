"""Protocol/lifecycle fixture; never imports a GPU library or loads weights."""
import argparse
import json
import signal
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("command")
    parser.add_argument("model")
    parser.add_argument("--port", type=int)
    parser.add_argument("--session-token")
    args, _ = parser.parse_known_args(argv)
    if args.command == "inspect-model":
        print(json.dumps({"schema_version": 2, "runnable": False, "qualification": "unverified", "fits_memory": True, "architecture": "Gemma2ForCausalLM", "estimated_resident_bytes": 2147483648}))
        return 1
    worker = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(600)"])
    print(f"[fixture] worker {worker.pid}", flush=True)
    started = time.monotonic()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def reply(self, status, body):
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(body).encode())

        def authorized(self):
            if self.headers.get("Authorization") != "Bearer " + args.session_token:
                self.reply(401, {"error": {"message": "private token required"}})
                return False
            return True

        def do_GET(self):
            if not self.authorized():
                return
            if self.path == "/health":
                ready = time.monotonic() - started >= 2
                self.reply(200, {"inference_ready": ready, "control_ready": True, "status": "ready" if ready else "loading_model"})
            elif self.path == "/v1/models":
                self.reply(200, {"data": [{"id": "default_model", "object": "model"}]})
            else:
                self.reply(404, {"error": {"message": "unsupported route"}})

        def do_POST(self):
            if not self.authorized():
                return
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            if self.path != "/v1/chat/completions":
                self.reply(404, {"error": {"message": "unsupported route"}})
                return
            if body.get("model") != "default_model":
                self.reply(400, {"error": {"message": "wrong model alias"}})
                return
            if not body.get("stream"):
                self.reply(200, {"model": "default_model", "choices": [{"message": {"content": "Apple fixture"}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 9, "completion_tokens": 3, "total_tokens": 12}})
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            try:
                for word in ["Apple ", "fixture ", "response"]:
                    self.wfile.write(("data: " + json.dumps({"choices": [{"delta": {"content": word}}]}) + "\n\n").encode())
                    self.wfile.flush()
                    time.sleep(0.3)
                self.wfile.write(b'data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}}\n\ndata: [DONE]\n\n')
                self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass

    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    signal.signal(signal.SIGTERM, lambda *_: threading.Thread(target=server.shutdown).start())
    try:
        server.serve_forever(poll_interval=0.05)
    finally:
        server.server_close()
        worker.terminate()
        worker.wait(timeout=3)
    return 0
