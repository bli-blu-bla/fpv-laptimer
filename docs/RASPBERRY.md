# Laptimer auf dem Raspberry Pi

Ziel: Raspberry Pi 3 mit **64-bit Raspberry Pi OS Lite** (Bookworm oder neuer). Der Laptimer
läuft als Dienst des Nutzers, der ihn installiert; alles liegt in `~/.laptimer`.

## Pi vorbereiten

1. Mit dem Raspberry Pi Imager **Raspberry Pi OS Lite (64-bit)** auf die SD-Karte schreiben;
   in den Einstellungen des Imagers Hostname (z. B. `laptimer`), Nutzer mit Passwort, SSH
   und ggf. WLAN setzen.
2. Pi per LAN an den Router, einschalten, per `ssh <nutzer>@<hostname>.local` anmelden.
3. ESP-Boards per USB anstecken, Lautsprecher an die Klinke oder per USB.

## Installieren

Das Release (`laptimer-<ver>-aarch64.tar.gz`) aus Forgejo herunterladen und auf den Pi
kopieren, z. B. `scp laptimer-0.2.0-aarch64.tar.gz <nutzer>@<hostname>.local:`, dann auf dem Pi:

```sh
tar xzf laptimer-0.2.0-aarch64.tar.gz
cd laptimer-0.2.0
./install.sh
```

Als normaler Nutzer, nicht als root. Beim ersten Mal fragt das Script nach dem **Passwort für
die Rennleitung** (leer = zufällig, wird angezeigt) und einmal per sudo nach dem, was fehlt:

- Pakete `mpg123`, `alsa-utils`, `libttspico-utils` (Sprachausgabe; geht das nicht, `espeak-ng`),
  `curl` (Updates von Forgejo)
- Gruppen `dialout` (Boards an USB) und `audio`
- `loginctl enable-linger`: der Dienst startet beim Booten ohne Anmeldung
- `/etc/sudoers.d/laptimer`: nur `systemctl poweroff` und `reboot` ohne Passwort, damit die
  Seite den Pi herunterfahren kann

Kamen neue Gruppen dazu, einmal `sudo reboot`. Danach läuft der Laptimer unter
`http://<hostname>.local:5800` (das Script zeigt auch die IP).

| Pfad | Inhalt |
|---|---|
| `~/.laptimer/config.toml` | Port, Passwort (`[auth]`), Pfade; nach Änderungen `systemctl --user restart laptimer` |
| `~/.laptimer/current` | → `releases/<ver>`, die laufende Version |
| `~/.laptimer/previous` | → die Version davor |
| `~/.laptimer/releases/` | die drei neuesten Versionen (`current` und `previous` bleiben immer) |
| `~/.laptimer/data/` | Datenbank, Pegel, Backups, `hooks/` (Beispiele in `releases/<ver>/hooks-examples/`) |
| `~/.laptimer/update-pending`, `update-failed` | nur während bzw. nach einem missglückten Update über die Seite |
| `~/.config/systemd/user/laptimer.service` | der Dienst |

Bedienen:

```sh
systemctl --user status laptimer
systemctl --user restart laptimer
journalctl --user -u laptimer -f     # Log (ohne persistentes Journal: journalctl --user-unit laptimer -f)
~/.laptimer/current/install.sh --rollback    # zurück auf die vorige Version
~/.laptimer/current/install.sh --uninstall   # Dienst entfernen (Daten bleiben)
```

Eine neue Version installiert man genauso (entpacken, `./install.sh`): Config und Daten
bleiben, die alte Version wird `previous`.

## Update über die Seite

**Einstellungen → System** (mit Login; nicht während einer Session):

- **Paket hochladen:** aus dem Forgejo-Release `laptimer-<ver>-aarch64.tar.gz` und
  `laptimer-<ver>-aarch64.tar.gz.minisig` herunterladen und beide zusammen wählen (geht auch
  am Handy, ohne Internet am Platz).
