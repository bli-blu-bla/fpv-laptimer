# Danksagung und Herkunft

Der FPV Laptimer steht unter der **GPL-3.0-or-later** (Text in [`LICENSE`](LICENSE)). Er baut
auf der Arbeit anderer auf, und ohne sie gäbe es ihn nicht.

## widscan58

Die Firmware der ESP32-C5-Boards ist
**[widscan58](https://github.com/CapnBry/widscan58)** von **Bryan Mayland (CapnBry)**,
GPL-3.0-or-later. Sie macht aus dem Funkteil des ESP32 einen Empfänger, der die FPV-Kanäle reihum
misst, und ist damit das Herzstück der Messung.

- Mit unseren Änderungen (Befehle `LEVELBATCH`, `TIME?`, `NAME`; Liste mit Datum dort in
  `NOTICE.md`) liegt sie im Repository
  **[fpv-laptimer-firmware](https://github.com/bli-blu-bla/fpv-laptimer-firmware)**, per
  `git subtree` aus widscan58 übernommen. Bis 2026-10-31 lag sie hier im Ordner `firmware/`
  (in der Geschichte dieses Repos noch zu finden).
- `frontend/src/flasher.js` (Flashen aus dem Browser) ist abgeleitet aus
  `web/src/serial/flasher.ts` von widscan58.
- Die Kanalwahl der Seite (`ChannelPicker.jsx`) ist der Ansicht „Channels to scan“ von widscan58
  nachempfunden.

## C5VRX und esp-sdr

widscan58 wiederum gäbe es nicht ohne **[C5VRX](https://github.com/Twotoz/C5VRX)** und die davon
abgeleiteten **[esp-sdr](https://github.com/ESPARGOS/esp-sdr)**-Projekte (GPL-3.0-or-later). Sie
haben den versteckten Debug-Pfad gefunden, über den ein ESP32 rohe I/Q-Werte aus seinem eigenen
Funkteil liest. Der Code für Empfang, Abstimmung und Kalibrierung in widscan58 baut auf ihrer
Arbeit auf; die davon abgeleiteten Dateien (im Firmware-Repo `firmware/src/radio/` und `firmware/src/scan/`)
sagen das in ihren Köpfen. Wer dieses Projekt mag, sollte sich auch ihre ansehen.

## Quellcode

Den vollständigen Quellcode des Laptimers gibt es für jede Version unter ihrem Tag unter
<https://github.com/bli-blu-bla/fpv-laptimer>.

Die Release-Pakete enthalten die Firmware als fertige Binärdatei (`web/firmware/`); welche
Version, steht in `web/firmware/manifest.json` und in `firmware.env` des Laptimer-Repos. Ihren
vollständigen Quellcode gibt es unter dem Tag dieser Version in
<https://github.com/bli-blu-bla/fpv-laptimer-firmware>; den der unveränderten Firmware unter
<https://github.com/CapnBry/widscan58>.

## Weitere Bibliotheken

Server und Seite nutzen Bibliotheken aus crates.io und npm (u. a. React, Phosphor Icons, die
Schrift Inter, esptool-js, espflash) unter ihren eigenen, freien Lizenzen; sie stehen in
`server/Cargo.lock` und `frontend/package-lock.json`.
