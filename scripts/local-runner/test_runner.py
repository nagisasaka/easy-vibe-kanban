import base64
import copy
import json
from pathlib import Path
import tempfile
import time
import unittest
import uuid

from runner import Journal, Operation, artifacts, materialize, safe_path, sha, source_digest, validate_files, Client


def uid():
    return str(uuid.uuid4())


def delivery(interactive=True, script='printf hello'):
    op, runtime, workspace, resource = uid(), uid(), uid(), uid()
    files = [{'path': 'app.txt', 'sha256': sha(b'dirty'), 'content': base64.b64encode(b'dirty').decode()}]
    spec = {'runner': {'runner_id': uid(), 'source_id': uid(), 'interactive': interactive,
                       'desktop': False, 'cleanup_script': 'rm -f busy'},
            'claims': [{'resource_id': resource}], 'working_dir': '.', 'timeout_seconds': 20,
            'script': script, 'verification_script': 'test ! -e busy'}
    return {'operation': {'id': op, 'workspace_id': workspace, 'runtime_id': runtime,
                          'spec': spec, 'status': 'running', 'cancel_requested': False},
            'source': {'head': 'example', 'digest': source_digest(files), 'files': files},
            'fences': [{'resource_id': resource, 'operation_id': op, 'fence': 1}],
            'commands': [{'id': op, 'operation_id': op, 'kind': 'start', 'script': script, 'status': 'sent'}]}


class RunnerTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.journal = Journal(self.root)
        self.operations = []

    def tearDown(self):
        for operation in self.operations:
            operation.job.close()
        self.temp.cleanup()

    def operation(self, data):
        self.assertEqual(self.journal.admit(data), 'new')
        operation = Operation(data, self.root, self.journal, simulate=True)
        self.operations.append(operation)
        return operation

    def test_interactive_observe_followup_finish_and_artifacts(self):
        data = delivery(script='touch busy; printf one > "$LVK_ARTIFACT_DIR/result.txt"; sleep 60 &')
        operation = self.operation(data)
        operation.execute(data['commands'][0])
        first = operation.results[data['operation']['id']]
        self.assertTrue(first['success'])
        self.assertFalse(first['settled'])
        self.assertEqual(base64.b64decode(first['artifacts'][0]['content']), b'one')
        step = dict(id=uid(), operation_id=operation.op['id'], kind='step', script='test -e busy; printf two >> "$LVK_ARTIFACT_DIR/result.txt"')
        operation.execute(step)
        self.assertTrue(operation.results[step['id']]['success'])
        finish = dict(id=uid(), operation_id=operation.op['id'], kind='finish', script='')
        operation.execute(finish)
        self.assertTrue(operation.results[finish['id']]['success'])
        self.assertTrue(operation.results[finish['id']]['settled'])
        self.assertEqual(self.journal.admit(data), 'closed')

    def test_replayed_command_is_not_executed_twice(self):
        data = delivery(script='echo once >> counter')
        operation = self.operation(data)
        operation.execute(data['commands'][0])
        operation.execute(data['commands'][0])
        self.assertEqual((operation.source / 'counter').read_text(), 'once\n')
        self.assertEqual(self.journal.admit(data), 'existing')

    def test_unknown_receipt_never_reexecutes(self):
        data = delivery(script='touch must-not-exist')
        operation = self.operation(data)
        self.journal.begin(data['commands'][0])
        operation.execute(data['commands'][0])
        self.assertFalse(operation.results[operation.op['id']]['success'])
        self.assertFalse((operation.source / 'must-not-exist').exists())

    def test_cancel_running_process_and_cleanup(self):
        data = delivery(script='touch busy; sleep 60')
        operation = self.operation(data)
        operation.dispatch(data['commands'][0])
        deadline = time.monotonic() + 5
        while not (operation.source / 'busy').exists() and time.monotonic() < deadline:
            time.sleep(.01)
        operation.cancel()
        operation.thread.join(timeout=5)
        self.assertFalse(operation.thread.is_alive())
        self.assertFalse(operation.results[operation.op['id']]['success'])
        self.assertTrue(operation.closed)
        self.assertFalse((operation.source / 'busy').exists())

    def test_timeout_and_verification_failure(self):
        data = delivery(interactive=False, script='sleep 60')
        operation = self.operation(data)
        operation.deadline = time.monotonic() + .1
        operation.execute(data['commands'][0])
        self.assertFalse(operation.results[operation.op['id']]['success'])
        data = delivery(interactive=False)
        data['operation']['spec']['verification_script'] = 'exit 9'
        data['operation']['spec']['runner']['cleanup_script'] = 'echo cleanup >> cleanup-count'
        operation = self.operation(data)
        operation.execute(data['commands'][0])
        self.assertFalse(operation.results[operation.op['id']]['success'])
        self.assertTrue(operation.results[operation.op['id']]['settled'])
        self.assertEqual((operation.source / 'cleanup-count').read_text(), 'cleanup\n')

    def test_failed_test_still_returns_diagnostic_artifacts(self):
        data = delivery(interactive=False, script='printf evidence > "$LVK_ARTIFACT_DIR/failure.txt"; exit 7')
        operation = self.operation(data)
        operation.execute(data['commands'][0])
        result = operation.results[operation.op['id']]
        self.assertFalse(result['success'])
        self.assertTrue(result['settled'])
        self.assertEqual(base64.b64decode(result['artifacts'][0]['content']), b'evidence')

    def test_fence_and_content_conflicts(self):
        data = delivery()
        self.journal.admit(data)
        changed = copy.deepcopy(data)
        changed['operation']['spec']['script'] = 'evil'
        with self.assertRaises(ValueError): self.journal.admit(changed)
        other = delivery()
        other['operation']['spec']['claims'] = data['operation']['spec']['claims']
        other['fences'][0]['resource_id'] = data['fences'][0]['resource_id']
        with self.assertRaises(ValueError): self.journal.admit(other)
        self.journal.begin(data['commands'][0])
        changed = dict(data['commands'][0], script='changed')
        with self.assertRaises(ValueError): self.journal.begin(changed)

    def test_snapshot_immutable_and_workspace_separation(self):
        first = self.operation(delivery())
        second = self.operation(delivery())
        self.assertNotEqual(first.source, second.source)
        with self.assertRaises(FileExistsError): materialize(delivery()['source'], first.source)
        self.assertEqual((first.source / 'app.txt').read_text(), 'dirty')

    def test_unsafe_paths_hashes_and_artifact_symlinks(self):
        for path in ('../escape', '/etc/file', 'C:file', 'NUL.txt', 'a\\b', '.git/config', 'a.', 'a:ads'):
            with self.assertRaises(ValueError, msg=path): safe_path(path)
        data = delivery()['source']
        data['files'][0]['content'] = base64.b64encode(b'tampered').decode()
        with self.assertRaises(ValueError): materialize(data, self.root / 'tampered')
        output = self.root / 'artifacts'
        output.mkdir()
        (output / 'link').symlink_to('/etc/passwd')
        with self.assertRaises(ValueError): artifacts(output)
        root_link = self.root / 'root-link'
        root_link.symlink_to(output, target_is_directory=True)
        with self.assertRaises(ValueError): artifacts(root_link)
        files = delivery()['source']['files']
        files.append(dict(files[0], path='APP.TXT'))
        with self.assertRaises(ValueError): validate_files(files)

    def test_credentials_are_not_inherited_and_tls_required(self):
        import os
        from unittest.mock import patch
        with patch.dict(os.environ, LVK_RUNNER_TOKEN='private', LVK_HTTP_AUTH='private'):
            operation = self.operation(delivery())
            self.assertNotIn('LVK_RUNNER_TOKEN', operation.env)
            self.assertNotIn('LVK_HTTP_AUTH', operation.env)
        with self.assertRaises(ValueError): Client('http://example.com', uid(), 'private')


if __name__ == '__main__':
    unittest.main()
