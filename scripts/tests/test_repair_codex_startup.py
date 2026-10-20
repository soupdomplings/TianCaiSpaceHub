"""Synthetic SQLite/protocol checks only. Never launch Hub or installed Codex."""

import argparse
from contextlib import redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import shutil
import sqlite3
import sys
import tempfile
import unittest
from unittest import mock


ENGINE_PATH = Path(__file__).resolve().parents[1] / "repair-codex-startup.py"
SPEC = importlib.util.spec_from_file_location("startup_repair", ENGINE_PATH)
engine = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(engine)

OK = {"ok": True, "kind": "initialized-and-config-read", "sqliteFailure": False, "exitCode": 0}
SQLITE_ERROR = {"ok": False, "kind": "process-exit", "sqliteFailure": True, "exitCode": 1}


def make_state(path, native=23, extra_column=True, table=True):
    with engine.sqlite_open(path) as connection:
        connection.execute("CREATE TABLE _sqlx_migrations (version INTEGER PRIMARY KEY, success INTEGER NOT NULL)")
        connection.executemany("INSERT INTO _sqlx_migrations VALUES (?,1)", [(n,) for n in range(1, native + 1)])
        connection.execute("CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT)")
        connection.execute("INSERT INTO threads VALUES ('existing-chat','synthetic history')")
        if table:
            connection.execute(engine.BASE_DDL)
            if extra_column:
                connection.execute("ALTER TABLE remote_control_enrollments ADD COLUMN remote_control_enabled INTEGER")
            fields = list(engine.BASE_COLUMNS) + ([engine.OPTIONAL_COLUMN] if extra_column else [])
            values = ["ws://127.0.0.1/test", "synthetic-account", "", "server", "env", "Hub", 1]
            if extra_column:
                values.append(1)
            connection.execute("INSERT INTO remote_control_enrollments (" + ",".join(fields) + ") VALUES (" +
                               ",".join("?" for _ in fields) + ")", values)


def migrate_copy(_cli, _home, sqlite_home, _timeout):
    """Simulate native schema ownership. No subprocess or actual CLI is used."""
    path = sqlite_home / engine.DB_NAME
    state = engine.inspect_state(path)
    with engine.sqlite_open(path) as connection:
        if not state["native24"]:
            connection.execute(engine.BASE_DDL)
            connection.execute("INSERT INTO _sqlx_migrations VALUES (24,1)")
        if not state["native37"]:
            connection.execute("ALTER TABLE remote_control_enrollments ADD COLUMN remote_control_enabled INTEGER")
            connection.execute("INSERT INTO _sqlx_migrations VALUES (37,1)")
    return dict(OK)


class RepairTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="hub-repair-synthetic-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.home = self.root / "user-home"
        self.home.mkdir()
        self.source = self.home / engine.DB_NAME
        self.cli = self.root / "never-executed-codex"
        self.stop = mock.patch.object(engine, "require_stopped")
        self.stop.start()
        self.addCleanup(self.stop.stop)
        self.output = redirect_stdout(io.StringIO())
        self.output.__enter__()
        self.addCleanup(self.output.__exit__, None, None, None)

    def repair(self, probes=None):
        before = engine.fingerprint(self.source)
        state = engine.inspect_state(self.source)
        side_effect = probes or [dict(SQLITE_ERROR), migrate_copy, migrate_copy]
        # mock side_effect lists do not invoke functions contained in the list.
        iterator = iter(side_effect)

        def probe(*arguments):
            result = next(iterator)
            return result(*arguments) if callable(result) else dict(result)

        with mock.patch.object(engine, "native_probe", side_effect=probe):
            return engine.repair(self.home, self.source, self.cli, state, 5, before)

    def test_old_native_schema_alone_is_not_a_conflict(self):
        make_state(self.source, table=False)
        self.assertIsNone(engine.inspect_state(self.source)["problem"])

    def test_mature_schema_is_not_a_conflict(self):
        make_state(self.source, native=42)
        self.assertIsNone(engine.inspect_state(self.source)["problem"])

    def test_unfinished_migration_is_rejected(self):
        make_state(self.source)
        with engine.sqlite_open(self.source) as connection:
            connection.execute("UPDATE _sqlx_migrations SET success=0 WHERE version=23")
        with self.assertRaisesRegex(engine.RepairError, "未完成"):
            engine.inspect_state(self.source)

    def test_missing_ledger_is_rejected(self):
        make_state(self.source)
        with engine.sqlite_open(self.source) as connection:
            connection.execute("DROP TABLE _sqlx_migrations")
        with self.assertRaisesRegex(engine.RepairError, "迁移记录"):
            engine.inspect_state(self.source)

    def test_stage_preserves_native_receipts_and_all_remote_values(self):
        for native in (23, 36):
            with self.subTest(native=native):
                candidate = self.root / (str(native) + ".sqlite")
                make_state(candidate, native=native)
                state = engine.inspect_state(candidate)
                names, rows = engine.stage_conflict(candidate, state["problem"])
                after = engine.inspect_state(candidate)
                self.assertEqual(after["migrationCount"], native)
                self.assertEqual(after["maxMigration"], native)
                self.assertEqual(after["remoteTable"], native == 36)
                self.assertEqual(rows[0]["remote_control_enabled"], 1)
                self.assertEqual(names, list(engine.BASE_COLUMNS) + [engine.OPTIONAL_COLUMN])

    def test_unknown_remote_shape_is_rejected_without_promotion(self):
        make_state(self.source)
        with engine.sqlite_open(self.source) as connection:
            connection.execute("ALTER TABLE remote_control_enrollments ADD COLUMN future_field TEXT")
        before = engine.fingerprint(self.source)
        with self.assertRaisesRegex(engine.RepairError, "已确认的范围"):
            self.repair()
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_custom_trigger_is_rejected(self):
        make_state(self.source)
        with engine.sqlite_open(self.source) as connection:
            connection.execute("CREATE TRIGGER custom_enrollment AFTER UPDATE ON remote_control_enrollments BEGIN SELECT 1; END")
        with engine.sqlite_open(self.source) as connection:
            with self.assertRaisesRegex(engine.RepairError, "触发器"):
                engine.check_remote_shape(connection)

    def test_rollback_journal_requires_separate_recovery(self):
        make_state(self.source)
        journal = Path(str(self.source) + "-journal")
        journal.write_bytes(b"synthetic rollback journal")
        with self.assertRaisesRegex(engine.RepairError, "回滚日志"):
            engine.fingerprint(self.source)

    def test_table_and_column_repairs_preserve_history_and_restore(self):
        for native in (23, 36):
            with self.subTest(native=native):
                if self.source.exists():
                    self.source.unlink()
                make_state(self.source, native=native)
                before = engine.fingerprint(self.source)
                backup, changed = self.repair()
                self.assertTrue(changed)
                self.assertEqual(engine.fingerprint(backup / "original" / engine.DB_NAME), before)
                self.assertEqual(engine.thread_ids(self.source), {"existing-chat"})
                state = engine.inspect_state(self.source)
                self.assertTrue(state["native24"])
                self.assertTrue(state["native37"])
                self.assertIsNone(state["problem"])
                with engine.sqlite_open(self.source) as connection:
                    self.assertEqual(connection.execute("SELECT remote_control_enabled FROM remote_control_enrollments").fetchone()[0], 1)
                engine.restore(backup)
                self.assertEqual(engine.fingerprint(self.source), before)

    def test_source_with_committed_wal_is_preserved_and_restored_byte_for_byte(self):
        seed = self.root / "wal-seed.sqlite"
        make_state(seed)
        connection = sqlite3.connect(seed)
        connection.execute("PRAGMA journal_mode=WAL")
        connection.execute("INSERT INTO threads VALUES ('wal-chat','committed WAL history')")
        connection.commit()
        for suffix in engine.SUFFIXES:
            shutil.copy2(Path(str(seed) + suffix), Path(str(self.source) + suffix))
        connection.close()
        before = engine.fingerprint(self.source)
        # Never inspect the original family with SQLite; that can mutate SHM.
        copy = self.root / "inspection"
        engine.copy_family(self.source, copy)
        state = engine.inspect_state(copy / engine.DB_NAME)
        calls = iter([dict(SQLITE_ERROR), migrate_copy, migrate_copy])

        def probe(*arguments):
            item = next(calls)
            return item(*arguments) if callable(item) else item

        with mock.patch.object(engine, "native_probe", side_effect=probe):
            backup, changed = engine.repair(self.home, self.source, self.cli, state, 5, before)
        self.assertTrue(changed)
        self.assertEqual(engine.fingerprint(backup / "original" / engine.DB_NAME), before)
        verified = self.root / "verified-candidate"
        engine.copy_family(self.source, verified)
        self.assertEqual(engine.thread_ids(verified / engine.DB_NAME), {"existing-chat", "wal-chat"})
        engine.restore(backup)
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_successful_native_baseline_never_replaces_original(self):
        make_state(self.source)
        before = engine.fingerprint(self.source)
        _backup, changed = self.repair([OK])
        self.assertFalse(changed)
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_non_sqlite_startup_failure_never_replaces_original(self):
        make_state(self.source)
        before = engine.fingerprint(self.source)
        with self.assertRaisesRegex(engine.RepairError, "未确认"):
            self.repair([{"ok": False, "kind": "rpc-error", "sqliteFailure": False}])
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_candidate_failure_never_replaces_original(self):
        make_state(self.source)
        before = engine.fingerprint(self.source)
        with self.assertRaisesRegex(engine.RepairError, "修复副本"):
            self.repair([SQLITE_ERROR, SQLITE_ERROR])
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_lost_history_never_replaces_original(self):
        make_state(self.source)
        before = engine.fingerprint(self.source)

        def lose_thread(*arguments):
            migrate_copy(*arguments)
            with engine.sqlite_open(arguments[2] / engine.DB_NAME) as connection:
                connection.execute("DELETE FROM threads")
            return dict(OK)

        with self.assertRaisesRegex(engine.RepairError, "会话索引"):
            self.repair([SQLITE_ERROR, lose_thread])
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_lost_remote_record_never_replaces_original(self):
        make_state(self.source)
        before = engine.fingerprint(self.source)

        def lose_remote(*arguments):
            with engine.sqlite_open(arguments[2] / engine.DB_NAME) as connection:
                connection.execute("DELETE FROM remote_control_enrollments")
            return dict(OK)

        with self.assertRaisesRegex(engine.RepairError, "远程控制记录"):
            self.repair([SQLITE_ERROR, migrate_copy, lose_remote])
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_failed_backup_never_calls_native_or_replaces_original(self):
        make_state(self.source)
        before = engine.fingerprint(self.source)
        with mock.patch.object(engine.shutil, "copy2", side_effect=OSError("synthetic copy failure")):
            with self.assertRaises(OSError):
                self.repair()
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_interrupted_file_replacement_rolls_back(self):
        make_state(self.source)
        before = engine.fingerprint(self.source)
        real_replace = engine.os.replace

        def fail_candidate(source, target):
            if Path(source).name.startswith(".hub-state-repair-"):
                raise OSError("synthetic promotion failure")
            return real_replace(source, target)

        with mock.patch.object(engine.os, "replace", side_effect=fail_candidate):
            with self.assertRaises(OSError):
                self.repair()
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_concurrent_source_change_is_rejected(self):
        make_state(self.source)

        def concurrent_change(*arguments):
            with engine.sqlite_open(self.source) as connection:
                connection.execute("INSERT INTO threads VALUES ('new-chat','new work')")
            return dict(OK)

        with self.assertRaisesRegex(engine.RepairError, "原库"):
            self.repair([SQLITE_ERROR, migrate_copy, concurrent_change])
        self.assertEqual(engine.thread_ids(self.source), {"existing-chat", "new-chat"})

    def test_restore_refuses_to_overwrite_new_user_work(self):
        make_state(self.source)
        backup, _changed = self.repair()
        with engine.sqlite_open(self.source) as connection:
            connection.execute("INSERT INTO threads VALUES ('new-chat','new work')")
        before = engine.fingerprint(self.source)
        with self.assertRaisesRegex(engine.RepairError, "不会覆盖"):
            engine.restore(backup)
        self.assertEqual(engine.fingerprint(self.source), before)

    def test_custom_sqlite_location_has_config_precedence(self):
        (self.home / "config.toml").write_text("sqlite_home=" + json.dumps(str(self.root)) + "\n", encoding="utf-8")
        arguments = argparse.Namespace(codex_home=str(self.home), sqlite_home=None)
        with mock.patch.object(engine, "gui_value", return_value=""), \
                mock.patch.object(engine.Path, "home", return_value=self.home), \
                mock.patch.dict(engine.os.environ, {}, clear=True):
            home, sqlite_home = engine.resolve_locations(arguments)
        self.assertEqual(home, self.home.resolve())
        self.assertEqual(sqlite_home, self.root.resolve())


