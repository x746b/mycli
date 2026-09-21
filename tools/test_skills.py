#!/usr/bin/env python3
"""Offline skill catalog, invocation, tool-call, reload, and picker regression."""
import http.server
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/debug/mycli"


class API(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.requests.append(body)
        number = len(self.server.requests)
        if number == 2:
            delta = {"tool_calls": [{"index": 0, "id": "skill-call", "type": "function", "function": {
                "name": "Skill", "arguments": json.dumps({"skill": "external", "args": "tool-args"})}}]}
            reason = "tool_calls"
        else:
            delta = {"content": "mock complete"}
            reason = "stop"
        if number == 3:
            self.server.catalog.write_text("invalid TOML")
        if number == 4:
            self.server.catalog.write_text('''version=1
[skills.changed]
description="Updated internal skill"
argument_hint="<text>"
prompt="Changed $ARGUMENTS"
''')
        frames = [{"choices": [{"index": 0, "delta": delta, "finish_reason": reason}]}]
        payload = ("".join("data: " + json.dumps(frame) + "\n\n" for frame in frames) + "data: [DONE]\n\n").encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def menu(server, root, env):
    if not shutil.which("tmux"):
        print("SKIP: picker check (tmux unavailable)")
        return
    for index in range(40):
        (root / "extra" / f"zzz{index:02}.md").write_text(f"---\ndescription: {'A long description ' * 12}\n---\nCustom skill {index}")
    socket = str(root / "skills.sock")

    def tm(*args):
        return subprocess.check_output(["tmux", "-S", socket, *args], cwd=root, env=env, text=True)

    def wait(text):
        for _ in range(100):
            screen = tm("capture-pane", "-pt", "skills")
            if text in screen:
                return
            time.sleep(.05)
        raise AssertionError(f"Missing {text!r}: {screen}")

    def send(text):
        tm("send-keys", "-t", "skills", "-l", text)
        tm("send-keys", "-t", "skills", "Enter")

    tm("new-session", "-d", "-s", "skills", "-x", "140", "-y", "32", shlex.join([str(BINARY)]))
    try:
        wait("tools [full]")
        send("/skill")
        wait("Select skill")
        wait("changed [internal]")
        wait("external [")
        tm("send-keys", "-t", "skills", "Enter")
        wait("Arguments <text>")
        send("from-picker")
        wait("mock complete")
        assert server.requests[-1]["messages"][-1]["content"].endswith("Changed from-picker")
        count = len(server.requests)
        send("/skill")
        wait("Select skill")
        tm("send-keys", "-t", "skills", "Escape")
        time.sleep(.2)
        assert len(server.requests) == count
        send("/skill")
        wait("Select skill")
        tm("send-keys", "-t", "skills", "Up")
        wait("zzz39")
        tm("send-keys", "-t", "skills", "Enter")
        for _ in range(100):
            if len(server.requests) > count: break
            time.sleep(.05)
        assert "Custom skill 39" in server.requests[-1]["messages"][-1]["content"]
        print("PASS: picker lists both sources, accepts arguments, cancels, and scrolls a large catalog")
    finally:
        tm("kill-server")


def main():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), API)
    server.requests = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="mycli-skills-") as directory:
            root = Path(directory)
            config = root / "xdg/mycli"
            config.mkdir(parents=True)
            home = root / "home"
            home.mkdir()
            (config / "config.toml").write_text(f'''model="mock-model"
base_url="http://127.0.0.1:{server.server_port}/v1"
context_window=32768
tool_tier="full"
web_search=false
skill_paths=["extra"]
''')
            server.catalog = config / "skills-internal.toml"
            server.catalog.write_text('version=1\n[skills.echo]\ndescription="Echo test"\nprompt="Echo $ARGUMENTS"\n')
            extra = root / "extra/unrelated-folder"
            extra.mkdir(parents=True)
            (extra / "SKILL.md").write_text('---\nname: "external"\ndescription: >\n  External test\nargument-hint: "<text>"\n---\nExternal $ARGUMENTS; base=${CLAUDE_SKILL_DIR}')
            env = {key: value for key, value in os.environ.items() if not key.startswith("MYCLI_")}
            env.update(HOME=str(home), XDG_CONFIG_HOME=str(root / "xdg"), NO_PROXY="127.0.0.1", no_proxy="127.0.0.1", PYTHONDONTWRITEBYTECODE="1")
            commands = "\n".join([
                "/skill list", "/skill paths", "/skill external problem", "ask-tool",
                "/skill reload", "/skill echo retained", "/skill reload",
                "/skill echo removed", "/skill changed refreshed", "/skill external still-here", "/quit", "",
            ])
            result = subprocess.run([str(BINARY)], cwd=root, env=env, input=commands, capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, result.stderr
            assert len(server.requests) == 6, (len(server.requests), result.stderr)
            requests = server.requests
            assert "External problem" in requests[0]["messages"][-1]["content"]
            assert str(extra) in requests[0]["messages"][-1]["content"]
            assert any(tool["function"]["name"] == "Skill" for tool in requests[1]["tools"])
            assert any(m["role"] == "tool" and "External tool-args" in m["content"] for m in requests[2]["messages"])
            assert "Echo retained" in requests[3]["messages"][-1]["content"]
            assert "Changed refreshed" in requests[4]["messages"][-1]["content"]
            assert "External still-here" in requests[5]["messages"][-1]["content"]
            assert len(requests[4]["messages"]) > len(requests[3]["messages"]), "Reload reset conversation"
            assert "Skills unchanged" in result.stderr and "Skills reloaded" in result.stderr
            assert "Skill 'echo' not found" in result.stderr
            assert str(server.catalog) in result.stderr
            print("PASS: external and internal slash invocation, shared model tool, invalid reload rollback, add/remove, conversation preservation")
            menu(server, root, env)
            one = subprocess.run([str(BINARY), "--tools", "simple", "/skill changed one-shot"], cwd=root, env=env, capture_output=True, text=True, timeout=10)
            assert one.returncode == 0, one.stderr
            assert "Changed one-shot" in server.requests[-1]["messages"][-1]["content"]
            assert all(tool["function"]["name"] != "Skill" for tool in server.requests[-1]["tools"])
            print("PASS: single-shot skills work on simple tier without granting tools")
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
