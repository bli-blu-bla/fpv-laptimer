# Befehle

Jede Aktion ist ein Befehl mit Namen und JSON-Argumenten. Derselbe Befehl geht per
[HTTP](http.md) (REST-Route aus der Tabelle oder `POST /api/v1/cmd/<befehl>`), über den
[WebSocket](websocket.md) (`{"id", "cmd", "args"}`) und über die
[Server-Verbindung](server-verbindung.md). Was zurückkommt, beschreiben die
[Datenformate](datenformate.md).

Kanäle werden über ihren `index` angesprochen (0 = erster Kanal, wie Slot 1 auf der Seite).
Zeiten sind µs (Wanduhr: seit 1970). Die REST-Routen gelten ab `/api/v1`.

| Befehl | REST | Argumente | Ergebnis |
|---|---|---|---|
| `ping` | – | – | `{"pong": true, "now_us"}` |
| `state` | `GET /state` | – | Gesamtzustand ([unten](datenformate.md#state)) |
| `stats` | `GET /stats` | – | Bestzeiten, Rennen, Siege pro Pilot, gesamt und heute |
| `channels.list` | `GET /channels` | – | Liste der [Kanäle](datenformate.md#kanal) |
| `channels.set` | `PATCH /channels/{index}` | `pilot` (Callsign) oder `pilot_id`, `null` = niemand; `create`; `label`; `enabled`; `frequency` | der Kanal |
| `channels.assign` | `POST /channels/assign` | `pilots`: Liste pro Kanal (Callsign, ID oder `null`); `create` | alle Kanäle |
| `pilots.list` | `GET /pilots` | – | `[{"id", "name", "full_name"}]` |
| `pilots.get` | `GET /pilots/{id}` | `id` | Pilot |
| `pilots.create` | `POST /pilots` | `name` (Callsign, eindeutig), `full_name` | Pilot |
| `pilots.update` | `PATCH /pilots/{id}` | `id`, `name`, `full_name` (nur Angegebenes ändert sich) | Pilot |
| `pilots.delete` | `DELETE /pilots/{id}` | `id` | `{}` |
| `session.start` | `POST /session/start` | `race` (Standard `false` = Training); Rennen: `laps` oder `seconds`, `countdown_s`, `start_mode` | die [Session](datenformate.md#session) |
| `session.stop` | `POST /session/stop` | – | `{"session_id"}` |
| `session.cancel` | `POST /session/cancel` | – | `{"session_id"}`; stoppt und löscht die laufende Session |
| `session.confirm` | `POST /session/confirm` | `id` (Standard: das Rennen, das auf Bestätigung wartet) | `{"session_id"}` |
| `session.discard` | `POST /session/discard` | `id` (wie oben) | `{"session_id"}`; löscht die beendete Session |
| `sessions.list` | `GET /sessions?limit=20` | `limit` (Standard 20) | Liste von [Sessions](datenformate.md#session) |
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
  Mindest-Rundenzeit an einem anderen Durchflug desselben Kanals liegt (gegen prellende
  Taster; `passes.restore` und die Korrektur in der Seite prüfen nur auf 0,1 s), oder wenn `time_us`
  außerhalb der Session liegt: vor dem Druck auf Start oder nach dem Ende (läuft sie noch:
  mehr als 1 s in der Zukunft). Ohne Boards gut zum Testen.
