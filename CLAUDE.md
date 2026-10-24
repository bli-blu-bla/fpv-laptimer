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
| `server/` | Rust (Crate `laptimer`, Server `laptimer serve`) auf einem Raspberry Pi 3 (für den Pi auf dem Mac cross-kompilieren, der Pi 3 schafft keinen Rust-Build): ESPs per USB lesen, Durchflüge erkennen, SQLite (Sessions, Durchflüge, Rohpegel), MP3-Ansagen (`mpg123`), Web/WebSocket |
| `frontend/` | React, JavaScript + JSDoc |
| `tools/` | Flash-Script, `stage_firmware.py` (Firmware für die Seite), `build_release.sh` (Release für den Pi), Python-Scripts zum Messen und Auswerten |
| `.forgejo/workflows/` | CI auf der privaten Forgejo-Instanz (`git.homeserver.local`, Runner `ubuntu-latest`, Container `node:22-bookworm`, Rust per rustup): `ci.yml` testet jeden Push, `release.yml` baut bei Tag `v*` (Doku: `docs/RASPBERRY.md`) |

**Raspberry Pi** (`docs/RASPBERRY.md`): 64-bit Pi OS Lite, Dienst als systemd-User-Unit,
alles in `~/.laptimer`. `tools/build_release.sh [VERSION] [--no-firmware]` baut Firmware,
Seite und Server (`cargo zigbuild --target aarch64-unknown-linux-gnu.2.36`, also glibc ≥ 2.36)
und packt `dist/laptimer-<ver>-aarch64.tar.gz` (`bin/`, `web/` mit Firmware, `sounds/`,
`hooks-examples/`, `deploy/*`, `VERSION`, `minisign.pub`) + `.sha256`. `deploy/install.sh`
(als Nutzer, nicht root): sudo nur für Fehlendes (Pakete, Gruppen `dialout`/`audio`, Linger,
`/etc/sudoers.d/laptimer` nur für `systemctl poweroff/reboot`; geprüft mit `sudo -n -l`, weil
`sudoers.d` nicht lesbar ist), Release nach `~/.laptimer/releases/<ver>`, Symlinks `current`
und `previous` (atomar per `mv -T`), `config.toml` beim ersten Mal (Passwort fragen oder
zufällig), drei Releases behalten, User-Unit `deploy/laptimer.service` (`Restart=always`:
ein Update beendet den Server, systemd startet das neue `current`); `--rollback`,
`--uninstall`. Getestet im Container mit Attrappen für sudo/systemctl/loginctl, nicht auf
einem echten Pi. Updates gehen auch über die Seite (Einstellungen → System, `src/update.rs`):
Paket + `.minisig` hochladen oder von Forgejo holen (`[update] url`/`token` in `config.toml`),
signiert geprüft, dann ersetzt der Server `current` selbst und beendet sich; kommt die neue
Version nicht hoch, setzt `deploy/update-check.sh` (`ExecStartPre` der Unit) beim 4. Start
zurück. Die CI signiert mit
minisign (Secrets `RELEASE_SIGNING_KEY`, `RELEASE_SIGNING_PASSWORD`; das Passwort liest
minisign ohne Terminal aus stdin) und lädt per Forgejo-API (`secrets.GITHUB_TOKEN`) ins
Release. Gemessen 2026-10-23 lokal im selben Container: ganzer Build ~3 min ohne Cache
(Firmware 2:17), Paket 4,8 MB, die Binary braucht höchstens `GLIBC_2.34`. Der Plan mit den
weiteren Phasen steht in `todo.md`.

Netz: Pi per LAN am Mini-Router des Nutzers, Handys per WLAN; kein Pi-Hotspot. Bluetooth von
den ESPs geht nicht (das Funkmodul ist durch das Scannen belegt).

Setup am Stammtisch: meist 4 Piloten auf R1, R3, R6, R8; 3–4 ESP32-C5 vorhanden. Geplant:
2 ESPs an den unteren Gate-Ecken (Diversity), optional ein Freigabe-Gate. Erst messen
(1 gegen 2 Boards, automatische Schwellen), dann die Erkennung bauen.

## Firmware

Bauen und flashen: `tools/flash_c5_many.sh` (siehe README), oder über die Web-Seite (unten,
„Firmware über die Seite“). Die vollständige Firmware-Doku
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

`laptimer serve` (Port 5800; 8080/8090 belegt Docker auf dem Mac des Nutzers; `host`/`--host`
Standard `localhost`, die angelegten Configs von `install.sh` und `config.rs` setzen `0.0.0.0`) liest
`~/.laptimer/config.toml` (`config.rs`; unbekannte Einträge nur mit Warnung, Crate `serde_ignored`; fehlt sie, wird sie mit zufälligem Passwort angelegt;
`--config` für eine andere, die dann existieren muss). Reihenfolge: Kommandozeile, Config,
Standard. `--data` (Standard `~/.laptimer/data`: SQLite `laptimer.db` +
`levels/session_<id>.csv`), `--web`/`--sounds` (Standard `web/`/`sounds/` neben `bin/` des
installierten Release, sonst `../frontend/dist`/`../sounds` für `cargo run`), `--hooks`
(Standard `<data>/hooks`), `--replay <csv>` (Aufnahme in Schleife statt Boards; zum Testen
`--sounds` auf einen leeren Ordner, sonst piept der Mac). Zum Testen eine eigene Config mit
`--config` und `--data` in einen Scratch-Ordner, sonst schreibt der Test in `~/.laptimer`.
Version: `build.rs` setzt `LAPTIMER_VERSION` (aus der Umgebung, also der CI, sonst
`git describe`), `main.rs::VERSION`, in `state.version`, im API-`hello` und in den
Einstellungen.

