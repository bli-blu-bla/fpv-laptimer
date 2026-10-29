"""Client for the FPV lap timer's public API (`/api/v1`, see docs/api/).

Commands go over HTTP with the standard library only (`urllib`), so this
file also works copied on its own, e.g. next to a hook script on the Pi:

    from laptimer_client import Laptimer
    lt = Laptimer("http://pi:5800", key="lt_...")
    lt.assign_channels(["NITRO", "ACE"], create=True)
    lt.start_session(race=True, laps=3)

Events come over the WebSocket `/api/v1/ws` and need the package
`websockets` (`pip install "laptimer-client[events]"`), imported only when
used. Two ways to read them:

    # asyncio
    async for ev in lt.events("pilot_pass"):
        print(ev.data["pilot"], ev.data["lap_time_s"])

    # plain functions, for small scripts
    @lt.on("pilot_pass")
    def lap(ev): ...
    lt.run_forever()

Every command answers with the server's JSON (dicts and lists); errors
raise `LaptimerError` with the server's (German) message.
"""

from __future__ import annotations

import asyncio
import contextlib
import inspect
import itertools
import json
import os
import sys
import traceback
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from typing import Any, AsyncIterator, Callable

# Setzt der Workflow .github/workflows/pypi.yml aus dem Tag py-v<version> ein;
# im Repo bleibt der Platzhalter (ein lokaler Build heißt dann 0.0.0.dev0).
__version__ = "0.0.0.dev0"

__all__ = [
    "Laptimer",
    "Connection",
    "Event",
    "LaptimerError",
    "ConnectionLost",
    "NOBODY",
]


class LaptimerError(Exception):
    """An error from the server (or the connection to it).

    `message` is the server's text (German, meant to be shown as is);
    `status` is the HTTP status (400 bad arguments, 401 bad key, 404
    unknown command; the same over the WebSocket), or None for network errors.
    """

    def __init__(self, message: str, status: int | None = None):
        super().__init__(message)
        self.message = message
        self.status = status

    def __str__(self) -> str:
        return self.message  # meant to be shown as is

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.message!r}, status={self.status!r})"


class ConnectionLost(LaptimerError):
    """The WebSocket could not be opened, or closed (e.g. while a command
    was waiting for its reply)."""


class _Nobody:
    """Type of `NOBODY`."""

    def __repr__(self) -> str:
        return "NOBODY"


# `None` means "leave as it is" (the argument is not sent). To send an
# explicit JSON null, e.g. "no pilot on this channel", pass NOBODY.
NOBODY = _Nobody()


def _clean_args(args: dict[str, Any]) -> dict[str, Any]:
    """Drops None arguments and turns NOBODY into JSON null (None)."""
    return {k: (None if v is NOBODY else v) for k, v in args.items() if v is not None}


@dataclass
class Event:
    """One event from the server.

    `event` is the name (`pilot_pass`, `session_stopped`, ...), `seq` counts
    from 1 since the server started (a gap means missed events), `time_us`
    is when it happened (µs since 1970), `data` the event's fields as in
    docs/api/events.md.

    Two extra names come from the client itself, with `seq` = 0:
    `connected` (after every (re)connect; `data` = the `state`) and
    `lagged` (the client was too slow, `data["missed"]` events are lost).
    """

    event: str
    seq: int = 0
    time_us: int = 0
    data: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_json(cls, msg: dict[str, Any]) -> Event:
        """From a WebSocket message `{"type": "event", ...}`."""
        return cls(
            event=msg.get("event", ""),
            seq=msg.get("seq", 0),
            time_us=msg.get("time_us", 0),
            data=msg.get("data") or {},
        )

    @classmethod
    def from_env(cls) -> Event:
        """The event a hook script was started for (`LAPTIMER_JSON`)."""
        text = os.environ.get("LAPTIMER_JSON")
        if not text:
            raise LaptimerError("LAPTIMER_JSON fehlt: läuft das Script als Hook?")
        return cls.from_json(json.loads(text))


Handler = Callable[[Event], Any]


