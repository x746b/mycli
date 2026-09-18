#!/usr/bin/env python3
"""Offline Ctrl+C regression: python3 tools/test_cancellation.py [binary]."""

import http.server
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent.parent
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/debug/mycli"


class SilentAPI(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        self.rfile.read(int(self.headers["Content-Length"]))
        mode = self.server.mode
        if mode != "before-headers":
            self.send_response(500 if mode == "error-body" else 200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            if mode == "after-reasoning":
                data = b'data: {"choices":[{"index":0,"delta":{"reasoning_content":"Thinking..."}}]}\n\n'
                self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
            self.wfile.flush()
        self.server.ready.set()
        # No further output. Cancellation must work without a new SSE event.
        self.connection.settimeout(5)
        try:
            if self.connection.recv(1) == b"":
                self.server.disconnected.set()
        except ConnectionResetError:
            self.server.disconnected.set()
        except socket.timeout:
            pass


def main():
    server = http.server.ThreadingHTTPServer(("127.0.0.2", 0), SilentAPI)
    server.ready = threading.Event()
    server.disconnected = threading.Event()
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="mycli-cancel-test-") as directory:
            config = Path(directory) / ".mycli/config.toml"
            config.parent.mkdir()
            config.write_text(f'''
[local.cancellation_test]
base_url = "http://127.0.0.2:{server.server_port}/v1"
api_key = "offline-test"
model = "mock-model"
context_window = 32768
tool_tier = "simple"
''')
            for mode in ("before-headers", "silent-body", "after-reasoning", "error-body"):
                server.mode = mode
                server.ready.clear()
                server.disconnected.clear()
                proc = subprocess.Popen(
                    [str(BINARY), "--local", "cancellation_test", "Wait for interruption."],
                    cwd=directory,
                    env=dict(os.environ, NO_PROXY="127.0.0.2", no_proxy="127.0.0.2"),
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                )
                try:
                    assert server.ready.wait(5), "Request never reached the mock server"
                    started = time.monotonic()
                    proc.send_signal(signal.SIGINT)
                    stdout, stderr = proc.communicate(timeout=3)
                    assert proc.returncode == 0, stdout + stderr
                    assert server.disconnected.wait(2), "HTTP request kept running after Ctrl+C"
                    print(f"PASS: {mode}: Ctrl+C closed request in {time.monotonic() - started:.2f}s")
                finally:
                    if proc.poll() is None:
                        proc.kill()
                        proc.communicate()
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
