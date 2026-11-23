"""WSGI entry point behind the registered HTTPS origin."""
from http import HTTPStatus
import json
import logging
import os
import threading
import time

from providers import Site, Wechat, encode
from relay import Error, Relay, WIRE_MAX
from squad import Squad


def app_for(relay):
    def application(env, start_response):
        try:
            path, method = env["PATH_INFO"], env["REQUEST_METHOD"]
            if env.get("QUERY_STRING"):
                raise Error(400, "请求参数请放在正文")
            length = int(env.get("CONTENT_LENGTH") or 0)
            if length < 0 or length > WIRE_MAX + 2048:
                raise Error(413, "请求过大")
            if method == "POST" and env.get("CONTENT_TYPE", "").split(";")[0] != "application/json":
                raise Error(415, "请求应为 JSON")
            body = json.loads(env["wsgi.input"].read(length)) if length else {}
            if not isinstance(body, dict):
                raise Error(400, "无效的 JSON 请求")
            auth = env.get("HTTP_AUTHORIZATION", "")
            secret = auth[7:] if auth.startswith("Bearer ") else ""
            data = relay.dispatch(method, path, secret, body)
            status = 200
        except Error as err:
            status, data = err.status, {"error": {"message": str(err)}}
        except (ValueError, UnicodeError):
            status, data = 400, {"error": {"message": "无效的输入"}}
        except Exception:
            # Avoid logging provider URLs, bodies, identity codes or encrypted packet contents.
            logging.error("业务请求未完成")
            status, data = 500, {"error": {"message": "服务暂时不可用"}}
        result = encode(data)
        start_response(f"{status} {HTTPStatus(status).phrase}",
                       [("Content-Type", "application/json; charset=utf-8"), ("Cache-Control", "no-store"),
                        ("Content-Length", str(len(result)))])
        return [result]
    return application


def configured():
    wx = Wechat(os.environ["WX_APP_ID"], os.environ["WX_APP_SECRET"],
                os.getenv("WX_OFFER_ID", ""), os.getenv("WX_VIRTUAL_APP_KEY", ""))
    site = None
    if os.getenv("SQUAD_ORIGIN"):
        key = os.environ["SQUAD_BRIDGE_SECRET"]
        if len(key) < 32:
            raise ValueError("站点共享密钥至少 32 字符")
        site = Site(os.environ["SQUAD_ORIGIN"], key)
    db = os.environ["MOBILE_DB"]
    squad = Squad(db, site, wx, json.loads(os.getenv("WX_PRODUCTS", "{}")),
                  os.getenv("WX_PAYMENT_PLATFORMS", "").split(","))
    def reconcile():
        while True:
            time.sleep(60)
            try:
                squad.sweep()
            except Exception:
                logging.error("待付款订单核对暂不可用")
    threading.Thread(target=reconcile, daemon=True, name="payment-reconciliation").start()
    return app_for(Relay(db, wx, squad))


if __name__ == "__main__":
    from waitress import serve
    serve(configured(), host="127.0.0.1", port=int(os.getenv("MOBILE_PORT", "8080")),
          threads=int(os.getenv("MOBILE_THREADS", "128")), max_request_body_size=WIRE_MAX + 2048,
          channel_timeout=75, clear_untrusted_proxy_headers=True)
