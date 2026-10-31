# Laptimer installieren

Läuft auf dem **Raspberry Pi** (64-bit Raspberry Pi OS, Bookworm oder neuer), auf einem
**Linux-PC** (x86_64 oder ARM, glibc ≥ 2.36) und auf dem **Mac** (Apple Silicon oder Intel).
Der Laptimer läuft als der Nutzer, der ihn installiert; alles liegt in `~/.laptimer`.

## Installieren oder aktualisieren

Ein Befehl auf dem Zielrechner, als normaler Nutzer (nicht root):

```sh
curl -fsSL https://github.com/bli-blu-bla/fpv-laptimer/releases/latest/download/install.sh | bash
```

Das Script erkennt System und Prozessor, lädt das passende Paket aus dem neuesten Release,
prüft die Prüfsumme und richtet alles ein. Ist schon eine Version installiert, wird sie
aktualisiert: Einstellungen und Daten bleiben, die alte Version wird zur „vorigen“ (zurück mit
`--rollback`). Ist die neueste schon installiert, passiert nichts.

Beim ersten Mal fragt es:

- **Als Dienst einrichten?** Der Dienst startet den Laptimer beim Booten (Pi, Linux: systemd)
  bzw. bei der Anmeldung (Mac: launchd) und nach einem Update über die Seite neu. Auf dem Pi
  empfohlen (Standard Ja), auf dem Mac Standard Nein. Ohne Dienst startet man ihn selbst mit
  `~/.laptimer/current/bin/laptimer serve`.
- **Passwort für die Rennleitung** (leer = zufällig, wird angezeigt; steht in
  `~/.laptimer/config.toml`).

Unter Linux einmal per sudo, nur was fehlt:

- Pakete `mpg123`, `alsa-utils`, `libttspico-utils` (Sprachausgabe; geht das nicht,
  `espeak-ng`), `curl` (nur auf Debian/Pi OS; sonst zeigt das Script, was man selbst installiert)
- Gruppen `dialout` (Boards an USB) und `audio`; neue Gruppen gelten erst nach `sudo reboot`
- mit Dienst: `loginctl enable-linger` (startet beim Booten ohne Anmeldung) und
  `/etc/sudoers.d/laptimer` (nur `systemctl poweroff` und `reboot` ohne Passwort, damit die
  Seite den Pi herunterfahren kann)

Auf dem Mac braucht es nichts weiter (Ton über `afplay`, Sprache über `say`).

Optionen hängt man mit `bash -s --` an:

```sh
curl -fsSL …/install.sh | bash -s -- --service       # Dienst einrichten (auch nachträglich)
curl -fsSL …/install.sh | bash -s -- --no-service    # ohne Dienst (entfernt einen vorhandenen)
curl -fsSL …/install.sh | bash -s -- --version 0.2.0 # eine bestimmte Version
curl -fsSL …/install.sh | bash -s -- -y              # nichts fragen
curl -fsSL …/install.sh | bash -s -- --rollback      # zurück auf die vorige Version
curl -fsSL …/install.sh | bash -s -- --uninstall     # Dienst entfernen (Daten bleiben)
```

**Ohne Internet am Zielrechner:** das Paket für den Rechner aus dem Release laden
(`laptimer-<ver>-linux-aarch64.tar.gz` für den Pi, `-linux-x86_64`, `-macos-aarch64`,
`-macos-x86_64`), hinüberkopieren, dann

```sh
tar xzf laptimer-0.2.0-linux-aarch64.tar.gz
laptimer-0.2.0/install.sh
```

Das Script nimmt dann das Paket, in dem es liegt. Dahinter steht `bin/laptimer install
[--service | --no-service]`, das geht auch direkt.

### Raspberry Pi vorbereiten

1. Mit dem Raspberry Pi Imager **Raspberry Pi OS Lite (64-bit)** auf die SD-Karte schreiben;
   in den Einstellungen des Imagers Hostname (z. B. `laptimer`), Nutzer mit Passwort, SSH
   und ggf. WLAN setzen.
