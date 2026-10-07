import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest
import uuid
from unittest.mock import patch

from updater import (Busy, DB_RULES, Updater, assert_database_idle, atomic_json,
                     database_paths,
                     known_service_process, read_request, validate_release,
                     validate_request)


REQUEST = {"image": "ghcr.io/example/lvk@sha256:" + "a" * 64,
           "revision": "b" * 40, "run_id": 123}


class ValidationTests(unittest.TestCase):
    def test_debug_database_is_in_the_checkout_not_the_stable_home(self):
        container = {"service": "development", "info": {
            "Config": {"Env": ["LVK_DEV_REPO=/repos/fork"]},
            "Mounts": [{"Destination": "/repos", "Source": "/volumes/repos"}]}}
        self.assertEqual(database_paths(container), (Path("/volumes/repos/fork/dev_assets/db.v2.sqlite"), Path("/repos/fork/dev_assets/db.v2.sqlite")))
        container["info"]["Config"]["Env"] = ["LVK_DEV_REPO=/repos/../outside"]
        with self.assertRaises(Busy):
            database_paths(container)

    def test_rejects_other_registry_repository_tag_sha_and_commands(self):
        self.assertEqual(validate_request(REQUEST, "example/lvk"), REQUEST)
        for change in ({"image": "ghcr.io/example/lvk:latest"},
                       {"image": REQUEST["image"].replace("example", "attacker")},
                       {"revision": "main"}, {"run_id": True}, {"command": "touch /root/x"}):
            with self.assertRaises(ValueError):
                validate_request(REQUEST | change, "example/lvk")

    def test_release_identity_includes_success_workflow_revision_and_run(self):
        run = {"id": 123, "repository": {"full_name": "example/lvk"},
               "head_repository": {"full_name": "example/lvk"},
               "path": ".github/workflows/publish-server.yml", "event": "push",
               "status": "completed", "conclusion": "success", "head_sha": "b" * 40}
        labels = {"org.opencontainers.image.revision": "b" * 40,
                  "org.opencontainers.image.source": "https://github.com/example/lvk",
                  "io.lvk.actions-run-id": "123", "io.lvk.maintenance-queue-barrier": "1"}
        validate_release(run, labels, REQUEST, "example/lvk")
        for change in ({"conclusion": "failure"}, {"status": "in_progress"},
                       {"path": ".github/workflows/other.yml"}, {"event": "pull_request"},
                       {"head_sha": "c" * 40}, {"id": 124}):
            with self.assertRaises(ValueError):
                validate_release(run | change, labels, REQUEST, "example/lvk")
        with self.assertRaises(ValueError):
            validate_release(run, labels | {"io.lvk.actions-run-id": "124"}, REQUEST, "example/lvk")

    def test_request_symlinks_large_files_and_status_permissions(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "secret").write_text("{}")
            (root / "link").symlink_to(root / "secret")
            with self.assertRaises(OSError):
                read_request(root / "link")
            (root / "large").write_text("x" * 5000)
            with self.assertRaises(ValueError):
                read_request(root / "large")
            old = os.umask(0o077)
            try:
                atomic_json(root / "public", {"status": "waiting"}, 0o644)
            finally:
                os.umask(old)
            self.assertEqual((root / "public").stat().st_mode & 0o777, 0o644)

    def test_process_admission_rejects_untracked_commands(self):
        self.assertTrue(known_service_process("/usr/local/bin/server"))
        self.assertTrue(known_service_process("node /opt/lvk-development/development.mjs", True))
        for command in ("codex", "node test.js", "bash", "rustc --crate-name foo", "node /opt/lvk-server/server.mjs --evil"):
            self.assertFalse(known_service_process(command, True))


class DatabaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name) / "db.sqlite"
        self.db = sqlite3.connect(self.path)
        # Keep WAL open: the host must see committed records not checkpointed yet.
        self.db.execute("pragma journal_mode=wal")
        for table, (column, _) in DB_RULES.items():
            extra = ",id TEXT,session_id TEXT,created_at TEXT" if table == "agent_runs" else ""
            self.db.execute(f"CREATE TABLE {table} ({column} TEXT{extra})")
        self.db.execute("CREATE TABLE agent_run_state (agent_run_id TEXT,state_json TEXT)")
        self.db.execute("CREATE TABLE resource_holders (id TEXT)")
        self.db.execute("CREATE TABLE scheduled_tasks (enabled INTEGER)")
        self.db.commit()

    def tearDown(self):
        self.db.close()
        self.temp.cleanup()

    def test_empty_and_terminal_databases_are_idle(self):
        assert_database_idle(self.path)
        for table, (column, terminal) in DB_RULES.items():
            self.db.execute(f"INSERT INTO {table} ({column}) VALUES (?)", (terminal[0],))
        self.db.commit()
        assert_database_idle(self.path)

    def test_every_busy_owner_and_unknown_status_blocks_including_wal(self):
        for table, (column, _) in DB_RULES.items():
            with self.subTest(table=table):
                self.db.execute(f"INSERT INTO {table} ({column}) VALUES ('future_unknown_status')")
                self.db.commit()
                with self.assertRaises(Busy):
                    assert_database_idle(self.path)
                self.db.execute(f"DELETE FROM {table}")
                self.db.commit()

    def test_schedule_resource_and_latest_goal_block(self):
        for sql, cleanup in (("INSERT INTO resource_holders VALUES ('held')", "DELETE FROM resource_holders"),
                             ("INSERT INTO scheduled_tasks VALUES (1)", "DELETE FROM scheduled_tasks")):
            self.db.execute(sql)
            self.db.commit()
            with self.assertRaises(Busy):
                assert_database_idle(self.path)
            self.db.execute(cleanup)
            self.db.commit()
        self.db.execute("INSERT INTO agent_runs VALUES ('succeeded','a','s','2026-01-01')")
        self.db.execute('INSERT INTO agent_run_state VALUES (\'a\',\'{"goal":{"status":"active"}}\')')
        self.db.commit()
        with self.assertRaises(Busy):
            assert_database_idle(self.path)
        self.db.execute("INSERT INTO agent_runs VALUES ('succeeded','b','s','2026-01-02')")
        self.db.execute('INSERT INTO agent_run_state VALUES (\'b\',\'{"goal":{"status":"complete"}}\')')
        self.db.commit()
        assert_database_idle(self.path)

    def test_missing_database_or_unknown_schema_is_not_idle(self):
        with self.assertRaises(Busy):
            assert_database_idle(self.path.with_name("missing"))
        self.db.execute("DROP TABLE resource_operations")
        self.db.commit()
        with self.assertRaises(Busy):
            assert_database_idle(self.path)


class FakeUpdater(Updater):
    def __init__(self, root):
        super().__init__({"state_directory": str(root), "deployment_directory": str(root),
                          "compose_files": ["compose.yaml"], "repository": "example/lvk"})
        self.events = []
        self.idle_checks = 0
        self.busy_check = 0
        self.fail_verify = False

    def command(self, args, timeout=600):
        self.events.append(" ".join(args))
        return ""

    def verify_release(self, request):
        self.events.append("validated")

    def containers(self):
        return [{"id": "stable", "service": "lvk"}, {"id": "dev", "service": "development"}]

    def assert_idle(self, containers):
        self.idle_checks += 1
        if self.idle_checks == self.busy_check:
            raise Busy("new agent admitted")

    def volumes(self, containers):
        return {"home": "/fake"}

    def check_queues(self, containers):
        self.events.append("queue fence")

    def resume(self, containers):
        self.events.append("resumed")

    def backup(self, job):
        self.events.append("backup")
        job["backup_complete"] = True

    def set_image(self, image):
        self.events.append("set image")

    def start_and_verify(self):
        self.events.append("verify")
        if self.fail_verify:
            raise RuntimeError("bad migration or browser failure")

    def rollback(self, job):
        self.events.append("restore image AND data")
        self.gate.unlink(missing_ok=True)
        self.save(job, "rolled_back")


class LifecycleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        for name in ("jobs", "requests", "status", "maintenance"):
            (self.root / name).mkdir()
        self.updater = FakeUpdater(self.root)
        self.job = {"id": str(uuid.uuid4()), "status": "queued", "request": REQUEST}

    def tearDown(self):
        self.temp.cleanup()

    def test_success_backs_up_before_mutation_and_closes_admission_race(self):
        self.updater.process(self.job)
        events = self.updater.events
        self.assertEqual(self.job["status"], "succeeded")
        self.assertEqual(self.updater.idle_checks, 2)
        self.assertLess(events.index("docker pause dev"), events.index("docker kill stable"))
        self.assertLess(events.index("backup"), events.index("set image"))
        self.assertFalse(self.updater.gate.exists())

    def test_requesting_agent_must_finish_and_racing_admission_is_not_killed(self):
        for check in (1, 2):
            with self.subTest(check=check):
                self.updater = FakeUpdater(self.root)
                self.updater.busy_check = check
                self.updater.process(self.job)
                self.assertEqual(self.job["status"], "waiting")
                self.assertNotIn("docker kill stable", self.updater.events)
                self.assertNotIn("set image", self.updater.events)
                self.assertFalse(self.updater.gate.exists())

    def test_failed_health_restores_both_image_and_data(self):
        self.updater.fail_verify = True
        self.updater.process(self.job)
        self.assertEqual(self.job["status"], "rolled_back")
        self.assertIn("restore image AND data", self.updater.events)

    def test_interrupted_transaction_blocks_subsequent_updates(self):
        self.job["status"] = "applying"
        atomic_json(self.root / "jobs" / f"{self.job['id']}.json", self.job)
        self.updater.tick()
        stored = json.loads((self.root / "jobs" / f"{self.job['id']}.json").read_text())
        self.assertEqual(stored["status"], "recovery_required")
        self.assertEqual(self.updater.events, [])

    def test_completed_job_id_cannot_be_replayed_with_changed_input(self):
        self.job["status"] = "succeeded"
        atomic_json(self.root / "jobs" / f"{self.job['id']}.json", self.job)
        atomic_json(self.root / "requests" / f"{self.job['id']}.json", REQUEST | {"revision": "c" * 40})
        self.updater.tick()
        self.assertEqual(self.updater.events, [])

    def test_real_archives_restore_data_and_env_and_detect_corruption(self):
        volume = self.root / "data"
        volume.mkdir()
        (volume / "file").write_text("old content")
        (self.root / ".env").write_text("LVK_IMAGE=old\n")
        (self.root / "compose.yaml").write_text("services: {}\n")
        (self.root / "secrets").mkdir()
        (self.root / "backups").mkdir()
        job = self.job | {"volumes": {"home": str(volume)}}
        updater = self.updater
        def command(args, timeout=600):
            if args[0] in ("tar", "du"):
                return subprocess.check_output(args, text=True)
            return ""
        with patch.object(updater, "command", side_effect=command):
            Updater.backup(updater, job)
            self.assertTrue(job["backup_complete"])
            (volume / "file").write_text("new migration")
            (volume / "new-file").write_text("new data")
            (self.root / ".env").write_text("LVK_IMAGE=new\n")
            with patch.object(updater, "containers", return_value=[{"info": {"State": {"Health": {"Status": "healthy"}}}}]):
                Updater.rollback(updater, job)
            self.assertEqual((volume / "file").read_text(), "old content")
            self.assertFalse((volume / "new-file").exists())
            self.assertEqual((self.root / ".env").read_text(), "LVK_IMAGE=old\n")
            archive = self.root / "backups" / job["id"] / "home.tar"
            archive.write_bytes(b"corrupt")
            (volume / "file").write_text("must survive failed recovery")
            with self.assertRaisesRegex(RuntimeError, "checksum"):
                Updater.rollback(updater, job)
            self.assertEqual((volume / "file").read_text(), "must survive failed recovery")


if __name__ == "__main__":
    unittest.main()
