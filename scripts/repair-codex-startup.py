#!/usr/bin/env python3
"""Windows/macOS Codex startup diagnostics and staged state-schema repair.

No product is started in diagnose mode. Repair invokes only the installed
bundled app-server, against private copies, without creating a chat/turn.
The native CLI owns migration receipts; this helper never fabricates them.
"""

import argparse
import base64
import csv
from contextlib import contextmanager
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import queue
import re
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import uuid


VERSION = "2026-10-20.1"
TABLE = "remote_control_enrollments"
BASE_COLUMNS = (
    "websocket_url", "account_id", "app_server_client_name", "server_id",
    "environment_id", "server_name", "updated_at",
)
OPTIONAL_COLUMN = "remote_control_enabled"
DB_NAME = "state_5.sqlite"
SUFFIXES = ("", "-wal", "-shm")
BACKUP_DIR = "hub-startup-repair-backups"
PROCESS_NAMES = {
    "chatgpt", "codex", "codex-code-mode-host", "tiancaispacehub", "codexhub",
}
BASE_DDL = """CREATE TABLE remote_control_enrollments (
websocket_url TEXT NOT NULL, account_id TEXT NOT NULL,
app_server_client_name TEXT NOT NULL, server_id TEXT NOT NULL,
environment_id TEXT NOT NULL, server_name TEXT NOT NULL,
updated_at INTEGER NOT NULL,
PRIMARY KEY (websocket_url, account_id, app_server_client_name))"""


class RepairError(Exception):
    pass


def run_capture(arguments, timeout=12):
    return subprocess.run(
        arguments, capture_output=True, timeout=timeout,
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
    )


def powershell_json(script):
    source = "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); " + script
    encoded = base64.b64encode(source.encode("utf-16le")).decode("ascii")
    result = run_capture(["powershell.exe", "-NoProfile", "-NonInteractive",
                          "-EncodedCommand", encoded])
    if result.returncode:
        raise RepairError("无法读取当前用户的商店安装信息，请用 --codex-bin 指定内置程序。")
    raw = result.stdout.decode("utf-8-sig").strip()
    return json.loads(raw) if raw else []


def launchctl_value(name):
    result = run_capture(["/bin/launchctl", "getenv", name], timeout=5)
    return result.stdout.decode("utf-8", errors="replace").strip() if result.returncode == 0 else ""


def gui_value(name):
    if os.name == "nt":
        import winreg
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
                return str(winreg.QueryValueEx(key, name)[0]).strip()
        except FileNotFoundError:
            return ""
    return launchctl_value(name)


def running_products():
    found = []
    if os.name == "nt":
        result = run_capture(["tasklist.exe", "/FO", "CSV", "/NH"])
        if result.returncode:
            raise RepairError("无法核对正在运行的程序，已停止检查。")
        for row in csv.reader(io.StringIO(result.stdout.decode("mbcs", errors="replace"))):
            if len(row) < 2:
                continue
            name = row[0].lower().removesuffix(".exe")
            if name in PROCESS_NAMES:
                found.append({"name": row[0], "pid": row[1]})
    else:
        result = run_capture(["/bin/ps", "-axo", "pid=,comm="])
        if result.returncode:
            raise RepairError("无法核对正在运行的程序，已停止检查。")
        for line in result.stdout.decode("utf-8", errors="replace").splitlines():
            fields = line.strip().split(None, 1)
            if len(fields) == 2 and Path(fields[1]).name.lower() in PROCESS_NAMES:
                found.append({"name": Path(fields[1]).name, "pid": fields[0]})
    return found


def require_stopped():
    found = running_products()
    if found:
        names = ", ".join("{} (PID {})".format(item["name"], item["pid"]) for item in found)
        raise RepairError("请先正常退出 ChatGPT/Codex、Hub 及其后台进程，再执行。仍在运行：" + names)


