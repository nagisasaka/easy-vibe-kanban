#!/usr/bin/env python3
"""Operator/AI CLI for the LVK execution bridge (Python standard library only)."""
import argparse
import base64
import json
import os
from pathlib import Path
import urllib.request
import uuid

from runner import safe_path, validate_files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://127.0.0.1:3000')
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('resources')
    sub.add_parser('runners')
    rotate = sub.add_parser('rotate-token')
    rotate.add_argument('runner_id', type=uuid.UUID)
    inspect = sub.add_parser('source')
    inspect.add_argument('source_id', type=uuid.UUID)
    register = sub.add_parser('register-resource')
    register.add_argument('json_file', type=Path)
    enroll = sub.add_parser('enroll')
    enroll.add_argument('name')
    enroll.add_argument('execution_resource_id', type=uuid.UUID)
    capture = sub.add_parser('capture')
    capture.add_argument('session_id', type=uuid.UUID)
    capture.add_argument('working_dir')
    submit = sub.add_parser('submit')
    submit.add_argument('json_file', type=Path)
    for name in ('status', 'cancel', 'commands', 'finish', 'step', 'download'):
        cmd = sub.add_parser(name)
        cmd.add_argument('operation', type=uuid.UUID)
        if name in ('step', 'finish'):
            cmd.add_argument('--request-id', type=uuid.UUID, required=True)
        if name == 'step':
            cmd.add_argument('script_file', type=Path)
        if name == 'download':
            cmd.add_argument('directory', type=Path)
    args = parser.parse_args()
    headers = {'Content-Type': 'application/json'}
    if os.environ.get('LVK_HTTP_AUTH'):
        headers['Authorization'] = 'Basic ' + base64.b64encode(os.environ['LVK_HTTP_AUTH'].encode()).decode()
    from urllib.parse import urlparse
    parsed = urlparse(args.url)
    if parsed.scheme != 'https' and not (parsed.scheme == 'http' and parsed.hostname in ('localhost', '127.0.0.1', '::1')):
        parser.error('Use HTTPS or an SSH tunnel to localhost')
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None
    opener = urllib.request.build_opener(NoRedirect())

    def api(path, body=None):
        request = urllib.request.Request(args.url.rstrip('/') + '/api' + path,
                                         None if body is None else json.dumps(body).encode(), headers)
        with opener.open(request, timeout=40) as response:
            result = json.load(response)
        if not result['success']:
            raise RuntimeError(result.get('message', 'API failed'))
        return result['data']

    command = args.command
    resource = '/resource-coordination'
    bridge = '/execution-bridge'
    if command == 'resources': result = api(resource + '/snapshot')
    elif command == 'source': result = api(bridge + '/sources/' + str(args.source_id))
    elif command == 'rotate-token': result = api(bridge + '/runners/' + str(args.runner_id) + '/rotate-token', {})
    elif command == 'runners': result = api(bridge + '/runners')
    elif command == 'register-resource': result = api(resource + '/resources', json.loads(args.json_file.read_text(encoding='utf-8-sig')))
    elif command == 'enroll': result = api(bridge + '/runners', {'name': args.name, 'execution_resource_id': str(args.execution_resource_id)})
    elif command == 'capture': result = api(bridge + '/sources', {'session_id': str(args.session_id), 'working_dir': args.working_dir})
    elif command == 'submit': result = api(resource + '/operations', json.loads(args.json_file.read_text(encoding='utf-8-sig')))
    elif command == 'status': result = api(resource + '/operations/' + str(args.operation) + '?wait_seconds=25')
    elif command == 'cancel': result = api(resource + '/operations/' + str(args.operation) + '/cancel', {})
    elif command in ('step', 'finish'):
        result = api(bridge + '/operations/' + str(args.operation) + '/commands',
                     {'id': str(args.request_id), 'kind': command,
                      'script': args.script_file.read_text(encoding='utf-8-sig') if command == 'step' else ''})
    else:
        result = api(bridge + '/operations/' + str(args.operation) + '/commands')
        if command == 'download':
            args.directory.mkdir(parents=True, exist_ok=False)
            for entry in result:
                if entry['result']:
                    dest = args.directory / str(uuid.UUID(entry['id']))
                    dest.mkdir()
                    (dest / 'result.json').write_text(json.dumps(entry['result'], ensure_ascii=False, indent=2), encoding='utf-8')
                    artifact_dir = dest / 'artifacts'
                    artifact_dir.mkdir()
                    for name, data in validate_files(entry['result']['artifacts']):
                        file = artifact_dir / safe_path(name)
                        file.parent.mkdir(parents=True, exist_ok=True)
                        if file.parent.resolve() != file.parent.absolute():
                            raise ValueError('Artifact parent resolves through an alias or link')
                        with file.open('xb') as stream:
                            stream.write(data)
            result = {'directory': str(args.directory), 'commands': len(result)}
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