2. Pi per LAN an den Router, einschalten, per `ssh <nutzer>@<hostname>.local` anmelden.
3. ESP-Boards per USB anstecken (vorher geflasht, siehe [Boards flashen](#boards-flashen)),
   Lautsprecher an die Klinke oder per USB.
4. Den Befehl oben ausführen. Danach läuft der Laptimer unter `http://<hostname>.local:5800`
   (das Script zeigt auch die IP).

### Boards flashen

Neue ESP32-C5 brauchen einmal die Firmware. Am einfachsten an einem Computer mit Chrome oder
Edge auf der **[Firmware-Seite](https://bli-blu-bla.github.io/fpv-laptimer-firmware/)**:

1. Board per USB anstecken, **Board verbinden …** und den Port wählen.
2. **Aufspielen**: spielt die neueste Firmware auf (etwa 10 s).
3. Optional einen **Namen** geben (z. B. „Gate links“). Er bleibt auf dem Board, auch beim
   nächsten Flashen, und der Laptimer zeigt ihn unter **Boards** statt der MAC.

Die Seite zeigt außerdem Version und Chip eines Boards, die Pegel der Kanäle live (zum
Prüfen der Antenne) und eine Konsole für Befehle. Hängt ein Board beim Flashen: BOOT halten,
RESET drücken, BOOT loslassen, dann noch einmal.

Steckt ein Board schon am Pi, geht es auch ohne Computer: in der Laptimer-Seite unter
**Boards → Firmware** (siehe [Boards und Debug](funktionen/boards.md)). Der Laptimer bringt
eine feste Firmware-Version mit; eine neuere von der Firmware-Seite zeigt er als „neuer“ an,
das ist kein Fehler.

## Was wo liegt

| Pfad | Inhalt |
|---|---|
| `~/.laptimer/config.toml` | Host, Port, Passwort (`[auth]`), Update-Quelle (`[update]`), Pfade; danach neu starten |
| `~/.laptimer/current` | → `releases/<ver>`, die laufende Version |
| `~/.laptimer/previous` | → die Version davor |
| `~/.laptimer/releases/` | die drei neuesten Versionen (`current` und `previous` bleiben immer) |
| `~/.laptimer/data/` | Datenbank, Pegel, Backups, `hooks/` (Beispiele in `releases/<ver>/hooks-examples/`) |
| `~/.laptimer/update-pending`, `update-failed` | nur während bzw. nach einem missglückten Update über die Seite |
| `~/.config/systemd/user/laptimer.service` | der Dienst (Linux) |
| `~/Library/LaunchAgents/local.laptimer.plist` | der Dienst (Mac), Log in `~/.laptimer/laptimer.log` |

Bedienen (Linux):

```sh
systemctl --user status laptimer
systemctl --user restart laptimer
journalctl --user -u laptimer -f     # Log (ohne persistentes Journal: journalctl --user-unit laptimer -f)
```

Mac: `launchctl kickstart -k gui/$(id -u)/local.laptimer` startet neu,
`tail -f ~/.laptimer/laptimer.log` zeigt das Log.

## Update über die Seite

**Einstellungen → System** (mit Login; nicht während einer Session):

- **Nach Updates suchen:** fragt die GitHub-Releases, wenn der Rechner ins Internet kommt, und
  holt das Paket für diesen Rechner.
- **Paket hochladen:** Paket und Signatur aus dem Release laden
  (`laptimer-<ver>-<system>.tar.gz` und `….tar.gz.minisig`) und beide zusammen wählen (geht auch
  am Handy, ohne Internet am Platz).

Der Server prüft die Signatur (nur Pakete, die die CI signiert hat), zeigt die Version und
installiert erst nach „Installieren“: Backup `…_vor-update`, neue Version nach `releases/`,
`current` umgesetzt, Neustart (die Seite lädt sich danach selbst neu). Mit Dienst startet
systemd bzw. launchd die neue Version, ohne Dienst ersetzt sich der Server selbst durch sie.
Eine ältere oder dieselbe Version nur nach Rückfrage.

Kommt die neue Version nicht hoch (startet nicht oder stürzt in den ersten 15 s ab), geht der
Dienst beim 4. Start automatisch auf die vorige zurück (`update-check.sh`); die Seite zeigt das
dann an. Von Hand: „Zurück auf …“ auf der Seite, oder `install.sh --rollback`.

Eine andere Quelle (ein Fork, eine eigene Forgejo-Instanz) in `~/.laptimer/config.toml`:

```toml
[update]
url = "https://github.com/<besitzer>/<repo>"   # oder https://<forgejo>/<besitzer>/<repo>; "" = nur Hochladen
token = "…"   # nur für ein privates Repo (GitHub: Fine-grained, Contents lesen; Forgejo: read:repository)
```

Dort auch: Laptimer neu starten, Pi neu starten, Pi herunterfahren (vor dem Strom-Abziehen).

## Release bauen (GitHub Actions)

Ein Tag `v*` baut in der CI (`.github/workflows/release.yml`) ein Release:

```sh
git tag v0.2.0
git push origin v0.2.0
```

Die CI testet (Server, Seite), baut die Seite einmal mit der Firmware aus `firmware.env`
(ein Release von [fpv-laptimer-firmware](https://github.com/bli-blu-bla/fpv-laptimer-firmware),
geprüft per sha256; `tools/fetch_firmware.sh`), den Server für Linux
(`aarch64`, `x86_64`, per cargo-zigbuild) und auf einem macOS-Runner für den Mac (`aarch64`,
`x86_64`), packt alles (`tools/build_release.sh`), signiert die Pakete und legt das Release an:

| Datei | Inhalt |
|---|---|
| `install.sh` | das Installations-Script, Version und Repo eingetragen (`releases/latest/download/install.sh` zeigt immer auf das neueste) |
| `laptimer-<ver>-<system>.tar.gz` | `bin/laptimer`, `web/` (mit Firmware), `sounds/`, `hooks-examples/`, `install.sh`, `laptimer.service`, `update-check.sh`, `VERSION`, `minisign.pub` |
| `….tar.gz.minisig` | Signatur (minisign) |
| `….tar.gz.sha256` | Prüfsumme (prüft `install.sh`) |

Eine Version mit Bindestrich (`v0.2.0-rc1`) wird als Vorabversion markiert; `latest` überspringt
sie. Jeder Push (ohne Tag) läuft nur durch die Tests (`ci.yml`).

### Einmalig: Signatur-Schlüssel

Updates über die Seite prüft der Server gegen den öffentlichen Schlüssel `minisign.pub` im Repo
(fest in die Binary eingebaut; weitere Schlüssel, z. B. nach einem Schlüsselwechsel, mit
`extra_keys = ["RW…"]` unter `[update]` in `config.toml`). Den privaten Schlüssel kennt nur die CI:

1. Schlüsselpaar erzeugen (fragt nach einem Passwort):
   ```sh
   minisign -G -p minisign.pub -s ~/.minisign/laptimer.key
   ```
   `minisign.pub` ins Repo committen. Den privaten Schlüssel **nicht** ins Repo (`.gitignore`
   schließt `*.key` aus) und sicher aufheben: ohne ihn lassen sich keine Updates mehr
   signieren, die bestehende Installationen annehmen.
2. Auf GitHub: Repository → Settings → Secrets and variables → Actions → New repository secret:
   - `RELEASE_SIGNING_KEY`: der ganze Inhalt der Schlüsseldatei (beide Zeilen)
   - `RELEASE_SIGNING_PASSWORD`: das Passwort

Ein Paket von Hand prüfen:

```sh
minisign -Vm laptimer-0.2.0-linux-aarch64.tar.gz -p minisign.pub
```

### Lokal bauen

Ohne CI, z. B. zum Testen auf dem Pi:

```sh
pip install cargo-zigbuild ziglang                         # einmal
rustup target add aarch64-unknown-linux-gnu                # einmal
tools/build_release.sh                                     # Pi, Version aus git describe
tools/build_release.sh 0.2.0 --no-firmware                 # Firmware in frontend/public/firmware so lassen
tools/build_release.sh --target linux-x86_64 --target macos-aarch64   # andere Ziele (macOS nur auf dem Mac)
```

Ergebnis in `dist/`. Die Linux-Server sind gegen glibc 2.36 gelinkt (Pi OS Bookworm) und
laufen auch auf neueren Systemen.