def locate_cli(explicit):
    if explicit:
        path = Path(explicit).expanduser().resolve()
        candidates = [path]
    else:
        roots = []
        if os.name == "nt":
            packages = powershell_json(
                "@(Get-AppxPackage | Where-Object { $_.Name -in @('OpenAI.Codex','OpenAI.ChatGPT') } | "
                "Select-Object -ExpandProperty InstallLocation) | ConvertTo-Json -Compress"
            ) or []
            if isinstance(packages, str):
                packages = [packages]
            roots = [Path(value) for value in packages]
            relative = ("app/resources/codex.exe", "resources/codex.exe",
                        "app/resources/codex-cli/codex.exe")
        else:
            roots = [directory / name for directory in (Path("/Applications"), Path.home() / "Applications")
                     for name in ("ChatGPT.app", "Codex.app")]
            relative = ("Contents/Resources/codex",
                        "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex")
        candidates = sorted({(root / rel).resolve() for root in roots for rel in relative
                             if (root / rel).is_file()}, key=str)
    candidates = [path for path in candidates if path.is_file()
                  and (os.name == "nt" or os.access(path, os.X_OK))]
    if not candidates:
        raise RepairError("未找到当前应用的内置 codex 程序。请用 --codex-bin 指定实际路径；无需重装或删配置。")
    if len(candidates) != 1:
        raise RepairError("检测到多份内置程序，请用 --codex-bin 选择本次应用使用的路径：\n" +
                          "\n".join(str(path) for path in candidates))
    return candidates[0]


def resolve_locations(arguments):
    desktop_home = Path.home() / ".codex"
    shell_home = os.environ.get("CODEX_HOME", "").strip()
    desktop_override = gui_value("CODEX_HOME")
    if arguments.codex_home:
        home = Path(arguments.codex_home).expanduser().resolve()
    else:
        if any(not Path(value).expanduser().is_absolute() for value in (shell_home, desktop_override) if value):
            raise RepairError("CODEX_HOME 是相对路径，请用 --codex-home 指定桌面端的绝对路径。")
        possible = {Path(value).expanduser().resolve() for value in (shell_home, desktop_override) if value}
        if len(possible) > 1:
            raise RepairError("终端和桌面端的 CODEX_HOME 不一致，请用 --codex-home 指定本次应用目录。")
        home = next(iter(possible), desktop_home.resolve())
        if home != desktop_home.resolve():
            print("检测到自定义 CODEX_HOME：" + str(home))
    if not home.is_dir():
        raise RepairError("找不到 Codex 用户目录：" + str(home))
    config_sqlite = None
    config = home / "config.toml"
    if config.is_file():
        raw = config.read_text(encoding="utf-8-sig")
        if sys.version_info >= (3, 11):
            import tomllib
            try:
                config_sqlite = tomllib.loads(raw).get("sqlite_home")
            except tomllib.TOMLDecodeError:
                raise RepairError("config.toml 无法解析；本脚本不会覆盖配置，请先处理配置格式错误。")
        elif not arguments.sqlite_home:
            raise RepairError("Python 3.9/3.10 无法完整解析配置中的状态库路径。请使用 Python 3.11+，"
                              "或用 --sqlite-home 明确指定实际状态库目录（默认通常与用户目录相同）。")
    if config_sqlite is not None and not isinstance(config_sqlite, str):
        raise RepairError("sqlite_home 配置不是字符串，已停止。")
    shell_sqlite = os.environ.get("CODEX_SQLITE_HOME", "").strip()
    desktop_sqlite = gui_value("CODEX_SQLITE_HOME")
    if not arguments.sqlite_home and not config_sqlite and shell_sqlite and desktop_sqlite and shell_sqlite != desktop_sqlite:
        raise RepairError("终端和桌面端的 CODEX_SQLITE_HOME 不一致，请用 --sqlite-home 指定。")
    value = arguments.sqlite_home or config_sqlite or desktop_sqlite or shell_sqlite
    sqlite_home = Path(value).expanduser() if value else home
    if not sqlite_home.is_absolute():
        raise RepairError("状态库路径是相对路径，无法确定桌面启动目录。请用 --sqlite-home 指定绝对路径。")
    return home, sqlite_home.resolve()


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def family(path):
    return [Path(str(path) + suffix) for suffix in SUFFIXES]


def fingerprint(path):
    if Path(str(path) + "-journal").exists():
        raise RepairError("检测到 SQLite 回滚日志，可能涉及崩溃恢复；已停止自动修复，请保留原库进一步诊断。")
    result = {}
    for item in family(path):
        if item.is_symlink():
            raise RepairError("状态库或边车文件是符号链接，已停止自动操作。")
        if item.exists():
            if not item.is_file():
                raise RepairError("状态库路径包含非普通文件，已停止。")
            result[item.name] = digest(item)
    return result


