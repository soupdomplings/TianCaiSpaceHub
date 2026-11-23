# -*- coding: utf-8 -*-
# TianCaiSpaceHub: this script is run inside the selected Xshell tab.
import binascii
import json
import os
import re
import time
import uuid

ROOT = binascii.unhexlify("HUB_DIRECTORY_HEX").decode("utf-8")
LAUNCH_ID = "HUB_LAUNCH_ID"
CHECKS = dict(json.loads('HUB_BUILTIN_CHECKS'))
EXTRAS = frozenset(json.loads('HUB_EXTRA_COMMANDS'))
FLAGS = dict(json.loads('HUB_CHECK_FLAGS'))
AUTH = re.compile(r"(password|passphrase|verification code|one-time|otp|密码|口令|验证码|密碼|驗證碼)", re.I)
PROMPT = re.compile(r"[#$%]\s*$")
REQUEST_ID = re.compile(r"request-([0-9a-f-]{36})\.json$")
YES_NO = 4


def clock():
    return int(time.time() * 1000)


def connected():
    return (str(xsh.Session.RemoteAddress), int(xsh.Session.RemotePort)) if xsh.Session.Connected else None


def confirm(message):
    return int(xsh.Dialog.MessageBox(message, "XshellB-MCP", YES_NO)) == 6


