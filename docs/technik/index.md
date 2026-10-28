# Technik

Für alle, die wissen wollen, wie es innen aussieht, mitentwickeln oder eigene Programme
anbinden.

| Seite | Inhalt |
|---|---|
| [Architektur](architektur.md) | Teile, Threads, Datenfluss, Uhren, Speicher, Installation und Updates |
| [Durchflug-Erkennung](erkennung.md) | vom Rohpegel zum Durchflug: Hüllkurve, gelernte Schwellen, Zurücknehmen, Freigabe |
| [Firmware und Protokoll](firmware.md) | widscan58, die Befehle des Servers, das Frame-Format, Uhrabgleich |
| [API-Referenz](../API.md) | alle Befehle, Ereignisse und Datenformate der API, Hooks |
| [Entwickeln](entwickeln.md) | bauen, ohne Boards testen, Aufnahmen, Releases |

Das Repository: [github.com/bli-blu-bla/fpv-laptimer](https://github.com/bli-blu-bla/fpv-laptimer)

| Ordner | Inhalt |
|---|---|
| `firmware/` | widscan58 (per `git subtree`) plus die Befehle `LEVELBATCH`, `TIME?`, `NAME` |
| `server/` | Rust: Server, Erkennung, Datenbank, Ansagen, Installation |
| `frontend/` | React in JavaScript mit JSDoc-Typen (`// @ts-check`) |
| `deploy/` | `install.sh`, systemd-Unit, `update-check.sh` |
| `tools/` | Release-Build, Flash-Script, Auswerte- und Demo-Scripts |