@contextmanager
def sqlite_open(path, readonly=False):
    if readonly:
        connection = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=2)
        connection.execute("PRAGMA query_only=ON")
    else:
        connection = sqlite3.connect(str(path), timeout=2)
    connection.row_factory = sqlite3.Row
    try:
        yield connection
        if not readonly:
            connection.commit()
    except BaseException:
        if not readonly:
            connection.rollback()
        raise
    finally:
        connection.close()


def sqlite_snapshot(source, destination):
    with sqlite_open(source, readonly=True) as src, sqlite_open(destination) as dst:
        src.backup(dst)


def inspect_state(path):
    with sqlite_open(path, readonly=True) as connection:
        if connection.execute("PRAGMA quick_check").fetchone()[0] != "ok":
            raise RepairError("状态库完整性检查未通过。不会自动删库，请保留原库进一步诊断。")
        tables = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        if "_sqlx_migrations" not in tables:
            raise RepairError("缺少原生迁移记录，不能确认数据库来源；停止自动修复。")
        receipts = list(connection.execute("SELECT version, success FROM _sqlx_migrations ORDER BY version"))
        if any(not row["success"] for row in receipts):
            raise RepairError("存在未完成的原生迁移，需按具体错误处理；不会伪造或修改迁移记录。")
        versions = {int(row["version"]) for row in receipts}
        columns = list(connection.execute("PRAGMA table_info(remote_control_enrollments)"))
        names = [row["name"] for row in columns]
        problem = None
        maximum = max(versions, default=0)
        if TABLE in tables and 24 not in versions and maximum < 24:
            problem = "table_before_native_migration_24"
        elif TABLE in tables and OPTIONAL_COLUMN in names and 24 in versions and 37 not in versions and maximum < 37:
            problem = "column_before_native_migration_37"
        return {"migrationCount": len(versions), "maxMigration": maximum,
                "native24": 24 in versions, "native37": 37 in versions,
                "remoteTable": TABLE in tables, "remoteColumns": names, "problem": problem}


def check_remote_shape(connection):
    columns = list(connection.execute("PRAGMA table_info(remote_control_enrollments)"))
    names = [row["name"] for row in columns]
    if names not in [list(BASE_COLUMNS), list(BASE_COLUMNS) + [OPTIONAL_COLUMN]]:
        raise RepairError("远程控制表结构不属于已确认的范围，不进行通用删库。")
    for row in columns:
        expected_type = "INTEGER" if row["name"] in ("updated_at", OPTIONAL_COLUMN) else "TEXT"
        expected_pk = BASE_COLUMNS.index(row["name"]) + 1 if row["name"] in BASE_COLUMNS[:3] else 0
        if (row["type"].upper() != expected_type or row["pk"] != expected_pk
                or row["dflt_value"] is not None or bool(row["notnull"]) != (row["name"] in BASE_COLUMNS)):
            raise RepairError("远程控制表类型、主键或默认值发生变化，停止自动修复。")
    ddl = connection.execute("SELECT sql FROM sqlite_master WHERE type='table' AND name=?", (TABLE,)).fetchone()[0]
    if re.search(r"\b(?:CHECK|REFERENCES|GENERATED|STRICT|WITHOUT|UNIQUE|COLLATE|CONFLICT|DEFERRABLE|ASC|DESC|AUTOINCREMENT)\b", ddl, re.I):
        raise RepairError("远程控制表存在额外约束，停止自动修复。")
    custom = list(connection.execute(
        "SELECT name FROM sqlite_master WHERE tbl_name=? AND type IN ('trigger','index') AND sql IS NOT NULL", (TABLE,)))
    if custom:
        raise RepairError("远程控制表存在自定义索引或触发器，需人工核对。")
    for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'"):
        quoted = '"' + row[0].replace('"', '""') + '"'
        if any(foreign[2] == TABLE for foreign in connection.execute("PRAGMA foreign_key_list(" + quoted + ")")):
            raise RepairError("其他表引用了远程控制表，停止自动重建此表。")
    return names


