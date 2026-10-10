#!/usr/bin/env python3
"""LVK pull runner: Python 3.11+, Windows 10+; Linux is a test simulator only."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import sqlite3
import subprocess
import threading
import time
import unicodedata
import urllib.request
import urllib.parse
import uuid

LIMIT = 16 * 1024 * 1024


def sha(data):
    return hashlib.sha256(data).hexdigest()


def safe_path(name):
    if not name or len(name.encode('utf-8')) > 240 or any(c in name for c in '\\:\0'):
        raise ValueError('Unsafe path')
    reserved = {'CON', 'PRN', 'AUX', 'NUL', 'CONIN$', 'CONOUT$', 'COM¹', 'COM²', 'COM³', 'LPT¹', 'LPT²', 'LPT³'} | {f'{p}{i}' for p in ('COM', 'LPT') for i in range(1, 10)}
    for part in name.split('/'):
        if (not part or part in ('.', '..') or part.endswith((' ', '.'))
                or part.split('.')[0].upper() in reserved
                or part.lower() in ('.git', '.lvk-shared')
                or any(unicodedata.category(c) == 'Cc' or c in '<>"|?*' for c in part)):
            raise ValueError('Unsafe path')
    return name


def validate_files(files):
    if len(files) > 10000:
        raise ValueError('Too many files')
    seen, total, decoded = set(), 0, []
    for f in files:
        path = safe_path(f['path'])
        if path.upper() in seen:
            raise ValueError('Case-colliding file')
        seen.add(path.upper())
        data = base64.b64decode(f['content'], validate=True)
        total += len(data)
        if total > LIMIT or sha(data) != f['sha256']:
            raise ValueError('Invalid content digest or size')
        decoded.append((path, data))
    for path in seen:
        parts = path.split('/')
        if any('/'.join(parts[:i]) in seen for i in range(1, len(parts))):
            raise ValueError('File/directory collision on Windows')
    return decoded


def source_digest(files):
    return sha(b''.join(f['path'].encode() + b'\0' + f['sha256'].encode() + b'\n' for f in files))


def materialize(source, root):
    decoded = validate_files(source['files'])
    if source_digest(source['files']) != source['digest']:
        raise ValueError('Source manifest digest mismatch')
    # An operation gets a new directory exactly once. Never overwrite a live tree.
    root.mkdir(parents=True, exist_ok=False)
    for path, data in decoded:
        dest = root / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        if dest.parent.resolve() != dest.parent.absolute():
            raise ValueError('Source parent resolves through an alias or link')
        # Exclusive creation also rejects Windows short-name aliases that map
        # distinct manifest entries to an already materialised file.
        with dest.open('xb') as stream:
            stream.write(data)


def artifacts(root):
    if root.resolve() != root.absolute():
        raise ValueError('Artifact root contains a symlink or junction')
    files, total = [], 0
    # Do not traverse links/junctions placed by a project command.
    for directory, dirs, names in os.walk(root, followlinks=False):
        dirs[:] = [d for d in dirs if not (Path(directory) / d).is_symlink()
                   and not getattr(Path(directory) / d, 'is_junction', lambda: False)()]
        for name in sorted(names):
            file = Path(directory) / name
            if file.is_symlink() or not file.resolve().is_relative_to(root.resolve()):
                raise ValueError('Artifact escapes operation')
            path = safe_path(file.relative_to(root).as_posix())
            if file.stat().st_size + total > LIMIT:
                raise ValueError('Artifacts exceed 16 MiB')
            with file.open('rb') as stream:
                data = stream.read(LIMIT - total + 1)
            total += len(data)
            if total > LIMIT:
                raise ValueError('Artifact grew beyond 16 MiB during collection')
            files.append({'path': path, 'sha256': sha(data), 'content': base64.b64encode(data).decode()})
    validate_files(files)
    return files


class Journal:
    def __init__(self, root):
        self.path = root / 'journal.sqlite3'
        with self.connect() as db:
            db.executescript('''
                CREATE TABLE IF NOT EXISTS commands(id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result TEXT);
                CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, closed INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE IF NOT EXISTS fences(resource TEXT PRIMARY KEY, fence INTEGER NOT NULL, operation TEXT NOT NULL);
            ''')

    def connect(self):
        db = sqlite3.connect(self.path, timeout=30)
        db.execute('PRAGMA synchronous=FULL')
        return db

    def admit(self, delivery):
        op = delivery['operation']
        identity = sha(json.dumps([op['spec'], op['runtime_id'], delivery['source']['digest'], delivery['fences']], sort_keys=True).encode())
        with self.connect() as db:
            db.execute('BEGIN IMMEDIATE')
            row = db.execute('SELECT fingerprint,closed FROM operations WHERE id=?', (op['id'],)).fetchone()
            if row:
                if row[0] != identity:
                    raise ValueError('Dispatch identity changed')
                return 'closed' if row[1] else 'existing'
            claims = {c['resource_id'] for c in op['spec']['claims']}
            if claims != {f['resource_id'] for f in delivery['fences']}:
                raise ValueError('Incomplete fences')
            for fence in delivery['fences']:
                if fence['operation_id'] != op['id']:
                    raise ValueError('Wrong fence owner')
                old = db.execute('SELECT fence FROM fences WHERE resource=?', (fence['resource_id'],)).fetchone()
                if old and old[0] >= fence['fence']:
                    raise ValueError('Stale resource fence')
                db.execute('INSERT OR REPLACE INTO fences VALUES(?,?,?)', (fence['resource_id'], fence['fence'], op['id']))
            db.execute('INSERT INTO operations(id,fingerprint) VALUES(?,?)', (op['id'], identity))
            return 'new'

    def close(self, operation):
        with self.connect() as db:
            db.execute('UPDATE operations SET closed=1 WHERE id=?', (operation,))

    def begin(self, command):
        # Status changes from pending to sent are transport, not command identity.
        fingerprint = sha(json.dumps([command[k] for k in ('id', 'operation_id', 'kind', 'script')]).encode())
        with self.connect() as db:
            db.execute('BEGIN IMMEDIATE')
            old = db.execute('SELECT fingerprint,result FROM commands WHERE id=?', (command['id'],)).fetchone()
            if old:
                if old[0] != fingerprint:
                    raise ValueError('Command UUID content conflict')
                return False, json.loads(old[1]) if old[1] else None
            db.execute('INSERT INTO commands VALUES(?,?,NULL)', (command['id'], fingerprint))
            return True, None

    def complete(self, command, result):
        with self.connect() as db:
            db.execute('UPDATE commands SET result=? WHERE id=?', (json.dumps(result), command))


class SimulatedJob:
    """Linux test helper, not production containment against setsid/breakaway."""
    def __init__(self, operation):
        self.processes = []

    def spawn(self, argv, cwd, env, log):
        process = subprocess.Popen(argv, cwd=cwd, env=env, stdout=log, stderr=log, stdin=subprocess.DEVNULL, start_new_session=True)
        self.processes.append(process)
        return process

    def stop(self):
        for process in self.processes:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait(timeout=10)
        self.processes.clear()

    def close(self):
        self.stop()


class Operation:
    def __init__(self, delivery, root, journal, simulate=False, resumed=False):
        self.op = delivery['operation']
        self.target = self.op['spec']['runner']
        self.journal = journal
        self.simulate = simulate
        self.root = root / str(uuid.UUID(self.op['workspace_id'])) / str(uuid.UUID(self.op['id']))
        self.source = self.root / 'source'
        self.output = self.root / 'artifacts'
        self.stop_requested = threading.Event()
        self.deadline = time.monotonic() + self.op['spec']['timeout_seconds']
        self.thread = None
        self.closed = False
        self.cleanup_attempted = False
        self.settlement = None
        self.results = {}
        self.job = SimulatedJob(self.op['id']) if simulate else __import__('windows_job').Job(self.op['id'])
        self.output.mkdir(parents=True, exist_ok=True)
        if resumed:
            # No unknown command is rerun. Windows reconnect terminates the named
            # job; Linux simulator cannot prove old detached process termination.
            if simulate:
                raise RuntimeError('Simulator restart needs manual inspection; no settlement claimed')
            self.job.close()
            self.closed = True
            self.journal.close(self.op['id'])
            self.settlement = 'Runner restarted; named Windows Job terminated and active process count confirmed zero'
        else:
            materialize(delivery['source'], self.source)
        self.cwd = (self.source / self.op['spec']['working_dir']).resolve()
        if not resumed and (not self.cwd.is_relative_to(self.source.resolve()) or not self.cwd.is_dir()):
            raise ValueError('Working directory escapes or is missing in snapshot')
        self.env = os.environ.copy()
        # Never inherit runner or reverse-proxy credentials into project commands.
        self.env.pop('LVK_RUNNER_TOKEN', None)
        self.env.pop('LVK_HTTP_AUTH', None)
        self.env.update(LVK_ARTIFACT_DIR=str(self.output), LVK_SOURCE_DIGEST=delivery['source']['digest'],
                        LVK_SOURCE_ID=self.target['source_id'], LVK_SOURCE_HEAD=delivery['source'].get('head', ''),
                        LVK_RESOURCE_OPERATION_ID=self.op['id'], LVK_RESOURCE_FENCES=json.dumps(delivery['fences']))

    def run_script(self, script, label, allow_cancel=True):
        suffix = '.sh' if self.simulate else '.ps1'
        path = self.root / (label + suffix)
        if self.simulate:
            text = 'set -eu\n' + script
            argv = ['bash', str(path)]
        else:
            text = "$ErrorActionPreference = 'Stop'\n[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)\n$OutputEncoding = [Console]::OutputEncoding\n" + script + '\nif ($LASTEXITCODE) { exit $LASTEXITCODE }\n'
            argv = [shutil.which('pwsh') or shutil.which('powershell') or 'powershell.exe', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', str(path)]
        path.write_text(text, encoding='utf-8-sig' if not self.simulate else 'utf-8')
        log_path = self.root / (label + '.log')
        with log_path.open('wb') as log:
            process = self.job.spawn(argv, self.cwd, self.env, log)
            # Cleanup has a separate bounded deadline, still inside ownership.
            deadline = self.deadline if allow_cancel else time.monotonic() + 15
            while process.poll() is None:
                if time.monotonic() > deadline or (allow_cancel and self.stop_requested.is_set()):
                    self.job.stop()
                    process.poll()
                    raise RuntimeError('Command interrupted or timed out')
                if log_path.stat().st_size > 1_048_576:
                    self.job.stop()
                    process.poll()
                    raise RuntimeError('Log limit exceeded')
                time.sleep(.05)
            code = process.poll()
        text = log_path.read_bytes()[:1_048_576].decode('utf-8', errors='replace')
        if code != 0:
            raise RuntimeError(f'{label} exited {code}\n{text}')
        return text

    def finish(self):
        if self.cleanup_attempted:
            raise RuntimeError('Cleanup was already attempted; no automatic replay')
        self.cleanup_attempted = True
        output = ''
        try:
            output = self.run_script(self.target['cleanup_script'], 'cleanup', allow_cancel=False)
        finally:
            self.job.stop()
        # Verification runs after all previous job members have terminated.
        output += self.run_script(self.op['spec']['verification_script'], 'verify', allow_cancel=False)
        self.job.close()  # Verification children must also be settled.
        self.closed = True
        self.journal.close(self.op['id'])
        self.settlement = 'Cleanup and verification completed; process containment has zero active members'
        return output

    def execute(self, command):
        fresh, saved = self.journal.begin(command)
        if saved is not None:
            self.results[command['id']] = saved
            return
        result = dict(runtime_id=self.op['runtime_id'], command_id=command['id'], success=False,
                      settled=False, output='', artifacts=[])
        try:
            if not fresh or self.closed or self.stop_requested.is_set():
                raise RuntimeError('Previously admitted command has unknown completion; never re-executed')
            if self.target['desktop'] and (self.simulate or not __import__('windows_job').desktop_available()):
                raise RuntimeError('Interactive user desktop is unavailable or locked')
            if command['kind'] != 'finish':
                result['output'] = self.run_script(command['script'], command['id'])
            if command['kind'] == 'finish' or not self.target['interactive']:
                result['output'] += self.finish()
            result['artifacts'] = artifacts(self.output)
            result['success'] = True
        except Exception as exc:
            result['output'] += '\n' + str(exc)
            try:
                if not self.closed:
                    self.finish()
            except Exception as cleanup_error:
                result['output'] += '\nCleanup/verification: ' + str(cleanup_error)
                # Quiescence and resource state are different evidence. Even a
                # failed verifier may permit operator recovery after job closure.
                try:
                    self.job.close()
                    self.closed = True
                    self.journal.close(self.op['id'])
                    self.settlement = 'Process containment closed; resource state requires operator inspection'
                except Exception as stop_error:
                    result['output'] += '\nContainment: ' + str(stop_error)
            try:
                result['artifacts'] = artifacts(self.output)
            except Exception as artifact_error:
                result['output'] += '\nArtifacts: ' + str(artifact_error)
        result['settled'] = self.closed
        result['output'] = result['output'].encode('utf-8')[:1_000_000].decode('utf-8', errors='ignore')
        self.journal.complete(command['id'], result)
        self.results[command['id']] = result

    def dispatch(self, command):
        if self.thread and self.thread.is_alive():
            return
        self.thread = threading.Thread(target=self.execute, args=(command,), daemon=False)
        self.thread.start()

    def cancel(self):
        self.stop_requested.set()
        if not self.closed and not (self.thread and self.thread.is_alive()):
            def stop():
                try:
                    self.finish()
                except Exception:
                    self.job.close()
                    self.closed = True
                    self.journal.close(self.op['id'])
                    self.settlement = 'Containment closed after cancellation; operator must verify external state'
            self.thread = threading.Thread(target=stop, daemon=False)
            self.thread.start()


class Client:
    def __init__(self, url, runner, token, http_auth=None, installation=None):
        self.url = url.rstrip('/') + '/api/execution-bridge/runners/' + str(uuid.UUID(runner))
        self.headers = {'Content-Type': 'application/json', 'X-LVK-Runner-Token': token,
                        'X-LVK-Runner-Installation': str(uuid.UUID(installation or runner))}
        if http_auth:
            self.headers['Authorization'] = 'Basic ' + base64.b64encode(http_auth.encode()).decode()
        parsed = urllib.parse.urlparse(url)
        if parsed.scheme != 'https' and not (parsed.scheme == 'http' and parsed.hostname in ('localhost', '127.0.0.1', '::1')):
            raise ValueError('Use HTTPS, or an SSH tunnel to localhost')
        # Credentials must never follow a redirect to another authority.
        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, *args, **kwargs):
                return None
        self.opener = urllib.request.build_opener(NoRedirect())

    def post(self, path, value):
        request = urllib.request.Request(self.url + path, json.dumps(value).encode(), self.headers, method='POST')
        with self.opener.open(request, timeout=15) as response:
            result = json.load(response)
        if not result['success']:
            raise RuntimeError(result.get('message', 'Bridge API failed'))
        return result['data']


def root_lock(root):
    handle = (root / 'runner.lock').open('a+b')
    handle.seek(0)
    handle.write(b'0')
    handle.flush()
    handle.seek(0)
    if os.name == 'nt':
        import msvcrt
        msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
    else:
        import fcntl
        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
    return handle


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', required=True)
    parser.add_argument('--runner', required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--simulate', action='store_true')
    args = parser.parse_args()
    if os.name != 'nt' and not args.simulate:
        parser.error('Production runner requires Windows; --simulate is test-only')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    lock = root_lock(root)
    identity = root / 'identity.json'
    current = {'url': args.url.rstrip('/'), 'runner': str(uuid.UUID(args.runner))}
    saved = json.loads(identity.read_text()) if identity.exists() else {}
    if saved and any(saved.get(key) != value for key, value in current.items()):
        raise RuntimeError('This journal belongs to another authority/runner; preserve its recovery state')
    current['installation'] = saved.get('installation') or str(uuid.uuid4())
    identity.write_text(json.dumps(current))
    journal = Journal(root)
    client = Client(args.url, args.runner, os.environ['LVK_RUNNER_TOKEN'], os.environ.get('LVK_HTTP_AUTH'), current['installation'])
    active = {}
    print('LVK runner starting outbound polling. Ctrl+C requests local cleanup.', flush=True)
    try:
        while True:
            for operation in active.values():
                if time.monotonic() > operation.deadline and not operation.closed:
                    operation.cancel()
            try:
                capabilities = {'protocol': 1, 'os': platform.system(),
                                'desktop': False if args.simulate else __import__('windows_job').desktop_available(),
                                'tools': {name: shutil.which(name) for name in ('python', 'dotnet', 'pwsh', 'powershell', 'adb')},
                                'simulator': args.simulate}
                deliveries = client.post('/poll', {'capabilities': capabilities, 'known_operations': list(active)[-1000:]})
                for delivery in deliveries:
                    op = delivery['operation']
                    state = journal.admit(delivery)
                    operation = active.get(op['id'])
                    if operation is None:
                        operation = Operation(delivery, root, journal, args.simulate, resumed=state != 'new')
                        active[op['id']] = operation
                    path = '/operations/' + op['id']
                    cancelled = op['cancel_requested'] or op['status'] != 'running'
                    if cancelled:
                        operation.cancel()
                    acknowledged = set()
                    for command in delivery['commands']:
                        if command['id'] in operation.results:
                            client.post(path + '/report', operation.results[command['id']])
                            acknowledged.add(command['id'])
                        else:
                            operation.dispatch(command)
                    if operation.settlement and all(c['id'] in acknowledged for c in delivery['commands']):
                        client.post(path + '/settled', {'runtime_id': op['runtime_id'], 'evidence': operation.settlement})
                        active.pop(op['id'], None)
                delivered = {d['operation']['id'] for d in deliveries}
                for operation_id, operation in list(active.items()):
                    if operation.closed and operation_id not in delivered:
                        active.pop(operation_id)
                # Keep durable journal/artifacts, not completed objects in RAM.
            except Exception as exc:
                # Transport failure is not permission to execute again or unlock.
                print(type(exc).__name__ + ': ' + str(exc), flush=True)
            time.sleep(1)
    except KeyboardInterrupt:
        for operation in active.values():
            operation.cancel()
        for operation in active.values():
            if operation.thread:
                operation.thread.join()
        print('Local cleanup requested. Reconnect with the same root to deliver retained evidence.', flush=True)
    finally:
        lock.close()


if __name__ == '__main__':
    main()
