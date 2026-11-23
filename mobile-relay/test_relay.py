import base64
from concurrent.futures import ThreadPoolExecutor
from io import BytesIO
import json
import os
import tempfile
import time
import unittest

from app import app_for
from providers import Wechat
from relay import Error, Relay
from squad import Squad


class FakeWechat(Wechat):
    def __init__(self):
        super().__init__("test-app", "test-secret", "test-offer", "test-app-key")
        self.status, self.fee, self.notifications = 2, 1050, 0

    def exchange(self, code):
        return "user-" + code, "test-session-key"

    def query(self, uid, order):
        return {"order_id": order, "order_type": 0, "wx_order_id": "test-wx-" + order,
                "status": self.status, "paid_fee": self.fee}

    def delivered(self, uid, order):
        self.notifications += 1


class Site:
    def __init__(self):
        self.orders, self.credited = set(), set()
        self.bonus = 12

    def call(self, op, **args):
        if op == "redeem":
            return {"link": "opaque-site-link-" + args["code"]}
        if op == "overview":
            return {"balance": self.bonus * len(self.credited), "announcements": [], "usage": []}
        if op == "quote":
            return {"payable": 10.5, "credit": self.bonus}
        if op == "create":
            if self.bonus != args["credit"]:
                raise Error(409, "quote changed")
            self.orders.add(args["merchant"])
            return {}
        if op == "settle":
            assert args["merchant"] in self.orders
            self.credited.add(args["merchant"])
            return {}
        raise AssertionError(op)


class TestRelay(unittest.TestCase):
    def setUp(self):
        self.wx = FakeWechat()
        self.relay = Relay(":memory:", self.wx, poll_seconds=0.1, rpc_seconds=1)
        self.addCleanup(self.relay.db.close)
        self.hub = self.relay.register({"name": "家里的电脑"})["deviceId"]
        self.uid, _ = self.wx.exchange("one")

    def pair(self, approve=True):
        invite = self.relay.invite(self.hub)
        body = {"deviceId": self.hub, "inviteId": invite["inviteId"], "name": "我的手机"}
        bind = self.relay.pair(self.uid, body)["bindingId"]
        if approve:
            self.relay.decisions(self.hub, {"bindingId": bind, "allow": True}, False)
        return body, bind

    def packet(self, bind):
        return {"bindingId": bind, "requestId": "test-request-1234", "nonce": "A" * 16,
                "ciphertext": base64.urlsafe_b64encode(os.urandom(48)).decode().rstrip("=")}

    def test_one_use_approval_account_isolation(self):
        pair, bind = self.pair(False)
        with self.assertRaises(Error):
            self.relay.pair("other", pair)
        self.assertEqual(self.relay.devices("other")["devices"], [])
        self.assertEqual(self.relay.poll(self.hub)["pending"][0]["inviteId"], pair["inviteId"])
        with self.assertRaisesRegex(Error, "未配对"):
            self.relay.rpc(self.uid, self.packet(bind))
        self.relay.decisions(self.hub, {"bindingId": bind, "allow": True}, False)
        self.assertEqual(self.relay.devices(self.uid)["devices"][0]["state"], "active")

    def test_ciphertext_roundtrip_and_revocation(self):
        _, bind = self.pair()
        self.relay.poll(self.hub)
        req = self.packet(bind)
        with ThreadPoolExecutor(1) as pool:
            result = pool.submit(self.relay.rpc, self.uid, req)
            self.assertEqual(self.relay.poll(self.hub)["requests"], [req])
            with self.assertRaisesRegex(Error, "已结束"):
                self.relay.reply("wrong-device", req)
            resp = self.packet(bind)
            self.relay.reply(self.hub, resp)
            self.assertEqual(result.result(1), resp)
            result = pool.submit(self.relay.rpc, self.uid, req)
            self.assertEqual(self.relay.poll(self.hub)["requests"], [req])
            self.relay.decisions(self.hub, {"bindingId": bind}, True)
            with self.assertRaisesRegex(Error, "撤销"):
                result.result(1)
            with self.assertRaises(Error):
                self.relay.reply(self.hub, resp)
        self.assertFalse(self.relay.jobs)
        self.assertNotIn(req["ciphertext"], "\n".join(self.relay.db.iterdump()))

    def test_binding_survives_restart_polls_do_not(self):
        with tempfile.TemporaryDirectory() as temp:
            db = os.path.join(temp, "relay.sqlite")
            a = Relay(db, self.wx)
            h = a.register({"name": "PC"})
            login = a.login({"code": "one"})
            key = a.invite(h["deviceId"])["inviteId"]
            b = a.pair(self.uid, {"name": "phone", "inviteId": key, "deviceId": h["deviceId"]})["bindingId"]
            a.decisions(h["deviceId"], {"bindingId": b, "allow": True}, False)
            a.db.close()
            a = Relay(db, self.wx)
            try:
                device = a.dispatch("GET", "/v1/devices", login["token"], {})["devices"][0]
                self.assertEqual((device["state"], device["online"]), ("active", False))
                self.assertEqual(a.identity(h["token"], True), h["deviceId"])
            finally:
                a.db.close()

    def test_expiry_and_http_boundary(self):
        invite = self.relay.invite(self.hub)["inviteId"]
        self.relay.db.execute("UPDATE invites SET expires=0")
        with self.assertRaises(Error):
            self.relay.pair(self.uid, {"name": "phone", "inviteId": invite, "deviceId": self.hub})
        app = app_for(self.relay)
        status = []
        def call(path, body, auth=""):
            return json.loads(app({"PATH_INFO":path,"REQUEST_METHOD":"POST","CONTENT_TYPE":"application/json",
                "CONTENT_LENGTH":str(len(body)),"HTTP_AUTHORIZATION":"Bearer " + auth,"wsgi.input":BytesIO(body)},
                lambda s,h:status.append(s))[0])
        result = call("/v1/login",b'{"code":"ok"}')
        self.assertIn("token",result)
        call("/v1/hub/revoke",b'{"bindingId":"anything"}', result["token"])
        self.assertTrue(status[-1].startswith("401"))
        call("/v1/pair",b'{"key":"PSK"}', result["token"])
        self.assertTrue(status[-1].startswith("400"))