def stage_conflict(path, problem):
    with sqlite_open(path) as connection:
        names = check_remote_shape(connection)
        rows = [dict(row) for row in connection.execute("SELECT * FROM remote_control_enrollments")]
        # Work only on a snapshot. The real DB and the SQLx ledger stay intact.
        connection.execute("DROP TABLE remote_control_enrollments")
        if problem == "column_before_native_migration_37":
            connection.execute(BASE_DDL)
            fields = ",".join(BASE_COLUMNS)
            connection.executemany("INSERT INTO remote_control_enrollments (" + fields + ") VALUES (?,?,?,?,?,?,?)",
                                   [tuple(row[name] for name in BASE_COLUMNS) for row in rows])
        return names, rows


def restore_remote_rows(path, names, rows):
    with sqlite_open(path) as connection:
        current = {row["name"] for row in connection.execute("PRAGMA table_info(remote_control_enrollments)")}
        if not set(names).issubset(current):
            raise RepairError("内置程序没有建立所需的原生结构，副本不会替换原库。")
        fields = ",".join(names)
        updates = ",".join(name + "=excluded." + name for name in names if name not in BASE_COLUMNS[:3])
        sql = ("INSERT INTO remote_control_enrollments (" + fields + ") VALUES (" +
               ",".join("?" for _ in names) + ") ON CONFLICT(websocket_url,account_id,app_server_client_name) "
               "DO UPDATE SET " + updates)
        connection.executemany(sql, [tuple(row[name] for name in names) for row in rows])


def thread_ids(path):
    with sqlite_open(path, readonly=True) as connection:
        present = connection.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='threads'").fetchone()
        return {row[0] for row in connection.execute("SELECT id FROM threads")} if present else set()


def verify_remote_rows(path, names, rows):
    with sqlite_open(path, readonly=True) as connection:
        for row in rows:
            saved = connection.execute(
                "SELECT " + ",".join(names) + " FROM remote_control_enrollments "
                "WHERE websocket_url=? AND account_id=? AND app_server_client_name=?",
                tuple(row[name] for name in BASE_COLUMNS[:3]),
            ).fetchone()
            if saved is None or tuple(saved) != tuple(row[name] for name in names):
                raise RepairError("副本中的远程控制记录未完整保留，原库保持不变。")


def stop_child(process):
    if process.poll() is not None:
        return
    try:
        process.stdin.close()
        process.wait(timeout=3)
        return
    except (OSError, subprocess.TimeoutExpired):
        pass
    if os.name == "nt":
        run_capture(["taskkill.exe", "/PID", str(process.pid), "/T", "/F"], timeout=8)
    else:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=3)


