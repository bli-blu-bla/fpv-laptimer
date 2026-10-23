# Laptimer auf dem Raspberry Pi

Ziel: Raspberry Pi 3 mit **64-bit Raspberry Pi OS Lite** (Bookworm oder neuer). Der Laptimer
läuft als Dienst des Nutzers, der ihn installiert; alles liegt in `~/.laptimer`.

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
| `laptimer-<ver>-aarch64.tar.gz` | `bin/laptimer`, `web/` (mit Firmware), `sounds/`, `hooks-examples/`, `install.sh`, `VERSION`, `minisign.pub` |
| `….tar.gz.minisig` | Signatur (minisign) |
| `….tar.gz.sha256` | Prüfsumme |

Eine Version mit Bindestrich (`v0.2.0-rc1`) wird als Vorabversion markiert. Jeder Push
(ohne Tag) läuft nur durch die Tests (`ci.yml`).

### Einmalig: Signatur-Schlüssel

Updates prüft der Server gegen den öffentlichen Schlüssel `minisign.pub` im Repo (fest in
die Binary eingebaut). Den privaten Schlüssel kennt nur die CI:

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
