"""Exercise the actual Runner process against a disposable pull-protocol peer."""
import copy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.error

from runner import Client
from test_runner import delivery, uid


class TransportTest(unittest.TestCase):
    def test_outbound_auth_lost_receipt_retry_and_interactive_finish(self):
        data = delivery(script='echo once >> executions; printf start > "$LVK_ARTIFACT_DIR/result.txt"')
        operation = data['operation']['id']
        runner = data['operation']['spec']['runner']['runner_id']
        commands = [data['commands'][0],
                    dict(id=uid(), operation_id=operation, kind='step', script='test -f executions; printf step >> "$LVK_ARTIFACT_DIR/result.txt"', status='sent'),
                    dict(id=uid(), operation_id=operation, kind='finish', script='', status='sent')]
        state = {'index': 0, 'dropped': False, 'reports': {}, 'settled': False, 'polls': 0}
        lock = threading.Lock()

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                if self.headers.get('X-LVK-Runner-Token') != 'test-token':
                    self.send_error(403)
                    return
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                with lock:
                    if self.path.endswith('/poll'):
                        state['polls'] += 1
                        packet = copy.deepcopy(data)
                        packet['commands'] = commands[state['index']:state['index']+1]
                        if operation in body.get('known_operations', []):
                            packet['source'].pop('files')
                        result = [] if state['settled'] else [packet]
                    elif self.path.endswith('/report'):
                        key = body['command_id']
                        if key in state['reports']:
                            assert state['reports'][key] == body, 'Retry mutated receipt'
                        state['reports'][key] = body
                        if not state['dropped']:
                            state['dropped'] = True
                            self.close_connection = True  # Accepted receipt, lost reply.
                            return
                        state['index'] += 1
                        result = None
                    elif self.path.endswith('/settled'):
                        assert state['index'] == 3, 'Settlement hid an unacknowledged command'
                        state['settled'] = True
                        result = None
                    else:
                        self.send_error(404)
                        return
                encoded = json.dumps({'success': True, 'data': result, 'message': None}).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)

        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        try:
            url = f'http://127.0.0.1:{server.server_port}'
            with self.assertRaises(urllib.error.HTTPError):
                Client(url, runner, 'wrong-token').post('/poll', {'capabilities': {}})
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                env = dict(os.environ, LVK_RUNNER_TOKEN='test-token')
                with (root / 'runner.log').open('wb') as log:
                    process = subprocess.Popen([sys.executable, str(Path(__file__).with_name('runner.py')),
                                                '--url', url, '--runner', runner, '--root', str(root), '--simulate'],
                                               env=env, stdout=log, stderr=log)
                    try:
                        deadline = time.monotonic() + 25
                        while time.monotonic() < deadline and not state['settled'] and process.poll() is None:
                            time.sleep(.05)
                        self.assertTrue(state['settled'], (root / 'runner.log').read_text())
                        self.assertEqual(len(state['reports']), 3)
                        self.assertTrue(all(r['success'] for r in state['reports'].values()))
                        counter = root / data['operation']['workspace_id'] / operation / 'source' / 'executions'
                        self.assertEqual(counter.read_text(), 'once\n')
                    finally:
                        process.send_signal(signal.SIGINT)
                        process.wait(timeout=10)
                        self.assertEqual(process.returncode, 0)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == '__main__':
    unittest.main()
