#!/usr/bin/env python3
"""Offline prompt/config/reload regression: python3 tools/test_prompts.py [binary]."""
import http.server
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading

ROOT = Path(__file__).resolve().parent.parent
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/debug/mycli"


class API(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.requests.append(body)
        index = len(self.server.requests)
        if index == 2:
            self.server.catalog.write_text("version=1\n[personas.reviewer]\nprompt='REVIEWER'\n")
        elif index == 3:
            self.server.catalog.write_text("invalid TOML")
        elif index == 4:
            self.server.catalog.write_text("version=1\n[personas]\n")
        chunks = [
            {"choices": [{"index": 0, "delta": {"content": "Mock response."}, "finish_reason": None}]},
            {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
        ]
        payload = ("".join("data: " + json.dumps(chunk) + "\n\n" for chunk in chunks) + "data: [DONE]\n\n").encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def main():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), API)
    server.requests = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="mycli-prompts-") as directory:
            root = Path(directory)
            home = root / "home"
            config = root / "xdg/mycli"
            config.mkdir(parents=True)
            (home / ".mycli").mkdir(parents=True)
            # Both global paths exist; the preferred one must supply model/endpoint.
            (home / ".mycli/config.toml").write_text('model="legacy-wrong"\nbase_url="http://127.0.0.1:1/v1"\n')
            (config / "config.toml").write_text(f'''model="mock-model"
base_url="http://127.0.0.1:{server.server_port}/v1"
context_window=32768
tool_tier="simple"
web_search=false
''')
            server.catalog = config / "system-prompts.toml"
            server.catalog.write_text("version=1\n[personas.code]\nprompt='CUSTOM CODE'\n")
            project = root / "project"
            (project / ".config/mycli").mkdir(parents=True)
            (project / ".config/mycli/instructions.md").write_text("NEW PROJECT INSTRUCTIONS")
            env = {key: value for key, value in os.environ.items() if not key.startswith("MYCLI_")}
            env.update(HOME=str(home), XDG_CONFIG_HOME=str(root / "xdg"), NO_PROXY="127.0.0.1", no_proxy="127.0.0.1")
            commands = "\n".join([
                "first", "/prompts path", "/persona Neutral", "second",
                "/prompts reload", "/persona reviewer", "third",
                "/prompts reload", "fourth", "/prompts reload", "fifth", "/quit", "",
            ])
            result = subprocess.run([str(BINARY)], cwd=project, env=env,
                                    input=commands, capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, result.stdout + result.stderr
            requests = server.requests
            assert len(requests) == 5, (len(requests), result.stdout, result.stderr)
            def system(request):
                return next(message["content"] for message in request["messages"] if message["role"] == "system")
            prompts = [system(request) for request in requests]
            assert prompts[0].startswith("CUSTOM CODE\n")
            assert not prompts[1].startswith("CUSTOM CODE")
            assert "Be concise and direct" not in prompts[1]
            assert prompts[2].startswith("REVIEWER\n")
            assert prompts[3].startswith("REVIEWER\n"), "Invalid reload lost active prompts"
            assert not prompts[4].startswith("REVIEWER"), "Removed active persona did not fall back"
            assert "Be concise and direct" not in prompts[4]
            assert all("NEW PROJECT INSTRUCTIONS" in prompt for prompt in prompts)
            assert all(request["model"] == "mock-model" for request in requests)
            assert "Prompts unchanged" in result.stderr
            assert str(server.catalog) in result.stderr
            assert len(requests[3]["messages"]) > len(requests[2]["messages"]), "Failed reload reset conversation"
            assert len(requests[4]["messages"]) == len(requests[0]["messages"]), "Successful reload did not reset conversation"
            print("PASS: XDG precedence, project instructions, Neutral, persona add/remove, reload rollback, conversation reset")
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