class _Commands:
    """One method per command. Both `Laptimer` (HTTP, returns the result)
    and `Connection` (WebSocket, returns a coroutine to await) use them;
    each subclass only provides `command(name, **args)`."""

    def command(self, name: str, /, **args: Any) -> Any:  # pragma: no cover
        raise NotImplementedError

    # --- general ---

    def ping(self) -> Any:
        """`{"pong": true, "now_us": ...}`: is the server there, and its time."""
        return self.command("ping")

    def state(self) -> Any:
        """Everything: `session` (running), `unconfirmed` (race waiting for
        confirmation), `channels`, `pilots`, `boards`, `now_us`."""
        return self.command("state")

    def stats(self) -> Any:
        """Best laps, races and wins per pilot, overall and today."""
        return self.command("stats")

    # --- channels ---

    def channels(self) -> Any:
        """All channels: `index`, `name` ("R1"), `mhz`, `enabled`,
        `pilot_id`, `pilot`, `label`, `shown`."""
        return self.command("channels.list")

    def set_channel(
        self,
        index: int,
        pilot: str | int | _Nobody | None = None,
        *,
        create: bool | None = None,
        label: str | None = None,
        enabled: bool | None = None,
        frequency: str | int | None = None,
    ) -> Any:
        """Sets one channel (0 = first); only the given fields change.

        `pilot`: callsign or pilot id, `NOBODY` = no pilot. A pilot already
        on another channel is swapped with this channel's pilot.
        `create=True` makes an unknown callsign a new pilot.
        `label`: free text instead of a pilot ("" = none).
        `enabled=False`: the channel doesn't fly.
        `frequency`: "R1" ... "R8" or MHz (not during a session; restarts
        the boards). Returns the channel.
        """
        args: dict[str, Any] = {"index": index, "create": create, "label": label,
                                "enabled": enabled, "frequency": frequency}
        # A number is a pilot id, text a callsign (bool is an int in Python: not an id).
        if isinstance(pilot, int) and not isinstance(pilot, bool):
            args["pilot_id"] = pilot
        else:
            args["pilot"] = pilot
        return self.command("channels.set", **args)

    def assign_channels(self, pilots: list[str | int | None], create: bool = False) -> Any:
        """Puts pilots on the channels in order: one entry per channel
        (callsign, pilot id or None = nobody). Channels after the list stay
        as they are. Returns all channels."""
        return self.command("channels.assign", pilots=list(pilots), create=create)

    # --- pilots ---

    def pilots(self) -> Any:
        """All pilots: `[{"id", "name", "full_name"}]`."""
        return self.command("pilots.list")

    def pilot(self, id: int) -> Any:
        """One pilot."""
        return self.command("pilots.get", id=id)

    def create_pilot(self, name: str, full_name: str = "") -> Any:
        """A new pilot; `name` is the callsign (unique). Returns the pilot."""
        return self.command("pilots.create", name=name, full_name=full_name)

    def update_pilot(self, id: int, name: str | None = None, full_name: str | None = None) -> Any:
        """Changes the given fields of a pilot. Returns the pilot."""
        return self.command("pilots.update", id=id, name=name, full_name=full_name)

    def delete_pilot(self, id: int) -> Any:
        """Deletes a pilot (old sessions keep the name)."""
        return self.command("pilots.delete", id=id)

    # --- sessions ---

    def start_session(
        self,
        race: bool = False,
        *,
        laps: int | None = None,
        seconds: int | None = None,
        countdown_s: int | None = None,
        start_mode: str | None = None,
    ) -> Any:
        """Starts a training (default) or a race.

        For a race: end after `laps` (1-100) or `seconds` (10-3600),
        `countdown_s` (0-60), `start_mode` ("holeshot", "from_start",
        "flying"); given values are saved as the race format, missing ones
        come from the settings. Refused while a session runs or a race waits
        for confirmation. Returns the session.
        """
        return self.command("session.start", race=race, laps=laps, seconds=seconds,
                            countdown_s=countdown_s, start_mode=start_mode)

    def stop_session(self) -> Any:
        """Ends the running session (a race then waits for confirmation).
        Returns `{"session_id"}`."""
        return self.command("session.stop")

    def cancel_session(self) -> Any:
        """Stops the running session and deletes it."""
        return self.command("session.cancel")

    def confirm(self, id: int | None = None) -> Any:
        """Confirms a race's result (default: the race waiting for it)."""
        return self.command("session.confirm", id=id)

    def discard(self, id: int | None = None) -> Any:
        """Deletes an ended session (default: the race waiting for confirmation)."""
        return self.command("session.discard", id=id)

    def sessions(self, limit: int = 20) -> Any:
        """The newest sessions (with results), newest first."""
        return self.command("sessions.list", limit=limit)

    def session(self, id: int) -> Any:
        """One session with results and all its `passes`."""
        return self.command("sessions.get", id=id)

    # --- passes ---

    def add_pass(self, channel: int, time_us: int | float | None = None,
                 session_id: int | None = None) -> Any:
        """A pass by hand on channel `channel` (index), counted like a
        detected one. `time_us`: µs since 1970 (default: now);
        `session_id`: default the running session. Refused if closer than
        the minimum lap to another pass of the channel.
        Returns `{"pass_id", "session_id", "time_us"}`."""
        if time_us is not None:
            time_us = round(time_us)  # the server wants whole µs
        return self.command("passes.add", channel=channel, time_us=time_us, session_id=session_id)

    def delete_pass(self, id: int) -> Any:
        """Deletes a pass (detected ones are only marked, see restore_pass)."""
        return self.command("passes.delete", id=id)

    def restore_pass(self, id: int) -> Any:
        """Brings back a deleted (detected) pass."""
        return self.command("passes.restore", id=id)