def put(path, data):
    tmp = path + ".tmp"
    try:
        with open(tmp, "w", encoding="utf-8") as handle:
            json.dump(data, handle, ensure_ascii=False)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def screen():
    rows = int(xsh.Screen.Rows)
    cols = int(xsh.Screen.Columns)
    if rows < 1 or cols < 1:
        return ""
    start = max(1, rows - max(1, min(80, 10000 // cols)) + 1)
    return str(xsh.Screen.Get(start, 1, rows, cols))[-10000:]


def last_line(body):
    return body.rstrip().splitlines()[-1] if body.strip() else ""


def launch_directory():
    return os.path.join(ROOT, "launch-" + LAUNCH_ID)


def startup_error(code, manual_text):
    if not LAUNCH_ID:
        xsh.Dialog.MsgBox(manual_text)
        return
    try:
        put(os.path.join(launch_directory(), "failure"), code)
    except OSError:
        pass  # The Hub may have already left this launch.


def consume_launch():
    # The Hub publishes this only after it submits our unique path to the
    # chosen window. Xshell 8 embeds Python in its child XshellCore process.
    folder = launch_directory()
    permit = os.path.join(folder, "permit.json")
    claimed = os.path.join(folder, "claimed")
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline and os.path.isdir(folder):
        try:
            with open(permit, encoding="utf-8") as handle:
                grant = json.load(handle)
        except FileNotFoundError:
            if os.path.exists(claimed):
                return False
            time.sleep(0.05)
            continue
        if grant["pid"] not in (os.getpid(), os.getppid()):
            startup_error("owner", "")
            return False
        if grant["expires"] <= clock():
            return False
        try:
            os.rename(permit, claimed)
        except OSError:
            return False
        return True
    return False


def extra_allowed(key):
    if key in EXTRAS:
        return True
    name, separator, args = key.partition(" ")
    return bool(separator and len(key) <= 96 and name in FLAGS and all(
        len(arg) > 1 and arg.startswith("-") and all(c in FLAGS[name] for c in arg[1:])
        for arg in args.split(" ")))


def check_rule(key):
    command = CHECKS.get(key)
    try:
        with open(os.path.join(ROOT, "check-policy.json"), encoding="utf-8") as handle:
            p = json.load(handle)
        if not isinstance(p, dict) or not all(
                isinstance(p.get(k), list) and all(isinstance(s, str) for s in p[k])
                for k in ("approved", "extra")):
            return command, False
        if key in p["extra"] and extra_allowed(key):
            command = key
        return command, bool(command) and key in p["approved"]
    except FileNotFoundError:
        if command:
            try:
                with open(os.path.join(ROOT, "approve-built-in-checks"), "rb") as handle:
                    return command, handle.read(5) == b"true"
            except OSError:
                pass
    except (OSError, ValueError):
        pass
    return command, False


def do_request(path, target, prompt):
    with open(path, "r", encoding="utf-8") as handle:
        request = json.loads(handle.read(1024))
    expiry = int(request["expires"])
    operation = request["operation"]
    if clock() >= expiry or connected() != target:
        return {"ok": False, "message": "会话已变化或请求已过期。"}
    body = screen()
    last = last_line(body)
    if AUTH.search(last):
        return {"ok": False, "message": "检测到认证提示，请在 Xshell 中完成。"}
    if operation == "screen":
        return {"ok": True, "screen": body}
    cmd, preapproved = check_rule(operation)
    if not cmd:
        return {"ok": False, "message": "未提供此检查。"}
    if bool(xsh.Screen.IsAltScreen()) or not prompt or last != prompt:
        return {"ok": False, "message": "标签页不在接入时的空闲提示符，请清空输入或在新提示符重新接入。"}
    via_client = request.get("clientForm") is True
    approved_once = via_client and request.get("clientApproved") is True
    if not (preapproved or approved_once):
        if via_client:
            return {"ok": False, "message": "巡检授权已更改。请在客户端重新发起这条巡检。"}
        if not confirm("目标：%s:%s\n\n在当前标签页运行这条只读命令？\n\n%s" % (target[0], target[1], cmd)):
            return {"ok": False, "message": "你在 Xshell 中拒绝了本次命令。"}
        # An optional command might have been removed while the dialog was open.
        if check_rule(operation)[0] != cmd:
            return {"ok": False, "message": "这条巡检命令已移除，未发送。"}
    if (clock() >= expiry or not os.path.isfile(path) or connected() != target
            or bool(xsh.Screen.IsAltScreen()) or screen() != body):
        return {"ok": False, "message": "确认期间会话状态变化或请求过期，命令未发送。"}
    xsh.Screen.Send(cmd + "\r")
    return {"ok": True, "sent": cmd, "message": "已发送；请读取屏幕核对实际结果。"}


def Main():
    if LAUNCH_ID and not consume_launch():
        return
    target = connected()
    if not target:
        startup_error("login", "请先登录目标标签页，再运行 XshellB-MCP。")
        return
    prompt = last_line(screen())
    if not PROMPT.search(prompt) or AUTH.search(prompt) or bool(xsh.Screen.IsAltScreen()):
        startup_error("prompt", "请先回到空闲的 Shell 提示符，再接入 XshellB-MCP。")
        return
    if not LAUNCH_ID and not confirm("允许本机配置了 XshellB-MCP 的客户端读取当前标签页的可见文字？\n\n目标：%s:%s\n\n请确认当前 Shell 输入行为空。可在 Hub 选择自动同意哪些巡检。" % target):
        return
    if connected() != target or last_line(screen()) != prompt:
        startup_error("changed", "目标标签页已变化，请重新选择。")
        return
    if LAUNCH_ID:
        # Connecting/sampling the terminal can take longer than the Hub's wait.
        try:
            with open(os.path.join(launch_directory(), "claimed"), encoding="utf-8") as handle:
                if json.load(handle)["expires"] <= clock():
                    return
        except FileNotFoundError:
            return
    directory = os.path.join(ROOT, "sessions", LAUNCH_ID or str(uuid.uuid4()))
    os.makedirs(directory)
    heartbeat = os.path.join(directory, "state.json")
    published = 0
    try:
        while connected() == target:
            now = clock()
            if now - published >= 1000:
                put(heartbeat, {"updated": now, "connected": True, "clientConfirmation": True, "batchChecks": True, "host": target[0], "port": target[1],
                                "tab": str(xsh.Session.TabText)[:160]})
                published = now
            for filename in sorted(os.listdir(directory)):
                path = os.path.join(directory, filename)
                match = REQUEST_ID.fullmatch(filename)
                if match:
                    try:
                        response = do_request(path, target, prompt)
                    except FileNotFoundError:
                        continue
                    except Exception:
                        response = {"ok": False, "message": "Xshell 未能完成此次操作。"}
                    try:
                        os.unlink(path)
                    except FileNotFoundError:
                        continue
                    put(os.path.join(directory, "reply-" + match.group(1) + ".json"), response)
                    break
                if filename.startswith("reply-"):
                    try:
                        if time.time() - os.path.getmtime(path) > 40:
                            os.unlink(path)
                    except FileNotFoundError:
                        pass
            xsh.Session.Sleep(200)
    finally:
        if os.path.isfile(heartbeat):
            os.unlink(heartbeat)
