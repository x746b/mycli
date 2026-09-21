#!/usr/bin/env python3
"""Offline session persistence, resume, memory, naming, exit and interruption tests."""
import http.server
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / 'target/debug/mycli'


class API(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.server.requests.append(body)
        summary = any(m['role'] == 'system' and 'Summarize conversation evidence' in m['content'] for m in body['messages'])
        if self.server.mode == 'silent':
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            self.wfile.write(b'data: {"choices":[{"index":0,"delta":{"content":"Partial draft.\\n"}}]}\n\n')
            self.wfile.flush()
            self.server.started.set()
            self.connection.settimeout(5)
            try:
                if self.connection.recv(1) == b'': self.server.disconnected.set()
            except ConnectionResetError: self.server.disconnected.set()
            except socket.timeout: pass
            return
        last = body['messages'][-1]
        if last.get('role') == 'user' and last.get('content') == 'USETOOL':
            delta = {'tool_calls': [{'index': 0, 'id': 'call_read', 'type': 'function', 'function': {
                'name': 'Read', 'arguments': json.dumps({'file_path': str(self.server.project / 'sample.txt')})}}]}
            stop = 'tool_calls'
        else:
            delta = {'content': 'Summary: preserve requirements and pending tasks.' if summary else 'Mock answer.'}
            stop = 'stop'
        response = {'choices': [{'index': 0, 'delta': delta, 'finish_reason': stop}], 'usage': {'prompt_tokens': 100, 'completion_tokens': 10}}
        payload = ('data: ' + json.dumps(response) + '\n\ndata: [DONE]\n\n').encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def wait_until(predicate, description):
    for _ in range(120):
        result = predicate()
        if result: return result
        time.sleep(.05)
    raise AssertionError('Timed out: ' + description)


def main():
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), API)
    server.requests = []
    server.mode = 'ok'
    server.started = threading.Event()
    server.disconnected = threading.Event()
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='mycli-sessions-') as directory:
            root = Path(directory)
            project = root / 'project'
            project.mkdir()
            server.project = project
            (project / 'sample.txt').write_text('DURABLE TOOL RESULT')
            config = root / 'xdg/mycli'
            config.mkdir(parents=True)
            home = root / 'home'
            home.mkdir()
            (config / 'config.toml').write_text(f'''model="mock-model"
base_url="http://127.0.0.1:{server.server_port}/v1"
api_key="TEST-KEY-NOT-FOR-SESSION-FILES"
context_window=32768
max_tokens=2048
tool_tier="simple"
web_search=false
''')
            env = {k: v for k, v in os.environ.items() if not k.startswith('MYCLI_')}
            env.update(HOME=str(home), XDG_CONFIG_HOME=str(root / 'xdg'), NO_PROXY='127.0.0.1', no_proxy='127.0.0.1')
            sessions = config / 'sessions'

            def snapshots():
                out = {}
                if sessions.exists():
                    for path in sessions.glob('*/context.json'):
                        try: out[path.parent.name] = json.loads(path.read_text())
                        except (OSError, ValueError): pass
                return out

            def run(lines, *args):
                server.requests.clear()
                result = subprocess.run([str(BINARY), *args], cwd=project, env=env,
                                        input='\n'.join(lines) + '\n', capture_output=True, text=True, timeout=20)
                assert result.returncode == 0, result.stdout + result.stderr
                return result.stdout + result.stderr

            first = 'ORIGINAL-ARCHIVE-MARKER ' + 'important history ' * 100
            output = run(['/sessions rename Design notes', '/remember Prefer concise status updates.', first,
                          'Latest task', '/compact', '/quit', 'Y'])
            saved = snapshots()
            assert len(saved) == 1
            ident, snapshot = next(iter(saved.items()))
            folder = sessions / ident
            assert snapshot['metadata']['name'] == 'Design notes'
            assert 'ORIGINAL-ARCHIVE-MARKER' not in json.dumps(snapshot['messages'])
            transcript = (folder / 'transcript.jsonl').read_text()
            assert 'ORIGINAL-ARCHIVE-MARKER' in transcript and 'context_summary' in transcript
            assert 'Keep this session' in output and 'Session kept' in output
            assert 'Prefer concise status updates.' in (config / 'MEMORY.md').read_text()
            assert 'TEST-KEY-NOT-FOR-SESSION-FILES' not in transcript + (folder / 'context.json').read_text()
            print('PASS: naming, global memory, append-only archive across compaction, private metadata')

            old_usage = snapshot['usage']['input_tokens']
            old_archive = (folder / 'transcript.jsonl').read_bytes()
            run(['Continue saved task', '/quit', 'y'], '--resume', ident)
            assert len(snapshots()) == 1
            assert 'context_summary' in json.dumps(server.requests[0]['messages'])
            assert 'Latest task' in json.dumps(server.requests[0]['messages'])
            assert 'ORIGINAL-ARCHIVE-MARKER' not in json.dumps(server.requests[0]['messages'])
            assert any('Prefer concise status updates.' in m.get('content', '') for m in server.requests[0]['messages'] if m['role'] == 'system')
            assert snapshots()[ident]['usage']['input_tokens'] > old_usage
            assert (folder / 'transcript.jsonl').read_bytes().startswith(old_archive)
            print('PASS: resume restores active checkpoint and totals, not the full archive')

            before = set(snapshots())
            output = run(['/sessions rename Trash', 'discard me', '/quit', 'n'])
            assert set(snapshots()) == before
            assert 'Session deleted' in output and (config / 'MEMORY.md').exists()
            run(['EOF preserved'])
            assert len(snapshots()) == 2
            print('PASS: explicit N deletes only the current session; EOF keeps it')

            before = set(snapshots())
            run(['USETOOL', '/quit', 'Y'], '-y')
            tool_id = (set(snapshots()) - before).pop()
            tool_log = (sessions / tool_id / 'transcript.jsonl').read_text()
            assert 'call_read' in tool_log and 'DURABLE TOOL RESULT' in tool_log
            print('PASS: tool calls and results are checkpointed')

            def start(lines):
                proc = subprocess.Popen([str(BINARY)], cwd=project, env=env,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                proc.stdin.write('\n'.join(lines) + '\n'); proc.stdin.flush()
                return proc

            proc = start(['idle saved', '/sessions rename Idle kept'])
            try:
                idle = wait_until(lambda: next((s for s in snapshots().values() if s['metadata']['name'] == 'Idle kept'), None), 'idle checkpoint')
                time.sleep(.1)
                proc.send_signal(signal.SIGINT)
                stdout, stderr = proc.communicate(timeout=5)
                assert proc.returncode == 0, stdout + stderr
                assert idle['metadata']['id'] in snapshots()
                assert 'Keep this session' not in stdout + stderr
            finally:
                if proc.poll() is None: proc.kill(); proc.communicate()
            print('PASS: idle Ctrl+C keeps the session without a keep/delete question')

            before = set(snapshots())
            server.mode = 'silent'
            proc = start(['Generate partial text'])
            try:
                assert server.started.wait(5)
                time.sleep(.2)
                proc.send_signal(signal.SIGINT)
                assert server.disconnected.wait(3)
                def partial():
                    for sid, snap in snapshots().items():
                        if sid not in before and '[Response interrupted]' in json.dumps(snap['messages']): return snap
                interrupted = wait_until(partial, 'interrupted checkpoint')
                assert 'Partial draft' in json.dumps(interrupted['messages'])
                time.sleep(.6)
                proc.send_signal(signal.SIGINT)
                proc.communicate(timeout=5)
                assert interrupted['metadata']['id'] in snapshots()
            finally:
                if proc.poll() is None: proc.kill(); proc.communicate()
            server.mode = 'ok'
            print('PASS: cancelled streamed text is preserved; exiting afterward retains it')

            output = run(['/resume Design notes', '/sessions rename Resumed design', '/quit', 'y'])
            assert snapshots()[ident]['metadata']['name'] == 'Resumed design'
            assert 'Resumed Design notes' in output
            output = subprocess.check_output([str(BINARY), '/sessions list'], cwd=project, env=env, text=True)
            assert 'Resumed design' in output and ident in output
            print('PASS: in-session resume by name, rename, and offline session listing')

            if shutil.which('tmux'):
                sock = str(root / 'test.sock')
                def tm(*args): return subprocess.check_output(['tmux', '-S', sock, *args], cwd=project, env=env, text=True)
                def screen(): return tm('capture-pane', '-pt', 'session')
                def wait(text): wait_until(lambda: text in screen(), text)
                def send(text):
                    tm('send-keys', '-t', 'session', '-l', text)
                    tm('send-keys', '-t', 'session', 'Enter')
                tm('new-session', '-d', '-s', 'session', '-x', '150', '-y', '36', shlex.join([str(BINARY)]))
                try:
                    wait('Session:')
                    send('/sessions')
                    wait('Select session to resume')
                    # Most recently renamed saved session is first; current session is excluded.
                    wait('Resumed design')
                    tm('send-keys', '-t', 'session', 'Enter')
                    wait('Resumed Resumed design')
                    send('/sessions rename')
                    wait('Session name:')
                    send('Picker named')
                    wait('Session named: Picker named')
                    send('/quit')
                    wait('Keep this session')
                    send('N')
                    wait_until(lambda: not folder.exists(), 'interactive No deletion')
                    print('PASS: session picker, interactive rename, and Y/N exit prompt')
                finally:
                    subprocess.run(['tmux', '-S', sock, 'kill-server'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    finally:
        server.shutdown(); server.server_close()


if __name__ == '__main__': main()
