# laptimer-client (Python)

Python-Client für die API des Laptimers (`/api/v1`, ausführlich in
[`docs/API.md`](../../docs/API.md)). Befehle laufen über HTTP und brauchen nur die
Standardbibliothek; für Events (WebSocket) braucht es zusätzlich das Paket `websockets`.
Python ab 3.10.

## Installieren

```sh
pip install ./clients/python             # nur Befehle (HTTP)
pip install "./clients/python[events]"   # dazu Events über den WebSocket
```

Oder ohne Installation: `laptimer_client/__init__.py` als `laptimer_client.py` neben das
eigene Script kopieren. Befehle funktionieren dann sofort (auch auf dem Pi in Hook-Scripts);
für Events zusätzlich `pip install websockets` (ab Version 13).

Einen API-Schlüssel legst du auf der Seite *API & Hooks → Schlüssel* an.

## Kurz

```python
from laptimer_client import Laptimer, LaptimerError

lt = Laptimer("http://pi:5800", key="lt_…")
lt.assign_channels(["NITRO", "ACE", "BOB"], create=True)   # Kanal 0, 1, 2
lt.start_session(race=True, laps=3, countdown_s=5)
…
lt.confirm()                                                # Ergebnis bestätigen
```

- Jede Methode gibt das JSON des Servers als `dict`/`list` zurück (Felder wie in
  [`docs/API.md`](../../docs/API.md#datenformate)).
- Argumente mit `None` werden nicht mitgeschickt, d. h. „bleibt, wie es ist“ bzw. „Standard“.
  Ein ausdrückliches `null` (z. B. „kein Pilot auf diesem Kanal“) ist `NOBODY`:
  `lt.set_channel(2, NOBODY)`.
- Fehler werfen `LaptimerError`: `e.message` ist die (deutsche) Meldung des Servers zum
  direkten Anzeigen, `e.status` der HTTP-Status (400 falsche Argumente, 401 Schlüssel
  falsch, 404 unbekannter Befehl; `None` bei WebSocket-Antworten und wenn der Server nicht
  erreichbar ist).
- `Laptimer()` ohne Angaben nimmt `LAPTIMER_API_URL` (Standard `http://localhost:5800`) und
  `LAPTIMER_API_KEY` aus der Umgebung. Die Adresse darf auch `pi:5800` oder `…/api/v1` sein.

## Beispiele

### Rennen mit 3 Runden starten, jede Runde ausgeben, Ergebnis bestätigen

Mit Handler-Funktionen, ohne `asyncio` (`lt.on(...)` und `lt.run_forever()`):

```python
from laptimer_client import Laptimer

lt = Laptimer("http://pi:5800", key="lt_…")

@lt.on("connected")            # nach dem Verbinden: erst dann starten, damit nichts fehlt
def start(ev):
    if ev.data["session"] is None:
        lt.start_session(race=True, laps=3)

@lt.on("pilot_pass")
def lap(ev):
    d = ev.data
    if d["kind"] == "lap":
        print(f'{d["pilot"]}: Runde {d["lap"]} in {d["lap_time_s"]:.2f} s, Platz {d["position"]}')

@lt.on("session_stopped")
def stopped(ev):
    print("Sieger:", ev.data["winner"])
    if ev.data["needs_confirmation"]:
        lt.confirm(ev.data["session_id"])
    lt.stop()                  # run_forever() beenden

lt.run_forever()               # bis lt.stop() oder Strg+C; verbindet sich neu, wenn nötig
```

`"*"` als Event-Name bekommt jedes Event. Handler dürfen auch `async def` sein. Eine
Ausnahme im Handler wird ausgegeben, die Schleife läuft weiter.

### Dasselbe mit asyncio

```python
import asyncio
from laptimer_client import Laptimer

lt = Laptimer("http://pi:5800", key="lt_…")

async def main():
    async for ev in lt.events("pilot_pass", "session_stopped"):
        if ev.event == "pilot_pass" and ev.data["kind"] == "lap":
            print(ev.data["pilot"], ev.data["lap"], ev.data["lap_time_s"])
        elif ev.event == "session_stopped":
            break

asyncio.run(main())
```

`lt.events(*namen)` liefert die Events (ohne Namen: alle) und verbindet sich nach einem
Abbruch selbst neu (`reconnect=False` beendet die Schleife stattdessen).

Befehle über dieselbe WebSocket-Verbindung (ohne Neuverbinden):

```python
async with lt.connect() as conn:
    print(conn.initial_state["session"])         # Zustand beim Verbinden (aus „hello“)
    await conn.start_session(race=True, laps=3)  # alle Methoden wie oben, mit await
    ev = await conn.wait_for("session_stopped", timeout=600)
    await conn.confirm()
    async for ev in conn:                        # alle weiteren Events
        ...
```

### Ein Event ist ein `Event`

| Feld | Inhalt |
|---|---|
| `event` | Name, z. B. `pilot_pass` (Liste in [`docs/API.md`](../../docs/API.md#events)) |
| `seq` | laufende Nummer ab Serverstart (Lücke = Events verpasst) |
| `time_us` | wann, µs seit 1970 |
| `data` | die Felder des Events als `dict` |

Zwei Events kommen vom Client selbst (`seq` = 0): `connected` nach jedem (Neu-)Verbinden,
`data` = der Zustand (wie `state()`), und `lagged`, wenn der Client zu langsam war
(`data["missed"]` Events fehlen; Zustand neu holen).

### Hook-Script

Hooks (Seite *API & Hooks → Hooks*) starten bei einem Event ein Script; das Event steht in
`LAPTIMER_JSON`. `Event.from_env()` liest es:

```python
#!/usr/bin/env python3
from pathlib import Path
from laptimer_client import Event, Laptimer

ev = Event.from_env()
if ev.event == "session_stopped" and ev.data["race"] and ev.data["reason"] == "finished":
    lt = Laptimer("http://localhost:5800", key=Path("api_key.txt").read_text().strip())
    lt.confirm(ev.data["session_id"])     # alle im Ziel: Ergebnis von selbst bestätigen
```

Ins Script-Ordner (Standard `server/data/hooks/`) gehören dann das Script (`chmod +x`),
`laptimer_client.py` und `api_key.txt`. Hooks laufen im Script-Ordner.

Fertige Beispiele in [`examples/`](examples/):

- `race.py`: Piloten setzen, Rennen starten, Countdown und Runden ausgeben, am Ende
  bestätigen oder verwerfen (`python examples/race.py --url http://pi:5800 --key lt_… --laps 3 NITRO ACE`).
- `hook_autoconfirm.py`: Hook, der fertige Rennen bestätigt und den Sieger in eine Datei schreibt.

## Methoden

Alle gibt es auf `Laptimer` (HTTP, blockierend) und auf `Connection` (WebSocket, mit
`await`). Kanäle sind Indizes ab 0, Zeiten µs seit 1970.

| Methode | Befehl | Ergebnis |
|---|---|---|
| `ping()` | `ping` | `{"pong", "now_us"}` |
| `state()` | `state` | laufende Session, `unconfirmed`, Kanäle, Piloten, Boards |
| `stats()` | `stats` | Bestzeiten, Rennen, Siege pro Pilot |
| `channels()` | `channels.list` | Liste der Kanäle |
| `set_channel(index, pilot=None, *, create, label, enabled, frequency)` | `channels.set` | der Kanal. `pilot`: Callsign, Pilot-ID oder `NOBODY`; `frequency`: `"R3"` oder MHz |
| `assign_channels(pilots, create=False)` | `channels.assign` | alle Kanäle. `pilots`: pro Kanal Callsign, ID oder `None` |
| `pilots()` | `pilots.list` | `[{"id", "name", "full_name"}]` |
| `pilot(id)` | `pilots.get` | Pilot |
| `create_pilot(name, full_name="")` | `pilots.create` | Pilot |
| `update_pilot(id, name=None, full_name=None)` | `pilots.update` | Pilot |
| `delete_pilot(id)` | `pilots.delete` | `{}` |
| `start_session(race=False, *, laps, seconds, countdown_s, start_mode)` | `session.start` | Session |
| `stop_session()` | `session.stop` | `{"session_id"}` |
| `cancel_session()` | `session.cancel` | stoppt und löscht |
| `confirm(id=None)` | `session.confirm` | `{"session_id"}`; ohne `id` das wartende Rennen |
| `discard(id=None)` | `session.discard` | löscht die beendete Session |
| `sessions(limit=20)` | `sessions.list` | neueste Sessions |
| `session(id)` | `sessions.get` | Session mit `passes` |
| `add_pass(channel, time_us=None, session_id=None)` | `passes.add` | `{"pass_id", "session_id", "time_us"}` |
| `delete_pass(id)` | `passes.delete` | erkannte werden nur markiert |
| `restore_pass(id)` | `passes.restore` | |
| `command(name, **args)` | jeder | z. B. `lt.command("session.start", race=True)` |

Nur auf `Laptimer`: `events(*namen, reconnect=True, retry_s=2.0)`, `connect()`,
`on(event, handler)`, `run_forever()`, `stop()`, `ws_url()`.
Nur auf `Connection`: `hello`, `initial_state` (Zustand beim Verbinden; `await conn.state()` holt ihn neu), `commands`,
`next_event(timeout=None)`, `wait_for(*namen, timeout=None)`, `close()`;
`async for ev in conn` endet, wenn die Verbindung zu ist.

## Tests

Gegen einen laufenden Server, **nicht den vom Stammtisch** (die Tests starten und löschen
Sessions und legen Piloten an). Ohne `LAPTIMER_API_KEY` legen sie selbst einen Schlüssel an.

```sh
cd clients/python
python3 -m venv /tmp/lt-venv && /tmp/lt-venv/bin/pip install websockets
LAPTIMER_API_URL=http://localhost:5802 /tmp/lt-venv/bin/python -m unittest discover -s tests -v
```

Ohne `websockets` laufen nur die HTTP-Tests, die Event-Tests werden übersprungen.
