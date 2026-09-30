"""Local import-contract fixture. No real credentials, inference, or request logging.

Run: python scripts/mock-hub-import.py --port 18765
Open http://127.0.0.1:18765 in a browser. HTTP imports need no environment
switch. Use an isolated CODEXHUB_HOME when testing with a separate Hub instance.
"""
import argparse
import html
import json
import secrets
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlencode, urlparse

TICKETS = {}
LOCK = threading.Lock()
PROTOCOLS = ("openai_responses", "anthropic_messages", "chat_completions", "grok_responses")


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    @property
    def origin(self):
        return f"http://127.0.0.1:{self.server.server_port}"

    def send(self, status, data, content_type="application/json"):
        body = (json.dumps(data, ensure_ascii=False) if content_type == "application/json" else data).encode()
        self.send_response(status)
        self.send_header("Content-Type", content_type + "; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        parsed = urlparse(self.path)
        if parsed.path == "/":
            links = "".join(f'<li><a href="/new?protocol={p}">{p}</a></li>' for p in PROTOCOLS)
            self.send(200, '<!doctype html><meta charset="utf-8"><title>Hub import fixture</title><h1>Hub 网页导入联调</h1>'
                      f'<ul>{links}<li><a href="/new?empty=1">空模型</a></li><li><a href="/new?failure=1">模型发现失败</a></li>'
                      '<li><a href="/new?expired=1">过期导入码</a></li><li><a href="/new?key=other">同名不同 Key</a></li></ul>', "text/html")
        elif parsed.path == "/new":
            query = parse_qs(parsed.query)
            protocol = query.get("protocol", [PROTOCOLS[0]])[0]
            if protocol not in PROTOCOLS:
                self.send(400, {"code": 400, "message": "unsupported protocol"})
                return
            ticket = secrets.token_urlsafe(32)
            scenario = {"protocol": protocol, "empty": "empty" in query, "failure": "failure" in query, "key": query.get("key", ["123"])[0]}
            with LOCK:
                now = time.monotonic()
                for old in [key for key, (expiry, _) in TICKETS.items() if expiry <= now]:
                    TICKETS.pop(old)
                TICKETS[ticket] = (now + (-1 if "expired" in query else 120), scenario)
            deeplink = "tiancaispacehub://import/v1?" + urlencode({"origin": self.origin, "ticket": ticket})
            self.send(200, '<!doctype html><meta charset="utf-8"><title>Open Hub</title><h1>导入测试渠道</h1>'
                      f'<p><a href="{html.escape(deeplink)}">打开 TianCaiSpace Hub</a></p>'
                      f'<p>开发调试链接（120 秒有效，仅含测试码）：</p><textarea cols="100" rows="4">{html.escape(deeplink)}</textarea>'
                      '<p>保存成功后返回首页再次导入，可测试重复来源。</p>', "text/html")
        elif parsed.path in ("/v1/models", "/empty/v1/models"):
            if self.headers.get("Authorization") != "Bearer mock-hub-key-not-a-real-secret":
                self.send(401, {"code": 401})
                return
            models = [] if parsed.path.startswith("/empty/") else [{"id": "mock-model-a"}, {"id": "mock-model-b"}]
            self.send(200, {"object": "list", "data": models})
        elif parsed.path == "/failure/v1/models":
            self.send(503, {"code": 503, "message": "fixture failure"})
        else:
            self.send(404, {"code": 404})

    def do_POST(self):
        if self.path != "/api/v1/external-import/resolve":
            self.send(404, {"code": 404})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 8192:
                raise ValueError()
            request = json.loads(self.rfile.read(length))
            if request.get("target") != "tiancaispace-hub" or request.get("schema_version") != 1:
                raise ValueError()
        except (ValueError, TypeError):
            self.send(400, {"code": 400})
            return
        with LOCK:
            entry = TICKETS.pop(request.get("ticket", ""), None)
        if entry is None or entry[0] <= time.monotonic():
            self.send(404, {"code": 404, "message": "expired or used"})
            return
        scenario = entry[1]
        path = "/failure" if scenario["failure"] else "/empty" if scenario["empty"] else ""
        self.send(200, {"code": 0, "message": "success", "data": {
            "schema_version": 1, "target": "tiancaispace-hub",
            "source": {"origin": self.origin, "site_name": "本地联调站点", "key_id": scenario["key"], "key_name": "开发密钥"},
            "provider": {"name": "联调渠道 · 开发密钥", "platform": "openai", "protocol": scenario["protocol"],
                         "base_url": self.origin + "/antigravity/v1", "models_url": self.origin + path + "/v1/models",
                         "api_key": "mock-hub-key-not-a-real-secret", "models": [] if scenario["empty"] else ["mock-model-a", "mock-model-b"], "model_aliases": {}}
        }})


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=18765)
    args = parser.parse_args()
    print(f"Mock import site: http://127.0.0.1:{args.port}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
