"""Single-process mobile relay. Only identities and pairing metadata reach SQLite."""
from __future__ import annotations

import hashlib
import re
import secrets
import sqlite3
import threading
import time

WIRE_MAX = 6 * 1024 * 1024
ID = re.compile(r"[A-Za-z0-9_-]{8,128}\Z")
B64 = re.compile(r"[A-Za-z0-9_-]+\Z")


class Error(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status


def token():
    return secrets.token_urlsafe(32)


def digest(secret):
    return hashlib.sha256(secret.encode()).hexdigest()


def field(data, key, limit=128):
    value = data.get(key)
    if not isinstance(value, str) or not value.strip() or len(value) > limit:
        raise Error(400, "缺少或无效的 " + key)
    return value.strip()


def identifier(data, key):
    value = field(data, key)
    if not ID.fullmatch(value):
        raise Error(400, "无效的 " + key)
    return value


def envelope(data):
    if set(data) != {"bindingId", "requestId", "nonce", "ciphertext"}:
        raise Error(400, "密文封包字段不正确")
    result = {k: identifier(data, k) for k in ("bindingId", "requestId")}
    nonce = field(data, "nonce", 16)
    body = field(data, "ciphertext", WIRE_MAX)
    if len(nonce) != 16 or len(body) < 22 or len(body) % 4 == 1 or not B64.fullmatch(nonce) or not B64.fullmatch(body):
        raise Error(400, "密文封包格式错误")
    result.update(nonce=nonce, ciphertext=body)
    return result


class Relay:
    def __init__(self, database, wechat, squad=None, poll_seconds=15, rpc_seconds=40):
        self.db = sqlite3.connect(database, check_same_thread=False)
        self.db.row_factory = sqlite3.Row
        self.db.executescript("""
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS hubs(id TEXT PRIMARY KEY, name TEXT NOT NULL, auth TEXT UNIQUE NOT NULL);
            CREATE TABLE IF NOT EXISTS phones(auth TEXT PRIMARY KEY, uid TEXT NOT NULL, expires INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS invites(id TEXT PRIMARY KEY, hub TEXT NOT NULL, expires INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS binds(id TEXT PRIMARY KEY, hub TEXT NOT NULL, uid TEXT NOT NULL,
                invitation TEXT NOT NULL, name TEXT NOT NULL, state TEXT NOT NULL, expires INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS binds_hub ON binds(hub);
            CREATE INDEX IF NOT EXISTS binds_uid ON binds(uid);
        """)
        self.wechat, self.squad = wechat, squad
        self.lock = threading.Condition(threading.RLock())
        self.jobs, self.seen, self.revision, self.reported = {}, {}, {}, {}
        self.polling = set()
        self.poll_seconds, self.rpc_seconds = poll_seconds, rpc_seconds

    def register(self, data):
        name = field(data, "name", 80)
        device, secret = token(), token()
        with self.lock, self.db:
            self.db.execute("INSERT INTO hubs VALUES(?,?,?)", (device, name, digest(secret)))
        return {"deviceId": device, "token": secret}

    def login(self, data):
        uid, _ = self.wechat.exchange(field(data, "code", 256))
        secret = token()
        with self.lock, self.db:
            self.db.execute("DELETE FROM phones WHERE expires<=?", (time.time(),))
            self.db.execute("INSERT INTO phones VALUES(?,?,?)", (digest(secret), uid, int(time.time()) + 86400))
        return {"token": secret}

    def identity(self, secret, is_hub):
        key = digest(secret)
        with self.lock:
            if is_hub:
                row = self.db.execute("SELECT id FROM hubs WHERE auth=?", (key,)).fetchone()
            else:
                row = self.db.execute("SELECT uid FROM phones WHERE auth=? AND expires>?", (key, time.time())).fetchone()
        if row is None:
            raise Error(401, "登录已过期，请重新连接")
        return row[0]

    def invite(self, hub):
        inv, exp = token(), int(time.time()) + 300
        with self.lock, self.db:
            self.db.execute("DELETE FROM invites WHERE hub=? OR expires<=?", (hub, time.time()))
            self.db.execute("DELETE FROM binds WHERE state='pending' AND expires<=?", (time.time(),))
            self.db.execute("INSERT INTO invites VALUES(?,?,?)", (inv, hub, exp))
        return {"inviteId": inv, "expiresAt": exp * 1000}

    def changed(self, hub):
        self.revision[hub] = self.revision.get(hub, 0) + 1
        self.lock.notify_all()

    def pair(self, uid, data):
        if set(data) != {"inviteId", "deviceId", "name"}:
            raise Error(400, "配对请求字段不正确")
        inv, hub, name = identifier(data, "inviteId"), identifier(data, "deviceId"), field(data, "name", 80)
        bind = token()
        with self.lock, self.db:
            used = self.db.execute("DELETE FROM invites WHERE id=? AND hub=? AND expires>?", (inv, hub, time.time()))
            if not used.rowcount:
                raise Error(410, "配对码过期或已经使用")
            self.db.execute("INSERT INTO binds VALUES(?,?,?,?,?,'pending',?)",
                            (bind, hub, uid, inv, name, int(time.time()) + 300))
            self.changed(hub)
        return {"bindingId": bind, "state": "pending"}

    def decisions(self, hub, data, revoke):
        bind = identifier(data, "bindingId")
        now = time.time()
        with self.lock, self.db:
            if revoke:
                self.db.execute("DELETE FROM binds WHERE hub=? AND id=?", (hub, bind))
            else:
                if type(data.get("allow")) is not bool:
                    raise Error(400, "需要确认或拒绝配对")
                row = self.db.execute("SELECT state FROM binds WHERE hub=? AND id=? AND (expires>? OR state='active')",
                                      (hub, bind, now)).fetchone()
                if row is None:
                    raise Error(410, "配对请求已失效")
                if data["allow"]:
                    self.db.execute("UPDATE binds SET state='active' WHERE id=?", (bind,))
                elif row["state"] == "pending":
                    self.db.execute("DELETE FROM binds WHERE id=?", (bind,))
            self.changed(hub)
        return {"ok": True}

    def devices(self, uid):
        with self.lock:
            rows = self.db.execute("""SELECT b.id bindingId,b.hub deviceId,h.name,b.state
                FROM binds b JOIN hubs h ON h.id=b.hub WHERE uid=? AND (state='active' OR expires>?)
                ORDER BY b.rowid DESC""", (uid, time.time()))
            return {"devices": [dict(row, online=self.online(row["deviceId"])) for row in rows]}

    def online(self, hub):
        return time.monotonic() - self.seen.get(hub, float("-inf")) < self.poll_seconds + 25

    def poll(self, hub):
        with self.lock:
            if hub in self.polling:
                raise Error(409, "设备已有轮询连接")
            if len(self.polling) >= 48:
                raise Error(503, "当前连接数已满，请稍后重连")
            self.polling.add(hub)
            try:
                self.seen[hub] = time.monotonic()
                self.lock.wait_for(lambda: self.reported.get(hub, -1) != self.revision.get(hub, 0)
                                    or any(j["hub"] == hub and not j["sent"] for j in self.jobs.values()),
                                    self.poll_seconds)
                self.reported[hub] = self.revision.get(hub, 0)
                rows = list(self.db.execute("SELECT * FROM binds WHERE hub=? AND (state='active' OR expires>?)",
                                            (hub, time.time())))
                active = {r["id"] for r in rows if r["state"] == "active"}
                pending = [{"bindingId": r["id"], "inviteId": r["invitation"], "name": r["name"]} for r in rows
                           if r["state"] == "pending"]
                packets, size = [], 0
                for job in self.jobs.values():
                    if job["hub"] != hub or job["sent"] or job["req"]["bindingId"] not in active:
                        continue
                    size += len(job["req"]["ciphertext"])
                    if packets and size > WIRE_MAX:
                        break
                    packets.append(job["req"])
                    job["sent"] = True
                self.seen[hub] = time.monotonic()
                return {"pending": pending, "bindings": [{"bindingId": r["id"], "name": r["name"]} for r in rows
                                                          if r["state"] == "active"], "requests": packets}
            finally:
                self.polling.discard(hub)

    def owner(self, uid, bind):
        row = self.db.execute("SELECT hub FROM binds WHERE id=? AND uid=? AND state='active'", (bind, uid)).fetchone()
        if not row:
            raise Error(403, "设备未配对或授权已撤销")
        return row["hub"]

    def rpc(self, uid, data):
        packet = envelope(data)
        key = (packet["bindingId"], packet["requestId"])
        with self.lock:
            hub = self.owner(uid, key[0])
            if not self.online(hub):
                raise Error(503, "目标电脑离线")
            if key in self.jobs:
                raise Error(409, "请求正在处理")
            if len(self.jobs) >= 40 or sum(j["req"]["bindingId"] == key[0] for j in self.jobs.values()) >= 6:
                raise Error(429, "正在处理其他请求，请稍后再试")
            job = self.jobs[key] = {"req": packet, "hub": hub, "sent": False, "res": None}
            self.lock.notify_all()
            try:
                # Re-evaluate the binding before returning even a queued reply.
                self.lock.wait_for(lambda: self.owner(uid, key[0]) and job["res"] is not None, self.rpc_seconds)
                if job["res"] is None:
                    raise Error(504, "目标电脑未及时回复，请核对原会话")
                return job["res"]
            finally:
                del self.jobs[key]

    def reply(self, hub, data):
        packet = envelope(data)
        key = (packet["bindingId"], packet["requestId"])
        with self.lock:
            job = self.jobs.get(key)
            active = self.db.execute("SELECT 1 FROM binds WHERE id=? AND hub=? AND state='active'", (key[0], hub)).fetchone()
            if not active or not job or job["hub"] != hub or not job["sent"] or job["res"] is not None:
                raise Error(410, "请求已结束")
            job["res"] = packet
            self.lock.notify_all()
        return {"ok": True}

    def dispatch(self, method, path, secret, data):
        if (method, path) == ("POST", "/v1/login"):
            return self.login(data)
        if (method, path) == ("POST", "/v1/hub/register"):
            return self.register(data)
        is_hub = path.startswith("/v1/hub/")
        who = self.identity(secret, is_hub)
        routes = {
            ("GET", "/v1/devices"): lambda: self.devices(who),
            ("POST", "/v1/pair"): lambda: self.pair(who, data),
            ("POST", "/v1/rpc"): lambda: self.rpc(who, data),
            ("POST", "/v1/hub/invite"): lambda: self.invite(who),
            ("POST", "/v1/hub/decide"): lambda: self.decisions(who, data, False),
            ("POST", "/v1/hub/revoke"): lambda: self.decisions(who, data, True),
            ("GET", "/v1/hub/poll"): lambda: self.poll(who),
            ("POST", "/v1/hub/reply"): lambda: self.reply(who, data),
        }
        route = routes.get((method, path))
        if route:
            return route()
        if self.squad and path.startswith("/v1/squad"):
            return self.squad.dispatch(method, path, who, data)
        raise Error(404, "入口不存在")