| Datei | Inhalt |
|---|---|
| `src/api.rs` | Öffentliche API `/api/v1` (Doku: `docs/API.md`): jede Aktion ist ein Befehl (`command(app, name, args)`, z. B. `session.start`), erreichbar als REST-Route, als `POST /api/v1/cmd/<name>` oder über den WebSocket `/api/v1/ws` (`{"id","cmd","args"}` → `reply`; vorher `hello` mit `state`, dann jedes Event). Schlüssel-Prüfung als Middleware (`Authorization: Bearer`, `X-API-Key` oder `?key=`; in der DB nur sha256, Tabelle `api_keys`). Befehle laufen in `spawn_blocking`. Für die Seite ohne Schlüssel: `/api/keys` (GET/POST/DELETE), `/api/hooks` (GET: Ordner, Scripts, Hooks, letzte Läufe; POST), `/api/hooks/{id}` (PUT/DELETE), `/api/hooks/{id}/test`. Die Weboberfläche selbst nutzt weiter `/api` ohne Schlüssel |
| `src/auth.rs` | Passwort der Rennleitung (`[auth]` in `config.toml`: `enabled` Standard an, ohne `password` startet der Server nicht; abschalten nur mit `enabled = false`). Middleware `require_login` auf `/api`: Lesen frei, außer `/keys`, `/hooks`, `/backups`; alles andere als GET/HEAD braucht ein Login (nicht `/v1`, das hat API-Schlüssel; nicht `/login`, `/logout`, `/auth`). `POST /api/login {password}` → Cookie `laptimer_login` (HttpOnly, SameSite=Strict, 30 Tage; in der DB nur sha256, Tabelle `logins`), `POST /api/logout`, `/api/logout/all`, `GET /api/auth` (`{enabled, logged_in}`). Falsches Passwort: 1 s warten, nach 5 in Folge 1 min Sperre pro IP (`ConnectInfo`, daher `into_make_service_with_connect_info`). 401 mit `login: true` lässt die Seite den Login-Dialog öffnen |
| `src/events.rs` | `Events::emit(name, data)`: Event `{"type":"event","event","seq","time_us","data"}` an den API-WebSocket (eigener `broadcast`) und an die Hooks. Erzeugt in `core.rs`: `session_started`, `countdown`/`start_signal` (in `tick`), `pilot_pass` (`report_passes`: neue, nicht gelöschte Durchflüge der laufenden Session mit `kind` lap/holeshot/start/ignored), `pilot_finished`, `session_stopped` (`end_session(reason)`), `race_confirmed`, `session_deleted`, `sessions_reset`, `passes_changed` (Korrekturen, Zurücknahme), `channels_changed`, `pilot_added/updated/deleted` (Piloten deshalb über `Core::add_pilot` usw.) |
| `src/data.rs` | Export, Backup, Reset (ohne Schlüssel, für die Seite): `GET /api/export?sessions=1,2&channels=0,2&content=laps\|results&format=csv\|json` (eine Datei für eine oder mehrere Sessions, alle oder einzelne Kanäle; CSV mit `;`, Dezimalkomma und BOM für Excel; JSON mit `core::standings` je Session). `POST /api/backups` (`Db::copy_to` = `VACUUM INTO` unter der Core-Sperre, dann gzip ohne sie nach `<data>/backups/laptimer_<datum>_<zeit>.db.gz`), `GET /api/backups`, `GET`/`DELETE /api/backups/{name}`. `POST /api/reset`: erst Backup (`…_vor-reset`), dann `Core::reset_sessions` (alle Sessions mit Sitzen, Durchflügen, Pegel-Dateien; Event `sessions_reset`) und den Graph-Cache leeren; Piloten, Settings, Schlüssel, Hooks bleiben. **Debug-Paket** `GET /api/export/debug?session=3&channels=0,2` (Export-Dialog → Debug, eine Session): `.tar.gz` mit `session.json` (Config, Sitze, aktuelle Settings, alle Durchflüge auch gelöschte, Ergebnis, Version), `levels.csv` (Rohpegel der gewählten Kanäle, mit `laptimer replay` neu auswertbar), `trace.csv` (je 10-ms-Slot Hüllkurve kombiniert und je Board, Enter/Exit), `detector_events.csv` (Neuberechnung mit den Session-Einstellungen, auch Zurücknahmen), `README.txt`; im Speicher gebaut |
| `src/hooks.rs` | Hooks: Scripts im Ordner `--hooks` (Standard `<data>/hooks`, nur ausführbare Dateien direkt darin, `check_script`), Zuordnung in der DB (Tabelle `hooks`: Event oder `*`, Script, Argumente an Leerzeichen getrennt ohne Shell, Hintergrund, an/aus), im Speicher gehalten (`set_list` nach jeder Änderung). Ein Thread führt sie nacheinander aus (im Script-Ordner, 10 s Limit; Hintergrund: eigener Thread wartet, ohne Limit), Umgebung `LAPTIMER_EVENT/SEQ/TIME_US/JSON` und `LAPTIMER_<FELD>` für jedes einfache Feld; die letzten 30 Läufe für die Seite. Beispiele in `tools/hooks/` |
| `src/server.rs` | axum: `/api/state`, `/api/session/start {race}`, `/api/session/stop`, `/api/sessions[/{id}]` (GET, DELETE), `/api/sessions/{id}/confirm` (Ergebnis eines Rennens bestätigen), `/api/sessions/{id}/passes {channel,time_us}` (von Hand), `/api/passes/{id} {deleted}`, `/api/settings` (GET/PUT; Kanäle oder Gain ändern startet die Boards neu), `POST /api/boards/rescan` (Ports neu suchen und alle Boards neu starten; nicht während Session oder Flash), `/api/pilots[/{id}]`; `/ws` (erst `state`, dann `state`/`live`/`session_changed`); sonst `frontend/dist`. Threads: Boards → std-Kanal → Core-Thread (`on_board_event`, `tick` alle 10 ms); `Core` in einem `std::sync::Mutex`, nie über ein `.await` halten |
| `src/core.rs` | `Core`: Settings (DB-Key `settings`), aktive Session, Durchflüge → DB → `results` → `state`-Nachricht; Ansagen (`refresh`, `schedule_start`; Bausteine siehe `sound.rs`); `live` mit 10 Hz. Zeiten in der DB: Wanduhr-µs (`origin_unix_us + host_us`) |
| `src/laps.rs` | Runden aus Durchflügen (rein, getestet): Start `holeshot`/`from_start`/`flying`, Ende `open`/`laps`/`seconds`, beste Runde, beste N in Folge, ignorierte Durchflüge |
| `src/db.rs` | Tabellen `pilots`, `sessions` (Config als JSON, damit alte Ergebnisse bleiben; `confirmed_us`, siehe unten), `seats`, `passes` (`manual`, `deleted` fürs Rückgängigmachen; vom Detektor zurückgenommene werden gelöscht), `settings`, `api_keys` (Name, Präfix, sha256), `hooks`. Session-IDs werden nie wiederverwendet (Setting `last_session_id`; SQLite nähme nach dem Löschen die höchste wieder): Entwürfe im Browser und Export-Dateien hängen an der ID |
| `src/trace.rs` | Pegelgraph einer Session: die Pegel-Datei noch einmal durch den Detektor (`Detector::with_trace` zeichnet pro 10-ms-Slot Hüllkurve kombiniert, je Board, Eintritt, Austritt auf), dann `graph` (Zeitbereich auf N Punkte, je Punkt das Maximum, damit kein Gipfel verloren geht) und `snap` (höchster Punkt ±1 s, von dort ±50 ms-weise bergauf, Buckel bis 3 dB darunter, darin Mitte zwischen erstem und letztem Punkt innerhalb 1 dB, wie der Detektor). Im Server zwischengespeichert pro Session und Dateigröße (höchstens 4); gelesen ohne die Core-Sperre (`spawn_blocking`). `/api/sessions/{id}/graph?channel&from_us&to_us&points`, `/api/sessions/{id}/snap?channel&time_us`; `POST …/passes` mit `snap: true`; `POST …/edit {channel, moves: [[id, t]], deletes, restores, adds: [t]}` (alles in einer Transaktion, `Db::edit_passes`; verschobene Durchflüge gelten danach als von Hand; abgelehnt, wenn zwei Durchflüge näher als die Mindestrunde liegen) |
| `src/stats.rs` | Bestzeit, Sessions, Rennen, Siege pro Pilot (nach `pilot_id`, sonst nach Label), gesamt und heute; `race_order` (mehr Runden, dann frühere Gesamtzeit). `/api/stats` |
| `src/source.rs` | Boards oder Replay (Replay liefert die Pegel nach Kanal-Position der Aufnahme, nicht nach Frequenz: nach einem Kanaltausch passen sie nicht mehr) |
| `src/update.rs` | Updates über die Seite, nur aus einem installierten Release (`<root>/releases/<ver>/bin/laptimer`, `<root>/current` vorhanden; sonst `installed: false`). Zwei Schritte: **Bereitstellen** (`POST /api/update/upload` multipart `package` + `signature`, bis 64 MB; oder `GET /api/update/releases` + `POST /api/update/download {tag}` von Forgejo per `curl`, Token über stdin, Anhänge über `/attachments/<uuid>`): Signatur gegen den eingebauten `minisign.pub` (`include_str!`) oder `[update] extra_keys` (Crate `minisign-verify`), entpacken nach `<root>/staging/`, `VERSION`, ELF-Architektur von `bin/laptimer`. **Installieren** (`POST /api/update/install {allow_older}`): keine Session, kein Flash; Backup `…_vor-update`, nach `releases/<ver>`, `update-pending` (`to`, `from`, `previous`, `attempts`) schreiben, `previous`/`current` umsetzen (Symlink + rename), geänderte Unit nach `~/.config/systemd/user` + `daemon-reload`, aufräumen (3 behalten, zuerst eine nicht hochgekommene), nach 0,5 s `exit(0)`. Älter/gleich/unklar (`compare`: Zahlen, dann ohne Zusatz > `-rc1`, sonst unklar) nur mit `allow_older`. Die neue Version löscht `update-pending` nach 15 s; `update-check.sh` zählt die Starts und setzt beim 4. zurück (`update-failed`, auf der Seite mit `POST /api/update/dismiss` ausgeblendet). `POST /api/update/rollback` (`current` ↔ `previous`), `DELETE /api/update/staged`, `POST /api/system/restart` (nur unter systemd, `INVOCATION_ID`), `/api/system/reboot`, `/api/system/poweroff` (`sudo -n /usr/bin/systemctl …`, Regel von `install.sh`). Alles unter `/update` und `/system` braucht das Login, auch GET |
| `src/sound.rs` | Ansage = Liste von `Part`s (Clip `sounds/<name>.mp3` mit Ersatztext, oder Text), nacheinander in eigenem Thread. Texte per Sprachausgabe als WAV in `<data>/tts/<hash>.wav` (beim Start geleert; `say -v Anna` auf macOS, sonst `pico2wave`, ersatzweise `espeak-ng`); Pilotennamen beim Session-Start vorab (`prepare`, eigener Thread, damit der Countdown nicht wartet). Ausgabe Server (`afplay`, sonst `mpg123`/`aplay`) oder Browser: WebSocket `announce {items: [{url} \| {text}]}`, Dateien unter `/api/audio/clips/…` und `/api/audio/tts/…`; `POST /api/announce/test` |

