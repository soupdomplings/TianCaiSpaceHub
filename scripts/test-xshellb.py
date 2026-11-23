"""Offline tests of the XshellB script; no Windows app or server is opened."""
from pathlib import Path
from types import SimpleNamespace
import json
import os
import tempfile
import unittest
import uuid


class ScriptTests(unittest.TestCase):
    def setUp(self):
        source = Path(__file__).resolve().parents[1] / "assets/xshellb/attach.py"
        self.screen = SimpleNamespace(
            Get=lambda *_: self.text, IsAltScreen=lambda: self.alt,
            Rows=24, Columns=80, Send=lambda text: self.sent.append(text),
        )
        self.session = SimpleNamespace(Connected=True, RemoteAddress="test", RemotePort=22, TabText="fixture")
        self.text = self.prompt = "tester@test:~$"
        self.answer, self.alt, self.sent, self.calls = 6, False, [], []

        def confirm(*args):
            self.calls.append(args)
            return self.answer

        self.dialog = SimpleNamespace(MessageBox=confirm)
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / "request.json"
        text = source.read_text(encoding="utf-8").replace("HUB_DIRECTORY_HEX", self.tmp.name.encode().hex()).replace("HUB_LAUNCH_ID", "")
        # Rust injects its command catalog when it renders the tab's script.
        fixture = {"directory":"pwd","identity":"id","host":"hostname","uptime":"uptime","disk":"df -h","memory":"free -h"}
        text = text.replace("HUB_BUILTIN_CHECKS", json.dumps(list(fixture.items()))).replace("HUB_EXTRA_COMMANDS", json.dumps(["ls -la", "date"]))
        text = text.replace("HUB_CHECK_FLAGS", json.dumps([["free", "bhkmgtw"], ["uname", "asnrvm"]]))
        self.scope = {"xsh": SimpleNamespace(Session=self.session, Screen=self.screen, Dialog=self.dialog)}
        exec(compile(text, str(source), "exec"), self.scope)

    def request(self, operation="directory", delta=25000, **fields):
        self.path.write_text(json.dumps({"operation": operation, "expires": self.scope["clock"]() + delta, **fields}))
        return self.scope["do_request"](str(self.path), ("test", 22), self.prompt)

    def test_uses_existing_terminal_and_returns_readback(self):
        self.assertTrue(self.request()["ok"])
        self.assertEqual(self.sent, ["pwd\r"])
        self.assertIn("test:22", self.calls[0][0])
        self.text = "tester@test:~$ pwd\n/home/tester\n" + self.prompt
        self.assertIn("/home/tester", self.request("screen")["screen"])
        self.assertEqual(len(self.calls), 1)

    def test_rejection_and_non_catalogued_input_never_send(self):
        self.answer = 7
        self.assertFalse(self.request()["ok"])
        self.assertFalse(self.request("custom command")["ok"])
        self.assertEqual(self.sent, [])
        self.assertEqual(len(self.calls), 1)

    def test_live_opt_in_and_revocation_only_apply_to_known_checks(self):
        self.answer = 7
        setting = Path(self.tmp.name) / "approve-built-in-checks"
        setting.write_bytes(b"true")
        for check, cmd in self.scope["CHECKS"].items():
            self.assertTrue(self.request(check)["ok"])
            self.assertEqual(self.sent[-1], cmd + "\r")
        count = len(self.sent)
        self.assertFalse(self.request("custom command")["ok"])
        self.text = "Password:"
        self.assertFalse(self.request()["ok"])
        self.text, self.alt = self.prompt, True
        self.assertFalse(self.request()["ok"])
        self.alt = False
        self.assertFalse(self.request(delta=-1)["ok"])
        self.assertEqual(len(self.calls), 0)
        for value in [b"false", b"trueX"]:
            setting.write_bytes(value)
            self.assertFalse(self.request()["ok"])
        self.assertEqual(len(self.calls), 2)
        self.assertEqual(len(self.sent), count)

    def test_refuses_auth_prompt_alt_screen_pending_text_and_expiry(self):
        self.text = "Password:"
        self.assertFalse(self.request("screen")["ok"])
        self.text = self.prompt + " pending"
        self.assertFalse(self.request()["ok"])
        self.text, self.alt = self.prompt, True
        self.assertFalse(self.request()["ok"])
        self.alt = False
        self.assertFalse(self.request(delta=-1)["ok"])
        self.assertEqual(self.calls, [])
        self.assertEqual(self.sent, [])

    def test_selective_policy_is_live_and_optional_checks_require_registration(self):
        policy = Path(self.tmp.name) / "check-policy.json"
        policy.write_text(json.dumps({"approved":["directory","date","ls -la","pwd;date"], "extra":["date","pwd;date"]}))
        self.answer = 7
        self.assertTrue(self.request()["ok"])
        self.assertFalse(self.request("host")["ok"])
        self.assertTrue(self.request("date")["ok"])
        for key in ["ls -la", "pwd;date", "date +%s"]:
            self.assertFalse(self.request(key, clientForm=True, clientApproved=True)["ok"])
        self.assertEqual(self.sent, ["pwd\r", "date\r"])
        self.assertEqual(len(self.calls), 1)
        (Path(self.tmp.name) / "approve-built-in-checks").write_bytes(b"true")
        policy.write_text('{"extra":null}')
        self.assertFalse(self.request("directory")["ok"])
        self.assertFalse(self.request("date")["ok"])
        self.assertEqual(len(self.calls), 2)

    def test_native_confirmation_has_no_duplicate_dialog_or_fallback_on_revoke(self):
        self.answer = 7
        self.assertTrue(self.request("host", clientForm=True, clientApproved=True)["ok"])
        self.assertFalse(self.request("host", clientForm=True)["ok"])
        self.assertFalse(self.request("host", clientForm=True, clientApproved="yes")["ok"])
        self.text = self.prompt + " input"
        self.assertFalse(self.request("host", clientForm=True, clientApproved=True)["ok"])
        self.assertEqual(self.sent, ["hostname\r"])
        self.assertEqual(self.calls, [])

    def test_batch_is_dispatched_as_individual_registered_readonly_checks(self):
        entries = ["free -m", "uname -sr", "free -m;uname -sr", "free /var", "uname --other"]
        (Path(self.tmp.name) / "check-policy.json").write_text(json.dumps({"extra":entries, "approved":entries}))
        self.answer = 7
        for cmd in entries[:2]:
            self.assertTrue(self.request(cmd)["ok"])
        for cmd in entries[2:] + ["uname -a"]:
            self.assertFalse(self.request(cmd)["ok"])
        self.assertEqual(self.sent, ["free -m\r", "uname -sr\r"])
        self.assertEqual(self.calls, [])

    def test_removing_optional_check_while_dialog_open_prevents_dispatch(self):
        policy = Path(self.tmp.name) / "check-policy.json"
        policy.write_text('{"extra":["date"],"approved":[]}')
        def remove(*_):
            policy.write_text('{"extra":[],"approved":[]}')
            return 6
        self.dialog.MessageBox = remove
        self.assertFalse(self.request("date")["ok"])
        self.assertEqual(self.sent, [])

    def test_host_change_or_cancellation_during_approval_prevents_send(self):
        def change(*_):
            self.session.RemoteAddress = "different"
            return 6

        self.dialog.MessageBox = change
        self.assertFalse(self.request()["ok"])
        self.session.RemoteAddress = "test"

        def cancel(*_):
            self.path.unlink()
            return 6

        self.dialog.MessageBox = cancel
        self.assertFalse(self.request()["ok"])
        self.assertEqual(self.sent, [])

    def grant(self, pid, delta=10000):
        key = self.scope["LAUNCH_ID"] = str(uuid.uuid4())
        folder = Path(self.scope["launch_directory"]())
        folder.mkdir()
        (folder / "permit.json").write_text(json.dumps({"pid": pid, "expires": self.scope["clock"]()+delta}))
        return key, folder

    def test_click_attaches_only_its_session_and_preserves_command_approval(self):
        key, _ = self.grant(os.getppid())  # XshellCore child of the selected Xshell.
        seen = {}

        def observe(_ms):
            state = Path(self.tmp.name) / "sessions" / key / "state.json"
            seen.update(json.loads(state.read_text(encoding="utf-8")))
            self.session.Connected = False

        self.session.Sleep = observe
        self.scope["Main"]()
        self.assertTrue(seen["connected"])
        self.assertTrue(seen["clientConfirmation"])
        self.assertEqual(seen["tab"], "fixture")
        self.assertEqual(self.calls, [])
        self.assertFalse(self.scope["consume_launch"]())  # A copied/reopened launch cannot enroll twice.
        self.session.Connected, self.answer = True, 7
        self.assertFalse(self.request()["ok"])
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(self.sent, [])

    def test_expired_or_other_process_ticket_cannot_attach(self):
        self.grant(os.getpid(), -1)
        self.scope["Main"]()
        _, folder = self.grant(-1)
        self.scope["Main"]()
        self.assertEqual(json.loads((folder / "failure").read_text(encoding="utf-8")), "owner")
        self.assertFalse((Path(self.tmp.name) / "sessions").exists())
        self.assertEqual(self.calls, [])

    def test_manual_start_still_asks_and_failed_automatic_login_reports_in_hub(self):
        self.answer = 7
        self.scope["Main"]()
        self.assertEqual(len(self.calls), 1)
        self.assertFalse((Path(self.tmp.name) / "sessions").exists())
        self.session.Connected = False
        _, folder = self.grant(os.getpid())
        self.scope["Main"]()
        self.assertEqual(json.loads((folder / "failure").read_text(encoding="utf-8")), "login")
        self.assertEqual(len(self.calls), 1)


if __name__ == "__main__":
    unittest.main()
