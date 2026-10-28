# Hooks

Der Server startet bei Events Scripts, z. B. um am Pi per GPIO eine LED zu schalten.

**Scripts** liegen in einem Ordner, Standard `<data>/hooks` (also meist `~/.laptimer/data/hooks/`,
legt der Server beim Start an), anders mit `laptimer serve --hooks <ordner>`. Wählbar sind nur
ausführbare Dateien direkt in diesem Ordner (`chmod +x`, mit `#!/bin/sh`, `#!/usr/bin/env
python3` o. ä. in der ersten Zeile). Ein neues Script erscheint ohne Neustart.

**Hooks** legst du auf der Seite *API & Hooks → Hooks* an; sie stehen in der Datenbank
(Tabelle `hooks`). Ein Hook ist:

| Feld | Bedeutung |
|---|---|
| Event | ein Event aus der [Liste](events.md) oder `*` (jedes Event) |
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

Beispiele zum Kopieren in den Script-Ordner: [`tools/hooks/`](https://github.com/bli-blu-bla/fpv-laptimer/tree/main/tools/hooks) (`led.sh` für
GPIO-LEDs mit `pinctrl`, `log.sh`). Eine Ampel am Start z. B. so:

| Event | Script | Argumente | Hintergrund |
|---|---|---|---|
| `session_started` | `led.sh` | `17 on` | |
| `start_signal` | `led.sh` | `17 off` | |
| `start_signal` | `led.sh` | `27 on` | |
| `pilot_pass` | `led.sh` | `27 blink` | ja |
| `session_stopped` | `led.sh` | `27 off` | |
| `*` | `log.sh` | | |

In Python liest der [Client](clients.md) das Event aus der Umgebung (`Event.from_env()`),
Beispiel: `clients/python/examples/hook_autoconfirm.py`.