**Boards und Gates:** Jedes Board hat eine feste Kennung, die USB-Seriennummer (= MAC,
`find_ports`; macOS über `serialport`, Linux aus `/dev/serial/by-id`; Replay: `replay-N`).
`settings.board_roles` ordnet sie zu: `finish` (Standard), `arming` (Freigabe-Gate) oder
`off`. `Core` hat zwei `Detector`: Ziel und Freigabe; jedes Board speist den seiner Rolle,
mehrere Boards pro Gate werden kombiniert (Maximum). Freigabe-Regel: Gibt es ein
funktionierendes Freigabe-Board, zählt ein Ziel-Durchflug nur, wenn seit dem letzten
gezählten Durchflug des Kanals ein Freigabe-Durchflug kam; der erste nach dem Start zählt
immer. Abgelehnte werden als `deleted` gespeichert (im Editor wiederherstellbar).
Freigabe-Durchflüge stehen mit `gate = 1` in `passes` (Spalte wird in alten Datenbanken
ergänzt), machen keine Runden (`laps::compute` filtert `gate == 0`) und erscheinen im
Editor als gelbe Punktlinien. Die Pegel-Datei einer Session hat `# roles: 0=finish 1=arming`;
der Pegelgraph nimmt nur die Ziel-Boards. `live.boards[].level_db`: Pegel pro Board zum
Zuordnen; `live.arming`: Status des Freigabe-Gates.

