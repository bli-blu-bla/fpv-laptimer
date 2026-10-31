# FPV Laptimer

Einfache Rundenzeitmessung für FPV-Flüge im kleinen Rahmen mit Freunden: ESP32-C5-Boards am Gate
messen die Videosignale der Piloten, ein Raspberry Pi erkennt die Durchflüge,
speichert sie und sagt an. Bedienung im Browser, ohne Rennen und Heats anlegen.

Status: im Aufbau. Doku mit Screenshots: **[bli-blu-bla.github.io/fpv-laptimer](https://bli-blu-bla.github.io/fpv-laptimer/)**

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
| `server/` | Rust: der Server (`laptimer serve`), dazu `record`/`replay` für Testflüge |
| `frontend/` | Oberfläche in React mit JavaScript + JSDoc |
| `sounds/` | Fertige Ansage-Bausteine als MP3 (Platzhalter von `tools/make_sounds.sh`, gern durch eigene mit gleichem Namen ersetzen; fehlt einer, wird sein Text gesprochen). Namen und Rundenzeiten spricht die Sprachausgabe: auf dem Pi `sudo apt install libttspico-utils` (oder `espeak-ng`). Einstellungen → Ansagen: welche Ereignisse angesagt werden, und ob der Server oder ein Browser (z. B. Laptop mit Bluetooth-Box) sie abspielt |
| `tools/` | Release-Build, Firmware holen (`fetch_firmware.sh`), Mess- und Auswerte-Scripts |

## Installieren

Auf dem Raspberry Pi (64-bit Pi OS), einem Linux-PC oder dem Mac, als normaler Nutzer:

```sh
curl -fsSL https://github.com/bli-blu-bla/fpv-laptimer/releases/latest/download/install.sh | bash
```

Derselbe Befehl aktualisiert eine installierte Version. Danach Updates auch über die Seite
(Einstellungen → System). Dienst, Optionen, Installation ohne Internet, Releases bauen:
[docs/INSTALL.md](docs/INSTALL.md).

## Firmware flashen

Die Firmware der Boards hat ein eigenes Repo mit eigenen Versionen:
**[fpv-laptimer-firmware](https://github.com/bli-blu-bla/fpv-laptimer-firmware)**. Am
einfachsten auf ihrer Seite
**[bli-blu-bla.github.io/fpv-laptimer-firmware](https://bli-blu-bla.github.io/fpv-laptimer-firmware/)**
(Chrome oder Edge, Board per USB): neueste Firmware aufspielen, Version und Name des Boards
ansehen und setzen, Pegel live, Konsole.

Jeder Laptimer bringt eine feste Firmware-Version mit (`firmware.env`: Version + sha256 des
Release-Pakets). Sie steht unter **Boards → Firmware**:

- **Aufspielen** neben einem Board: Der Server flasht ein Board, das an ihm steckt (z. B. am
  Pi am Gate), auch vom Handy aus. Alle Boards pausieren dafür etwa 15 s; während einer
  Session geht es nicht.
- **Board wählen …**: Ein Board am Rechner mit dem Browser flashen (Web Serial). Das geht nur in
  Chrome oder Edge und nur, wenn die Seite über `localhost` oder HTTPS offen ist, also nicht über
  `http://<pi>:5800`. Läuft auf demselben Rechner `laptimer serve`, hat der Server die Ports
  offen; dann über „Aufspielen“ flashen.

Neben jedem Board steht seine Firmware-Version: „aktuell“, „veraltet“ (älter als die
mitgebrachte) oder „neuer“ (z. B. von der Firmware-Seite). Beim Entwickeln die Firmware für die
Seite holen:

```sh
tools/fetch_firmware.sh                          # die Version aus firmware.env → frontend/public/firmware
tools/fetch_firmware.sh --update                 # neuestes Firmware-Release eintragen und holen
tools/fetch_firmware.sh --from ../firmware/web/public/firmware   # eine selbst gebaute
```

## Zeitmessung starten

```sh
(cd frontend && npm install && npm run build)   # einmal, und nach Änderungen an der Oberfläche
cd server
cargo run --release -- serve                     # alle angesteckten Boards
```

Der Server liest `~/.laptimer/config.toml`. Fehlt die Datei, legt er sie beim ersten Start an,
mit einem zufälligen **Passwort für die Rennleitung**, das er einmal ausgibt (es steht dann in
der Datei). Ohne Anmeldung kann man alles ansehen (Live, Sessions, Bestzeiten, Zuschauer,
Export); Starten, Korrigieren, Piloten, Einstellungen, Boards, API und Hooks brauchen das
Passwort („Anmelden“ oben rechts, das Gerät bleibt 30 Tage angemeldet).

```toml
# ~/.laptimer/config.toml
host = "0.0.0.0"                # Standard "localhost": nur dieser Rechner, nicht die Handys
# port = 5800
# data = "~/.laptimer/data"     # Datenbank, Pegel, Backups, hooks/
[auth]
enabled = true                  # abschalten nur ausdrücklich: enabled = false
password = "…"
```

Die Kommandozeile geht vor (`--host`, `--port`, `--data`, `--web`, `--sounds`, `--hooks`, `--config`).
Die Daten liegen standardmäßig in `~/.laptimer/data`; ältere Daten aus `server/data` mit
`--data data` weiterverwenden (oder den Ordner verschieben).

Dann im Browser `http://<Rechner>:5800` öffnen (auf dem Rechner selbst `http://localhost:5800`),
auch vom Handy im selben WLAN. Training oder Rennen starten, Runden live, Korrekturen per
× (Durchflug löschen) und „+ Durchflug eben verpasst“. Nach einem Rennen das Ergebnis prüfen
und „Ergebnis bestätigen“ (oder verwerfen): erst dann lässt sich der nächste Lauf starten. Unter „Sessions“ → „Pegel“ der
Signalverlauf jedes Piloten: dort Durchflüge antippen zum Löschen oder neben einem Gipfel
tippen zum Einfügen. Die Datenbank
liegt in `~/.laptimer/data/` (dort auch die Pegel jeder Session).

**Export:** unter „Sessions“ eine Session über ⤓ exportieren, oder mit „Auswählen“ mehrere auf
einmal: als CSV (Runden oder Ergebnis, für Excel/LibreOffice) oder JSON, alle Kanäle oder nur
einzelne. **Backup und Neuanfang:** unter „Einstellungen“ → „Daten“. „Backup erstellen“ legt
eine gepackte Kopie der Datenbank nach `~/.laptimer/data/backups/` (dort auch herunterladen).
„Alle Sessions löschen“ löscht alle Trainings und Rennen mit Runden und Pegeln; Piloten,
Einstellungen, API-Schlüssel und Hooks bleiben, vorher wird automatisch ein Backup gemacht.
Ein Backup zurückspielen: Server stoppen, dann
`gunzip -c ~/.laptimer/data/backups/<datei>.db.gz > ~/.laptimer/data/laptimer.db` (vorher die
`laptimer.db-wal` und `laptimer.db-shm` daneben löschen).

Ohne Boards, zum Ausprobieren: `cargo run --release -- serve --replay recordings/<aufnahme>.csv`
spielt eine Aufnahme in Endlosschleife ab.

Entwickeln an der Oberfläche: Server wie oben starten, dann `cd frontend && npm run dev` und
`http://localhost:5173` öffnen (lädt bei jeder Änderung neu). `npm run check` prüft die
JSDoc-Typen.

## API und Hooks

Andere Programme steuern den Laptimer über die API unter `/api/v1` (HTTP oder WebSocket, mit
API-Schlüssel): Piloten anlegen und Kanälen zuteilen, Training/Rennen starten und stoppen,
Ergebnis bestätigen, und jedes Ereignis live mitlesen (`pilot_pass`, `start_signal`,
`race_confirmed`, …). Schlüssel, Kurzreferenz und ein Tester stehen auf der Seite
**API & Hooks**.

Hooks starten bei Ereignissen eigene Scripts mit den Daten als Umgebungsvariablen, z. B. eine
LED am GPIO des Pi beim Startsignal. Die Scripts kommen in den Ordner `~/.laptimer/data/hooks/`
(ausführbar), welches Script bei welchem Ereignis läuft, stellst du auf der Seite ein.
Beispiele in `tools/hooks/`.

Alles im Detail: [docs/api/](docs/api/index.md) (HTTP, WebSocket, Server-Verbindung, Events,
Befehle, Hooks).

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

Die Firmware ist [widscan58](https://github.com/CapnBry/widscan58) von Bryan Mayland
(CapnBry), die den Funkteil des ESP32 als Empfänger für 5,8 GHz nutzt; ohne sie gäbe es den
Laptimer nicht; sie liegt mit unseren Erweiterungen in
[fpv-laptimer-firmware](https://github.com/bli-blu-bla/fpv-laptimer-firmware). widscan58 wiederum baut auf [C5VRX](https://github.com/Twotoz/C5VRX) und den
davon abgeleiteten [esp-sdr](https://github.com/ESPARGOS/esp-sdr)-Projekten auf, die den
versteckten I/Q-Debugpfad des ESP32 gefunden haben. Woher was stammt: [NOTICE.md](NOTICE.md).

## Lizenz

GPL-3.0-or-later, wie die Firmware (siehe `LICENSE`).
