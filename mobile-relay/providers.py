"""Outbound APIs. Secrets stay on the server and are never part of error messages."""
import hashlib
import hmac
import json
import threading
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

from relay import Error


def encode(data):
    return json.dumps(data, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode()


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *args):
        return None


def fetch(url, data=None, headers=None):
    req = Request(url, data=data, headers={"Content-Type": "application/json", **(headers or {})})
    try:
        with build_opener(NoRedirect()).open(req, timeout=8) as result:
            raw = result.read(2 * 1024 * 1024 + 1)
            if len(raw) > 2 * 1024 * 1024:
                raise Error(502, "业务数据超出响应上限")
            reply = json.loads(raw)
            if not isinstance(reply, dict):
                raise ValueError()
            return reply
    except HTTPError as exc:
        if exc.code in (400, 401, 403, 404, 409, 410, 422, 429):
            raise Error(exc.code, "业务服务未接受请求，请核对绑定、金额或订单状态") from None
        raise Error(502, "业务服务暂时不可用") from None
    except (OSError, ValueError, URLError):
        raise Error(502, "业务服务暂时不可用") from None


def sign(key, message):
    return hmac.new(key.encode(), message, hashlib.sha256).hexdigest()


class Wechat:
    host = "https://api.weixin.qq.com"

    def __init__(self, app_id, secret, offer="", key=""):
        self.app_id, self.secret, self.offer, self.key = app_id, secret, offer, key
        self.lock = threading.Lock()
        self.access = ("", 0)

    def exchange(self, code):
        params = urlencode({"appid": self.app_id, "secret": self.secret, "js_code": code, "grant_type": "authorization_code"})
        reply = fetch(self.host + "/sns/jscode2session?" + params)
        if not reply.get("openid") or not reply.get("session_key"):
            raise Error(401, "微信登录凭证已失效，请重试")
        return reply["openid"], reply["session_key"]

    def access_token(self):
        with self.lock:
            if self.access[1] > time.monotonic():
                return self.access[0]
            reply = fetch(self.host + "/cgi-bin/stable_token",
                          encode({"appid": self.app_id, "secret": self.secret, "grant_type": "client_credential"}))
            if not reply.get("access_token"):
                raise Error(502, "微信服务凭证暂不可用")
            self.access = reply["access_token"], time.monotonic() + max(1, int(reply["expires_in"]) - 200)
            return self.access[0]

    def payment(self, session_key, merchant_id, product, cents):
        data = encode({"offerId": self.offer, "buyQuantity": 1, "env": 0, "currencyType": "CNY",
                       "productId": product, "goodsPrice": cents, "outTradeNo": merchant_id, "attach": merchant_id})
        return {"mode": "short_series_goods", "signData": data.decode(),
                "paySig": sign(self.key, b"requestVirtualPayment&" + data), "signature": sign(session_key, data)}

    def xpay(self, path, data):
        body = encode(data)
        query = urlencode({"access_token": self.access_token(), "pay_sig": sign(self.key, path.encode() + b"&" + body)})
        result = fetch(self.host + path + "?" + query, body)
        if result.get("errcode") != 0:
            raise Error(502, "微信订单核对暂未完成，请稍后查看原订单")
        return result

    def query(self, uid, order):
        return self.xpay("/xpay/query_order", {"openid": uid, "env": 0, "order_id": order})["order"]

    def delivered(self, uid, order):
        return self.xpay("/xpay/notify_provide_goods", {"openid": uid, "env": 0, "order_id": order})


class Site:
    def __init__(self, origin, secret):
        url = urlsplit(origin)
        if url.scheme != "https" or not url.netloc or url.username or url.password or url.path.strip("/") or url.fragment or url.query:
            raise ValueError("SQUAD_ORIGIN 必须是 HTTPS 站点源")
        self.origin, self.secret = origin.rstrip("/"), secret

    def call(self, op, **fields):
        result = fetch(self.origin + "/api/v1/mini-bridge/" + op, encode(fields),
                       {"Authorization": "Bearer " + self.secret})
        if result.get("code") != 0 or not isinstance(result.get("data"), dict):
            raise Error(502, "小分队业务接口返回异常")
        return result["data"]