class TestPayment(unittest.TestCase):
    def setUp(self):
        self.wx, self.site = FakeWechat(), Site()
        self.s = Squad(":memory:", self.site, self.wx, {"10":{"id":"points_ten","priceFen":1050}}, ["android"])
        self.addCleanup(self.s.db.close)
        self.uid = "user-one"
        self.s.bind(self.uid, {"code":"one"})

    def purchase(self):
        quote = self.s.quote(self.uid, {"amount":10, "platform":"android"})
        return {"quoteId":quote["quoteId"], "requestId":"single-payment-1", "code":"one"}

    def test_checkout_and_retry_uses_one_site_order(self):
        draft = self.purchase()
        a = self.s.order(self.uid, draft)
        b = self.s.order(self.uid, draft)
        self.assertEqual(a,b)
        sign = json.loads(a["payment"]["signData"])
        self.assertEqual((sign["goodsPrice"], sign["outTradeNo"]),(1050,a["id"]))
        self.assertEqual(len(self.site.orders),1)
        self.assertEqual(self.s.dashboard(self.uid)["balance"],0)
        pending = self.s.one("SELECT * FROM mini_orders WHERE id=?",a["id"])
        self.s.reconcile(pending)
        self.s.reconcile(pending)
        self.assertEqual(self.s.lookup(self.uid,a["id"])["state"],"paid")
        self.assertEqual(self.s.dashboard(self.uid)["balance"],12)
        self.assertEqual(len(self.site.credited),1)

    def test_other_account_repriced_or_wrong_amount_cannot_credit(self):
        draft=self.purchase()
        with self.assertRaises(Error):
            self.s.order(self.uid,{**draft,"code":"two"})
        self.site.bonus=13
        with self.assertRaises(Error):
            self.s.order(self.uid,draft)
        self.site.bonus=12
        order=self.s.order(self.uid,draft)
        self.s.bind("user-two",{"code":"two"})
        with self.assertRaises(Error):
            self.s.lookup("user-two",order["id"])
        self.wx.fee=100
        with self.assertRaises(Error):
            self.s.lookup(self.uid,order["id"])
        self.assertFalse(self.site.credited)

    def test_payment_disabled_on_unapproved_platform_and_incomplete_settings(self):
        with self.assertRaises(Error):
            self.s.quote(self.uid,{"platform":"ios","amount":10})
        self.wx.key=""
        self.assertFalse(self.s.dashboard(self.uid)["payment"]["enabled"])
        with self.assertRaises(Error):
            self.s.quote(self.uid,{"platform":"android","amount":10})


if __name__ == "__main__":
    unittest.main()
