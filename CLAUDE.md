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
| `firmware/` | widscan58 (ESP32-C5 als SDR, 5,8 GHz), per `git subtree` von [CapnBry/widscan58](https://github.com/CapnBry/widscan58), plus `LEVELBATCH`/`TIME?`/`NAME` (unten). GPL-3.0-or-later |
| `server/` | Rust (Crate `laptimer`, Server `laptimer serve`) auf einem Raspberry Pi 3 (für den Pi auf dem Mac cross-kompilieren, der Pi 3 schafft keinen Rust-Build): ESPs per USB lesen, Durchflüge erkennen, SQLite (Sessions, Durchflüge, Rohpegel), MP3-Ansagen (`mpg123`), Web/WebSocket |
| `frontend/` | React, JavaScript + JSDoc |
| `tools/` | Flash-Script, `stage_firmware.py` (Firmware für die Seite), `build_release.sh` (Release-Pakete), Python-Scripts zum Messen und Auswerten |
| `.github/workflows/` | CI auf GitHub Actions (Repo `bli-blu-bla/fpv-laptimer`): `ci.yml` testet jeden Push, `release.yml` baut bei Tag `v*` (Doku: `docs/INSTALL.md`), `docs.yml` veröffentlicht die Doku-Seite, `pypi.yml` lädt den Python-Client (`clients/python`, Version aus dem Tag, im Workflow in `__version__` geschrieben; im Repo Platzhalter `0.0.0.dev0`) bei Tag `py-v*` per Trusted Publishing auf PyPI (von Hand gestartet: TestPyPI), `npm.yml` ebenso den JS-Client (`clients/js`, Paket `fpv-laptimer-client`, Platzhalter `0.0.0-dev`, `.d.ts` per `prepack` aus dem JSDoc) bei Tag `js-v*` auf npm (Vorabversion → Dist-Tag `next`; von Hand nur `--dry-run`; die erste Version von Hand, weil npm Trusted Publishing erst für ein bestehendes Paket anbietet) |
| `NOTICE.md` | Herkunft und Danksagung (widscan58, C5VRX, esp-sdr), die Änderungen an der Firmware mit Datum (auch im Kopf jeder geänderten Firmware-Datei vermerken, GPL §5a), Quellcode-Hinweis; kommt in jedes Release-Paket. Bei neuen Firmware-Änderungen ergänzen |
| `docs/`, `mkdocs.yml` | Doku-Seite (gebaut mit Zensical, dem Nachfolger von Material for MkDocs; liest `mkdocs.yml` unverändert. Nur dunkel in den App-Farben, ohne Google Fonts) auf GitHub Pages: Start, Konzept, Funktionen, Installation (`INSTALL.md`), API (`docs/api/`: Überblick, HTTP, WebSocket, Server-Verbindung, Events, Befehle, Datenformate, Hooks, Clients), FAQ, Technik. Screenshots in `docs/assets/screens/` von `tools/docs_screenshots.cjs` (playwright-core) gegen einen Server mit der erfundenen Aufnahme von `tools/make_demo_recording.py` (4 Piloten, 2 Ziel- + 1 Freigabe-Board); Ablauf im Kopf des Scripts. Anker mit Umlauten (`pymdownx.slugs`), damit Links auf GitHub und auf der Seite gehen |

**Installation** (`docs/INSTALL.md`): Pi (64-bit Pi OS Lite), Linux-PC und Mac, alles in
`~/.laptimer`. Ein Befehl am Zielrechner: `curl -fsSL
https://github.com/bli-blu-bla/fpv-laptimer/releases/latest/download/install.sh | bash`.
`deploy/install.sh` (als Nutzer, nicht root; bash 3.2 tauglich wegen Mac, daher keine leeren
Arrays unter `set -u` auf dem Mac-Pfad; Fragen über `/dev/tty`, weil stdin das Script ist):
erkennt `linux|macos`-`aarch64|x86_64`, lädt `laptimer-<ver>-<plattform>.tar.gz` + `.sha256`
aus dem GitHub-Release (Version und Repo trägt `build_release.sh` für `@VERSION@`/`@REPO@`
ein; ohne sie fragt es die API nach dem neuesten), oder nimmt das entpackte Paket, in dem es
liegt (offline). Gleiche Version schon installiert → nichts. Unter Linux sudo nur für
Fehlendes (Pakete per apt, Gruppen `dialout`/`audio`; mit Dienst auch Linger und
`/etc/sudoers.d/laptimer` nur für `systemctl poweroff/reboot`, geprüft mit `sudo -n -l`, weil
`sudoers.d` nicht lesbar ist). Dann `bin/laptimer install [--service|--no-service]`
(`src/install.rs`, Passwort über die Umgebung `LAPTIMER_PASSWORD`): Paket nach
`releases/<ver>`, Symlinks `current`/`previous` (`update::set_link`, rename), `config.toml`
beim ersten Mal, drei Releases behalten, Dienst optional (erste Installation fragt, Update
behält): systemd-User-Unit `deploy/laptimer.service` bzw. launchd-Agent
`~/Library/LaunchAgents/local.laptimer.plist` (in `install.rs` erzeugt, `KeepAlive`, Log
`~/.laptimer/laptimer.log`). Beide setzen `LAPTIMER_SERVICE`; ohne Dienst ersetzt sich der
Server nach Update/Neustart per `exec` selbst (`update::restart_as`). `--rollback` und
`--uninstall` rufen `laptimer rollback`/`uninstall` der installierten Version. Getestet
2026-10-28 in einem Test-HOME mit Attrappen für sudo/dpkg/systemctl/loginctl und einem curl,
das die GitHub-URLs auf `dist/` umleitet (Erstinstallation, gleiche Version, Dienst
nachträglich, Update, Rollback, offline, Aufräumen, Deinstallieren), und Update über die
Seite ohne Dienst mit Test-Schlüssel in `extra_keys`; nicht auf einem echten Pi oder Mac.

`tools/build_release.sh [VERSION] [--target T]... [--no-firmware] [--no-web] [--web-only]`:
Ziele `linux-aarch64` (Standard), `linux-x86_64` (`cargo zigbuild --target
<triple>.2.36`, also glibc ≥ 2.36), `macos-aarch64`, `macos-x86_64` (nur auf einem Mac),
Pakete `dist/laptimer-<ver>-<ziel>.tar.gz` (`bin/`, `web/` mit Firmware, `sounds/`,
`hooks-examples/`, `install.sh`, `laptimer.service`, `update-check.sh`, `VERSION`,
`minisign.pub`) + `.sha256`, dazu `dist/install.sh`. Die Release-CI: Job `web` (Tests,
Firmware, Seite), dann `linux` (ubuntu) und `macos` (macos-14) parallel mit `--no-web`, dann
`release`: minisign-Signatur (Secrets `RELEASE_SIGNING_KEY`, `RELEASE_SIGNING_PASSWORD`; das
Passwort liest minisign ohne Terminal aus stdin) und `gh release create`. Lokal gemessen
2026-10-28: `linux-x86_64` per zigbuild ~1 min, Paket 5,4 MB. Noch nicht auf GitHub gelaufen.

Updates über die Seite (Einstellungen → System, `src/update.rs`): Paket + `.minisig`
hochladen oder von GitHub holen (Standard `update::DEFAULT_SOURCE`; `[update] url` für eine
andere Quelle, GitHub oder Forgejo, `url = ""` schaltet ab; `token` für private Repos),
signiert geprüft, dann ersetzt der Server `current` selbst und beendet sich; kommt die neue
Version nicht hoch, setzt `deploy/update-check.sh` (`ExecStartPre` der Unit, im launchd-Agent
davor; `mv -T` bzw. auf dem Mac `mv -h`) beim 4. Start zurück. Der Plan mit den weiteren
Phasen steht in `todo.md`.

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
- **`NAME <text>`** speichert einen Namen fürs Board im NVS (Schlüssel `name`, Namespace
  `widscan58`; bleibt beim Flashen, weil weder espflash noch esptool-js NVS löschen),
  `NAME` allein löscht ihn, `NAME?` antwortet `NAME <text>` bzw. `NAME`. Höchstens 32 Byte,
  UTF-8 erlaubt, kein `|` und keine Steuerzeichen (`ERR name`). Geht auch im RotorHazard-Modus.
- Frame-Rahmen: `A5 'R' type version u16 len payload u32 crc32` (Little Endian,
  zlib-CRC über alles davor). Textzeilen (Antworten) stehen zwischen den Frames.

Gemessen (2026-10-22, XIAO ESP32-C5, R1/R3/R6/R8, `LEVELBATCH 10`, auch zwei Boards
gleichzeitig): ~491–493 Pegel/s pro Kanal (alle 2,0 ms, längste Lücke ~3,0 ms), 96 Frames/s,
~18 KB/s, keine Verluste. `TIME?` über USB am Mac: kürzeste Umlaufzeit 0,7–0,9 ms an zwei
USB-Ports, an einem dritten 2,4 ms; also ±0,4 bis ±1,2 ms Unsicherheit.

## Server (`server/`)

`laptimer serve` (Port 5800; 8080/8090 belegt Docker auf dem Mac des Nutzers; `host`/`--host`
Standard `localhost`, die angelegte Config von `config.rs` setzt `0.0.0.0`) liest
`~/.laptimer/config.toml` (`config.rs`; unbekannte Einträge nur mit Warnung, Crate `serde_ignored`; fehlt sie, wird sie mit zufälligem Passwort angelegt;
`--config` für eine andere, die dann existieren muss). Reihenfolge: Kommandozeile, Config,
Standard. `--data` (Standard `~/.laptimer/data`: SQLite `laptimer.db` +
`levels/session_<id>.csv`), `--web`/`--sounds` (Standard `web/`/`sounds/` neben `bin/` des
installierten Release, sonst `../frontend/dist`/`../sounds` für `cargo run`), `--hooks`
(Standard `<data>/hooks`), `--replay <csv>` (Aufnahme in Schleife statt Boards; zum Testen
`--sounds` auf einen leeren Ordner, sonst piept der Mac; dasselbe ohne Neustart über die Seite,
Boards → Quelle, siehe `source.rs`). Zum Testen eine eigene Config mit
`--config` und `--data` in einen Scratch-Ordner, sonst schreibt der Test in `~/.laptimer`.
Version: `build.rs` setzt `LAPTIMER_VERSION` (aus der Umgebung, also der CI, sonst
`git describe`), `main.rs::VERSION`, in `state.version`, im API-`hello` und in den
Einstellungen.

| Datei | Inhalt |
|---|---|
| `src/api.rs` | Öffentliche API `/api/v1` (Doku: `docs/api/`): jede Aktion ist ein Befehl (`command(app, name, args)`, z. B. `session.start`), erreichbar als REST-Route, als `POST /api/v1/cmd/<name>` oder über den WebSocket `/api/v1/ws` (`{"id","cmd","args"}` → `reply`; vorher `hello` mit `state`, dann jedes Event). Schlüssel-Prüfung als Middleware (`Authorization: Bearer`, `X-API-Key` oder `?key=`; in der DB nur sha256, Tabelle `api_keys`). Befehle laufen in `spawn_blocking`. Für die Seite ohne Schlüssel: `/api/keys` (GET/POST/DELETE), `/api/hooks` (GET: Ordner, Scripts, Hooks, letzte Läufe; POST), `/api/hooks/{id}` (PUT/DELETE), `/api/hooks/{id}/test`. Die Weboberfläche selbst nutzt weiter `/api` ohne Schlüssel |
| `src/auth.rs` | Passwort der Rennleitung (`[auth]` in `config.toml`: `enabled` Standard an, ohne `password` startet der Server nicht; abschalten nur mit `enabled = false`). Middleware `require_login` auf `/api`: Lesen frei, außer `/keys`, `/hooks`, `/backups`, `/update`, `/system`, `/remote`; alles andere als GET/HEAD braucht ein Login (nicht `/v1`, das hat API-Schlüssel; nicht `/login`, `/logout`, `/auth`). `POST /api/login {password}` → Cookie `laptimer_login` (HttpOnly, SameSite=Strict, 30 Tage; in der DB nur sha256, Tabelle `logins`), `POST /api/logout`, `/api/logout/all`, `GET /api/auth` (`{enabled, logged_in}`). Falsches Passwort: 1 s warten, nach 5 in Folge 1 min Sperre pro IP (`ConnectInfo`, daher `into_make_service_with_connect_info`). 401 mit `login: true` lässt die Seite den Login-Dialog öffnen |
| `src/events.rs` | `Events::emit(name, data)`: Event `{"type":"event","event","seq","time_us","data"}` an den API-WebSocket (eigener `broadcast`) und an die Hooks. Erzeugt in `core.rs`: `session_started`, `countdown`/`start_signal` (in `tick`), `pilot_pass` (`report_passes`: neue, nicht gelöschte Durchflüge der laufenden Session mit `kind` lap/holeshot/start/ignored), `pilot_finished`, `session_stopped` (`end_session(reason)`), `race_confirmed`, `session_deleted`, `sessions_reset`, `passes_changed` (Korrekturen, Zurücknahme), `channels_changed`, `pilot_added/updated/deleted` (Piloten deshalb über `Core::add_pilot` usw.) |
| `src/data.rs` | Export, Backup, Reset (ohne Schlüssel, für die Seite): `GET /api/export?sessions=1,2&channels=0,2&content=laps\|results&format=csv\|json` (eine Datei für eine oder mehrere Sessions, alle oder einzelne Kanäle; CSV mit `;`, Dezimalkomma und BOM für Excel, Runden mit „Erfassung“ (erkannt/verschoben/von Hand), „Verschoben um (s)“ und „Korrekturen“ (aus `pass_changes`, „(live)“ während der Session) des End-Durchflugs; JSON mit `core::standings`, den Ziel-Durchflügen samt `source` und `changes` je Session). `POST /api/backups` (`Db::copy_to` = `VACUUM INTO` unter der Core-Sperre, dann gzip ohne sie nach `<data>/backups/laptimer_<datum>_<zeit>.db.gz`), `GET /api/backups`, `GET`/`DELETE /api/backups/{name}`. `POST /api/reset`: erst Backup (`…_vor-reset`), dann `Core::reset_sessions` (alle Sessions mit Sitzen, Durchflügen, Pegel-Dateien; Event `sessions_reset`) und den Graph-Cache leeren; Piloten, Settings, Schlüssel, Hooks bleiben. **Debug-Paket** `GET /api/export/debug?session=3&channels=0,2` (Export-Dialog → Debug, eine Session): `.tar.gz` mit `session.json` (Config, Sitze, aktuelle Settings, alle Durchflüge auch gelöschte, Änderungsprotokoll, Ergebnis, Version), `levels.csv` (Rohpegel der gewählten Kanäle, mit `laptimer replay` neu auswertbar), `trace.csv` (je 10-ms-Slot Hüllkurve kombiniert und je Board, Enter/Exit), `detector_events.csv` (Neuberechnung mit den Session-Einstellungen, auch Zurücknahmen), `README.txt`; im Speicher gebaut |
| `src/transfer.rs` | Sessions von einem Laptimer zum anderen (z. B. vom Pi in die Testumgebung): `GET /api/export/package?sessions=3,5` (ohne Login wie `/export`) streamt ein `.tar.gz` (`laptimer_sessions/manifest.json` mit `format` `laptimer-sessions` und `version`, je Session `session_<id>.json` = `SessionRow` + alle Durchflüge + Änderungsprotokoll (`changes`, beim Import auf die neuen Pass-IDs umgeschrieben) + Piloten der Sitze, und `session_<id>.csv` = Pegel-Datei), gebaut in `spawn_blocking` und über einen Kanal in den Body (`ChannelWriter`), nicht im Speicher. `POST /api/import` (multipart `package`, bis 2 GB, erst als Datei nach `<data>/import_*.tar.gz`): neue IDs, Piloten nach Callsign zugeordnet oder angelegt, schon vorhandene (gleiches `created_us` + `start_us`) übersprungen, unbestätigte Rennen gelten als bestätigt (sonst blockieren sie den Start), Zeiten bleiben (Wanduhr). Getestet 2026-10-30: 3 Sessions (39 MB Pegel, Paket 13 MB) aus einer Kopie der echten Daten in einen leeren Server, Ergebnis gleich, zweiter Import überspringt alles |
| `src/remote.rs` | Server-Verbindung (API & Hooks → Server, `components/RemotePanel.jsx`): der Timer verbindet sich als Client (tokio-tungstenite, `wss://` über rustls + ring, kein OpenSSL) zu einer URL, optional mit `Authorization`-Header (wie eingegeben) und/oder Token als erste Nachricht `{"type":"auth","token"}`, und spricht das Protokoll von `/api/v1/ws` (`hello` mit `state` und `buffered`, dann Events; Befehle über `api::ws_command`, abschaltbar → 403; Nachrichten ohne `cmd` werden ignoriert, z. B. ein „auth ok“). Ein Task auf tokio (`run`, gestartet in `server::run`), dauerhaft auf `api_tx` abonniert; ohne Verbindung Events im Speicher (höchstens 5 min, 2000), nach dem nächsten `hello` nachgeschickt, ältere verworfen (Live-Daten, kein Protokoll). Neuversuch 2 s verdoppelnd bis 30 s, Ping alle 30 s, 75 s Stille = tot; neue Einstellungen per `Notify`. Einstellungen in der DB unter `remote` (nicht in `settings`, die sind ohne Login lesbar), `GET/PUT /api/remote` (Login auch fürs Lesen, `auth.rs`; die Geheimnisse kommen nie zurück, nur `authorization_set`/`auth_token_set`; Feld weglassen = behalten, `""` = entfernen). Getestet 2026-10-28 gegen einen Python-Testserver (Token, Befehle an/aus, Nachliefern nach Abbruch, TLS gegen github.com), nicht gegen einen echten Dienst |
| `src/hooks.rs` | Hooks: Scripts im Ordner `--hooks` (Standard `<data>/hooks`, nur ausführbare Dateien direkt darin, `check_script`), Zuordnung in der DB (Tabelle `hooks`: Event oder `*`, Script, Argumente an Leerzeichen getrennt ohne Shell, Hintergrund, an/aus), im Speicher gehalten (`set_list` nach jeder Änderung). Ein Thread führt sie nacheinander aus (im Script-Ordner, 10 s Limit; Hintergrund: eigener Thread wartet, ohne Limit), Umgebung `LAPTIMER_EVENT/SEQ/TIME_US/JSON` und `LAPTIMER_<FELD>` für jedes einfache Feld; die letzten 30 Läufe für die Seite. Beispiele in `tools/hooks/` |
| `src/server.rs` | axum: `/api/state`, `/api/session/start {race}`, `/api/session/stop`, `/api/sessions[/{id}]` (GET, DELETE), `/api/sessions/{id}/confirm` (Ergebnis eines Rennens bestätigen), `/api/sessions/{id}/passes {channel,time_us}` (von Hand), `/api/passes/{id} {deleted}`, `/api/settings` (GET/PUT; Kanäle oder Gain ändern startet die Boards neu), `POST /api/boards/rescan` (Ports neu suchen und alle Boards neu starten; nicht während Session oder Flash), `POST /api/boards/name {id, name}` (Name aufs Board schreiben: alle Boards anhalten, `board::set_name`, neu starten; `""` löscht; alte Firmware ohne `NAME` → Hinweis aufs Flashen), `/api/pilots[/{id}]`; `/ws` (erst `state`, dann `state`/`live`/`session_changed`); sonst `frontend/dist`. Threads: Boards → std-Kanal → Core-Thread (`on_board_event`, `tick` alle 10 ms); `Core` in einem `std::sync::Mutex`, nie über ein `.await` halten |
| `src/core.rs` | `Core`: Settings (DB-Key `settings`), aktive Session, Durchflüge → DB → `results` → `state`-Nachricht; Ansagen (`refresh`, `schedule_start`; Bausteine siehe `sound.rs`); `live` mit 10 Hz. Zeiten in der DB: Wanduhr-µs (`origin_unix_us + host_us`); auch Start, Ende, `tick` und `now_us` nehmen diese eine Uhr (`Core::now_us`), nicht die Systemuhr: der Pi bootet ohne Akku-Uhr mit alter Zeit, NTP stellt sie später (Session 10, 2026-10-25: +27 h, alle Durchflüge lagen vor dem Start und wurden verworfen). `resync_clock` folgt einem Sprung der Systemuhr beim Start einer Session, nie währenddessen |
| `src/laps.rs` | Runden aus Durchflügen (rein, getestet): Start `holeshot`/`from_start`/`flying`, Ende `open`/`laps`/`seconds`, beste Runde, beste N in Folge, ignorierte Durchflüge |
| `src/db.rs` | Tabellen `pilots`, `sessions` (Config als JSON, damit alte Ergebnisse bleiben; `confirmed_us`, siehe unten), `seats`, `passes` (`manual` = von Hand eingefügt, `deleted` fürs Rückgängigmachen; vom Detektor zurückgenommene werden gelöscht; `detected_us` = wo der Detektor ihn gesetzt hat, `NULL` bei von Hand, ≠ `time_us` = verschoben, `PassRecord::source` → erkannt/verschoben/von Hand; in alten Datenbanken ergänzt, früher verschobene hatten `manual` gesetzt und gelten als von Hand), `pass_changes` (Änderungsprotokoll: jede Korrektur mit `action` add/delete/restore/move/reject, Zeit vorher/nachher, bisheriger Ablehnungsgrund beim Wiederherstellen, `live` = Session lief noch, `origin` seite/editor/api/server, `at_us`; geschrieben in `Db::add_manual_pass`, `set_pass_deleted`, `edit_passes`, `reject_pass` mit `By {origin, at_us}`, nicht beim Zurücknehmen durch den Detektor und nicht beim Kürzen; `Db::pass_changes`, in `sessions.get`/`GET /api/sessions/{id}` als `changes`, im Editor als „Bisher: …“ beim gewählten Durchflug), `settings`, `api_keys` (Name, Präfix, sha256), `hooks`. Session-IDs werden nie wiederverwendet (Setting `last_session_id`; SQLite nähme nach dem Löschen die höchste wieder): Entwürfe im Browser und Export-Dateien hängen an der ID |
| `src/trace.rs` | Pegelgraph einer Session: die Pegel-Datei noch einmal durch den Detektor (`Detector::with_trace` zeichnet pro 10-ms-Slot Hüllkurve kombiniert, je Board, Eintritt, Austritt auf), dann `graph` (Zeitbereich auf N Punkte, je Punkt das Maximum, damit kein Gipfel verloren geht) und `snap` (höchster Punkt ±1 s, von dort ±50 ms-weise bergauf, Buckel bis 3 dB darunter, darin Mitte zwischen erstem und letztem Punkt innerhalb 1 dB, wie der Detektor). Im Server zwischengespeichert pro Session und Dateigröße (höchstens 8 Einträge, Ziel und Freigabe je einer); gelesen ohne die Core-Sperre (`spawn_blocking`); beim `session_stopped` rechnet `server::warm_traces` beide Gates im Hintergrund vor, damit der Editor nach dem Rennen nicht wartet. Pegelzeilen liest `record::parse_row` von Hand (4× schneller als `split`/`parse`; auch in `debug.rs`, `source.rs`); ohne Freigabe-Board in `# roles:` bricht die Freigabe-Abfrage nach dem Kopf ab `/api/sessions/{id}/graph?channel&from_us&to_us&points[&gate=arming]` (`gate_trace`: Freigabe-Boards mit deren Schwellen; Cache-Schlüssel (Session, Gate)), `/api/sessions/{id}/snap?channel&time_us`; `POST …/passes` mit `snap: true`; `POST …/edit {channel, moves: [[id, t]], deletes, restores, adds: [t]}` (alles in einer Transaktion, `Db::edit_passes`; verschobene behalten `detected_us`, gelöscht werden sie nur markiert, der Detektor nimmt sie nicht mehr zurück; die Mindestrunde gilt nicht, abgelehnt nur zwei Durchflüge näher als 0,1 s, `MANUAL_GAP_US`); `POST …/trim` (`Core::trim_session`, nur beendete Sessions): Pegel-Datei 10 s nach dem letzten nicht gelöschten Ziel-Durchflug abschneiden (`trace::trim_file`, neu schreiben + rename, unter der Core-Sperre), Durchflüge danach löschen, Ende vorziehen; nichts zu kürzen = Fehler |
| `src/debug.rs` | Board-Debugging (Boards → Debug): `Analysis` mit eigenen Detektoren (Ziel kombiniert = Serie 0, Freigabe kombiniert = 1, jedes Board allein = 2 + Index; dieselben Einstellungen, ändern nichts an den Runden), `Detector::drain_trace` liefert die 10-ms-Slots. Live in `Core::debug` (letzte 60 s, gefüttert mit allen Boards, auch „aus“; neu bei Board-Start, Rollen/Erkennung geändert, Uhrsprung). Aufnahme (`Core::recording`): `<data>/recordings/rec_<datum>_<zeit>.csv` im Format der Session-Dateien plus `# board i {json}` und `# detection: {json}` (auch mit `laptimer replay` lesbar). Routen: `GET /api/debug/live?after_us&window_s&channels` (Slots als Pegelcodes ab `t0`, alle 10 ms; Durchflüge), `POST /api/debug/record`, `…/record/stop`, `GET /api/debug/recordings` (mit `info` = `debug::file_info`: Kanäle, Boards mit Rolle und Name, Dauer; nur Kopf und letzte 4 KB gelesen), `GET /api/debug/session-files` (dasselbe je Session-ID, für die Listen), `GET`/`DELETE …/{name}`, `GET /api/debug/graph?source=rec:<name>\|session:<id>&channel&from_us&to_us&points` (Datei neu ausgewertet, frische Detektoren: Automatik muss erst anlernen; Cache `App::debug_files`, höchstens 2) |
| `src/stats.rs` | Bestzeit, Sessions, Rennen, Siege pro Pilot (nach `pilot_id`, sonst nach Label), gesamt und heute; `race_order` (mehr Runden, dann frühere Gesamtzeit). `/api/stats` |
| `src/source.rs` | Boards oder Replay (Replay liefert die Pegel nach Kanal-Position der Aufnahme, nicht nach Frequenz: nach einem Kanaltausch passen sie nicht mehr; ein Board pro Index in der Datei, Kennung `replay-N`, Rolle aus `# roles:` als Vorgabe, solange `board_roles` keine hat; jeder Session-Start spult an den Anfang zurück, `Core::replay_rewind`, damit ein aufgenommenes Rennen samt Countdown mit der neuen Session beginnt). Umschalten über die Seite (Boards → Einrichtung → Quelle, `components/SourcePanel.jsx`): `GET/PUT /api/source {replay: "rec:<name>"\|"session:<id>"\|null}` (`server.rs`, Datei über `debug::source_file`; nicht während Session oder Flash; startet die Datei nicht, bleibt die alte Quelle). Entweder–oder: während der Wiedergabe sind die Boards gestoppt, Board-Namen, Neu suchen und Flashen gesperrt; `state.replay` `{source, label, channels}`, der Kopf zeigt „Wiedergabe statt Boards“ mit gelbem Punkt. Nicht gespeichert: nach einem Neustart wieder die Boards (außer `--replay`) |
| `src/update.rs` | Updates über die Seite, nur aus einem installierten Release (`<root>/releases/<ver>/bin/laptimer`, `<root>/current` vorhanden; sonst `installed: false`). Zwei Schritte: **Bereitstellen** (`POST /api/update/upload` multipart `package` + `signature`, bis 64 MB; oder `GET /api/update/releases` + `POST /api/update/download {tag}` von GitHub (`api.github.com`, mit Token über die API-URL des Anhangs) oder Forgejo (`/attachments/<uuid>`) per `curl`, Token über stdin; Paketname `laptimer-<ver>-<OS>-<ARCH>.tar.gz`, `update::platform()`): Signatur gegen den eingebauten `minisign.pub` (`include_str!`) oder `[update] extra_keys` (Crate `minisign-verify`), entpacken nach `<root>/staging/`, `VERSION`, System und Prozessor von `bin/laptimer` (ELF oder Mach-O). **Installieren** (`POST /api/update/install {allow_older}`): keine Session, kein Flash; Backup `…_vor-update`, nach `releases/<ver>`, `update-pending` (`to`, `from`, `previous`, `attempts`) schreiben, `previous`/`current` umsetzen (Symlink + rename), geänderte Unit nach `~/.config/systemd/user` + `daemon-reload`, aufräumen (3 behalten, zuerst eine nicht hochgekommene), nach 0,5 s `exit(0)` (mit Dienst) bzw. `exec` von `current/bin/laptimer` mit denselben Argumenten (ohne). Älter/gleich/unklar (`compare`: Zahlen, dann ohne Zusatz > `-rc1`, sonst unklar) nur mit `allow_older`. Die neue Version löscht `update-pending` nach 15 s; `update-check.sh` zählt die Starts und setzt beim 4. zurück (`update-failed`, auf der Seite mit `POST /api/update/dismiss` ausgeblendet). `POST /api/update/rollback` (`current` ↔ `previous`), `DELETE /api/update/staged`, `POST /api/system/restart` (Dienst: `LAPTIMER_SERVICE` oder `INVOCATION_ID`, sonst `exec`), `/api/system/reboot`, `/api/system/poweroff` (`sudo -n /usr/bin/systemctl …`, Regel von `install.sh`). Alles unter `/update` und `/system` braucht das Login, auch GET |
| `src/install.rs` | `laptimer install [--service\|--no-service]`, `rollback`, `uninstall` (siehe „Installation“ oben); systemd-Teile unter `cfg(not(target_os = "macos"))`, launchd unter `cfg(target_os = "macos")` |
| `src/sound.rs` | Ansage = Liste von `Part`s (Clip `sounds/<name>.mp3` mit Ersatztext, oder Text), nacheinander in eigenem Thread. Texte per Sprachausgabe als WAV in `<data>/tts/<hash>.wav` (beim Start geleert; `say -v Anna` auf macOS, sonst `pico2wave`, ersatzweise `espeak-ng`); Pilotennamen beim Session-Start vorab (`prepare`, eigener Thread, damit der Countdown nicht wartet). Ausgabe Server (`afplay`, sonst `mpg123`/`aplay`) oder Browser: WebSocket `announce {items: [{url} \| {text}]}`, Dateien unter `/api/audio/clips/…` und `/api/audio/tts/…`; `POST /api/announce/test` |

**Boards und Gates:** Jedes Board hat eine feste Kennung, die USB-Seriennummer (= MAC,
`find_ports`; macOS über `serialport`, Linux aus `/dev/serial/by-id`; Replay: `replay-N`).
`settings.board_roles` ordnet sie zu: `finish` (Standard), `arming` (Freigabe-Gate) oder
`off`. `Core` hat zwei `Detector`: Ziel und Freigabe; jedes Board speist den seiner Rolle,
mehrere Boards pro Gate werden kombiniert (Maximum). Freigabe-Regel: Gibt es ein
funktionierendes Freigabe-Board, zählt ein Ziel-Durchflug nur, wenn ein Freigabe-Durchflug
des Kanals zwischen 1 s nach dem letzten gezählten und 1 s nach diesem liegt (`core::arms`,
`ARM_SLACK_US`: Freigabe-Gate kurz vor dem Ziel, Zeiten ±0,3–0,4 s); der erste nach dem Start
zählt immer. Meldet der Freigabe-Detektor später als der Ziel-Detektor (beide 1 s nach dem
Austritt, der Freigabe-Buckel endet oft später), zählt `on_arming` den schon abgelehnten
nachträglich (`Core::unarmed`, Ansage, `passes_changed`). Session 26 (2026-10-30, Freigabe
0,4 s vor dem Ziel): vorher 3 von 4 Runden auf R3 abgelehnt, jetzt alle gezählt (Wiedergabe). Abgelehnte werden als `deleted` mit `rejected = 'no_arming'` gespeichert (Spalte wird
in alten Datenbanken ergänzt; Löschen/Wiederherstellen von Hand setzt sie auf `NULL`), stehen
in der Durchflug-Liste der Rennleitung als „Keine Freigabe“ und sind dort und im Editor
wiederherstellbar. Nicht gezählte Durchflüge nennen ihren Grund (`model.js::notCountedReason`:
Keine Freigabe, Gelöscht, Vor dem Start, Nach dem Ziel; `pilot_pass` hat `reason`
`before_start`/`after_finish` aus `PilotResult::ignored_reason`).
Freigabe-Durchflüge stehen mit `gate = 1` in `passes` (Spalte wird in alten Datenbanken
ergänzt), machen keine Runden (`laps::compute` filtert `gate == 0`) und erscheinen im
Editor als gelbe Punktlinien. Die Pegel-Datei einer Session hat `# roles: 0=finish 1=arming` und (seit 2026-10-31) wie eine Aufnahme `# board i {json}` mit MAC und Namen;
der Pegelgraph nimmt nur die Ziel-Boards. `live.boards[].level_db`: Pegel pro Board zum
Zuordnen; `live.arming`: Status des Freigabe-Gates.

**Erkennungsmodus pro Kanal und Gate** (`settings.detection`, `detect::DetectionSettings`):
`finish[i]` / `arming[i]` haben `enter_db`, `exit_db`, `mode` (`auto` = gelernte
Schwellen, `classic` = feste Enter/Exit; `null` = der Gate-Modus `finish_mode` /
`arming_mode`) und `min_rise_db` (`null` = der gemeinsame `min_rise_db`; `min_rise()` →
`Detector::set_min_rise`). `fixed()` liefert pro Kanal `Some((enter, exit))` oder `None` für
`Detector::set_fixed` (feste Kanäle ohne Anlernen und Zurücknehmen). Eingestellt in der
Rennleitung (Slot antippen); „Alle automatisch / Alle manuell“ auf der Boards-Seite setzt den
Gate-Modus und löscht die Kanal-Modi. Auch während einer Session: geändert werden nur
Schwellen, Modi und Mindestausschlag (`Detector::set_detection`); Gelerntes, ein laufender
Durchflug und der letzte (Mindestrunde) bleiben, damit die Rennleitung zwischen zwei Runden
nachstellen kann. Mindestrunde oder Rollen ändern baut die Detektoren neu. Ältere Einstellungen mit einem `mode` werden als `finish_mode`
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

Laps werden nie gespeichert, nur Durchflüge: jede Korrektur rechnet alles neu. Von
Hand (Seite) gesetzte oder wiederhergestellte Durchflüge dürfen kürzer als die Mindestrunde
liegen, nur nicht näher als 0,1 s an einem anderen (`MANUAL_GAP_US`); `passes.add` über die
API (Taster, prellt) prüft weiter die Mindestrunde. Ein Rennen stoppt von selbst, wenn alle Slots im Ziel sind.
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
vermutlich ein Vielfaches. Gemessen 2026-10-31 (Linux-PC, Session 23, 159 MB, 5 Mio. Zeilen):
erstes Mal vorher 1,0 s je Gate, jetzt 0,47 s, danach ~2 ms; JSON 58 KB, mit gzip 7,6 KB.
gzip (`CompressionLayer` in `server.rs`) für alles außer `application/gzip` und `audio/`;
das JS-Bundle 637 → 176 KB. Noch nicht auf dem Pi gemessen.

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
| `pages/Boards.jsx` | **Boards**, Reiter Einrichtung / Debug (`components/BoardDebug.jsx`: Live oder Aufnahme/Session; pro Kanal ein Graph mit jeder Serie in eigener Farbe, Serien, Schwellen und Durchflüge ein-/ausblendbar; Tabelle legt die Durchflüge der Serien nebeneinander (innerhalb 1 s = derselbe, Abstand zum kombinierten Ziel); live 300-ms-Abfrage, 3 min im Browser, Anhalten + Zurückblättern, Aufnehmen). Einrichtung: „Boards neu suchen“ (später eingesteckte ESPs), Name pro Board (Stift, gespeichert auf dem Board, `BoardView.name` aus `NAME?` beim Verbinden; ohne Namen das Ende der MAC), Rolle pro Board (Ziel / Freigabe / Aus) mit Live-Pegel, darunter Ziel- und Freigabe-Gate mit dem Modus jedes Kanals und „Alle automatisch / Alle manuell“ |
| `components/ThresholdPanel.jsx` | Erkennung eines Kanals in der Rennleitung (wie RotorHazards Node-Anzeige): Slot-Karte antippen, darunter Gate-Wahl, Automatisch/Manuell für diesen Kanal, Pegelverlauf der letzten 30 s mit Enter/Exit-Linien (im manuellen Modus ziehbar, Maus und Touch, 0,5 dB, Exit bleibt ≥ 0,5 dB unter Enter, gespeichert beim Loslassen; Werte als HTML-Beschriftung, weil SVG-Text mit `preserveAspectRatio="none"` verzerrt), Regler (0,5 dB, Speichern verzögert) und „Aus Spitze“ (Enter = Spitze − 4 dB, Exit = − 9 dB). Ein automatisches Gate zeigt seine gelernten Linien und den Mindestausschlag des Kanals (zurück auf den Standard). Am Ziel-Gate `PresetRow`: Preset des Piloten im Slot, „geändert“ + „In … speichern“, „Neues Preset“ |
| `components/LevelFileList.jsx` | Aufnahmen und Sessions mit Pegel-Datei zum Abspielen (Quelle, `SourcePanel`) oder Öffnen (Board-Debug, dort mit Herunterladen/Löschen für Aufnahmen): eine Zeitachse nach Tag (Heute/Gestern/„Do, 29.10.“), Filter Alle/Rennen/Training/Aufnahmen, Suche (Pilot, Kanal, #ID); kompakte Liste, je Eintrag Zeit, eine Titelzeile (Art, ID, Format mit Sieger bzw. Bestzeit) und eine Zeile Details: Kanäle mit Slot-Farbe (Piloten im Tooltip und in der Suche), Boards (Ziel grün, Freigabe gelb gestrichelt, mit Namen) und Dauer aus `/api/debug/session-files` bzw. `info` der Aufnahmen; ohne Pegel gesperrt. Unter 720 px Kacheln (Container-Query). Entwurf „Aufnahme-Auswahl“ (Claude Design) |
| `pages/Watch.jsx` | **Zuschauer**: laufende oder letzte Session, Karten ohne Bedienung und ohne Signalverlauf, Rangliste, Countdown |
| `pages/Best.jsx` | **Bestzeiten** heute / gesamt: Bestzeit, beste N in Folge, Rennen, Siege |
| `components/PassEditor.jsx` | Korrektur wie RotorHazards Marshal-Seite: Kanal oben wählen, darunter großer Graph (340 px, Übersicht + Ausschnitt). Durchflug ziehen = verschieben, antippen = auswählen (Löschen/Wiederherstellen, am Gipfel einrasten), freie Stelle antippen = neu (per `/snap` am Gipfel), Fläche ziehen = Ausschnitt verschieben; „Freigabe-Gate“ blendet Pegel und Eintrittsschwelle des Freigabe-Gates gelb ein (nur wenn die Session Freigabe-Boards hatte). Entwurf pro Kanal im Browser; Runden-Vorschau mit `computeLaps` (zu kurze Runden rot, dann kein Speichern); „Speichern“ schickt pro Kanal ein `POST /api/sessions/{id}/edit`. Darunter „Graph kürzen …“ (Rückfrage im Knopf, gesperrt bei ungespeicherten Änderungen; während der Rückfrage zeigen Graph und Übersicht den Schnitt als rote Linie mit getöntem Rest und springen hin, Zeit im Text; berechnet wie der Server: letzter nicht gelöschter Ziel-Durchflug aller Kanäle + 10 s): `POST …/trim`, danach lädt der Graph neu |
| `components/PilotDialog.jsx` | Wer fliegt (Tipp auf den Namen in der Karte; Entwurf „Rennleitung: Pilot & Kanal“ auf Claude Design): Liste der Piloten (ab 6 mit Suche und Filter Alle / Ohne Slot / Heute dabei, nach `stats.today`; der aktuelle bleibt immer sichtbar; alphabetisch, Ring in der Slot-Farbe, „Slot N · tauschen“), „Kein Pilot“ (= `enabled: false`, ersetzt den Schalter „Slot belegt“), Gast ohne Profil (Freitext). Antippen speichert und schließt; Pilot aus einem anderen Slot wird getauscht, samt Preset. Hat der Pilot mehrere Presets, bleibt der Dialog offen und zeigt sie („Keins“) |
| `components/ChannelDialog.jsx` | Kanal eines Slots (Tipp auf den Kanal in der Karte): aktueller Kanal mit IMD aller Kanäle, Band R/A/B/E/F/L (öffnet beim Band des aktuellen, Punkt markiert es), acht Kanäle mit „Slot N“ bei belegten (Tausch), freie MHz. Während einer Session gesperrt |
| `components/RaceDialog.jsx` | Start (Holeshot / Ab Startton / Fliegend), Ende nach Runden oder Zeit, Countdown aus/3/5/10 s |
| `pages/Sessions.jsx` | Liste (`components/SessionItem.jsx`) + Detail (`Ranking`); „Runden bearbeiten“ blendet die Liste aus und zeigt den `PassEditor` in voller Breite, oben „Zur Liste“ (bei ungespeicherten Änderungen erst Rückfrage im Knopf, `onUnsaved`); Titel mit Session-ID; Löschen; Export (⤓ im Detail, oder „Auswählen“ in der Liste für mehrere); „Importieren“ (`components/ImportButton.jsx`, mit Login: Paket hochladen mit Fortschritt per XHR, Ergebnis im Dialog; auch bei leerer Liste) |
| `components/ExportDialog.jsx` | Format CSV/JSON/Debug/Paket (Paket = `transfer.rs`, zum Importieren), Inhalt Runden/Ergebnis, Kanäle (alle oder einzelne); lädt per `fetch` (Fehler im Dialog) und speichert unter dem Namen aus `Content-Disposition` |
| `pages/Pilots.jsx` | Callsign + Name, PB, Rennen, Siege; Dialog zum Anlegen/Bearbeiten/Löschen, darin die Presets (umbenennen, löschen; angelegt werden sie in der Rennleitung) |
| `pages/Api.jsx` | **API & Hooks** (am Handy nicht in der Tab-Leiste, sondern über einen Knopf in den Einstellungen): Reiter Schlüssel (anlegen, einmal anzeigen, löschen), Doku, Testen (Befehl per HTTP oder WebSocket, curl-Zeile, Event-Log; Schlüssel in `localStorage`), Hooks (anlegen/bearbeiten mit Event, Script aus dem Ordner, Argumente, Hintergrund; an/aus, einmal ausführen, letzte Läufe; alle 2 s neu geladen), Server (`RemotePanel`, `src/remote.rs`). Befehle und Events beschreibt `apiCatalog.js` |
| `auth.js`, `components/LoginDialog.jsx` | Login-Zustand für die ganze Seite (`useAuth()` → `canEdit`); `api()` meldet ein 401 mit `login` (`loginNeeded`), dann öffnet `App.jsx` den Dialog. Ohne Login (und mit Passwort): Kopf „Anmelden“, Navigation nur Zuschauer, Bestzeiten, Sessions, Piloten (`CONTROL_ONLY` in `App.jsx` blendet Rennleitung, Boards, Einstellungen, API aus, erst nach `checkAuth`; wer dort stand oder sich abmeldet, sieht die Zuschauer, nach dem Login wieder seine Seite); Sessions ohne Löschen/Bearbeiten, Piloten nur ansehen. Die Seiten selbst prüfen weiter `canEdit` (`Locked`), das Ausblenden ist nur Navigation |
| `pages/Settings.jsx` | Nach dem Entwurf „Einstellungen“ (Claude Design, ohne die Auswertungs-Profile Indoor/Outdoor/Whoop). Drei Tabs (`components/Tabs.jsx`: unterstrichene Reiter wie bei API & Hooks, Warnpunkt, rechts „Gespeichert“): Messung, Ansagen, System; je Tab die Karten nebeneinander (am Handy untereinander). **Messung:** Kanäle (Leiste mit den Frequenzen, IMD-Bewertung, eine Zeile pro Slot mit Warnung unter 30 MHz Abstand, Slot entfernen; „Kanäle wählen“ → `ChannelPicker`), Auswertung (`Stepper box`: Mindestrunde, Mindestausschlag, Gain mit 0,8 s Verzögerung, weil er die Boards neu startet, beste N in Folge), Vorschau (`DetectPreview`: erfundenes Signal mit Normalpegel und Schwelle + Mindestausschlag). **Ansagen:** Ereignisse mit Icons, Ausgabe, Beispiel + „Probe-Ansage“. **System:** `components/SystemPanel.jsx` (Version, Update hochladen oder bei GitHub suchen, bereitgestellte Version mit Warnung bei älter/gleich, Installieren; zurück auf die vorige Version, Laptimer/Pi neu starten, herunterfahren, jeweils mit Rückfrage und gesperrt während einer Session; nach dem Neustart lädt sich die Seite selbst neu), Zugang, Backups (Jetzt sichern, Herunterladen/Löschen), Gefahrenzone („Alle Sessions löschen“ mit Rückfrage im Knopf, gesperrt während einer Session) |
| `components/ChannelPicker.jsx` | Kanalwahl wie widscan58 („Channels to scan“): Zeilen R, A, B, E, F auf einer Achse 5630–5960 MHz, L darunter mit eigener Achse; Kanal = Glocke (antippen), Bandname wählt alle acht, gestrichelte Linien zeigen die gewählten Frequenzen in den anderen Bändern, Farben = Slot, den der Kanal bekommt. Darunter IMD-Bewertung mit den schlimmsten Mischprodukten (rote Marken auf der Achse), „Optimal verteilen“, freie MHz. Am Handy seitlich scrollbar (SVG mindestens 760 px) |
| `imd.js` | `BANDS`, `channelName`, `imdRating` (wie ET's IMDTabler, den RotorHazard als Java aufruft: für jedes Paar 2·f1 − f2, Abstand zum nächsten gewählten Kanal, unter 35 MHz kostet (35 − Abstand)², Summe / Anzahl / 5 von 100 abgezogen; Produkte über 6100 MHz zählen nicht; geprüft gegen etserv.etheli.com/IMDTabler mit Raceband −635, IMD 6C 29, ET5 97, Racebnd4 100), `bestSet(n)` (alle Sätze mit ≥ 35 MHz Abstand durchprobieren: beste Bewertung, dann meiste Raceband-Kanäle, dann größter Abstand; ein paar zehn ms), `assignSlots` (Slots behalten ihre Frequenz, sonst bekommen sie an ihrer Stelle eine neue; Sitz und `detection.finish/arming` wandern mit) |
| `announcer.js` | Ansagen im Browser: spielt `announce`-Nachrichten nur, wenn dieser Browser als Lautsprecher eingeschaltet ist (`localStorage`); blockiert der Browser den Ton (kein Tipp seit dem Laden), zeigt der Kopf „Ton freigeben“. Texte ohne Datei per `speechSynthesis` |
| `model.js` | `slotsOf`, `standings` (Rennen: Runden, dann Gesamtzeit; Training: Bestzeit; Abstand), `passLog`, `computeLaps` (JS-Kopie von `laps.rs::compute`, für die Vorschau im Editor) |

Zeiten an die API immer als ganze µs (`Math.round`): berechnete Bereiche haben Nachkommastellen,
und die Query-Parameter sind `i64` (sonst 400 Bad Request beim Verschieben des Graphen). Der
Dialog schließt nur bei einem Tipp, der auf dem Hintergrund *begonnen* hat (ein Ziehen, das
außerhalb endet, klickt sonst den Hintergrund).

Verbindung (`api.js::useServer`): Nach Bildschirmsperre/Ruhezustand bleibt der WebSocket oft
„offen“, ohne dass etwas kommt (die Seite zeigte dann Rennen, die längst vorbei waren). Der
Server schickt `live` mit 10 Hz, also: 3 s still oder Seite wieder sichtbar (`visibilitychange`,
`pageshow`, `online`) ohne frische Nachricht in der letzten Sekunde → sofort neu verbinden,
ohne aufs `close` der alten zu warten (deren Nachrichten werden ignoriert). Verbunden gilt erst
ab dem ersten `state`; jedes Neuverbinden zählt `changed` hoch (verpasste `session_changed`).
Uhrabgleich: größter Wert von `now_us − Browserzeit` der letzten 100 Nachrichten (jede ist
höchstens zu spät, nie zu früh), nicht der letzte: ein Rückstau alter Nachrichten verstellte
sonst Countdown und Rennuhr. Mehr als 2 s verspätete `announce` werden nicht gespielt.
Getestet 2026-10-30 in Node gegen den Server, eingefroren per `SIGSTOP` (neu nach 3–4 s bzw.
sofort bei `visibilitychange`); nicht in einem echten Browser.

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
| `src/detect.rs` | Durchflug-Erkennung ohne I/O (Schritte im Kopfkommentar): 10-ms-Slots, Hüllkurve (Max 20 ms, Median 100 ms) pro Board, Maximum über die Boards, Schwellen aus dem Flug (Kurs = Median bei Sender an, > −46 dBFS; Gipfel = Median der letzten 5 Durchflug-Gipfel, vorher 99,5-%-Perzentil; Eintritt 60 %, mindestens `min_rise_db` (Standard 12 dB, Einstellungen → Auswertung → Mindestausschlag; `replay --min-rise`) über dem Kurs, Austritt 20 %), Durchflug 1 s nach Austritt gemeldet, Mindestrunde 3 s. Nach dem 3. Durchflug werden frühe Fehltreffer zurückgenommen (`Event::Retract`). Gelernte Schwellen gibt es erst nach 5 s über −46 dBFS (`WARMUP_SLOTS`); bis dahin hält jeder Kanal seine letzten 10 s (`lookback`) und geht sie bei den ersten Schwellen einmal durch, ein dort gefundener Durchflug kommt verspätet mit seiner echten Zeit (Schritt 8; Session 32, 2026-10-31: Holeshot von R8 bei 31,0 s, auf dem Ziel-Gate leise −46,5 dBFS, Schwellen erst bei 33,2 s, jetzt bei 31,1 s erkannt). Feste Schwellen brauchen das nicht |
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
