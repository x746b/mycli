#!/usr/bin/env python3
"""Offline automatic/manual compaction and cancellation regression."""
import http.server
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / 'target/debug/mycli'
MODEL = 'Qwen-Fable-Cold-Fusion-test'


class API(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps({'data': [{'id': MODEL, 'max_model_len': 8192, 'context_length': 262144}]}).encode())

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.server.requests.append(body)
        summary = any(m['role'] == 'system' and 'Summarize conversation evidence' in m['content'] for m in body['messages'])
        if summary and self.server.mode == 'silent':
            self.server.started.set()
            self.connection.settimeout(4)
            try:
                if self.connection.recv(1) == b'': self.server.disconnected.set()
            except ConnectionResetError:
                self.server.disconnected.set()
            except socket.timeout:
                pass
            return
        text = 'DECISION-X remains required; continue the outstanding task.' if summary else 'Mock answer.'
        reason = 'stop'
        if summary and self.server.mode == 'empty': text = ''
        if summary and self.server.mode == 'truncated': reason = 'length'
        usage = {'prompt_tokens': 6000 if self.server.mode == 'auto' and not summary else 100, 'completion_tokens': 10}
        payload = 'data: ' + json.dumps({'choices': [{'index': 0, 'delta': {'content': text}, 'finish_reason': reason}], 'usage': usage}) + '\n\ndata: [DONE]\n\n'
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload.encode())))
        self.end_headers()
        self.wfile.write(payload.encode())


def main():
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), API)
    server.requests = []
    server.mode = 'ok'
    server.started = threading.Event()
    server.disconnected = threading.Event()
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='mycli-compact-') as directory:
            root = Path(directory)
            config = root / 'xdg/mycli'
            config.mkdir(parents=True)
            home = root / 'home'
            home.mkdir()
            (config / 'config.toml').write_text(f'''model="{MODEL}"
base_url="http://127.0.0.1:{server.server_port}/v1"
max_tokens=512
tool_tier="simple"
web_search=false
reasoning_effort="spoon"
thinking=false
''')
            env = {k: v for k, v in os.environ.items() if not k.startswith('MYCLI_')}
            env.update(HOME=str(home), XDG_CONFIG_HOME=str(root / 'xdg'), NO_PROXY='127.0.0.1', no_proxy='127.0.0.1')
            first = 'ORIGINAL-REQUIREMENTS DECISION-X ' + 'evidence ' * 150

            def run(commands, mode='ok'):
                server.requests.clear()
                server.mode = mode
                result = subprocess.run([str(BINARY)], cwd=root, env=env,
                                        input='\n'.join(commands + ['/quit', '']), capture_output=True, text=True, timeout=20)
                assert result.returncode == 0, result.stderr
                return list(server.requests), result.stdout + result.stderr

            requests, output = run(['/compact', '/compact status'])
            assert not requests
            assert '8192 window' in output and '6656 usable' in output
            assert 'Nothing compacted' in output
            print('PASS: metadata precedence, input/output reserve, empty-history no-op')

            requests, output = run([first, 'Latest task', '/compact preserve DECISION-X', 'Continue'])
            summary = [r for r in requests if not r.get('tools')]
            assert len(summary) == 1, output
            assert summary[0]['reasoning_effort'] == 'spoon'
            assert summary[0]['chat_template_kwargs']['enable_thinking'] is False
            assert summary[0]['max_tokens'] <= 512
            assert 'preserve DECISION-X' in summary[0]['messages'][-1]['content']
            retained = json.dumps(requests[-1]['messages'])
            assert 'context_summary' in retained and 'Latest task' in retained
            assert 'ORIGINAL-REQUIREMENTS' not in retained
            assert 'Compacted' in output
            print('PASS: manual compaction, focus, preserved tail, selected model options')

            for mode in ['empty', 'truncated']:
                requests, output = run([first, 'Latest task', '/compact', 'Continue'], mode)
                assert 'ORIGINAL-REQUIREMENTS' in json.dumps(requests[-1]['messages'])
                assert 'history unchanged' in output
            print('PASS: empty/truncated summaries preserve history')

            requests, output = run([first, 'Continue'], 'auto')
            assert len(requests) == 3, output
            assert 'context_summary' in json.dumps(requests[-1]['messages'])
            assert 'context budget reached' in output
            print('PASS: automatic compaction across user prompts')

            server.requests.clear()
            server.mode = 'silent'
            proc = subprocess.Popen([str(BINARY)], cwd=root, env=env, stdin=subprocess.PIPE,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                proc.stdin.write(first + '\nLatest task\n/compact\n')
                proc.stdin.flush()
                assert server.started.wait(5), 'Summary request not started'
                proc.send_signal(signal.SIGINT)
                assert server.disconnected.wait(3), 'Cancelled summary connection stayed open'
                proc.stdin.write('Continue after cancellation\n/quit\n')
                proc.stdin.flush()
                stdout, stderr = proc.communicate(timeout=5)
                assert proc.returncode == 0, stdout + stderr
                assert 'ORIGINAL-REQUIREMENTS' in json.dumps(server.requests[-1]['messages'])
                assert 'Cancelled' in stderr or 'Cancelling' in stderr
                print('PASS: Ctrl+C closes summary request and permits a follow-up with original history')
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.communicate()
    finally:
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()
