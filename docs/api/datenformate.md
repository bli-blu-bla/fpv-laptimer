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

## state

```json
{"now_us": …, "session": {Session} | null, "unconfirmed": {Session} | null,
 "channels": [Kanal], "pilots": [Pilot], "boards": [{"id", "name", "port", "role", "ok", "error", "levels_per_s"}]}
```

`unconfirmed`: das beendete Rennen, das auf Bestätigung wartet (vorher startet nichts).
