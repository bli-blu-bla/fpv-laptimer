# API und Hooks

Andere Programme (Stream-Overlay, Discord-Bot, Taster am Gate, LEDs am Pi …) steuern den
Laptimer über die API unter `/api/v1` und bekommen jedes Ereignis live über einen WebSocket.
Für Scripts direkt auf dem Pi gibt es die Hooks: bei einem Event startet der Server ein
Script.

Auf der Seite **API & Hooks** (Seitenleiste; am Handy unter Einstellungen) legst du Schlüssel
an, findest eine Kurzreferenz, kannst jeden Befehl ausprobieren (HTTP oder WebSocket, mit
Live-Event-Log) und legst Hooks an.

Inhalt: [Schlüssel](#schlüssel) · [Zwei Wege](#zwei-wege-http-und-websocket) ·
[Befehle](#befehle) · [Events](#events) · [Datenformate](#datenformate) · [Hooks](#hooks) ·
[Beispiele](#beispiele)

## Schlüssel

Alles unter `/api/v1` braucht einen API-Schlüssel (`lt_` und 40 Hex-Zeichen). Anlegen auf der
Seite *API & Hooks → Schlüssel*; er wird nur einmal angezeigt (gespeichert ist nur sein
sha256). Löschen sperrt alle Programme, die ihn benutzen.

Mitschicken auf eine dieser Arten:

```
Authorization: Bearer lt_…
X-API-Key: lt_…
?key=lt_…            (z. B. für WebSockets aus dem Browser, der keine Header setzen kann)
```

Ohne gültigen Schlüssel: `401 {"error": "…"}`.

Die Weboberfläche selbst nutzt weiter `/api` ohne Schlüssel. Wer im Netz die Seite öffnen
kann, kann also auch Schlüssel anlegen; der Schlüssel trennt Programme von der Seite, er
schützt nicht vor Leuten im WLAN.

Der Server spricht HTTP, nicht HTTPS (lokales Netz am Platz, ohne Internet). Wer HTTPS
braucht, stellt einen Reverse-Proxy davor (z. B. Caddy).

## Zwei Wege: HTTP und WebSocket

Jede Aktion ist ein **Befehl** mit Namen und JSON-Argumenten. Derselbe Befehl geht auf drei
Arten:

1. **REST**: z. B. `POST /api/v1/session/start` mit `{"race": true}` als Body (Tabelle unten).
2. **Generisch per HTTP**: `POST /api/v1/cmd/<befehl>` mit den Argumenten als Body, z. B.
   `POST /api/v1/cmd/session.start` `{"race": true}`.
3. **WebSocket** `ws://<pi>:5800/api/v1/ws?key=lt_…`:
   ```
   ← {"id": 1, "cmd": "session.start", "args": {"race": true}}
   → {"type": "reply", "id": 1, "ok": true, "data": {…}}
   → {"type": "reply", "id": 2, "ok": false, "status": 400, "error": "Es läuft schon eine Session – erst stoppen."}
   ```
   `id` ist frei wählbar und kommt in der Antwort zurück. `status` bei Fehlern wie per HTTP:
   400 abgelehnt, 404 unbekannter Befehl (auch 400 für eine Nachricht, die kein Befehl ist).

Antworten per HTTP: 200 mit dem Ergebnis als JSON; Fehler 400 `{"error": "…"}` (deutsch, zum
direkten Anzeigen), unbekannter Befehl 404.

**WebSocket-Nachrichten** vom Server (`type`):

| type | wann |
|---|---|
| `hello` | gleich nach dem Verbinden: `{"type":"hello","api":1,"state":{…},"commands":[…]}` (`state` wie der Befehl `state`) |
| `event` | jedes Ereignis, siehe [Events](#events) |
| `reply` | Antwort auf einen Befehl |
| `lagged` | der Client war zu langsam, `missed` Events sind verloren: `state` neu holen |

## Befehle

Kanäle werden über ihren `index` angesprochen (0 = erster Kanal, wie Slot 1 auf der Seite).
Zeiten sind µs (Wanduhr: seit 1970).

| Befehl | REST | Argumente | Ergebnis |
|---|---|---|---|
| `ping` | – | – | `{"pong": true, "now_us"}` |
| `state` | `GET /state` | – | Gesamtzustand ([unten](#state)) |
| `stats` | `GET /stats` | – | Bestzeiten, Rennen, Siege pro Pilot, gesamt und heute |
| `channels.list` | `GET /channels` | – | Liste der [Kanäle](#kanal) |
| `channels.set` | `PATCH /channels/{index}` | `pilot` (Callsign) oder `pilot_id`, `null` = niemand; `create`; `label`; `enabled`; `frequency` | der Kanal |
| `channels.assign` | `POST /channels/assign` | `pilots`: Liste pro Kanal (Callsign, ID oder `null`); `create` | alle Kanäle |
| `pilots.list` | `GET /pilots` | – | `[{"id", "name", "full_name"}]` |
| `pilots.get` | `GET /pilots/{id}` | `id` | Pilot |
| `pilots.create` | `POST /pilots` | `name` (Callsign, eindeutig), `full_name` | Pilot |
| `pilots.update` | `PATCH /pilots/{id}` | `id`, `name`, `full_name` (nur Angegebenes ändert sich) | Pilot |
| `pilots.delete` | `DELETE /pilots/{id}` | `id` | `{}` |
| `session.start` | `POST /session/start` | `race` (Standard `false` = Training); Rennen: `laps` oder `seconds`, `countdown_s`, `start_mode` | die [Session](#session) |
| `session.stop` | `POST /session/stop` | – | `{"session_id"}` |
| `session.cancel` | `POST /session/cancel` | – | `{"session_id"}`; stoppt und löscht die laufende Session |
| `session.confirm` | `POST /session/confirm` | `id` (Standard: das Rennen, das auf Bestätigung wartet) | `{"session_id"}` |
| `session.discard` | `POST /session/discard` | `id` (wie oben) | `{"session_id"}`; löscht die beendete Session |
| `sessions.list` | `GET /sessions?limit=20` | `limit` (Standard 20) | Liste von [Sessions](#session) |
| `sessions.get` | `GET /sessions/{id}` | `id` | Session mit `passes` (alle Durchflüge) |
| `passes.add` | `POST /passes` | `channel` (Index), `time_us` (Standard: jetzt), `session_id` (Standard: die laufende) | `{"pass_id", "session_id", "time_us"}` |
| `passes.delete` | `DELETE /passes/{id}` | `id` | `{"session_id"}`; erkannte werden nur markiert, von Hand eingefügte gelöscht |
| `passes.restore` | `POST /passes/{id}/restore` | `id` | `{"session_id"}`; nur für erkannte (von Hand eingefügte sind nach dem Löschen weg) |

Einzelheiten:

- **`channels.set`**: Nur angegebene Felder ändern sich. Ein Pilot, der schon auf einem anderen
  Kanal fliegt, wird getauscht (der andere Kanal bekommt den bisherigen Piloten), ebenso eine
  Frequenz – wie auf der Seite. `create: true` legt einen unbekannten Callsign als Piloten an
  (sonst Fehler). `label`: Freitext statt eines Piloten. `enabled: false`: der Kanal fliegt
  nicht mit. `frequency`: `"R1"` … `"R8"` oder MHz (5300–6000); nicht während einer Session,
  startet die Boards neu. Einen Piloten setzen schaltet den Kanal ein.
- **`channels.assign`**: `{"pilots": ["NITRO", "ACE", null, 4], "create": true}` – der Reihe
  nach pro Kanal; Kanäle hinter der Liste bleiben. Ein Pilot darf nur einmal vorkommen.
- **`session.start`**: Abgelehnt, wenn eine Session läuft oder ein Rennen noch auf
  Bestätigung wartet. Bei einem Rennen werden angegebene `laps`/`seconds` (1–100 / 10–3600),
  `countdown_s` (0–60) und `start_mode` (`holeshot`, `from_start`, `flying`) als Rennformat
  gespeichert, die Seite zeigt sie dann auch. Ohne Angaben gilt das eingestellte Format.
- **Rennende**: Ein Rennen stoppt von selbst, wenn alle Piloten im Ziel sind (Event
  `session_stopped` mit `reason: "finished"`), und wartet dann auf `session.confirm` oder
  `session.discard`.
- **`passes.add`**: zählt wie ein erkannter Durchflug (Ansage, Event `pilot_pass` mit
  `manual: true`), z. B. von einem Taster. Abgelehnt, wenn er näher als die
  Mindest-Rundenzeit an einem anderen Durchflug desselben Kanals liegt, oder wenn `time_us`
  außerhalb der Session liegt: vor dem Druck auf Start oder nach dem Ende (läuft sie noch:
  mehr als 1 s in der Zukunft). Ohne Boards gut zum Testen.

## Events

Jedes Event kommt über den WebSocket als

```json
{"type": "event", "event": "pilot_pass", "seq": 17, "time_us": 1792697215501133, "data": {…}}
```

`seq` zählt ab dem Serverstart von 1 an; eine Lücke heißt, dass Events verpasst wurden.
`time_us`: wann das Event entstand. Dieselben Events starten die [Hooks](#hooks).

Felder zu einem Piloten (kurz **Sitz**): `channel` (Index), `channel_name` (`"R3"`), `mhz`,
`pilot_id` (oder `null` bei Freitext), `pilot` (angezeigter Name: Callsign, Freitext oder
Kanalname).

| Event | wann | data |
|---|---|---|
| `session_started` | Start gedrückt (vor dem Countdown) | `session_id`, `race`, `start_mode`, `end` (`open`/`laps`/`seconds`), `end_value`, `countdown_s`, `created_us`, `start_us`, `pilot_count`, `seats` (Liste der Sitze) |
| `countdown` | jede Sekunde des Countdowns | `session_id`, `race`, `seconds_left` |
| `start_signal` | Startsignal; Training: gleich nach `session_started`. Kommt immer vor dem ersten `pilot_pass` nach dem Start | `session_id`, `race`, `start_us` |
| `pilot_pass` | ein Pilot fliegt durchs Ziel | Sitz, `session_id`, `race`, `pass_id`, `time_us`, `manual`, `kind`, `lap`, `lap_time_us`, `lap_time_s`, `laps`, `best_lap_us`, `best_lap_s`, `personal_best`, `session_best`, `holeshot_us`, `finished`, `position` |
| `pilot_finished` | ein Pilot hat das Rennende erreicht | Sitz, `session_id`, `laps`, `total_us`, `total_s`, `best_lap_us`, `best_lap_s`, `position` |
| `session_stopped` | Session beendet | `session_id`, `race`, `reason` (`stopped` von Hand, `finished` alle im Ziel, `cancelled` wird gleich gelöscht), `ended_us`, `needs_confirmation`, `winner`, `results` |
| `race_confirmed` | Ergebnis eines Rennens bestätigt | `session_id`, `race`, `confirmed_us`, `winner`, `results` |
| `session_deleted` | Session gelöscht (verworfen, abgebrochen) | `session_id`, `race` |
| `passes_changed` | Durchflüge korrigiert (Editor, Löschen, die Erkennung nimmt einen Fehltreffer zurück) | `session_id` – Ergebnisse per `sessions.get` neu holen |
| `channels_changed` | Kanäle oder ihre Piloten geändert | `channels` (Liste der [Kanäle](#kanal)) |
| `pilot_added`, `pilot_updated` | Pilot angelegt / geändert | `pilot_id`, `name`, `full_name` |
| `pilot_deleted` | Pilot gelöscht | `pilot_id`, `name` |

**`pilot_pass` im Detail**:

- `kind`: `lap` (der Durchflug beendet Runde `lap`, Dauer `lap_time_us`), `holeshot` (erster
  Durchflug nach dem Start bei Holeshot-Start), `start` (erster Durchflug im Training: startet
  die erste Runde), `ignored` (vor dem Startsignal oder nach dem Ziel; zählt nicht).
- `laps`: fertige Runden des Piloten bis jetzt; `finished`: dieser Durchflug war der
  Zieldurchflug.
- `personal_best`: diese Runde ist die beste des Piloten in der Session; `session_best`: die
  beste aller Piloten.
- `position`: Platz jetzt (Rennen: mehr Runden, dann frühere Gesamtzeit; Training: Bestzeit;
  `null` ohne Runde).
- Durchflüge, die die Freigabe-Regel ablehnt, erscheinen nicht (sie sind als gelöscht
  gespeichert).

Beispiel:

```json
{"type": "event", "event": "pilot_pass", "seq": 23, "time_us": 1792697173739494, "data": {
  "session_id": 1, "race": true, "pass_id": 6, "time_us": 1792697173739494, "manual": false,
  "channel": 1, "channel_name": "R3", "mhz": 5732, "pilot_id": 2, "pilot": "ACE",
  "kind": "lap", "lap": 2, "lap_time_us": 13712000, "lap_time_s": 13.712, "laps": 2,
  "best_lap_us": 13104106, "best_lap_s": 13.104, "personal_best": false, "session_best": false,
  "holeshot_us": 2303763, "finished": false, "position": 1}}
```

## Datenformate

### Kanal

```json
{"index": 0, "name": "R1", "mhz": 5658, "enabled": true,
 "pilot_id": 3, "pilot": "NITRO", "label": "", "shown": "NITRO"}
```

`pilot`: Callsign des zugeteilten Piloten; `label`: Freitext (hat Vorrang); `shown`: was die
Seite anzeigt.

### Session

```json
{"id": 4, "race": true, "phase": "running", "start_mode": "holeshot", "end": "laps",
 "end_value": 3, "countdown_s": 5, "min_lap_s": 3.0, "created_us": …, "start_us": …,
 "ended_us": null, "confirmed_us": null, "needs_confirmation": false, "results": [ … ]}
```

`phase`: `countdown`, `running` oder `ended`. `results` – ein Eintrag pro Sitz:

```json
{"channel": 0, "channel_name": "R1", "mhz": 5658, "pilot_id": 3, "pilot": "NITRO",
 "position": 1, "laps": 3, "lap_times_us": [14102000, 13712000, 13950000],
 "last_lap_us": 13950000, "best_lap_us": 13712000, "best_consecutive_us": 41764000,
 "total_us": 44100000, "holeshot_us": 2336000, "finished": true}
```

### state

```json
{"now_us": …, "session": {Session} | null, "unconfirmed": {Session} | null,
 "channels": [Kanal], "pilots": [Pilot], "boards": [{"id", "port", "role", "ok", "error", "levels_per_s"}]}
```

`unconfirmed`: das beendete Rennen, das auf Bestätigung wartet (vorher startet nichts).

## Hooks

Der Server startet bei Events Scripts, z. B. um am Pi per GPIO eine LED zu schalten.

**Scripts** liegen in einem Ordner, Standard `<data>/hooks` (also meist `server/data/hooks/`,
legt der Server beim Start an), anders mit `laptimer serve --hooks <ordner>`. Wählbar sind nur
ausführbare Dateien direkt in diesem Ordner (`chmod +x`, mit `#!/bin/sh`, `#!/usr/bin/env
python3` o. ä. in der ersten Zeile). Ein neues Script erscheint ohne Neustart.

**Hooks** legst du auf der Seite *API & Hooks → Hooks* an; sie stehen in der Datenbank
(Tabelle `hooks`). Ein Hook ist:

| Feld | Bedeutung |
|---|---|
| Event | ein Event aus der [Liste](#events) oder `*` (jedes Event) |
| Script | Dateiname im Script-Ordner |
| Argumente | an Leerzeichen getrennt, ohne Shell (kein `$VAR`, keine Anführungszeichen, kein `&`): `27 on` |
| Im Hintergrund | nicht warten, bis das Script fertig ist |
| An/aus | ausgeschaltete Hooks bleiben eingetragen, laufen aber nicht |

- Das Script läuft **im Script-Ordner** (relative Pfade also von dort aus). Ausgaben landen
  in der Ausgabe des Servers (bei systemd im Journal).
- Die Hooks laufen **nacheinander in der Reihenfolge der Events** (LED an kommt vor LED aus),
  pro Event in der Reihenfolge, in der sie angelegt wurden. Ein Hook darf **10 s** laufen,
  dann wird er abgebrochen. Längeres (Blinken) als *im Hintergrund* eintragen: das läuft
  ohne Zeitlimit neben den anderen her.
- Die Seite zeigt die letzten 30 Läufe (Event, Script, Dauer, Exit-Code, Fehler; Läufe im
  Hintergrund, sobald sie fertig sind). ▷ startet einen Hook sofort einmal, mit den Daten des
  letzten echten Events (oder ohne Daten) und `LAPTIMER_TEST=1`; WebSocket-Clients bekommen
  davon nichts.

**Umgebungsvariablen** für jeden Hook:

| Variable | Inhalt |
|---|---|
| `LAPTIMER_EVENT` | Name des Events, z. B. `pilot_pass` |
| `LAPTIMER_SEQ` | laufende Nummer (0 beim Test über ▷) |
| `LAPTIMER_TIME_US` | Zeit des Events, µs seit 1970 |
| `LAPTIMER_JSON` | das ganze Event als JSON (wie im WebSocket) |
| `LAPTIMER_<FELD>` | jedes einfache Feld aus `data`, Name in Großbuchstaben: `LAPTIMER_PILOT`, `LAPTIMER_CHANNEL_NAME`, `LAPTIMER_LAP`, `LAPTIMER_LAP_TIME_S`, `LAPTIMER_KIND`, `LAPTIMER_POSITION`, `LAPTIMER_WINNER`, `LAPTIMER_SECONDS_LEFT`, … |

`true`/`false` werden `1`/`0`, `null` ein leerer Text. Listen und Objekte (`seats`, `results`,
`channels`) stehen nur in `LAPTIMER_JSON` (z. B. mit `jq` lesen).

Beispiele zum Kopieren in den Script-Ordner: [`tools/hooks/`](../tools/hooks/) (`led.sh` für
GPIO-LEDs mit `pinctrl`, `log.sh`). Eine Ampel am Start z. B. so:

| Event | Script | Argumente | Hintergrund |
|---|---|---|---|
| `session_started` | `led.sh` | `17 on` | |
| `start_signal` | `led.sh` | `17 off` | |
| `start_signal` | `led.sh` | `27 on` | |
| `pilot_pass` | `led.sh` | `27 blink` | ja |
| `session_stopped` | `led.sh` | `27 off` | |
| `*` | `log.sh` | | |

## Beispiele

**curl** (Schlüssel in `K`):

```sh
K=lt_…; B=http://pi:5800/api/v1
curl -H "Authorization: Bearer $K" $B/state
curl -H "Authorization: Bearer $K" -H 'content-type: application/json' \
     -d '{"pilots": ["NITRO", "ACE", "BOB", null], "create": true}' $B/channels/assign
curl -H "Authorization: Bearer $K" -H 'content-type: application/json' \
     -d '{"race": true, "laps": 3, "countdown_s": 5}' $B/session/start
curl -X POST -H "Authorization: Bearer $K" $B/session/confirm
```

**Python** (Paket `websockets`): alle Events mitlesen und ein Rennen starten

```python
import asyncio, json, websockets

async def main():
    async with websockets.connect("ws://pi:5800/api/v1/ws?key=lt_…") as ws:
        hello = json.loads(await ws.recv())
        print("verbunden, Session:", hello["state"]["session"])
        await ws.send(json.dumps({"id": 1, "cmd": "session.start", "args": {"race": True, "laps": 3}}))
        async for text in ws:
            msg = json.loads(text)
            if msg["type"] == "event" and msg["event"] == "pilot_pass" and msg["data"]["kind"] == "lap":
                d = msg["data"]
                print(f'{d["pilot"]}: Runde {d["lap"]} in {d["lap_time_s"]:.2f} s')
            elif msg["type"] == "event" and msg["event"] == "session_stopped" and msg["data"]["race"]:
                await ws.send(json.dumps({"id": 2, "cmd": "session.confirm"}))

asyncio.run(main())
```

**JavaScript** (Browser oder Node ≥ 22):

```js
const ws = new WebSocket('ws://pi:5800/api/v1/ws?key=lt_…')
ws.onmessage = (e) => {
  const m = JSON.parse(e.data)
  if (m.type === 'event') console.log(m.event, m.data)
}
ws.onopen = () => ws.send(JSON.stringify({ id: 1, cmd: 'channels.set', args: { index: 0, pilot: 'NITRO' } }))
```
