# Entwickeln

## Bauen und starten

Braucht [Rust](https://rustup.rs) und Node.js.

```sh
(cd frontend && npm install && npm run build)   # Seite nach frontend/dist
cd server
cargo run --release -- serve                     # alle angesteckten Boards, Port 5800
```

Für die Oberfläche mit Neuladen bei jeder Änderung zusätzlich `cd frontend && npm run dev`
und `http://localhost:5173` öffnen (leitet `/api` und `/ws` auf Port 5800 weiter).

Zum Testen eine eigene Config und einen eigenen Datenordner nehmen, sonst landet alles in
`~/.laptimer`:

```sh
cargo run --release -- serve --config /tmp/lt/config.toml --data /tmp/lt/data
```

## Ohne Boards

`--replay` spielt eine Aufnahme in Endlosschleife ab, als kämen die Pegel von Boards:

```sh
tools/make_demo_recording.py /tmp/demo.csv            # erfundene Aufnahme: 4 Piloten, 2 + 1 Boards
cargo run --release -- serve --replay /tmp/demo.csv --sounds /tmp/leer
```

`--sounds` auf einen leeren Ordner, sonst sagt der Rechner jede Runde an. Board 2 der
Demo-Aufnahme ist als Freigabe-Board gedacht (Boards → Einrichtung → Rolle „Freigabe“).
Mit dieser Aufnahme entstehen auch die Screenshots dieser Seite (`tools/docs_screenshots.cjs`).

## Aufnehmen und auswerten

```sh
cargo run --release -- ports                       # gefundene Boards
cargo run --release -- record --seconds 60         # Pegel aller Boards nach recordings/
cargo run --release -- replay recordings/<datei>.csv
```

Aufnahmen sind CSV: Kommentarzeilen mit `#` (Boards, Kanäle, Gain), dann
`board,channel,host_us,board_us,rssi,flags`. `host_us` ist für alle Boards dieselbe Uhr.
`tools/plot_recording.py` zeichnet eine Aufnahme, `tools/detect_passes.py` ist die Erkennung
in Python zum Ausprobieren.

## Prüfen

```sh
(cd server && cargo test --release)
(cd frontend && npm run check)       # JSDoc-Typen mit tsc, TypeScript nur als Prüfer
```

Die CI (`.github/workflows/ci.yml`) macht dasselbe bei jedem Push.

## Code-Stil

- **Rust einfach halten:** Structs und Funktionen, wenig Generics und Makros, knifflige
  Ownership- und async-Stellen kommentiert. Threads und `mpsc` statt async, wo es geht.
- **Frontend in JavaScript** mit JSDoc und `// @ts-check`; `src/types.js` spiegelt das JSON des
  Servers.
- Fehlermeldungen der API sind deutsch; die Seite zeigt sie direkt an.
- Zeiten an die API immer als ganze Mikrosekunden.

## Releases

Ein Tag `v*` baut in GitHub Actions die Pakete für Linux und Mac, signiert sie und legt das
Release an. Lokal: `tools/build_release.sh`. Alles dazu unter
[Installation → Release bauen](../INSTALL.md#release-bauen-github-actions).

## Diese Doku

```sh
pip install zensical
zensical serve                     # http://127.0.0.1:8000
```

Gebaut wird mit [Zensical](https://zensical.org), dem Nachfolger von Material for MkDocs; es
liest die `mkdocs.yml` unverändert.

Die Seiten liegen in `docs/`, die Konfiguration in `mkdocs.yml`. Jeder Push auf `main`
veröffentlicht sie über `.github/workflows/docs.yml` auf GitHub Pages.
