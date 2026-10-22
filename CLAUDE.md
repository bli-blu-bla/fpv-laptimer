# FPV Laptimer: Entwicklernotizen

Rundenzeitmessung für einen FPV-Stammtisch (Nutzerbeschreibung: [README.md](README.md)).
Ersetzt RotorHazard dort, wo es zu viel Verwaltung braucht: Sessions starten/stoppen/speichern,
Kanäle einstellen, Durchflüge nachkorrigieren, Auto-Modi (Top-Runde, beste N in Folge),
möglichst ohne Kalibrierung.

Antworten und Doku auf Deutsch. Der Nutzer kann Python, lernt Rust und schreibt kein
TypeScript: Rust einfach halten (Structs und Funktionen, wenig Generics/Makros, knifflige
Ownership-/async-Stellen kommentieren), Frontend in JavaScript mit JSDoc (`// @ts-check`).

## Aufbau

| Teil | Technik |
|---|---|
| `firmware/` | widscan58 (ESP32-C5 als SDR, 5,8 GHz), per `git subtree` von [CapnBry/widscan58](https://github.com/CapnBry/widscan58), plus `LEVELBATCH`/`TIME?` (unten). GPL-3.0-or-later |
| `server/` | Rust (Crate `laptimer`) auf einem Raspberry Pi 3 (für den Pi auf dem Mac cross-kompilieren, der Pi 3 schafft keinen Rust-Build): ESPs per USB lesen, Durchflüge erkennen, SQLite (Sessions, Durchflüge, Rohpegel), MP3-Ansagen (`mpg123`), Web/WebSocket |
| `frontend/` | React, JavaScript + JSDoc |
| `tools/` | Flash-Script, Python-Scripts zum Messen und Auswerten |

Netz: Pi per LAN am Mini-Router des Nutzers, Handys per WLAN; kein Pi-Hotspot. Bluetooth von
den ESPs geht nicht (das Funkmodul ist durch das Scannen belegt).

Setup am Stammtisch: meist 4 Piloten auf R1, R3, R6, R8; 3–4 ESP32-C5 vorhanden. Geplant:
2 ESPs an den unteren Gate-Ecken (Diversity), optional ein Freigabe-Gate. Erst messen
(1 gegen 2 Boards, automatische Schwellen), dann die Erkennung bauen.

## Firmware

Bauen und flashen: `tools/flash_c5_many.sh` (siehe README). Die vollständige Firmware-Doku
(Befehle, Messwerte, Flash-Probleme, RotorHazard-Modus) steht im Upstream-Repo in
`CLAUDE.md` und `PLAN.md`; hier nur, was der Laptimer braucht.

Die Firmware startet stumm im RotorHazard-Modus. Der Server stellt sie beim Verbinden ein:

```
PROTOCOL MONITOR
CHANNELS R1=5658 R3=5732 R6=5843 R8=5917
METRICS RSSI
FFT 64
CAPN 256
SPEC 0
LEVELBATCH 10
```

- **`LEVELBATCH <ms>`** (1–100, 0 = aus; `LEVELBATCH?`): Mit `METRICS RSSI|SNR` kommen die
  Pegel aller Hops der letzten `<ms>` als ein Frame (Typ 7, `FRAME_LEVEL_BATCH`,
  `firmware/src/proto/frames.h`), höchstens 128 Einträge. Kopf 18 Byte: `u32 seq`,
  `u64 t0_us`, `u16 lost`, `u8 gain`, `u8 mode`, `u16 n`; je Eintrag 8 Byte: `u32 dt_us`,
  `u8 id`, `u8 flags`, `u8 rssi`, `u8 snr`. Zeit = `t0_us + dt_us`, Mitte des Captures.
  Pegelcodes in 0,25 dB: dBFS = code / 4 − 63,75. Jedes `PROTOCOL MONITOR` setzt es auf 0.
- **`TIME?`** antwortet `TIME <us>` (dieselbe Uhr). Der Server nimmt die Antwort mit der
  kürzesten Umlaufzeit, um mehrere Boards abzugleichen.
- Frame-Rahmen: `A5 'R' type version u16 len payload u32 crc32` (Little Endian,
  zlib-CRC über alles davor). Textzeilen (Antworten) stehen zwischen den Frames.

Gemessen (2026-10-22, XIAO ESP32-C5, R1/R3/R6/R8, `LEVELBATCH 10`, auch zwei Boards
gleichzeitig): ~491–493 Pegel/s pro Kanal (alle 2,0 ms, längste Lücke ~3,0 ms), 96 Frames/s,
~18 KB/s, keine Verluste. `TIME?` über USB am Mac: kürzeste Umlaufzeit 0,7–0,9 ms an zwei
USB-Ports, an einem dritten 2,4 ms; also ±0,4 bis ±1,2 ms Unsicherheit.

## Server (`server/`)

Bisher nur das Aufzeichnen für Testflüge (`laptimer record`, `laptimer ports`):

| Datei | Inhalt |
|---|---|
| `src/esp/frame.rs` | Byte-Strom → Frames und Textzeilen (`Parser`), `LevelBatch` |
| `src/esp/board.rs` | `Board::start`: Port öffnen, einstellen (Befehle oben), Uhrabgleich, dann ein Thread pro Board, der `Event`s über einen `mpsc`-Kanal schickt |
| `src/clock.rs` | `ClockSync`: Board-Zeit → Host-Zeit (µs seit Programmstart). Je 10 `TIME?`-Antworten zählt die schnellste; durch diese Schätzungen der letzten 5 min eine Ausgleichsgerade (Drift erst ab 20 s Spanne) |
| `src/record.rs` | CSV schreiben, Live-Anzeige im Terminal |

Threads und `std::sync::mpsc` statt async: einfacher zu lesen. Für den Web-Server kommt
später vermutlich `tokio`/`axum` dazu.

CSV-Format: Kommentarzeilen mit `#` (Boards, Kanäle, Gain), dann
`board,channel,host_us,board_us,rssi,flags`. `host_us` ist für alle Boards dieselbe Uhr.

Gemessen mit zwei Boards (2026-10-22, 40 s, ohne VTX): ~511 Pegel/s pro Kanal und Board,
keine Verluste, Uhr ±0,1 ms (der Uhrabgleich vor dem Scan-Start ist schneller als im
Betrieb), Drift +4 und +8 ppm. In den ersten ~0,2 s nach dem Start fehlen bei einem frisch
gestarteten Board Werte: die Firmware kalibriert jeden LO beim ersten Besuch (~40 ms).

Einmal hingen beide Boards (keine Antwort, auch nicht auf `INFO`), bis sie per RTS
zurückgesetzt wurden; Ursache unbekannt, nicht wieder aufgetreten.

`serialport` ohne `libudev` (`default-features = false`), damit der Cross-Build für den Pi
keine C-Bibliothek braucht; Boards werden am Portnamen erkannt (`/dev/cu.usbmodem*`,
`/dev/ttyACM*`).

**Upstream-Updates holen:** `tools/update_firmware.sh` (zieht `firmware/` aus
CapnBry/widscan58 per `git subtree`).
