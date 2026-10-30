# Datenformate

Die Objekte, die Befehle zurückgeben und Events mitbringen.

## Kanal

```json
{"index": 0, "name": "R1", "mhz": 5658, "enabled": true,
 "pilot_id": 3, "pilot": "NITRO", "label": "", "shown": "NITRO"}
```

`pilot`: Callsign des zugeteilten Piloten; `label`: Freitext (hat Vorrang); `shown`: was die
Seite anzeigt.

## Session

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

## Durchflug

In `passes` von `sessions.get`:

```json
{"id": 17, "channel": 0, "time_us": …, "peak_db": -21.5, "manual": false,
 "deleted": false, "gate": 0, "rejected": null, "detected_us": …}
```

| Feld | Bedeutung |
|---|---|
| `manual` | von Hand eingefügt (Taster über `passes.add` oder Editor) |
| `deleted` | zählt nicht (gelöscht oder vom Server abgelehnt) |
| `gate` | 0 = Ziel, 1 = Freigabe-Gate (macht keine Runden) |
| `rejected` | vom Server abgelehnt, und warum (`no_arming`); `null` = nicht oder von Hand gelöscht |
| `detected_us` | wo die Erkennung ihn gesetzt hat; `null` = von Hand eingefügt. Anders als `time_us`: von Hand verschoben |

## Korrektur

In `changes` von `sessions.get`, älteste zuerst: jede Änderung an einem Durchflug, auch
während der Session.

```json
{"pass_id": 17, "channel": 0, "action": "restore", "from_us": …, "to_us": null,
 "rejected": "no_arming", "live": true, "origin": "seite", "at_us": …}
```

| Feld | Bedeutung |
|---|---|
| `pass_id` | der Durchflug; kann fehlen, wenn er nicht mehr existiert (von Hand eingefügt und wieder gelöscht). `null`: unbekannt (importiert) |
| `action` | `add` (von Hand eingefügt), `delete`, `restore`, `move`, `reject` (vom Server abgelehnt, z. B. ohne Freigabe) |
| `from_us`, `to_us` | Zeit des Durchflugs vorher und nachher (`move` beide, `add` nur `to_us`) |
| `rejected` | `reject`: der Grund; `restore`: der Grund, den er bis dahin hatte |
| `live` | während die Session lief |
| `origin` | `seite` (Rennleitung), `editor`, `api`, `server` |
| `at_us` | wann |

## state

```json
{"now_us": …, "session": {Session} | null, "unconfirmed": {Session} | null,
 "channels": [Kanal], "pilots": [Pilot], "boards": [{"id", "name", "port", "role", "ok", "error", "levels_per_s"}]}
```

`unconfirmed`: das beendete Rennen, das auf Bestätigung wartet (vorher startet nichts).
