"""Tests against a running lap timer server (not the one at the club!
They start and delete sessions and create pilots).

    LAPTIMER_API_URL=http://localhost:5802 python -m unittest discover -s tests -v

Without LAPTIMER_API_KEY a key named "python-tests" is made over the page
route `/api/keys`. The event tests need `websockets` and are skipped
without it.
"""

from __future__ import annotations

import asyncio
import contextlib
import importlib.util
import io
import json
import os
import sys
import threading
import time
import unittest
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from laptimer_client import NOBODY, ConnectionLost, Laptimer, LaptimerError  # noqa: E402

URL = os.environ.get("LAPTIMER_API_URL", "http://localhost:5802")
HAS_WEBSOCKETS = importlib.util.find_spec("websockets") is not None


def make_key() -> str:
    request = urllib.request.Request(
        URL + "/api/keys",
        data=json.dumps({"name": "python-tests"}).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        return json.load(response)["key"]


KEY = os.environ.get("LAPTIMER_API_KEY") or make_key()


def now_us() -> int:
    return time.time_ns() // 1000


def clean(lt: Laptimer) -> None:
    """No running session, no race waiting for confirmation."""
    state = lt.state()
    if state["session"]:
        lt.cancel_session()
    if lt.state()["unconfirmed"]:
        lt.discard()


class HttpTest(unittest.TestCase):
    def setUp(self) -> None:
        self.lt = Laptimer(URL, key=KEY)
        clean(self.lt)

    def test_url_forms(self) -> None:
        host = URL.split("://", 1)[1]
        for url in (URL, URL + "/", URL + "/api/v1", host):
            self.assertTrue(Laptimer(url, key=KEY).ping()["pong"], url)

    def test_state_and_channels(self) -> None:
        state = self.lt.state()
        for key in ("now_us", "session", "unconfirmed", "channels", "pilots", "boards"):
            self.assertIn(key, state)
        channels = self.lt.channels()
        self.assertEqual([c["index"] for c in channels], list(range(len(channels))))
        self.assertIsInstance(self.lt.stats(), dict)

    def test_pilots(self) -> None:
        name = f"PY{now_us() % 1_000_000}"
        p = self.lt.create_pilot(name, "Test Pilot")
        try:
            self.assertEqual(p["name"], name)
            self.assertEqual(self.lt.pilot(p["id"])["full_name"], "Test Pilot")
            p2 = self.lt.update_pilot(p["id"], full_name="Neu")
            self.assertEqual((p2["name"], p2["full_name"]), (name, "Neu"))
            self.assertIn(p["id"], [x["id"] for x in self.lt.pilots()])
            with self.assertRaises(LaptimerError) as cm:
                self.lt.create_pilot(name)  # callsigns are unique
            self.assertEqual(cm.exception.status, 400)
        finally:
            self.lt.delete_pilot(p["id"])
        self.assertNotIn(p["id"], [x["id"] for x in self.lt.pilots()])

    def test_channels_set_and_assign(self) -> None:
        a, b = f"PYA{now_us() % 100000}", f"PYB{now_us() % 100000}"
        try:
            ch = self.lt.set_channel(0, a, create=True)
            self.assertEqual(ch["pilot"], a)
            pid = ch["pilot_id"]
            self.assertEqual(self.lt.set_channel(1, pid)["pilot"], a)  # by id: swapped
            self.assertIsNone(self.lt.channels()[0]["pilot"])
            self.assertEqual(self.lt.set_channel(1, label="Gast")["shown"], "Gast")
            self.assertIsNone(self.lt.set_channel(1, NOBODY, label="")["pilot"])
            chans = self.lt.assign_channels([a, b], create=True)
            self.assertEqual([c["pilot"] for c in chans[:2]], [a, b])
            chans = self.lt.assign_channels([None, None])
            self.assertEqual([c["pilot"] for c in chans[:2]], [None, None])
            with self.assertRaises(LaptimerError):
                self.lt.set_channel(0, "GIBTSNICHT_" + a)
        finally:
            for p in self.lt.pilots():
                if p["name"] in (a, b):
                    self.lt.delete_pilot(p["id"])

    def test_race_lifecycle(self) -> None:
        s = self.lt.start_session(race=True, laps=2, countdown_s=0, start_mode="flying")
        self.assertTrue(s["race"])
        self.assertEqual((s["end"], s["end_value"]), ("laps", 2))
        with self.assertRaises(LaptimerError):
            self.lt.start_session()  # already running
        t = now_us()
        added = self.lt.add_pass(0, t)
        self.lt.add_pass(0, t + 4_000_000)
        with self.assertRaises(LaptimerError):
            self.lt.add_pass(0, t + 5_000_000)  # closer than the minimum lap
        self.assertEqual(self.lt.stop_session()["session_id"], s["id"])
        self.assertEqual(self.lt.state()["unconfirmed"]["id"], s["id"])
        with self.assertRaises(LaptimerError):
            self.lt.start_session()  # race waits for confirmation

        detail = self.lt.session(s["id"])
        self.assertEqual(len(detail["passes"]), 2)
        self.lt.delete_pass(added["pass_id"])  # manual: removed for good
        self.assertEqual(len(self.lt.session(s["id"])["passes"]), 1)

        self.assertEqual(self.lt.confirm()["session_id"], s["id"])
        self.assertIsNone(self.lt.state()["unconfirmed"])
        self.assertIn(s["id"], [x["id"] for x in self.lt.sessions(limit=5)])
        self.lt.discard(s["id"])
        with self.assertRaises(LaptimerError):
            self.lt.session(s["id"])

    def test_errors(self) -> None:
        with self.assertRaises(LaptimerError) as cm:
            Laptimer(URL, key="lt_falsch").state()
        self.assertEqual(cm.exception.status, 401)
        self.assertIn("Schlüssel", str(cm.exception))
        with self.assertRaises(LaptimerError) as cm:
            self.lt.command("gibt.es.nicht")
        self.assertEqual(cm.exception.status, 404)
        with self.assertRaises(LaptimerError) as cm:
            self.lt.start_session(race=True, laps=1000)
        self.assertEqual(cm.exception.status, 400)
        self.assertIn("laps", cm.exception.message)
        with self.assertRaises(LaptimerError) as cm:
            self.lt.stop_session()
        self.assertEqual(cm.exception.message, "Es läuft keine Session.")
        with self.assertRaises(LaptimerError) as cm:
            Laptimer("http://localhost:1", key=KEY, timeout=2).ping()
        self.assertIsNone(cm.exception.status)


@unittest.skipUnless(HAS_WEBSOCKETS, "websockets fehlt (pip install websockets)")
class EventTest(unittest.TestCase):
    def setUp(self) -> None:
        self.lt = Laptimer(URL, key=KEY)
        clean(self.lt)

    def test_connection_commands_and_events(self) -> None:
        async def main() -> None:
            async with self.lt.connect() as conn:
                self.assertIn("session.start", conn.commands)
                self.assertIn("channels", conn.initial_state)
                self.assertIn("boards", await conn.state())
                s = await conn.start_session()
                t = now_us()
                await conn.add_pass(0, t)
                await conn.add_pass(0, t + 3_500_000)
                with self.assertRaises(LaptimerError) as cm:
                    await conn.command("gibt.es.nicht")
                self.assertIn("Unbekannter Befehl", cm.exception.message)
                await conn.stop_session()

                started = await conn.wait_for("session_started", timeout=5)
                self.assertEqual(started.data["session_id"], s["id"])
                self.assertGreater(started.seq, 0)
                lap = None
                while lap is None:
                    ev = await conn.wait_for("pilot_pass", timeout=5)
                    if ev.data["kind"] == "lap":
                        lap = ev
                self.assertEqual(lap.data["lap"], 1)
                self.assertAlmostEqual(lap.data["lap_time_s"], 3.5)
                self.assertTrue(lap.data["manual"])
                stopped = await conn.wait_for("session_stopped", timeout=5)
                self.assertEqual(stopped.data["reason"], "stopped")
                await conn.discard(s["id"])
                deleted = await conn.wait_for("session_deleted", timeout=5)
                self.assertEqual(deleted.data["session_id"], s["id"])

        asyncio.run(main())

    def test_events_generator_filters(self) -> None:
        async def main() -> list[str]:
            seen = []
            # The commands run in a thread over HTTP, the events arrive here.
            async for ev in self.lt.events("connected", "pilot_added", "pilot_deleted"):
                seen.append(ev.event)
                if ev.event == "connected":
                    threading.Thread(target=self._add_and_remove_pilot).start()
                if ev.event == "pilot_deleted":
                    break
            return seen

        seen = asyncio.run(asyncio.wait_for(main(), 10))
        self.assertEqual(seen, ["connected", "pilot_added", "pilot_deleted"])

    def _add_and_remove_pilot(self) -> None:
        p = self.lt.create_pilot(f"PYEV{now_us() % 100000}")
        self.lt.delete_pilot(p["id"])

    def test_run_forever_handlers(self) -> None:
        got: list[tuple[str, object]] = []

        @self.lt.on("connected")
        def connected(ev):
            s = self.lt.start_session()  # plain HTTP inside a handler
            t = now_us()
            self.lt.add_pass(0, t)
            self.lt.add_pass(0, t + 3_200_000)
            self.lt.stop_session()
            got.append(("start", s["id"]))

        @self.lt.on("pilot_pass")
        async def passed(ev):  # async handlers work too
            got.append((ev.data["kind"], ev.data["lap_time_s"]))

        @self.lt.on("session_stopped")
        def stopped(ev):
            got.append(("stopped", ev.data["reason"]))
            self.lt.discard(ev.data["session_id"])
            self.lt.stop()

        @self.lt.on("*")
        def broken(ev):
            raise RuntimeError("wird nur ausgegeben")  # must not end the loop

        timer = threading.Timer(10, self.lt.stop)  # safety net
        timer.start()
        errors = io.StringIO()
        try:
            with contextlib.redirect_stderr(errors):
                self.lt.run_forever()
        finally:
            timer.cancel()
        self.assertIn("wird nur ausgegeben", errors.getvalue())
        kinds = [g[0] for g in got]
        self.assertEqual(kinds[0], "start")
        self.assertIn(("lap", 3.2), got)
        self.assertEqual(got[-1], ("stopped", "stopped"))

    def test_bad_key_and_unreachable(self) -> None:
        async def bad_key() -> None:
            async with Laptimer(URL, key="lt_falsch").connect():
                pass

        with self.assertRaises(LaptimerError) as cm:
            asyncio.run(bad_key())
        self.assertEqual(cm.exception.status, 401)

        async def unreachable() -> None:
            async for _ in Laptimer("http://localhost:1", key=KEY).events(reconnect=False):
                pass

        with self.assertRaises(ConnectionLost):
            asyncio.run(unreachable())


if __name__ == "__main__":
    unittest.main()
