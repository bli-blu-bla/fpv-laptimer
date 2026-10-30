# Events

Ein Event sagt: am Platz ist gerade etwas passiert. Eine Session startet, der Countdown
läuft, ein Pilot fliegt durchs Ziel, ein Ergebnis wird bestätigt, ein Pilot wird angelegt.
Jedes Event geht gleichzeitig

- an alle Programme am [WebSocket](websocket.md) `/api/v1/ws`,
- über die [Server-Verbindung](server-verbindung.md) (falls eingerichtet; ohne Verbindung
  bis zu 5 min gepuffert),
- an die [Hooks](hooks.md), die auf dieses Event (oder `*`) eingestellt sind.

Events sind Mitteilungen, keine Befehle: man kann nichts darauf antworten. Wer reagieren
will (z. B. ein Rennen bestätigen), schickt einen [Befehl](befehle.md).

## Aufbau

```json
{"type": "event", "event": "pilot_pass", "seq": 17, "time_us": 1792697215501133, "data": {…}}
```

| Feld | Inhalt |
|---|---|
| `type` | immer `"event"` (unterscheidet sie von `hello`, `reply` im WebSocket) |
| `event` | Name, siehe [unten](#alle-events) |
| `seq` | laufende Nummer, ab dem Start des Laptimers von 1 an. Steigt sie um mehr als 1, fehlen Events. Nach einem Neustart des Laptimers beginnt sie wieder bei 1 |
| `time_us` | wann das Event entstand (µs seit 1970, Uhr des Timers) |
| `data` | die Felder des Events, je nach Event |

`time_us` ist der Zeitpunkt der *Meldung*, nicht unbedingt des Geschehens: Ein Durchflug wird
erst gemeldet, wenn die Erkennung sicher ist (etwa 1 s, nachdem die Drohne das Gate verlassen
hat). Die Zeit des Durchflugs selbst steht in `data.time_us`, und mit der rechnen die Runden.

### Sitz

Viele Events betreffen einen Piloten auf einem Kanal. Diese Felder heißen hier kurz **Sitz**
und stehen direkt in `data`:

| Feld | Inhalt |
|---|---|
| `channel` | Index des Kanals (0 = Slot 1) |
| `channel_name` | `"R3"` |
| `mhz` | `5732` |
| `pilot_id` | ID des Piloten, `null` bei einem Gast (Freitext) |
| `pilot` | angezeigter Name: Callsign, Freitext oder Kanalname |

## Ablauf eines Rennens

So sieht ein Rennen über 2 Runden mit Holeshot-Start, 3 s Countdown und zwei Piloten aus
(vereinfacht):

| seq | event | data (Auszug) | was passiert |
|---|---|---|---|
| 1 | `session_started` | `race: true`, `countdown_s: 3`, `seats: [ACE, NITRO]` | Start gedrückt |
| 2 | `countdown` | `seconds_left: 3` | |
| 3 | `countdown` | `seconds_left: 2` | |
| 4 | `countdown` | `seconds_left: 1` | |
| 5 | `start_signal` | `start_us` | Startton |
| 6 | `pilot_pass` | ACE, `kind: "holeshot"` | erster Durchflug: die Zeit läuft |
| 7 | `pilot_pass` | NITRO, `kind: "holeshot"` | |
| 8 | `pilot_pass` | ACE, `kind: "lap"`, `lap: 1`, `lap_time_s: 13.1`, `position: 1` | |
| 9 | `pilot_pass` | NITRO, `kind: "lap"`, `lap: 1`, `lap_time_s: 13.8`, `position: 2` | |
| 10 | `pilot_pass` | ACE, `kind: "lap"`, `lap: 2`, `finished: true` | ACE im Ziel … |
| 11 | `pilot_finished` | ACE, `position: 1`, `total_s` | … und als eigenes Event |
| 12 | `pilot_pass` | NITRO, `kind: "lap"`, `lap: 2`, `finished: true` | |
| 13 | `pilot_finished` | NITRO, `position: 2` | |
| 14 | `session_stopped` | `reason: "finished"`, `needs_confirmation: true`, `winner: "ACE"`, `results` | alle im Ziel: das Rennen stoppt von selbst |
| 15 | `race_confirmed` | `winner: "ACE"`, `results` | jemand bestätigt (Seite oder `session.confirm`) |

Darauf kann man sich verlassen:

- `session_started` kommt vor allem anderen der Session, `start_signal` vor dem ersten
  `pilot_pass` nach dem Start.
- Der `pilot_pass`, der einen Piloten ins Ziel bringt (`finished: true`), kommt vor seinem
  `pilot_finished`, und alle `pilot_finished` vor dem `session_stopped` mit `reason: "finished"`.
- Ein beendetes Rennen wartet auf `race_confirmed` oder `session_deleted`; vorher startet keine
  neue Session.

Andere Fälle:

- **Training**: kein Countdown; `start_signal` kommt gleich nach `session_started`. Der erste
  Durchflug jedes Piloten ist `kind: "start"`, danach Runden. Es endet nur von Hand
  (`session_stopped` mit `reason: "stopped"`, `needs_confirmation: false`); bestätigt wird nichts.
- **Von Hand gestoppt**: `session_stopped` mit `reason: "stopped"`; ein Rennen braucht danach
  trotzdem Bestätigung.
- **Abgebrochen** (Seite: „Abbrechen“, API `session.cancel`): `session_stopped` mit
  `reason: "cancelled"`, gleich danach `session_deleted`.
- **Verworfen** (beendetes Rennen nicht bestätigt): `session_deleted`.

## Korrekturen

Ein `pilot_pass` ist eine Live-Meldung. Er kann später noch falsch werden:

- Die Erkennung nimmt nach dem 3. Durchflug eines Kanals frühe Fehltreffer zurück.
- Die Rennleitung löscht, verschiebt oder ergänzt Durchflüge im Editor, auch nach dem Rennen.
- Ein Programm ruft `passes.add`, `passes.delete` oder `passes.restore`.

Dann kommt **`passes_changed`** mit der `session_id`, ohne Einzelheiten: Runden, Plätze und
Bestzeiten können sich danach alle verschoben haben. Wer ein Ergebnis anzeigt, holt die Session
neu (`sessions.get`). Deshalb:

- Für Live-Anzeigen (Overlay, Ansage) `pilot_pass` nehmen.
- Für das, was zählt (Tabelle, Website, Rangliste), das Ergebnis aus `race_confirmed`, oder
  nach `passes_changed` neu holen. `session_stopped` hat auch ein Ergebnis, aber vor der
  Durchsicht der Rennleitung.

Durchflüge, die die Freigabe-Regel ablehnt ([Zwei Gates](../funktionen/gates.md)), erscheinen
gar nicht als `pilot_pass`; sie sind als gelöscht gespeichert, mit `rejected: "no_arming"` in
den `passes` von `sessions.get` (von Hand gelöschte haben `rejected: null`). Durchflüge am Freigabe-Gate
selbst sind keine Events.

## Alle Events

### Sessions

| Event | wann | data |
|---|---|---|
| `session_started` | Start gedrückt (vor dem Countdown) | `session_id`, `race`, `start_mode`, `end` (`open`/`laps`/`seconds`), `end_value`, `countdown_s`, `created_us` (Druck auf Start), `start_us` (Startsignal), `pilot_count`, `seats` (Liste der [Sitze](#sitz)) |
| `countdown` | jede volle Sekunde des Countdowns | `session_id`, `race`, `seconds_left` (z. B. 5, 4, … 1) |
| `start_signal` | Startsignal; im Training gleich nach `session_started` | `session_id`, `race`, `start_us` |
| `session_stopped` | Session beendet | `session_id`, `race`, `reason`, `ended_us`, `needs_confirmation`, `winner`, `results` |
| `race_confirmed` | Ergebnis eines Rennens bestätigt | `session_id`, `race`, `confirmed_us`, `winner`, `results` |
| `session_deleted` | Session gelöscht (verworfen, abgebrochen, auf der Seite gelöscht) | `session_id`, `race` |
| `sessions_reset` | alle Sessions gelöscht (Einstellungen → System; Piloten bleiben) | `deleted` (Anzahl) |

- `start_mode`: `holeshot` (Zeit läuft ab dem ersten Durchflug), `from_start` (ab dem
  Startsignal), `flying` (fliegender Start: der erste Durchflug startet die erste Runde).
- `reason`: `stopped` (von Hand), `finished` (alle im Ziel), `cancelled` (wird gleich gelöscht).
- `needs_confirmation`: `true` bei einem Rennen, das nicht abgebrochen wurde.
- `winner`: Name des Ersten (wie `pilot`), `null` im Training oder ohne Runde.
- `results`: ein Eintrag pro Sitz, wie in [Datenformate → Session](datenformate.md#session).

### Durchflüge

| Event | wann | data |
|---|---|---|
| `pilot_pass` | ein Pilot fliegt durchs Ziel (erkannt oder von Hand) | [Sitz](#sitz), `session_id`, `race`, `pass_id`, `time_us`, `manual`, `kind`, `reason`, `lap`, `lap_time_us`, `lap_time_s`, `laps`, `best_lap_us`, `best_lap_s`, `personal_best`, `session_best`, `holeshot_us`, `finished`, `position` |
| `pilot_finished` | ein Pilot hat das Rennende erreicht | [Sitz](#sitz), `session_id`, `laps`, `total_us`, `total_s`, `best_lap_us`, `best_lap_s`, `position` |
| `passes_changed` | Durchflüge korrigiert ([oben](#korrekturen)) | `session_id` |

**`pilot_pass` im Detail:**

| Feld | Inhalt |
|---|---|
| `pass_id` | ID des Durchflugs (für `passes.delete`) |
| `time_us` | Zeit des Durchflugs |
| `manual` | von Hand: Taster über `passes.add` oder Editor |
| `kind` | `lap`: beendet Runde `lap`. `holeshot`: erster Durchflug bei Holeshot-Start. `start`: erster Durchflug im Training oder bei fliegendem Start, startet die erste Runde. `ignored`: zählt nicht (Grund in `reason`) |
| `reason` | nur bei `kind: "ignored"`, sonst `null`: `before_start` (vor dem Startsignal; im Training vor dem Start der Session) oder `after_finish` (der Pilot ist schon im Ziel) |
| `lap`, `lap_time_us`, `lap_time_s` | Nummer und Dauer der Runde, nur bei `kind: "lap"`, sonst `null` |
| `laps` | fertige Runden des Piloten bis jetzt |
| `best_lap_us`, `best_lap_s` | seine beste Runde in dieser Session |
| `personal_best` | diese Runde ist seine beste in der Session |
| `session_best` | diese Runde ist die beste aller Piloten in der Session |
| `holeshot_us` | Zeit vom Startsignal bis zum ersten Durchflug (Holeshot), sonst `null` |
| `finished` | dieser Durchflug war der Zieldurchflug |
| `position` | Platz jetzt (Rennen: mehr Runden, dann frühere Gesamtzeit; Training: Bestzeit; `null` ohne Runde) |

Beispiel:

```json
{"type": "event", "event": "pilot_pass", "seq": 23, "time_us": 1792697174751210, "data": {
  "session_id": 1, "race": true, "pass_id": 6, "time_us": 1792697173739494, "manual": false,
  "channel": 1, "channel_name": "R3", "mhz": 5732, "pilot_id": 2, "pilot": "ACE",
  "kind": "lap", "reason": null, "lap": 2, "lap_time_us": 13712000, "lap_time_s": 13.712, "laps": 2,
  "best_lap_us": 13104106, "best_lap_s": 13.104, "personal_best": false, "session_best": false,
  "holeshot_us": 2303763, "finished": false, "position": 1}}
```

`total_us` in `pilot_finished`: Zeit vom Startsignal bis zum Zieldurchflug (beim Holeshot
also mit dem Weg bis zum ersten Durchflug); beim fliegenden Start `null`.

### Kanäle und Piloten

| Event | wann | data |
|---|---|---|
| `channels_changed` | Kanäle, ihre Frequenzen oder Piloten geändert (Seite oder API) | `channels` (Liste der [Kanäle](datenformate.md#kanal)) |
| `pilot_added` | Pilot angelegt | `pilot_id`, `name`, `full_name` |
| `pilot_updated` | Pilot geändert | `pilot_id`, `name`, `full_name` |
| `pilot_deleted` | Pilot gelöscht (er verlässt auch seinen Slot: danach `channels_changed`) | `pilot_id`, `name` |

## Testen

Auf der Seite *API & Hooks → Testen* läuft ein Log aller Events mit. Ohne Boards geht es
auch: Session starten und Durchflüge mit `passes.add` eintragen; die zählen wie erkannte
(`manual: true`).