**Erkennungsmodus pro Kanal und Gate** (`settings.detection`, `detect::DetectionSettings`):
`finish[i]` / `arming[i]` haben `enter_db`, `exit_db`, `mode` (`auto` = gelernte
Schwellen, `classic` = feste Enter/Exit; `null` = der Gate-Modus `finish_mode` /
`arming_mode`) und `min_rise_db` (`null` = der gemeinsame `min_rise_db`; `min_rise()` →
`Detector::set_min_rise`). `fixed()` liefert pro Kanal `Some((enter, exit))` oder `None` für
`Detector::set_fixed` (feste Kanäle ohne Anlernen und Zurücknehmen). Eingestellt in der
Rennleitung (Slot antippen); „Alle automatisch / Alle manuell“ auf der Boards-Seite setzt den
Gate-Modus und löscht die Kanal-Modi. Ältere Einstellungen mit einem `mode` werden als `finish_mode`
gelesen (`serde(alias)`). Die Session speichert die Einstellung in `config.detection`, damit
ihr Graph dieselben Schwellen zeigt.

**Presets** (pro Pilot mehrere, z. B. eine pro Drohne; Tabelle `presets`: `pilot_id`, Name
eindeutig pro Pilot, JSON `PresetValues` mit `mode`, `enter_db`, `exit_db`, `min_rise_db`,
`gain`): die Ziel-Gate-Erkennung eines Kanals. `seats[].preset_id` sagt, welches zuletzt in
den Kanal kopiert wurde. `Core::apply_presets` in `set_settings`: wechselt der Pilot eines
Slots, bekommt er sein Preset, wenn er genau eins hat (sonst keins; Freitext nie); ändert
sich `preset_id`, werden die Werte nach `detection.finish[i]` kopiert (Kopie, kein Verweis:
Nachstellen ändert das Preset nicht). Ohne Preset behält der Kanal seine Werte (Gäste).
`POST /api/presets {pilot_id, name, slot}` (aus dem aktuellen Kanal, Modus und
Mindestausschlag aufgelöst, plus Gain), `PUT /api/presets/{id} {name?, slot?}`,
`DELETE` (Slots behalten die Werte); Piloten löschen löscht ihre Presets (Cascade).
`state.presets`. Feste Schwellen hängen am Aufbau und Gain: die Seite weist auf einen anderen
Gain hin; beobachten, ob das am Stammtisch passt.

**Ergebnis bestätigen:** Ein beendetes Rennen muss bestätigt (`POST /api/sessions/{id}/confirm`,
`sessions.confirmed_us`) oder verworfen werden, bevor eine neue Session startet
(`start_session` lehnt sonst ab). `state.unconfirmed` ist das neueste unbestätigte Rennen; die
Rennleitung zeigt es auch nach dem Neuladen. Trainings brauchen keine Bestätigung; Sessions aus
Datenbanken von vor der Spalte gelten als bestätigt. Gedacht als Haken für spätere Automatisierungen.