def native_probe(cli, home, sqlite_home, timeout):
    """Initialize + config/read only; no thread/start, turn/start or model call."""
    child_env = os.environ.copy()
    child_env.update(CODEX_HOME=str(home), CODEX_SQLITE_HOME=str(sqlite_home),
                     OTEL_SDK_DISABLED="true", RUST_LOG="warn")
    arguments = [str(cli), "-c", "sqlite_home=" + json.dumps(str(sqlite_home)),
                 "-c", "log_dir=" + json.dumps(str(home / "probe-logs")), "app-server"]
    process = subprocess.Popen(
        arguments, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        env=child_env, cwd=str(home), start_new_session=os.name != "nt",
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
    )
    messages = queue.Queue(maxsize=2048)
    stderr = []
    exhausted = threading.Event()

    def collect(stream, is_error):
        count = 0
        while True:
            line = stream.readline(1024 * 1024)
            if not line:
                break
            count += len(line)
            if count > 4 * 1024 * 1024:
                exhausted.set()
                break
            if is_error:
                stderr.append(line.decode("utf-8", errors="replace"))
            else:
                try:
                    messages.put_nowait(json.loads(line))
                except (json.JSONDecodeError, queue.Full):
                    continue

    readers = [threading.Thread(target=collect, args=(process.stdout, False), daemon=True),
               threading.Thread(target=collect, args=(process.stderr, True), daemon=True)]
    for reader in readers:
        reader.start()

    def send(message):
        process.stdin.write((json.dumps(message) + "\n").encode("utf-8"))
        process.stdin.flush()

    outcome = {"ok": False, "kind": "process-exit"}
    rpc_error = ""
    try:
        send({"id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "tiancaispacehub_startup_repair", "title": "Hub startup repair", "version": VERSION}}})
        expected = 1
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline and not exhausted.is_set():
            try:
                response = messages.get(timeout=0.15)
            except queue.Empty:
                if process.poll() is not None:
                    break
                continue
            if not isinstance(response, dict) or response.get("id") != expected:
                continue
            if "error" in response or not isinstance(response.get("result"), dict):
                rpc_error = str(response.get("error", ""))
                outcome["kind"] = "rpc-error"
                break
            if expected == 1:
                if "userAgent" not in response["result"]:
                    outcome["kind"] = "unexpected-initialize"
                    break
                send({"method": "initialized", "params": {}})
                send({"id": 2, "method": "config/read", "params": {"includeLayers": False}})
                expected = 2
            else:
                outcome = {"ok": True, "kind": "initialized-and-config-read"}
                break
        else:
            outcome["kind"] = "output-limit" if exhausted.is_set() else "timeout"
    except (OSError, ValueError):
        outcome["kind"] = "transport-error"
    finally:
        premature_exit = process.poll()
        stop_child(process)
        for reader in readers:
            reader.join(timeout=2)
        process.stdout.close()
        process.stderr.close()
    outcome["exitCode"] = process.returncode
    if outcome["ok"] and premature_exit not in (None, 0):
        outcome.update(ok=False, kind="process-exit")
    combined = ("".join(stderr) + rpc_error).lower()
    outcome["sqliteFailure"] = bool(re.search(
        r"failed to initialize sqlite state runtime|failed to open state db|remote_control_enrollments.*already exists|duplicate column name.*remote_control_enabled",
        combined, re.S))
    if outcome["ok"] and outcome["sqliteFailure"]:
        outcome.update(ok=False, kind="sqlite-startup-error")
    return outcome


def write_manifest(directory, manifest):
    pending = directory / "manifest.pending.json"
    pending.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(pending, directory / "manifest.json")


def copy_family(source, directory):
    directory.mkdir(mode=0o700)
    for path in family(source):
        if path.is_file():
            shutil.copy2(path, directory / path.name)


def promote(source, candidate, backup, manifest):
    require_stopped()
    if fingerprint(source) != manifest["originalSHA256"]:
        raise RepairError("原库在检查后发生变化，已停止替换。请退出相关程序后重新检查。")
    displaced = backup / "displaced"
    displaced.mkdir(mode=0o700)
    pending = source.parent / (".hub-state-repair-" + uuid.uuid4().hex + ".sqlite")
    moved = []
    installed = False
    try:
        with candidate.open("rb") as src, pending.open("xb") as dst:
            shutil.copyfileobj(src, dst)
            dst.flush()
            os.fsync(dst.fileno())
        candidate_hash = digest(candidate)
        if digest(pending) != candidate_hash:
            raise RepairError("候选数据库复制校验失败。")
        require_stopped()
        if fingerprint(source) != manifest["originalSHA256"]:
            raise RepairError("原库发生变化，取消替换。")
        manifest["stage"] = "replacing"
        manifest["candidateSHA256"] = candidate_hash
        write_manifest(backup, manifest)
        for path in family(source):
            if path.exists():
                os.rename(path, displaced / path.name)
                moved.append(path)
        os.replace(pending, source)
        installed = True
        manifest["installedSHA256"] = fingerprint(source)
        manifest["stage"] = "applied"
        write_manifest(backup, manifest)
    except BaseException:
        if installed:
            os.rename(source, backup / "failed-candidate.sqlite")
        for path in reversed(moved):
            if path.exists():
                raise RepairError("替换中断且目标已被其他程序占用；请保留备份目录，勿继续运行修复。")
            os.rename(displaced / path.name, path)
        manifest["stage"] = "rolled-back"
        write_manifest(backup, manifest)
        raise
    finally:
        if pending.exists():
            pending.unlink()


def repair(home, source, cli, state, timeout, expected_hashes):
    require_stopped()
    if fingerprint(source) != expected_hashes:
        raise RepairError("原库在诊断后发生变化，停止修复，请重新检查。")
    parent = source.parent / BACKUP_DIR
    parent.mkdir(mode=0o700, exist_ok=True)
    backup = parent / (time.strftime("%Y%m%d-%H%M%S-") + uuid.uuid4().hex[:8])
    backup.mkdir(mode=0o700)
    original_hashes = fingerprint(source)
    copy_family(source, backup / "original")
    if fingerprint(backup / "original" / DB_NAME) != original_hashes or fingerprint(source) != original_hashes:
        raise RepairError("备份前后文件发生变化，停止修复。备份：" + str(backup))
    manifest = {"version": VERSION, "source": str(source), "codexHome": str(home),
                "originalSHA256": original_hashes, "problem": state["problem"], "stage": "backed-up"}
    write_manifest(backup, manifest)
    print("完整原库备份：" + str(backup))
    with tempfile.TemporaryDirectory(prefix="hub-codex-state-repair-") as temporary:
        work = Path(temporary)
        probe_home = work / "home"
        probe_home.mkdir(mode=0o700)
        for name in ("config.toml", "auth.json", "installation_id"):
            original = home / name
            if original.is_file():
                shutil.copy2(original, probe_home / name)
        baseline = work / "baseline"
        baseline.mkdir()
        # SQLite may update SHM even on a read-only connection. Never open the
        # byte-for-byte original backup itself with SQLite.
        copy_family(backup / "original" / DB_NAME, work / "seed-family")
        seed = work / "seed.sqlite"
        sqlite_snapshot(work / "seed-family" / DB_NAME, seed)
        sqlite_snapshot(seed, baseline / DB_NAME)
        preserved_threads = thread_ids(baseline / DB_NAME)
        print("正在副本上核对原始启动错误（最长 {} 秒）…".format(timeout))
        first = native_probe(cli, probe_home, baseline, timeout)
        manifest["baselineProbe"] = first
        write_manifest(backup, manifest)
        if first["ok"]:
            print("当前内置程序能够读取原库副本；未复现冲突，原库保持不变。")
            return backup, False
        if not first["sqliteFailure"]:
            raise RepairError("副本启动失败，但未确认是 SQLite 状态库错误，未修改原库。诊断类型：" + first["kind"])
        staged = work / "staged"
        staged.mkdir()
        sqlite_snapshot(seed, staged / DB_NAME)
        names, rows = stage_conflict(staged / DB_NAME, state["problem"])
        print("已在副本移开冲突结构，交给当前内置程序执行原生升级…")
        second = native_probe(cli, probe_home, staged, timeout)
        manifest["candidateProbe"] = second
        write_manifest(backup, manifest)
        if not second["ok"]:
            raise RepairError("修复副本未通过启动核对，原库保持不变。诊断类型：" + second["kind"])
        after = inspect_state(staged / DB_NAME)
        if not after["native24"] or (OPTIONAL_COLUMN in names and not after["native37"]):
            raise RepairError("内置程序未完成所需原生迁移，原库保持不变。")
        restore_remote_rows(staged / DB_NAME, names, rows)
        if not preserved_threads.issubset(thread_ids(staged / DB_NAME)):
            raise RepairError("副本中的历史会话索引未完整保留，原库保持不变。")
        # Recheck the restored preference rows as well, not just an empty table.
        final = native_probe(cli, probe_home, staged, timeout)
        manifest["finalProbe"] = final
        write_manifest(backup, manifest)
        if not final["ok"]:
            raise RepairError("恢复远程控制记录后副本启动未通过，原库保持不变。")
        inspect_state(staged / DB_NAME)
        if not preserved_threads.issubset(thread_ids(staged / DB_NAME)):
            raise RepairError("最终副本未完整保留历史索引，原库保持不变。")
        verify_remote_rows(staged / DB_NAME, names, rows)
        candidate = work / "candidate.sqlite"
        sqlite_snapshot(staged / DB_NAME, candidate)
        manifest["result"] = after
        promote(source, candidate, backup, manifest)
    return backup, True


def restore(backup_argument):
    backup = Path(backup_argument).expanduser().resolve()
    manifest = json.loads((backup / "manifest.json").read_text(encoding="utf-8"))
    source = Path(manifest["source"])
    if (source.name != DB_NAME or not source.is_absolute() or source.parent.resolve() != source.parent
            or backup.parent != source.parent / BACKUP_DIR or manifest.get("stage") != "applied"):
        raise RepairError("备份清单位置或状态不符合恢复要求，停止操作。")
    require_stopped()
    if fingerprint(source) != manifest.get("installedSHA256"):
        raise RepairError("修复后状态库已发生变化，可能已有新会话；不会覆盖。请保留备份进一步核对。")
    displaced = backup / "displaced"
    if fingerprint(displaced / DB_NAME) != manifest["originalSHA256"]:
        raise RepairError("备份校验失败，停止恢复。")
    current = backup / "before-restore"
    current.mkdir(mode=0o700)
    moved_new, moved_old = [], []
    try:
        for path in family(source):
            if path.exists():
                os.rename(path, current / path.name)
                moved_new.append(path)
        for path in family(source):
            old = displaced / path.name
            if old.exists():
                os.rename(old, path)
                moved_old.append(path)
        manifest["stage"] = "restored"
        write_manifest(backup, manifest)
    except BaseException:
        for path in reversed(moved_old):
            os.rename(path, displaced / path.name)
        for path in reversed(moved_new):
            os.rename(current / path.name, path)
        raise
    print("已恢复修复前的原库。原问题也可能随之恢复；请保留备份目录。")


def main():
    parser = argparse.ArgumentParser(description="Codex 无法加载组织设置：先检查，再备份修复已确认的状态库冲突。")
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--diagnose", action="store_true", help="默认：只检查，不启动 Codex、不修改用户库")
    action.add_argument("--repair", action="store_true", help="备份后在副本验证已确认的结构冲突，通过才替换原库")
    action.add_argument("--restore", metavar="BACKUP_DIR", help="恢复指定备份；修复后有新变化时拒绝覆盖")
    parser.add_argument("--codex-home", help="明确指定桌面端的用户目录")
    parser.add_argument("--sqlite-home", help="明确指定实际状态库目录（绝对路径）")
    parser.add_argument("--codex-bin", help="指定本次桌面应用自带的 codex 程序，避免混用独立 CLI")
    parser.add_argument("--timeout", type=int, default=30, choices=range(5, 61), metavar="5..60", help="每次副本启动核对的秒数，默认 30")
    arguments = parser.parse_args()
    if platform.system() not in ("Windows", "Darwin"):
        raise RepairError("本脚本仅面向 Windows 和 macOS。")
    os.umask(0o077)
    print("TianCaiSpaceHub / Codex 启动检查 " + VERSION)
    require_stopped()
    if arguments.restore:
        restore(arguments.restore)
        return 0
    home, sqlite_home = resolve_locations(arguments)
    source = sqlite_home / DB_NAME
    print("用户目录：" + str(home))
    print("状态库：" + str(source))
    cli = locate_cli(arguments.codex_bin)
    print("应用内置程序：" + str(cli))
    override = gui_value("CODEX_CLI_PATH")
    if override and (not Path(override).is_file() or Path(override).resolve() != cli):
        print("注意：桌面 CODEX_CLI_PATH 指向其他路径。本脚本不改全局环境变量；需另核对该覆盖项。")
    if not source.is_file():
        print("未找到本脚本支持的 state_5.sqlite，未执行修复。若应用仍报错，请核对实际状态库路径和启动日志。")
        return 0
    before = fingerprint(source)
    with tempfile.TemporaryDirectory(prefix="hub-codex-state-inspect-") as temporary:
        copied = Path(temporary)
        copy_family(source, copied / "state")
        if fingerprint(source) != before:
            raise RepairError("检查期间原库发生变化，请确认程序已完全退出。")
        state = inspect_state(copied / "state" / DB_NAME)
    print("原生迁移记录：{} 条，最高编号 {}；不把编号低直接判定为损坏。".format(state["migrationCount"], state["maxMigration"]))
    if not state["problem"]:
        print("未发现已确认的远程控制表/字段迁移冲突。未修改原库；其他原因需查看实际启动日志。")
        return 0
    description = "表已存在，但原生建表迁移尚未完成" if state["problem"].startswith("table_") else "字段已存在，但原生加字段迁移尚未完成"
    print("发现待核对的冲突：" + description)
    if not arguments.repair:
        print("当前为只检查模式。退出程序后使用修复模式，才会备份并在副本验证。")
        return 2
    backup, changed = repair(home, source, cli, state, arguments.timeout, before)
    if changed:
        print("已修复状态库冲突，保留原会话索引和远程控制记录。")
        print("下一步：重新打开 Hub，再增强启动；实际桌面使用仍需自行确认。")
        print("恢复备份参数：--restore \"{}\"".format(backup))
    return 0


if __name__ == "__main__":
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    try:
        sys.exit(main())
    except (RepairError, OSError, sqlite3.Error, ValueError, subprocess.SubprocessError) as error:
        print("已停止：" + str(error), file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        print("已取消。请保留已显示的备份目录。", file=sys.stderr)
        sys.exit(130)