- **Nach Updates suchen:** fragt Forgejo direkt, wenn der Pi ins Internet kommt. Dafür in
  `~/.laptimer/config.toml` (danach `systemctl --user restart laptimer`):
  ```toml
  [update]
  url = "https://git.homeserver.local/bli-blu-bla/LapTimer"
  token = "…"   # Forgejo: Einstellungen → Anwendungen → Token, Berechtigung read:repository
  ```
  Braucht `curl` (installiert `install.sh`).

Der Server prüft die Signatur (nur Pakete, die die CI signiert hat), zeigt die Version und
installiert erst nach „Installieren“: Backup `…_vor-update`, neue Version nach
`releases/`, `current` umgesetzt, Neustart (die Seite lädt sich danach selbst neu). Eine
ältere oder dieselbe Version nur nach Rückfrage.

Kommt die neue Version nicht hoch (startet nicht oder stürzt in den ersten 15 s ab), geht
systemd beim 4. Start automatisch auf die vorige zurück (`update-check.sh` in der Unit); die
Seite zeigt das dann an. Von Hand: „Zurück auf …“ auf der Seite, oder
`~/.laptimer/current/install.sh --rollback`.

Dort auch: Laptimer neu starten, Pi neu starten, Pi herunterfahren (vor dem Strom-Abziehen).

## Release bauen (Forgejo)

Ein Tag `v*` baut in der CI (`.forgejo/workflows/release.yml`) ein Release:

```sh
git tag v0.2.0
git push origin v0.2.0
```

Die CI testet (Server, Seite), baut die Firmware, die Seite und den Server für aarch64
(`tools/build_release.sh`), signiert das Paket und hängt es an das Release im Repo:

| Datei | Inhalt |
|---|---|
| `laptimer-<ver>-aarch64.tar.gz` | `bin/laptimer`, `web/` (mit Firmware), `sounds/`, `hooks-examples/`, `install.sh`, `laptimer.service`, `update-check.sh`, `VERSION`, `minisign.pub` |
| `….tar.gz.minisig` | Signatur (minisign) |
| `….tar.gz.sha256` | Prüfsumme |

Eine Version mit Bindestrich (`v0.2.0-rc1`) wird als Vorabversion markiert. Jeder Push
(ohne Tag) läuft nur durch die Tests (`ci.yml`).

### Einmalig: Signatur-Schlüssel

Updates prüft der Server gegen den öffentlichen Schlüssel `minisign.pub` im Repo (fest in
die Binary eingebaut; weitere Schlüssel, z. B. nach einem Schlüsselwechsel, mit
`extra_keys = ["RW…"]` unter `[update]` in `config.toml`). Den privaten Schlüssel kennt nur die CI:

1. Schlüsselpaar erzeugen (fragt nach einem Passwort):
   ```sh
   minisign -G -p minisign.pub -s ~/.minisign/laptimer.key
   ```
   `minisign.pub` ins Repo committen. Den privaten Schlüssel **nicht** ins Repo (`.gitignore`
   schließt `*.key` aus) und sicher aufheben: ohne ihn lassen sich keine Updates mehr
   signieren, die bestehende Installationen annehmen.
2. In Forgejo: Repository → Einstellungen → Actions → Secrets:
   - `RELEASE_SIGNING_KEY`: der ganze Inhalt der Schlüsseldatei (beide Zeilen)
   - `RELEASE_SIGNING_PASSWORD`: das Passwort
3. Actions müssen für das Repo eingeschaltet sein (Einstellungen → Einheiten → Actions), und
   ein Runner mit dem Label `ubuntu-latest` (Docker) muss laufen.

Ein Paket von Hand prüfen:

```sh
minisign -Vm laptimer-0.2.0-aarch64.tar.gz -p minisign.pub
```

### Lokal bauen

Ohne CI, z. B. zum Testen auf dem Pi:

```sh
pip install cargo-zigbuild ziglang          # einmal
rustup target add aarch64-unknown-linux-gnu  # einmal
tools/build_release.sh                       # Version aus git describe
tools/build_release.sh 0.2.0 --no-firmware   # ohne PlatformIO
```

Ergebnis in `dist/`. Der Server ist gegen glibc 2.36 gelinkt (Pi OS Bookworm) und läuft auch
auf neueren Systemen.