Laps werden nie gespeichert, nur Durchflüge: jede Korrektur rechnet alles neu. Ein von
Hand eingefügter oder wiederhergestellter Durchflug näher als die Mindestrunde an einem
anderen wird abgelehnt. Ein Rennen stoppt von selbst, wenn alle Slots im Ziel sind.
Piloten: `name` ist der Callsign (eindeutig), `full_name` optional (Spalte wird in alten
Datenbanken beim Öffnen ergänzt). Settings: `seats[].enabled` (freie Slots fehlen in der
Session), `sound` (alle Ansagen an/aus), `announce` (`output` server/browser; Ereignisse
`start`, `lap` = off/beep/number/time, `best` nur im Training, `finished`, `race_over`; das
frühere `lap_callout` wird nicht übernommen). Fehlermeldungen der API sind deutsch, die Seite zeigt
sie direkt an. Löschen
markiert erkannte Durchflüge (wiederherstellbar), von Hand eingefügte werden entfernt. Beim
Start schließt der Server Sessions, die ein früherer Lauf offen gelassen hat
(`close_open_sessions`: Ende = letzter Durchflug).

Einrasten gemessen an der Aufnahme 2026-10-22_15-12-35: Tipp bis ±1 s neben dem Gipfel
landet auf dem Zeitpunkt der Erkennung (bei einem Durchflug 165 ms daneben: dort hat der
Detektor zwei Buckel innerhalb der Mindestrunde zusammengelegt), weiter weg 0,1–0,4 s
daneben oder an einem anderen Buckel; deshalb zeigt die Seite die Stelle vor dem Einfügen.
Der Graph einer 2,4-min-Session braucht auf dem Mac ~80 ms (erstes Mal), auf dem Pi 3
vermutlich ein Vielfaches: für lange Sessions später den Verlauf beim Beenden speichern.

## Frontend (`frontend/`)

Vite + React, JavaScript mit JSDoc und `// @ts-check`; `npm run check` (tsc über
`jsconfig.json`, TypeScript nur als Prüfer). `src/types.js` spiegelt das JSON des Servers.

Design nach dem Entwurf „FPV Race Control“ des Nutzers (Claude Design, Design-System
„Nocturne“): **nur dunkel**, Akzent orange `#ff8a3d`, Schrift Inter und Icons (Phosphor)
als npm-Pakete gebündelt (am Platz gibt es kein Internet). Feste Farbe pro Slot
(`slotColor`). Regeln des Nutzers: keine überladene UX; Einstellungen dort, wo man sie
braucht; nicht Zusammengehöriges auf getrennten Seiten.