class FakeProcess:
    def __init__(self, stdout, stderr=b""):
        self.stdin = io.BytesIO()
        self.stdout = io.BytesIO(stdout)
        self.stderr = io.BytesIO(stderr)
        self.returncode = None

    def poll(self):
        return self.returncode


class ProtocolTests(unittest.TestCase):
    def probe(self, stdout, stderr=b""):
        fake = FakeProcess(stdout, stderr)
        sent = []

        def stop(process):
            sent.extend(json.loads(line) for line in process.stdin.getvalue().splitlines())
            process.returncode = 0

        with mock.patch.object(engine.subprocess, "Popen", return_value=fake) as popen, \
                mock.patch.object(engine, "stop_child", side_effect=stop):
            with tempfile.TemporaryDirectory(prefix="hub-repair-protocol-test-") as temporary:
                root = Path(temporary)
                result = engine.native_probe(root / "never-run-cli", root, root / "sqlite", 0.03)
                arguments = popen.call_args
        return result, sent, arguments

    def test_success_requires_initialize_and_config_result_with_matching_ids(self):
        output = b'{"id":1,"result":{"userAgent":"synthetic"}}\n{"id":2,"result":{"config":{}}}\n'
        result, sent, call = self.probe(output)
        self.assertTrue(result["ok"])
        self.assertEqual([message["method"] for message in sent], ["initialize", "initialized", "config/read"])
        self.assertIn("CODEX_SQLITE_HOME", call.kwargs["env"])
        self.assertEqual(call.args[0][-1], "app-server")

    def test_rpc_error_is_not_success(self):
        output = b'{"id":1,"result":{"userAgent":"synthetic"}}\n{"id":2,"error":{"code":-1}}\n'
        result, _sent, _call = self.probe(output)
        self.assertFalse(result["ok"])
        self.assertEqual(result["kind"], "rpc-error")

    def test_unmatched_response_cannot_satisfy_probe(self):
        result, _sent, _call = self.probe(b'{"id":99,"result":{"userAgent":"synthetic"}}\n')
        self.assertFalse(result["ok"])
        self.assertEqual(result["kind"], "timeout")

    def test_logged_sqlite_failure_overrides_successful_rpc(self):
        output = b'{"id":1,"result":{"userAgent":"synthetic"}}\n{"id":2,"result":{}}\n'
        result, _sent, _call = self.probe(output, b'failed to initialize sqlite state runtime\n')
        self.assertFalse(result["ok"])
        self.assertTrue(result["sqliteFailure"])


if __name__ == "__main__":
    unittest.main()