class Laptimer(_Commands):
    """The lap timer at `url`, e.g. `Laptimer("http://pi:5800", key="lt_...")`.

    Without arguments, `url` and `key` come from the environment variables
    `LAPTIMER_API_URL` and `LAPTIMER_API_KEY`. Commands are plain method
    calls over HTTP (blocking, `timeout` seconds); for events see
    `events()`, `connect()` and `on()` / `run_forever()`.
    """

    def __init__(self, url: str | None = None, key: str | None = None, timeout: float = 10.0):
        url = url or os.environ.get("LAPTIMER_API_URL") or "http://localhost:5800"
        key = key or os.environ.get("LAPTIMER_API_KEY")
        if not key:
            raise LaptimerError("API-Schlüssel fehlt (key=... oder LAPTIMER_API_KEY).")
        if "://" not in url:
            url = "http://" + url
        # Accept "http://pi:5800", ".../" and ".../api/v1".
        url = url.rstrip("/")
        if url.endswith("/api/v1"):
            url = url[: -len("/api/v1")]
        self.url = url
        self.key = key
        self.timeout = timeout
        self._handlers: list[tuple[str, Handler]] = []
        self._stopped = False
        self._task: asyncio.Task | None = None  # run_forever's task, for stop()

    def __repr__(self) -> str:
        return f"Laptimer({self.url!r}, key={self.key[:10]!r}...)"

    # --- HTTP ---

    def command(self, name: str, /, **args: Any) -> Any:
        """Runs any command by name (`POST /api/v1/cmd/<name>`), e.g.
        `lt.command("session.start", race=True, laps=3)`. None arguments
        are left out; NOBODY is sent as null."""
        return self._post(f"/api/v1/cmd/{urllib.parse.quote(name)}", _clean_args(args))

    def _post(self, path: str, body: dict[str, Any]) -> Any:
        request = urllib.request.Request(
            self.url + path,
            data=json.dumps(body).encode(),
            method="POST",
            headers={"Authorization": f"Bearer {self.key}", "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                return json.loads(response.read() or b"null")
        except urllib.error.HTTPError as e:
            # The server answers errors with {"error": "..."}.
            text = e.read().decode(errors="replace")
            try:
                message = json.loads(text)["error"]
            except (ValueError, KeyError, TypeError):
                message = text.strip() or e.reason
            raise LaptimerError(str(message), e.code) from None
        except urllib.error.URLError as e:
            raise LaptimerError(f"Laptimer unter {self.url} nicht erreichbar: {e.reason}") from None
        except TimeoutError:
            raise LaptimerError(f"Laptimer unter {self.url} antwortet nicht ({self.timeout} s).") from None

    # --- WebSocket ---

    def ws_url(self) -> str:
        """The WebSocket address (`ws://.../api/v1/ws`)."""
        if self.url.startswith("https://"):
            return "wss://" + self.url[len("https://"):] + "/api/v1/ws"
        return "ws://" + self.url.split("://", 1)[-1] + "/api/v1/ws"

    @contextlib.asynccontextmanager
    async def connect(self) -> AsyncIterator[Connection]:
        """One WebSocket connection, for commands and events (no reconnect):

            async with lt.connect() as conn:
                print(conn.initial_state["session"])
                await conn.start_session(race=True, laps=3)
                async for ev in conn:
                    ...

        Raises LaptimerError with `status` 401 for a wrong key, and
        ConnectionLost if the server can't be reached.
        """
        ws = await _open_websocket(self.ws_url(), self.key)
        try:
            try:
                hello = json.loads(await ws.recv())
            except Exception:
                raise ConnectionLost("Verbindung zum Laptimer getrennt.") from None
            conn = Connection(ws, hello)
            try:
                yield conn
            finally:
                await conn._stop()
        finally:
            await ws.close()

    async def events(self, *names: str, reconnect: bool = True,
                     retry_s: float = 2.0) -> AsyncIterator[Event]:
        """All events (or only those named), forever:

            async for ev in lt.events("pilot_pass", "session_stopped"):
                ...

        First, and after every reconnect, comes a `connected` event with
        the current state in `data` (if not filtered out). After a lost
        connection it tries again every `retry_s` seconds; events in
        between are lost. `reconnect=False`: the loop ends instead (or
        raises ConnectionLost if the server can't be reached). A wrong key
        raises LaptimerError right away.
        """
        wanted = set(names)
        while True:
            try:
                async with self.connect() as conn:
                    ev = Event("connected", data=conn.initial_state)
                    if not wanted or ev.event in wanted:
                        yield ev
                    async for ev in conn:
                        if not wanted or ev.event in wanted:
                            yield ev
            except ConnectionLost:
                if not reconnect:
                    raise
            if not reconnect:
                return
            await asyncio.sleep(retry_s)

    # --- simple handlers for scripts ---

    def on(self, event: str, handler: Handler | None = None) -> Any:
        """Calls `handler(ev)` for every event named `event` ("*" = all),
        once `run_forever()` runs. Also usable as a decorator:

            @lt.on("pilot_pass")
            def lap(ev):
                print(ev.data["pilot"])

        A handler may be `async def`. Inside, the normal (HTTP) methods
        work, e.g. `lt.confirm()`; `lt.stop()` ends `run_forever()`.
        """
        if handler is None:
            return lambda fn: self.on(event, fn)
        self._handlers.append((event, handler))
        return handler

    def stop(self) -> None:
        """Ends `run_forever()`: from a handler after it returns, from
        another thread (e.g. a timer) right away."""
        self._stopped = True
        task = self._task
        if task is not None:
            # call_soon_threadsafe: stop() may come from another thread.
            # (RuntimeError: the loop has just closed, nothing left to stop.)
            with contextlib.suppress(RuntimeError):
                task.get_loop().call_soon_threadsafe(task.cancel)

    def run_forever(self) -> None:
        """Receives events and calls the handlers registered with `on()`,
        until `stop()` or Ctrl+C; reconnects by itself. An exception in a
        handler is printed and the loop goes on."""
        self._stopped = False
        try:
            asyncio.run(self._dispatch())
        except (KeyboardInterrupt, asyncio.CancelledError):
            pass

    async def _dispatch(self) -> None:
        self._task = asyncio.current_task()
        try:
            await self._dispatch_events()
        finally:
            self._task = None

    async def _dispatch_events(self) -> None:
        async for ev in self.events():
            for name, handler in list(self._handlers):
                if name not in ("*", ev.event):
                    continue
                try:
                    result = handler(ev)
                    if inspect.isawaitable(result):
                        await result
                except Exception:
                    print(f"Fehler im Handler für {ev.event}:", file=sys.stderr)
                    traceback.print_exc()
                if self._stopped:
                    return
            if self._stopped:
                return


class Connection(_Commands):
    """An open WebSocket (from `Laptimer.connect()`).

    `hello` is the server's first message, `initial_state` its state at connect,
    `commands` the command names. All command methods work here too, but
    must be awaited: `await conn.start_session(race=True)`. Iterating
    (`async for ev in conn`) gives the events until the connection closes.
    """

    def __init__(self, ws: Any, hello: dict[str, Any]):
        self.ws = ws
        self.hello = hello
        self.initial_state: dict[str, Any] = hello.get("state") or {}
        self.commands: list[str] = hello.get("commands") or []
        self._ids = itertools.count(1)
        self._waiting: dict[int, asyncio.Future] = {}
        self._events: asyncio.Queue[Event | None] = asyncio.Queue()
        # One task reads everything and sorts it: replies to the waiting
        # command, events into the queue. Without it, a command waiting for
        # its reply would have to skip (and lose) the events in between.
        self._reader = asyncio.ensure_future(self._read())

    async def command(self, name: str, /, **args: Any) -> Any:
        """Runs a command over this connection and returns its result."""
        id = next(self._ids)
        reply = asyncio.get_running_loop().create_future()
        self._waiting[id] = reply
        try:
            await self.ws.send(json.dumps({"id": id, "cmd": name, "args": _clean_args(args)}))
            return await reply
        finally:
            self._waiting.pop(id, None)

    async def next_event(self, timeout: float | None = None) -> Event:
        """The next event; ConnectionLost when the connection is closed,
        asyncio.TimeoutError after `timeout` seconds."""
        ev = await asyncio.wait_for(self._events.get(), timeout)
        if ev is None:
            self._events.put_nowait(None)  # stays closed for later calls
            raise ConnectionLost("Verbindung zum Laptimer getrennt.")
        return ev

    async def wait_for(self, *names: str, timeout: float | None = None) -> Event:
        """Skips events until one of `names` comes (asyncio.TimeoutError
        after `timeout` seconds in total)."""

        async def wait() -> Event:
            while True:
                ev = await self.next_event()
                if ev.event in names:
                    return ev

        return await asyncio.wait_for(wait(), timeout)

    def __aiter__(self) -> Connection:
        return self

    async def __anext__(self) -> Event:
        try:
            return await self.next_event()
        except ConnectionLost:
            raise StopAsyncIteration from None

    async def close(self) -> None:
        """Closes the connection."""
        await self.ws.close()

    async def _read(self) -> None:
        try:
            async for text in self.ws:
                msg = json.loads(text)
                kind = msg.get("type")
                if kind == "event":
                    self._events.put_nowait(Event.from_json(msg))
                elif kind == "lagged":
                    self._events.put_nowait(Event("lagged", data={"missed": msg.get("missed", 0)}))
                elif kind == "reply":
                    reply = self._waiting.get(msg.get("id"))
                    if reply is None or reply.done():
                        continue
                    if msg.get("ok"):
                        reply.set_result(msg.get("data"))
                    else:
                        reply.set_exception(LaptimerError(msg.get("error") or "Fehler", msg.get("status")))
        except Exception:
            pass  # closed (normally or not): handled below
        finally:
            for reply in self._waiting.values():
                if not reply.done():
                    reply.set_exception(ConnectionLost("Verbindung zum Laptimer getrennt."))
            self._events.put_nowait(None)

    async def _stop(self) -> None:
        self._reader.cancel()
        with contextlib.suppress(asyncio.CancelledError, Exception):
            await self._reader


# --- websockets (optional, imported only for events) ---


async def _open_websocket(url: str, key: str) -> Any:
    """Opens the WebSocket with the key; errors become LaptimerError."""
    try:
        from websockets.asyncio.client import connect
        from websockets.exceptions import InvalidStatus
    except ImportError:
        raise LaptimerError(
            'Für Events fehlt das Paket "websockets" (ab Version 13): '
            'pip install "laptimer-client[events]" oder pip install websockets'
        ) from None
    try:
        return await connect(url, additional_headers={"Authorization": f"Bearer {key}"},
                             max_size=None, open_timeout=10)
    except InvalidStatus as e:
        # The server refused (401: wrong key); its body is {"error": "..."}.
        status = e.response.status_code
        try:
            message = json.loads(e.response.body)["error"]
        except (ValueError, KeyError, TypeError):
            message = f"WebSocket abgelehnt: {url}"
        raise LaptimerError(message, status) from None
    except Exception as e:  # refused, timeout, DNS, bad handshake ...
        raise ConnectionLost(f"Laptimer unter {url} nicht erreichbar: {e}") from None