| Datei | Inhalt |
|---|---|
| `App.jsx` | Navigation mit 8 Seiten (Seitenleiste ab 820 px, sonst Tab-Leiste unten mit Kurznamen), Kopf mit „Heute · N Rennen“, Board-Status, Ton an/aus; lädt `/api/stats`. `?view=zuschauer`: nur die Zuschauer-Ansicht, ohne Navigation (Fernseher/Tablet) |
| `pages/Control.jsx` | **Rennleitung**: Kopf mit Status, Uhr, Training / Rennen starten (+ Zahnrad → `RaceDialog`), während des Laufs Stopp / Abbrechen (löscht); nach dem Ende darunter eine Karte mit Verwerfen und „Ergebnis bestätigen“ (Rennen; bis dahin sind die Start-Knöpfe gesperrt), danach Schließen. Karten (`Lane`): Name antippen → `PilotDialog`, Kanal antippen → `ChannelDialog`; während des Laufs `PassLog`. Nach dem Ende: Ergebnis (`Ranking`) und darunter `PassEditor` (Nachbearbeitung) |
| `pages/Boards.jsx` | **Boards**: „Boards neu suchen“ (später eingesteckte ESPs), Rolle pro Board (Ziel / Freigabe / Aus) mit Live-Pegel, darunter Ziel- und Freigabe-Gate mit dem Modus jedes Kanals und „Alle automatisch / Alle manuell“ |
| `components/ThresholdPanel.jsx` | Erkennung eines Kanals in der Rennleitung (wie RotorHazards Node-Anzeige): Slot-Karte antippen, darunter Gate-Wahl, Automatisch/Manuell für diesen Kanal, Pegelverlauf der letzten 30 s mit Enter/Exit-Linien (im manuellen Modus ziehbar, Maus und Touch, 0,5 dB, Exit bleibt ≥ 0,5 dB unter Enter, gespeichert beim Loslassen; Werte als HTML-Beschriftung, weil SVG-Text mit `preserveAspectRatio="none"` verzerrt), Regler (0,5 dB, Speichern verzögert) und „Aus Spitze“ (Enter = Spitze − 4 dB, Exit = − 9 dB). Ein automatisches Gate zeigt seine gelernten Linien und den Mindestausschlag des Kanals (zurück auf den Standard). Am Ziel-Gate `PresetRow`: Preset des Piloten im Slot, „geändert“ + „In … speichern“, „Neues Preset“ |
| `pages/Watch.jsx` | **Zuschauer**: laufende oder letzte Session, Karten ohne Bedienung und ohne Signalverlauf, Rangliste, Countdown |
| `pages/Best.jsx` | **Bestzeiten** heute / gesamt: Bestzeit, beste N in Folge, Rennen, Siege |
| `components/PassEditor.jsx` | Korrektur wie RotorHazards Marshal-Seite: Kanal oben wählen, darunter großer Graph (340 px, Übersicht + Ausschnitt). Durchflug ziehen = verschieben, antippen = auswählen (Löschen/Wiederherstellen, am Gipfel einrasten), freie Stelle antippen = neu (per `/snap` am Gipfel), Fläche ziehen = Ausschnitt verschieben. Entwurf pro Kanal im Browser; Runden-Vorschau mit `computeLaps` (zu kurze Runden rot, dann kein Speichern); „Speichern“ schickt pro Kanal ein `POST /api/sessions/{id}/edit` |
| `components/PilotDialog.jsx` | Wer fliegt (Tipp auf den Namen in der Karte; Entwurf „Rennleitung: Pilot & Kanal“ auf Claude Design): Liste der Piloten (ab 6 mit Suche und Filter Alle / Ohne Slot / Heute dabei, nach `stats.today`; der aktuelle bleibt immer sichtbar; alphabetisch, Ring in der Slot-Farbe, „Slot N · tauschen“), „Kein Pilot“ (= `enabled: false`, ersetzt den Schalter „Slot belegt“), Gast ohne Profil (Freitext). Antippen speichert und schließt; Pilot aus einem anderen Slot wird getauscht, samt Preset. Hat der Pilot mehrere Presets, bleibt der Dialog offen und zeigt sie („Keins“) |
| `components/ChannelDialog.jsx` | Kanal eines Slots (Tipp auf den Kanal in der Karte): aktueller Kanal mit IMD aller Kanäle, Band R/A/B/E/F/L (öffnet beim Band des aktuellen, Punkt markiert es), acht Kanäle mit „Slot N“ bei belegten (Tausch), freie MHz. Während einer Session gesperrt |
| `components/RaceDialog.jsx` | Start (Holeshot / Ab Startton / Fliegend), Ende nach Runden oder Zeit, Countdown aus/3/5/10 s |
| `pages/Sessions.jsx` | Liste + Detail (`Ranking`); „Runden bearbeiten“ blendet die Liste aus und zeigt den `PassEditor` in voller Breite; Löschen; Export (⤓ im Detail, oder „Auswählen“ in der Liste für mehrere) |
| `components/ExportDialog.jsx` | Format CSV/JSON, Inhalt Runden/Ergebnis, Kanäle (alle oder einzelne); lädt per `fetch` (Fehler im Dialog) und speichert unter dem Namen aus `Content-Disposition` |
| `pages/Pilots.jsx` | Callsign + Name, PB, Rennen, Siege; Dialog zum Anlegen/Bearbeiten/Löschen, darin die Presets (umbenennen, löschen; angelegt werden sie in der Rennleitung) |
| `pages/Api.jsx` | **API & Hooks** (am Handy nicht in der Tab-Leiste, sondern über einen Knopf in den Einstellungen): Reiter Schlüssel (anlegen, einmal anzeigen, löschen), Doku, Testen (Befehl per HTTP oder WebSocket, curl-Zeile, Event-Log; Schlüssel in `localStorage`), Hooks (anlegen/bearbeiten mit Event, Script aus dem Ordner, Argumente, Hintergrund; an/aus, einmal ausführen, letzte Läufe; alle 2 s neu geladen). Befehle und Events beschreibt `apiCatalog.js` |
| `auth.js`, `components/LoginDialog.jsx` | Login-Zustand für die ganze Seite (`useAuth()` → `canEdit`); `api()` meldet ein 401 mit `login` (`loginNeeded`), dann öffnet `App.jsx` den Dialog. Ohne Login: Kopf „Anmelden“, Rennleitung ohne Start/Stopp/Zahnräder/Schwellen/Nachbearbeitung, Sessions ohne Löschen/Bearbeiten, Piloten nur ansehen, Boards/Einstellungen/API gesperrt (`Locked`) |
| `pages/Settings.jsx` | Nach dem Entwurf „Einstellungen“ (Claude Design, ohne die Auswertungs-Profile Indoor/Outdoor/Whoop). Drei Tabs (`components/Tabs.jsx`: unterstrichene Reiter wie bei API & Hooks, Warnpunkt, rechts „Gespeichert“): Messung, Ansagen, System; je Tab die Karten nebeneinander (am Handy untereinander). **Messung:** Kanäle (Leiste mit den Frequenzen, IMD-Bewertung, eine Zeile pro Slot mit Warnung unter 30 MHz Abstand, Slot entfernen; „Kanäle wählen“ → `ChannelPicker`), Auswertung (`Stepper box`: Mindestrunde, Mindestausschlag, Gain mit 0,8 s Verzögerung, weil er die Boards neu startet, beste N in Folge), Vorschau (`DetectPreview`: erfundenes Signal mit Normalpegel und Schwelle + Mindestausschlag). **Ansagen:** Ereignisse mit Icons, Ausgabe, Beispiel + „Probe-Ansage“. **System:** `components/SystemPanel.jsx` (Version, Update hochladen oder bei Forgejo suchen, bereitgestellte Version mit Warnung bei älter/gleich, Installieren; zurück auf die vorige Version, Laptimer/Pi neu starten, herunterfahren, jeweils mit Rückfrage und gesperrt während einer Session; nach dem Neustart lädt sich die Seite selbst neu), Zugang, Backups (Jetzt sichern, Herunterladen/Löschen), Gefahrenzone („Alle Sessions löschen“ mit Rückfrage im Knopf, gesperrt während einer Session) |
| `components/ChannelPicker.jsx` | Kanalwahl wie widscan58 („Channels to scan“): Zeilen R, A, B, E, F auf einer Achse 5630–5960 MHz, L darunter mit eigener Achse; Kanal = Glocke (antippen), Bandname wählt alle acht, gestrichelte Linien zeigen die gewählten Frequenzen in den anderen Bändern, Farben = Slot, den der Kanal bekommt. Darunter IMD-Bewertung mit den schlimmsten Mischprodukten (rote Marken auf der Achse), „Optimal verteilen“, freie MHz. Am Handy seitlich scrollbar (SVG mindestens 760 px) |
| `imd.js` | `BANDS`, `channelName`, `imdRating` (wie ET's IMDTabler, den RotorHazard als Java aufruft: für jedes Paar 2·f1 − f2, Abstand zum nächsten gewählten Kanal, unter 35 MHz kostet (35 − Abstand)², Summe / Anzahl / 5 von 100 abgezogen; Produkte über 6100 MHz zählen nicht; geprüft gegen etserv.etheli.com/IMDTabler mit Raceband −635, IMD 6C 29, ET5 97, Racebnd4 100), `bestSet(n)` (alle Sätze mit ≥ 35 MHz Abstand durchprobieren: beste Bewertung, dann meiste Raceband-Kanäle, dann größter Abstand; ein paar zehn ms), `assignSlots` (Slots behalten ihre Frequenz, sonst bekommen sie an ihrer Stelle eine neue; Sitz und `detection.finish/arming` wandern mit) |
| `announcer.js` | Ansagen im Browser: spielt `announce`-Nachrichten nur, wenn dieser Browser als Lautsprecher eingeschaltet ist (`localStorage`); blockiert der Browser den Ton (kein Tipp seit dem Laden), zeigt der Kopf „Ton freigeben“. Texte ohne Datei per `speechSynthesis` |
| `model.js` | `slotsOf`, `standings` (Rennen: Runden, dann Gesamtzeit; Training: Bestzeit; Abstand), `passLog`, `computeLaps` (JS-Kopie von `laps.rs::compute`, für die Vorschau im Editor) |

Zeiten an die API immer als ganze µs (`Math.round`): berechnete Bereiche haben Nachkommastellen,
und die Query-Parameter sind `i64` (sonst 400 Bad Request beim Verschieben des Graphen). Der
Dialog schließt nur bei einem Tipp, der auf dem Hintergrund *begonnen* hat (ein Ziehen, das
außerhalb endet, klickt sonst den Hintergrund).

`npm run dev` proxied `/api` und `/ws` auf Port 5800. Geprüft mit puppeteer-core und dem
installierten Chrome, Desktop 1280 px und Handy 390 px (Emulation; ohne sie wird Chrome
headless nicht schmaler als ~500 px). Achtung: CSS-Klassennamen sind global; `.main` ist
der Hauptbereich der App (eine gleichnamige Klasse im Slot-Dialog bekam dessen
Handy-Abstand von 84 px).

## Aufzeichnen und Auswerten

Bisher: Aufzeichnen mit Live-Erkennung (`laptimer record`), Erkennung auf einer Aufnahme
(`laptimer replay <csv> [--board N]`), `laptimer ports`:

| Datei | Inhalt |
|---|---|
| `src/esp/frame.rs` | Byte-Strom → Frames und Textzeilen (`Parser`), `LevelBatch` |
| `src/esp/board.rs` | `Board::start`: Port öffnen, einstellen (Befehle oben), Uhrabgleich, dann ein Thread pro Board, der `Event`s über einen `mpsc`-Kanal schickt |
| `src/clock.rs` | `ClockSync`: Board-Zeit → Host-Zeit (µs seit Programmstart). Je 10 `TIME?`-Antworten zählt die schnellste; durch diese Schätzungen der letzten 5 min eine Ausgleichsgerade (Drift erst ab 20 s Spanne) |
| `src/detect.rs` | Durchflug-Erkennung ohne I/O (Schritte im Kopfkommentar): 10-ms-Slots, Hüllkurve (Max 20 ms, Median 100 ms) pro Board, Maximum über die Boards, Schwellen aus dem Flug (Kurs = Median bei Sender an, > −46 dBFS; Gipfel = Median der letzten 5 Durchflug-Gipfel, vorher 99,5-%-Perzentil; Eintritt 60 %, mindestens `min_rise_db` (Standard 12 dB, Einstellungen → Auswertung → Mindestausschlag; `replay --min-rise`) über dem Kurs, Austritt 20 %), Durchflug 1 s nach Austritt gemeldet, Mindestrunde 3 s. Nach dem 3. Durchflug werden frühe Fehltreffer zurückgenommen (`Event::Retract`) |
| `src/record.rs` | CSV schreiben, Erkennung, Live-Anzeige (Pegel, Schwellen, Runden); Durchflüge nach `<aufnahme>_passes.csv` (`event,channel,time_us,peak_db`, `event` = `pass` oder `retract`) |
| `src/replay.rs` | Aufnahme durch `detect.rs` schicken |

Threads und `std::sync::mpsc` statt async: einfacher zu lesen. Für den Web-Server kommt
später vermutlich `tokio`/`axum` dazu.

CSV-Format: Kommentarzeilen mit `#` (Boards, Kanäle, Gain), dann
`board,channel,host_us,board_us,rssi,flags`. `host_us` ist für alle Boards dieselbe Uhr.

Gemessen mit zwei Boards (2026-10-22, 40 s, ohne VTX): ~511 Pegel/s pro Kanal und Board,
keine Verluste, Uhr ±0,1 ms (der Uhrabgleich vor dem Scan-Start ist schneller als im
Betrieb), Drift +4 und +8 ppm. In den ersten ~0,2 s nach dem Start fehlen bei einem frisch
gestarteten Board Werte: die Firmware kalibriert jeden LO beim ersten Besuch (~40 ms).

Mindestausschlag (`DetectionSettings.min_rise_db`, Standard `MIN_RISE_DB`, pro Kanal überschreibbar, immer, nicht nur im Anlauf): Ein Durchflug muss mindestens
12 dB über dem Kurspegel liegen. Session 4 (2026-10-23, Pi): Pilot flog meist 1–2 m über
dem Gate; diese Überflüge lagen ~14 dB über dem Kurs und zählten bei niedrigen Gipfeln
(Eintritt 35 %: 8 Treffer); mit 12 dB bleiben 3, mit 15 dB geht auch der echte bei −26 dBFS
verloren (Trennung nur über den Pegel ist knapp). Vorher galten 8 dB nur vor dem 3. Gipfel
(`BOOT_MIN_RISE`). Mit 3 dB zählten in der ersten echten Session des
Nutzers (Session 1, 2026-10-22, 2 Durchflüge, Server kurz vorher gestartet) Wellen 4–5 dB
über dem Kurs: 7 Treffer statt 2, und das Zurücknehmen griff nicht, weil die ersten drei
Gipfel alle falsch waren. Echte Durchflüge stiegen in allen Aufnahmen 13–26 dB. Mit 8 dB:
alle drei Aufnahmen richtig (6, 6, 2).

Erkennung auf den beiden Aufnahmen vom 2026-10-22 (ein Pilot, DJI 25 mW, R3, Boards lose
am Boden mit den beiliegenden externen Antennen): kombiniert beide richtig (6 von 6; in der
ersten wird ein Fehltreffer vor dem ersten Durchflug zurückgenommen). Einzelne Boards
verpassen oder erfinden Durchflüge. Für denselben Durchflug liegen die Boards bis 0,3–0,4 s
auseinander: der Gipfel ist ein breiter Buckel, die Zeit entsprechend ungenau. Halbe
Messrate (~255/s) ändert nichts, ~128/s gibt Fehltreffer. DJI: ~20 % der Messungen am Gate
fallen in Sendepausen (Rauschboden), längste Lücke 6 ms; daher Maximum statt Median.

Einmal hingen beide Boards (keine Antwort, auch nicht auf `INFO`), bis sie per RTS
zurückgesetzt wurden; Ursache unbekannt, nicht wieder aufgetreten.

`serialport` ohne `libudev` (`default-features = false`), damit der Cross-Build für den Pi
keine C-Bibliothek braucht; Boards werden am Portnamen erkannt (`/dev/cu.usbmodem*`,
`/dev/ttyACM*`).

**Firmware über die Seite** (Boards → Firmware, `components/FirmwarePanel.jsx`):
`tools/stage_firmware.py` kopiert den Build nach `frontend/public/firmware/` (nicht im Git) mit
`manifest.json` (ESP-Web-Tools-Format plus Größe und sha256; `version` = `project_version` des
Builds, wie es `INFO` meldet). Vite nimmt das nach `dist/firmware/` mit. Zwei Wege:
- **Server** (`src/esp/flash.rs`, Crate `espflash` ohne CLI): `GET /api/firmware` (Manifest,
  alle Ports aus `find_ports` mit `INFO` der laufenden Boards, Flash-Status), `POST
  /api/firmware/flash {port}` (nicht während einer Session). Ein Thread hält die
  Source-Sperre, stoppt alle Boards, flasht (erst Reset über USB, dann ohne Reset für ein
  Board im Download-Modus per BOOT + RESET), startet die Boards neu und vergleicht die
  Version im `INFO`. Fortschritt als WebSocket-Nachricht `flash` (`useServer().flash`).
  espflash kennt die Watchdog-Register des C5 nicht: Abschalten vor dem Schreiben und
  Watchdog-Reset danach (ein USB-Reset lässt den C5 im Download-Modus) machen wir selbst,
  Register wie in esptool. Gemessen 2026-10-22: ~10 s pro Board, beide Boards danach mit
  richtiger Version zurück.
- **Browser** (`src/flasher.js`, esptool-js, aus widscan58s `web/src/serial/flasher.ts`):
  Web Serial gibt es nur in sicherem Kontext (localhost/HTTPS), also nicht von
  `http://<pi>:5800`; dann zeigt die Seite nur einen Hinweis. esptool-js wird erst beim
  Flashen geladen (eigene Chunks). Nicht automatisch getestet (die Port-Auswahl braucht einen
  Klick).

**Upstream-Updates holen:** `tools/update_firmware.sh` (zieht `firmware/` aus
CapnBry/widscan58 per `git subtree`).
