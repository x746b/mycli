#!/usr/bin/env python3
"""Offline request/stream regression: python3 tools/test_reasoning.py [binary]."""

import http.server
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import shlex
import shutil


ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/debug/mycli"
REQUESTS = []
MODE = "text"


class MockAPI(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        REQUESTS.append((self.path, body))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        if self.path.endswith("/chat/completions"):
            frames = [
                {"choices": [{"delta": {"content": "mock complete"}, "finish_reason": "stop"}]},
                {"choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 12}},
            ]
            for frame in frames:
                self.wfile.write(("data: " + json.dumps(frame) + "\n\n").encode())
            self.wfile.write(b"data: [DONE]\n\n")
            return
        frames = [{"type": "response.created", "response": {"id": "resp_mock", "model": body["model"]}}]
        if MODE == "tool" and len(REQUESTS) == 1:
            reasoning = {"type": "reasoning", "id": "rs_mock", "summary": [], "encrypted_content": "opaque-mock-state"}
            frames += [
                {"type": "response.output_item.added", "output_index": 0, "item": reasoning},
                {"type": "response.reasoning_summary_text.delta", "output_index": 0, "delta": "Checking the file."},
                {"type": "response.output_item.done", "output_index": 0, "item": reasoning},
                {"type": "response.output_item.added", "output_index": 1, "item": {"type": "function_call", "call_id": "call_mock", "name": "Read"}},
                {"type": "response.function_call_arguments.delta", "output_index": 1, "delta": json.dumps({"file_path": str(ROOT / "Cargo.toml")})},
                {"type": "response.output_item.done", "output_index": 1, "item": {"type": "function_call"}},
            ]
        else:
            frames += [
                {"type": "response.output_item.added", "output_index": 0, "item": {"type": "message"}},
                {"type": "response.output_text.delta", "output_index": 0, "delta": "mock complete ✓"},
                {"type": "response.output_item.done", "output_index": 0, "item": {"type": "message"}},
            ]
        frames.append({"type": "response.completed", "response": {"status": "completed", "usage": {"input_tokens": 100, "output_tokens": 12, "input_tokens_details": {"cached_tokens": 80}}}})
        for frame in frames:
            # Split UTF-8 across transport writes: SSE decoding must preserve it.
            encoded = ("data: " + json.dumps(frame, ensure_ascii=False) + "\n\n").encode()
            for start in range(0, len(encoded), 7):
                self.wfile.write(encoded[start:start + 7])
        self.wfile.flush()


def run(server, model, effort, mode="text"):
    global MODE
    MODE = mode
    REQUESTS.clear()
    env = dict(os.environ, NO_PROXY="127.0.0.2", no_proxy="127.0.0.2")
    result = subprocess.run([
        str(BINARY), "--cloud", "openai", "-m", model,
        "--base-url", f"http://127.0.0.2:{server.server_port}/v1",
        "--api-key", "offline-test-key", "--reasoning", effort,
        "-t", "simple", "-y", "Read Cargo.toml if requested, then say mock complete.",
    ], cwd=ROOT, env=env, capture_output=True, text=True, timeout=20)
    assert result.returncode == 0, result.stderr
    assert "mock complete" in result.stdout + result.stderr, result.stderr
    return list(REQUESTS)


def main():
    server = http.server.ThreadingHTTPServer(("127.0.0.2", 0), MockAPI)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        requests = run(server, "gpt-5.6-luna", "high", "tool")
        assert len(requests) == 2, requests
        for path, body in requests:
            assert path == "/v1/responses"
            assert body["reasoning"]["effort"] == "high"
            assert body["store"] is False
            assert body["tools"][0]["type"] == "function"
            assert "function" not in body["tools"][0]
        items = requests[1][1]["input"]
        assert any(i.get("encrypted_content") == "opaque-mock-state" for i in items)
        assert any(i.get("type") == "function_call_output" and i["call_id"] == "call_mock" for i in items)
        print("PASS: OpenAI high effort, function tool round trip, encrypted reasoning replay")
        requests = run(server, "gpt-5.6-luna", "default")
        assert requests[0][0] == "/v1/responses"
        assert "effort" not in requests[0][1]["reasoning"]
        requests = run(server, "gpt-5.6-luna", "none")
        assert requests[0][1]["reasoning"]["effort"] == "none"
        print("PASS: default uses server default, explicit off remains off")
        for model, effort in [("deepseek-chat", "high"), ("deepseek-chat", "none"), ("kimi-k3", "max"), ("gemini-3.1-pro-preview", "medium")]:
            requests = run(server, model, effort)
            path, body = requests[0]
            assert path == "/v1/chat/completions"
            if model == "deepseek-chat":
                assert body["thinking"]["type"] == ("disabled" if effort == "none" else "enabled")
            if effort != "none":
                assert body["reasoning_effort"] == effort
        print("PASS: DeepSeek on/off, Kimi effort, Gemini effort")
        if shutil.which("tmux"):
            test_menu(server)
    finally:
        server.shutdown()
        server.server_close()


def test_menu(server):
    global MODE
    MODE = "text"
    REQUESTS.clear()
    env = dict(os.environ, NO_PROXY="127.0.0.2", no_proxy="127.0.0.2")
    with tempfile.TemporaryDirectory(prefix="mycli-reasoning-menu-") as directory:
        socket = str(Path(directory) / "tmux.sock")

        def tm(*args):
            return subprocess.check_output(["tmux", "-S", socket, *args], env=env, text=True)

        def screen():
            return tm("capture-pane", "-pt", "test")

        def wait_for(text):
            for _ in range(100):
                if text in screen():
                    return
                time.sleep(.05)
            raise AssertionError(f"Missing {text!r}: {screen()}")

        def command(text):
            tm("send-keys", "-t", "test", "-l", text)
            tm("send-keys", "-t", "test", "Enter")
            time.sleep(.2)

        cli = [str(BINARY), "--cloud", "openai", "-m", "gpt-5.6-luna", "--base-url",
               f"http://127.0.0.2:{server.server_port}/v1", "--api-key", "offline-test-key",
               "--reasoning", "low", "-t", "simple"]
        tm("new-session", "-d", "-s", "test", "-x", "120", "-y", "32", shlex.join(cli))
        try:
            wait_for("effort:low")
            command("first greeting")
            wait_for("mock complete")
            command("/reasoning")
            wait_for("Select reasoning level for gpt-5.6-luna")
            tm("send-keys", "-t", "test", "Down", "Down", "Enter")
            wait_for("effort:high")
            command("second greeting")
            for _ in range(100):
                if len(REQUESTS) == 2:
                    break
                time.sleep(.05)
            assert len(REQUESTS) == 2
            assert REQUESTS[0][1]["reasoning"]["effort"] == "low"
            assert REQUESTS[1][1]["reasoning"]["effort"] == "high"
            assert "first greeting" in json.dumps(REQUESTS[1][1]["input"])
            command("/cloud openai")
            wait_for("Select reasoning level for gpt-5.6-luna")
            tm("send-keys", "-t", "test", "Escape")
            time.sleep(.2)
            assert "effort:high" in screen()
            command("/reasoning ultra")
            wait_for("not supported")
            assert "effort:high" in screen()
            command("/reasoning default")
            wait_for("effort:default")
            command("/cloud")
            wait_for("Select cloud")
            tm("send-keys", "-t", "test", "Escape")
            time.sleep(.2)
            assert "effort:default" in screen()
            assert len(REQUESTS) == 2, "menu commands must not send model requests"
            assert screen().splitlines()[-3] == "─" * 120
            print("PASS: effort menu, cloud follow-up menu, Esc cancellation, validation, retained conversation, footer")
        finally:
            # Do not exit normally: the test must not save commands into user history.
            tm("kill-session", "-t", "test")


if __name__ == "__main__":
    main()
