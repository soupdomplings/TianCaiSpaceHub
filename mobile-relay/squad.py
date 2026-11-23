"""Mobile account bridge. Authoritative balances, quotes and credits belong to the site."""
from decimal import Decimal, InvalidOperation
import logging
import secrets
import sqlite3
import threading
import time

from relay import Error, field, identifier, token


def money(value):
    try:
        result = Decimal(str(value))
        if result.is_finite() and 0 < result <= 100000000 and result.as_tuple().exponent >= -2:
            return result.quantize(Decimal("0.01"))
    except (InvalidOperation, ValueError):
        pass
    raise Error(400, "请选择有效的充值金额")


class Squad:
    def __init__(self, database, site, wx, products=None, platforms=()):
        self.db = sqlite3.connect(database, check_same_thread=False)
        self.db.row_factory = sqlite3.Row
        self.lock = threading.RLock()
        self.site, self.wx = site, wx
        self.products = {}
        for amount, sku in (products or {}).items():
            if (not isinstance(sku, dict) or not isinstance(sku.get("id"), str) or not sku["id"].strip()
                    or type(sku.get("priceFen")) is not int or sku["priceFen"] <= 0):
                raise ValueError("WX_PRODUCTS 需提供已发布的 id 和正整数 priceFen")
            self.products[str(money(amount))] = (sku["id"], sku["priceFen"])
        self.platforms = [p for p in platforms if p in ("android", "ios")]
        self.db.executescript("""
            CREATE TABLE IF NOT EXISTS squad_links(uid TEXT PRIMARY KEY, ref TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS mini_quotes(id TEXT PRIMARY KEY, uid TEXT NOT NULL, ref TEXT NOT NULL,
                amount TEXT NOT NULL, payable TEXT NOT NULL, credit TEXT NOT NULL, product TEXT NOT NULL,
                platform TEXT NOT NULL, expires INT NOT NULL);
            CREATE TABLE IF NOT EXISTS mini_orders(id TEXT PRIMARY KEY, quote TEXT UNIQUE NOT NULL,
                request TEXT NOT NULL, uid TEXT NOT NULL, ref TEXT NOT NULL, amount TEXT NOT NULL,
                payable TEXT NOT NULL, credit TEXT NOT NULL, product TEXT NOT NULL, state TEXT NOT NULL,
                wx_trade TEXT UNIQUE, notified INT DEFAULT 0, checked REAL DEFAULT 0,
                UNIQUE(uid,request));
        """)

    def one(self, sql, *args):
        with self.lock:
            return self.db.execute(sql, args).fetchone()

    def update(self, sql, *args):
        with self.lock, self.db:
            self.db.execute(sql, args)

    def link(self, uid):
        row = self.one("SELECT ref FROM squad_links WHERE uid=?", uid)
        if not row:
            raise Error(403, "请先绑定天才小分队账户")
        return row["ref"]

    def capabilities(self):
        return {"enabled": bool(self.site and self.wx.offer and self.wx.key and self.platforms and self.products),
                "platforms": self.platforms, "amounts": sorted(map(float, self.products))}

    def dashboard(self, uid):
        row = self.one("SELECT ref FROM squad_links WHERE uid=?", uid)
        if not row:
            return {"bound": False, "payment": self.capabilities()}
        try:
            result = self.site.call("overview", link=row["ref"])
        except Error as err:
            if err.status != 410:
                raise
            self.update("DELETE FROM squad_links WHERE uid=? AND ref=?", uid, row["ref"])
            return {"bound": False, "payment": self.capabilities()}
        return {**result, "bound": True, "payment": self.capabilities()}

    def bind(self, uid, data):
        if not self.site:
            raise Error(503, "运营方尚未配置小分队服务")
        res = self.site.call("redeem", code=field(data, "code", 256))
        self.update("INSERT OR REPLACE INTO squad_links VALUES(?,?)", uid, field(res, "link", 256))
        return {"bound": True}

    def quote(self, uid, data):
        amount = str(money(data.get("amount")))
        platform = field(data, "platform", 16)
        if not self.capabilities()["enabled"] or platform not in self.platforms or amount not in self.products:
            raise Error(403, "当前终端或充值金额未开通")
        ref = self.link(uid)
        q = self.site.call("quote", link=ref, amount=float(amount))
        payable, credit = str(money(q["payable"])), str(Decimal(str(q["credit"])))
        if not Decimal(credit).is_finite() or Decimal(credit) <= 0:
            raise Error(502, "站点报价无效")
        product, price_fen = self.products[amount]
        if int(money(payable) * 100) != price_fen:
            raise Error(409, "站点报价与微信商品现价不同，请等待商品更新")
        key = token()
        with self.lock, self.db:
            self.db.execute("DELETE FROM mini_quotes WHERE expires<?", (time.time(),))
            self.db.execute("INSERT INTO mini_quotes VALUES(?,?,?,?,?,?,?,?,?)",
                            (key, uid, ref, amount, payable, credit, product, platform, int(time.time()) + 300))
        return {"quoteId": key, "amount": float(amount), "payable": float(payable), "credit": float(credit)}

    def order(self, uid, data):
        quote, rid = identifier(data, "quoteId"), identifier(data, "requestId")
        ref = self.link(uid)
        who, session_key = self.wx.exchange(field(data, "code", 256))
        if who != uid:
            raise Error(403, "当前微信账号已变化")
        row = self.one("SELECT * FROM mini_orders WHERE uid=? AND (request=? OR quote=?)", uid, rid, quote)
        if row and (row["quote"] != quote or row["ref"] != ref):
            raise Error(409, "原请求属于其他报价或账户")
        if not row:
            q = self.one("SELECT * FROM mini_quotes WHERE id=? AND uid=? AND ref=? AND expires>?", quote, uid, ref, time.time())
            if (not q or q["platform"] not in self.platforms or self.products.get(q["amount"])
                    != (q["product"], int(money(q["payable"]) * 100))):
                raise Error(409, "报价已变化，请重新查看")
            with self.lock, self.db:
                self.db.execute("""INSERT OR IGNORE INTO mini_orders(id, quote, request, uid, ref, amount, payable,
                    credit, product, state) VALUES(?,?,?,?,?,?,?,?,?,'creating')""",
                                ("tc" + secrets.token_hex(14), quote, rid, uid, ref, q["amount"], q["payable"],
                                 q["credit"], q["product"]))
                row = self.db.execute("SELECT * FROM mini_orders WHERE uid=? AND quote=? AND ref=?", (uid, quote, ref)).fetchone()
            if not row:
                raise Error(409, "请核对原订单")
        if row["state"] == "creating":
            # The site uses this merchant ID as its unique order identity even on a timeout.
            self.site.call("create", link=ref, merchant=row["id"], amount=float(row["amount"]),
                           payable=float(row["payable"]), credit=float(row["credit"]))
            self.update("UPDATE mini_orders SET state='pending' WHERE id=? AND state='creating'", row["id"])
        row = self.one("SELECT * FROM mini_orders WHERE id=?", row["id"])
        result = self.view(row)
        if row["state"] == "pending":
            result["payment"] = self.wx.payment(session_key, row["id"], row["product"], int(money(row["payable"]) * 100))
        return result

    @staticmethod
    def view(row):
        return {"id": row["id"], "state": "pending" if row["state"] == "creating" else row["state"],
                "credit": float(row["credit"]), "amount": float(row["amount"]), "payable": float(row["payable"])}

    def reconcile(self, row):
        order = row["id"]
        if row["state"] == "paid" and not row["notified"]:
            self.wx.delivered(row["uid"], order)
            self.update("UPDATE mini_orders SET notified=1 WHERE id=?", order)
            return
        if row["state"] != "pending":
            return
        remote = self.wx.query(row["uid"], order)
        if remote.get("order_id") != order or remote.get("order_type") not in (0, 7):
            raise Error(502, "微信订单标识不符")
        status = remote.get("status")
        if status == 6:
            self.update("UPDATE mini_orders SET state='closed' WHERE id=? AND state='pending'", order)
        elif status == 2:
            trade = field(remote, "wx_order_id", 128)
            cents = int(money(row["payable"]) * 100)
            if type(remote.get("paid_fee")) is not int or remote["paid_fee"] != cents:
                raise Error(409, "微信实付金额与站点订单不一致，请联系运营核对")
            # Claim the unique WeChat payment before making the retry-safe credit request.
            try:
                with self.lock, self.db:
                    updated = self.db.execute("""UPDATE mini_orders SET wx_trade=? WHERE id=?
                        AND (wx_trade IS NULL OR wx_trade=?)""", (trade, order, trade))
                    if not updated.rowcount:
                        raise Error(409, "订单的微信交易标识已变化")
            except sqlite3.IntegrityError:
                raise Error(409, "微信交易已关联到其他订单") from None
            self.site.call("settle", merchant=order, trade=trade, cents=cents)
            self.update("UPDATE mini_orders SET state='paid' WHERE id=?", order)
            self.reconcile(self.one("SELECT * FROM mini_orders WHERE id=?", order))

    def lookup(self, uid, order):
        row = self.one("SELECT * FROM mini_orders WHERE id=? AND uid=? AND ref=?", order, uid, self.link(uid))
        if not row:
            raise Error(404, "未找到当前账户的订单")
        if row["state"] == "pending" and time.time() - row["checked"] > 3:
            self.update("UPDATE mini_orders SET checked=? WHERE id=?", time.time(), order)
            self.reconcile(row)
        return self.view(self.one("SELECT * FROM mini_orders WHERE id=?", order))

    def sweep(self):
        with self.lock:
            rows = list(self.db.execute("""SELECT * FROM mini_orders WHERE
                (state='pending' OR (state='paid' AND notified=0)) AND checked<? ORDER BY checked LIMIT 100""",
                                        (time.time() - 60,)))
        for row in rows:
            self.update("UPDATE mini_orders SET checked=? WHERE id=?", time.time(), row["id"])
            try:
                self.reconcile(row)
            except (Error, KeyError, ValueError):
                logging.warning("支付核对等待重试，订单 %s", row["id"])

    def dispatch(self, method, path, uid, data):
        if method == "GET" and path == "/v1/squad":
            return self.dashboard(uid)
        if method == "GET" and path.startswith("/v1/squad/orders/"):
            return self.lookup(uid, path.split("/")[-1])
        actions = {"/v1/squad/bind": self.bind, "/v1/squad/quote": self.quote, "/v1/squad/orders": self.order}
        if method == "POST" and path in actions:
            return actions[path](uid, data)
        raise Error(404, "小分队接口不存在")
