# FPV Laptimer

Einfache Rundenzeitmessung für FPV-Rennen am Stammtisch: ESP32-C5-Boards am Gate
messen die Videosignale der Piloten, ein Raspberry Pi erkennt die Durchflüge,
speichert sie und sagt an. Bedienung im Browser, ohne Rennen und Heats anlegen.

Status: im Aufbau.

## Aufbau

```
 Gate: [ESP links] [ESP rechts]  (+ optional [ESP Freigabe-Gate])
              \        |        /
                    USB
                     |
          Raspberry Pi (Server, Datenbank, Ansagen) --LAN-- Mini-Router --WLAN-- Handys
```

| Ordner | Inhalt |
|---|---|
| `firmware/` | ESP32-C5-Firmware: [widscan58](https://github.com/CapnBry/widscan58) von Bryan Mayland (per `git subtree`) plus unsere Erweiterungen |
| `server/` | Rust: der Server (`laptimer serve`), dazu `record`/`replay` für Testflüge |
| `frontend/` | Oberfläche in React mit JavaScript + JSDoc |
| `sounds/` | Ansagen als MP3 (Platzhalter von `tools/make_sounds.sh`, gern durch eigene ersetzen) |
| `tools/` | Flash-Script, Mess- und Auswerte-Scripts |

## Firmware flashen

```sh
tools/flash_c5_many.sh          # baut einmal, dann ein Board nach dem anderen anstecken
tools/flash_c5_many.sh --all    # alle gerade angesteckten Boards
```

Braucht [PlatformIO](https://platformio.org) (`pio`) und Python 3.

## Zeitmessung starten

```sh
(cd frontend && npm install && npm run build)   # einmal, und nach Änderungen an der Oberfläche
cd server
cargo run --release -- serve                     # alle angesteckten Boards
```

Dann im Browser `http://<Rechner>:5800` öffnen (auf dem Rechner selbst `http://localhost:5800`),
auch vom Handy im selben WLAN. Training oder Rennen starten, Runden live, Korrekturen per
× (Durchflug löschen) und „+ Durchflug eben verpasst“. Unter „Sessions“ → „Pegel“ der
Signalverlauf jedes Piloten: dort Durchflüge antippen zum Löschen oder neben einem Gipfel
tippen zum Einfügen. Die Datenbank
liegt in `server/data/` (dort auch die Pegel jeder Session).

Ohne Boards, zum Ausprobieren: `cargo run --release -- serve --replay recordings/<aufnahme>.csv`
spielt eine Aufnahme in Endlosschleife ab.

Entwickeln an der Oberfläche: Server wie oben starten, dann `cd frontend && npm run dev` und
`http://localhost:5173` öffnen (lädt bei jeder Änderung neu). `npm run check` prüft die
JSDoc-Typen.

## Pegel aufzeichnen (Testflüge)

```sh
cd server
cargo run --release -- record          # alle angesteckten Boards, Strg-C beendet
cargo run --release -- record --seconds 60 --gain 45 --channels R1=5658,R3=5732,R6=5843,R8=5917
cargo run --release -- ports           # welche Boards gefunden werden
```

Zeigt Pegel, erkannte Durchflüge und Rundenzeiten live im Terminal (bei jedem Durchflug
piept es) und schreibt alle Messwerte nach `server/recordings/<Datum_Uhrzeit>.csv`, die
Durchflüge nach `…_passes.csv`. Braucht [Rust](https://rustup.rs).

```sh
cargo run --release -- replay recordings/<datei>.csv            # Erkennung auf einer Aufnahme
cargo run --release -- replay recordings/<datei>.csv --board 0  # nur ein Board
tools/.venv-analysis/bin/python tools/plot_recording.py <datei>.csv --channel R3   # Plot
```

## Danksagung

Die Firmware stammt aus [widscan58](https://github.com/CapnBry/widscan58)
(Bryan Mayland), die den Funkteil des ESP32 als SDR nutzt, über den I/Q-Debugpfad
aus [ESPARGOS/esp-sdr](https://github.com/ESPARGOS/esp-sdr).

## Lizenz

GPL-3.0-or-later, wie die Firmware (siehe `LICENSE`).
