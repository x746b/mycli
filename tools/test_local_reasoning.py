#!/usr/bin/env python3
"""Offline local-profile reasoning checks; no inference server or credentials needed."""
import http.server
import os
from pathlib import Path
import subprocess
import shlex
import shutil
import time
import tempfile
import threading

from test_reasoning import BINARY, MockAPI, REQUESTS


def check_picker(root, env):
    """Exercise the real picker on a private tmux socket, never the user's server."""
    if not shutil.which("tmux"):
        print("SKIP: interactive picker check (tmux unavailable)")
        return
    socket = str(root / "picker.sock")

    def tm(*args):
        return subprocess.check_output(["tmux", "-S", socket, *args], env=env, cwd=root, text=True)

    def wait_for(text):
        for _ in range(100):
            screen = tm("capture-pane", "-pt", "picker")
            if text in screen:
                return screen
            time.sleep(0.05)
        raise AssertionError(f"Missing {text!r}: {screen}")

    def command(text):
        tm("send-keys", "-t", "picker", "-l", text)
        tm("send-keys", "-t", "picker", "Enter")

    cli = shlex.join([str(BINARY), "--local", "custom"])
    tm("new-session", "-d", "-s", "picker", "-x", "120", "-y", "32", cli)
    try:
        wait_for("effort:deep")
        command("/reasoning")
        screen = wait_for("Select reasoning level")
        assert "brief" in screen and "deep" in screen
        tm("send-keys", "-t", "picker", "Up", "Enter")
        wait_for("effort:brief")
        REQUESTS.clear()
        command("hello")
        wait_for("mock complete")
        assert REQUESTS[0][1]["reasoning_effort"] == "brief"
        command("/local cold")
        wait_for("effort:default")
        command("/reasoning")
        wait_for("Spoon")
        tm("send-keys", "-t", "picker", "Escape")
        wait_for("Cancelled")
        print("PASS: local picker displays custom and Cold-Fusion levels; selection reaches provider")
    finally:
        tm("kill-server")


def main():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), MockAPI)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="mycli-local-reasoning-") as directory:
            root = Path(directory)
            config_dir = root / "xdg/mycli"
            config_dir.mkdir(parents=True)
            home = root / "home"
            home.mkdir()
            (config_dir / "config.toml").write_text(f'''
base_url = "http://127.0.0.1:{server.server_port}/v1"
context_window = 32768
tool_tier = "simple"
web_search = false
[local.custom]
model = "unknown-local-model"
context_window = 32768
reasoning_levels = ["brief", "deep"]
reasoning_effort = "deep"
[local.cold]
model = "DavidAU-Qwen-Cold-Fusion-709"
context_window = 32768
[local.disabled]
model = "DavidAU-Qwen-Cold-Fusion-709"
context_window = 32768
reasoning_levels = []
''')
            env = {key: value for key, value in os.environ.items() if not key.startswith("MYCLI_")}
            env.update(HOME=str(home), XDG_CONFIG_HOME=str(root / "xdg"),
                       NO_PROXY="127.0.0.1", no_proxy="127.0.0.1", PYTHONDONTWRITEBYTECODE="1")

            def run(profile, *args, commands=None, success=True):
                REQUESTS.clear()
                result = subprocess.run([str(BINARY), "--local", profile, *args],
                                        input=commands, cwd=root, env=env,
                                        capture_output=True, text=True, timeout=20)
                assert (result.returncode == 0) == success, result.stderr
                return list(REQUESTS), result

            requests, _ = run("custom", "hello")
            assert requests[0][1]["reasoning_effort"] == "deep"
            assert requests[0][0] == "/v1/chat/completions"
            requests, _ = run("custom", "--reasoning", "brief", "hello")
            assert requests[0][1]["reasoning_effort"] == "brief"
            requests, _ = run("custom", "--reasoning", "default", "hello")
            assert "reasoning_effort" not in requests[0][1]
            requests, result = run("custom", "--reasoning", "high", "hello", success=False)
            assert not requests and "Available: default, brief, deep" in result.stderr
            requests, _ = run("cold", "--reasoning", "spoon", "--no-thinking", "hello")
            assert requests[0][1]["reasoning_effort"] == "spoon"
            assert requests[0][1]["chat_template_kwargs"]["enable_thinking"] is False
            requests, _ = run("cold", "--reasoning", "high", "hello", success=False)
            assert not requests
            requests, _ = run("disabled", "--reasoning", "spoon", "hello", success=False)
            assert not requests
            requests, result = run("custom", commands="\n".join([
                "/reasoning brief", "first", "/reasoning typo", "second",
                "/reasoning default", "third", "/local cold", "/reasoning spoon", "fourth",
                "/local disabled", "/reasoning default", "fifth", "/quit", "",
            ]))
            assert len(requests) == 5, result.stderr
            bodies = [body for _, body in requests]
            assert [body.get("reasoning_effort") for body in bodies] == ["brief", "brief", None, "spoon", None]
            assert "Available: default, brief, deep" in result.stderr
            assert bodies[3]["model"] == "DavidAU-Qwen-Cold-Fusion-709"
            print("PASS: custom profile levels, CLI override, default reset, typo rejection, Cold-Fusion thinking switch, profile isolation")
            check_picker(root, env)
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
