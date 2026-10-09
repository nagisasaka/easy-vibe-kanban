#!/usr/bin/env python3
"""Smoke-test a disposable LVK server using a real outbound simulator process.

Run: python3 scripts/test-execution-bridge.py http://127.0.0.1:39117
Only accepts an empty shared-resource registry. No Windows/device claims.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('url')
    args = parser.parse_args()
    def api(path, body=None):
        request = urllib.request.Request(args.url.rstrip('/') + '/api' + path,
                                         data=None if body is None else json.dumps(body).encode(),
                                         headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=35) as response:
            value = json.load(response)
        assert value['success'], value
        return value['data']
    def wait(predicate, description, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                return value
            time.sleep(.3)
        raise AssertionError('Timed out: ' + description)
    def uid(): return str(uuid.uuid4())

    coord = '/resource-coordination'
    bridge = '/execution-bridge'
    assert not api(coord + '/snapshot')['resources'], 'Use a fresh disposable database; never a real resource authority'
    root = Path(tempfile.mkdtemp(prefix='lvk-bridge-smoke-'))
    repo_path = root / 'project'
    repo_path.mkdir()
    subprocess.run(['git', 'init', '-b', 'main', str(repo_path)], check=True, capture_output=True)
    (repo_path / 'app.txt').write_text('snapshot-input\n')
    subprocess.run(['git', '-C', str(repo_path), 'add', '.'], check=True)
    subprocess.run(['git', '-C', str(repo_path), '-c', 'user.name=Bridge Test', '-c', 'user.email=bridge@example.invalid', 'commit', '-m', 'Fixture'], check=True, capture_output=True)
    repo = api('/repos', {'path': str(repo_path), 'display_name': 'Bridge smoke fixture'})
    workspace = api('/workspaces', {'name': 'Bridge smoke'})
    api('/workspaces/' + workspace['id'] + '/repos', {'repo_id': repo['id'], 'target_branch': 'main'})
    session = api('/sessions', {'workspace_id': workspace['id'], 'executor': 'CODEX', 'name': 'Bridge smoke'})
    resource = api(coord + '/resources', {'resource_key': 'mock:bridge:execution', 'name': 'Disposable simulator slot', 'description': 'Test-only process group; no hardware or shared external state', 'state': 'idle'})
    runner = api(bridge + '/runners', {'name': 'Linux simulator', 'execution_resource_id': resource['id']})
    environment = dict(os.environ, LVK_RUNNER_TOKEN=runner['token'])
    runner_script = Path(__file__).parent / 'local-runner' / 'runner.py'
    with (root / 'runner.log').open('wb') as log:
        process = subprocess.Popen([sys.executable, str(runner_script), '--simulate', '--url', args.url,
                                    '--runner', runner['id'], '--root', str(root / 'runner')], env=environment, stdout=log, stderr=log)
        operation = None
        try:
            wait(lambda: any(r['id'] == runner['id'] and r['last_seen'] for r in api(bridge + '/runners')), 'runner connects')
            source = api(bridge + '/sources', {'session_id': session['id'], 'working_dir': repo['name']})
            metadata = api(bridge + '/sources/' + source['id'])
            assert metadata['manifest']['digest'] == source['digest']
            assert 'content' not in metadata['manifest']['files'][0]
            request = {'request_id': uid(), 'session_id': session['id'], 'purpose': 'Disposable bridge smoke test',
                       'claims': [{'resource_id': resource['id'], 'expected_revision': resource['revision'], 'resulting_state': None}],
                       'script': 'touch busy; cat app.txt > "$LVK_ARTIFACT_DIR/result.txt"; sleep 120 &',
                       'verification_script': 'test ! -e busy', 'working_dir': '.', 'timeout_seconds': 90,
                       'runner': {'runner_id': runner['id'], 'source_id': source['id'], 'interactive': True,
                                  'desktop': False, 'cleanup_script': 'rm -f busy'}}
            operation = api(coord + '/operations', request)
            assert api(coord + '/operations', request)['id'] == operation['id']
            commands_path = bridge + '/operations/' + operation['id'] + '/commands'
            def done(command):
                return next((c for c in api(commands_path) if c['id'] == command and c['status'] == 'done'), None)
            start = wait(lambda: done(operation['id']), 'start result')
            assert start['result']['success'] and not start['result']['settled'], start
            assert base64.b64decode(start['result']['artifacts'][0]['content']) == b'snapshot-input\n'
            assert api(coord + '/snapshot')['holders'][0]['operation_id'] == operation['id']
            step = {'id': uid(), 'kind': 'step', 'script': 'test -e busy; printf observed >> "$LVK_ARTIFACT_DIR/result.txt"'}
            api(commands_path, step)
            assert api(commands_path, step)['id'] == step['id']
            result = wait(lambda: done(step['id']), 'step result')
            assert result['result']['success'], result
            finish = {'id': uid(), 'kind': 'finish', 'script': ''}
            api(commands_path, finish)
            result = wait(lambda: done(finish['id']), 'finish result')
            assert result['result']['success'] and result['result']['settled'], result
            final = api(coord + '/operations/' + operation['id'] + '?wait_seconds=25')
            assert final['status'] == 'succeeded', final
            assert not api(coord + '/snapshot')['holders']
            events = api(coord + '/events')
            assert any(e['operation_id'] == operation['id'] and e['kind'] == 'succeeded' for e in events)
            print(json.dumps({'result': 'passed', 'operation': operation['id'], 'source_digest': source['digest'], 'fixture': str(root)}))
        finally:
            if operation:
                state = api(coord + '/operations/' + operation['id'])['status']
                if state in ('queued', 'launching', 'running'):
                    api(coord + '/operations/' + operation['id'] + '/cancel', {})
            process.send_signal(signal.SIGINT)
            process.wait(timeout=40)
            # Keep journal, logs, fixtures and server audit records for inspection.


if __name__ == '__main__':
    main()
