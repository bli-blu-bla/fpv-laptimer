# Danksagung

Der FPV Laptimer baut auf der Arbeit anderer auf. Ohne sie gäbe es ihn nicht.

## widscan58

Die Firmware der Boards ist **[widscan58](https://github.com/CapnBry/widscan58)** von
**Bryan Mayland (CapnBry)**. Sie macht aus dem Funkteil eines ESP32 einen Empfänger, der die
FPV-Kanäle reihum misst: das Herzstück der Messung. Im Laptimer liegt sie per `git subtree` im
Ordner `firmware/`, ergänzt um drei Befehle für den Server (`LEVELBATCH`, `TIME?`, `NAME`;
siehe [Firmware und Protokoll](technik/firmware.md)). Auch das Flashen aus dem Browser und die
Kanalwahl der Seite stammen aus bzw. folgen widscan58.

## C5VRX und esp-sdr

widscan58 wiederum gäbe es nicht ohne **[C5VRX](https://github.com/Twotoz/C5VRX)** und die
davon abgeleiteten **[esp-sdr](https://github.com/ESPARGOS/esp-sdr)**-Projekte. Sie haben den
versteckten Debug-Pfad gefunden, über den ein ESP32 rohe I/Q-Werte aus seinem eigenen Funkteil
liest, und damit eine ganze Reihe von Projekten möglich gemacht. Empfang, Abstimmung und
Kalibrierung in widscan58 bauen auf ihrer Arbeit auf. Wer dieses Projekt mag, sollte sich auch
ihre ansehen.

## RotorHazard

Von **[RotorHazard](https://github.com/RotorHazard/RotorHazard)** stammen viele Ideen:
Eintritts- und Austrittsschwellen und die Marshal-Seite zum Korrigieren. Die IMD-Bewertung der
Kanäle rechnet wie ETs [IMDTabler](https://etserv.etheli.com/IMDTabler/), den auch RotorHazard
nutzt. Code ist keiner übernommen.

## Lizenz

Der Laptimer steht wie widscan58 und esp-sdr unter der **GPL-3.0-or-later**. Welche Dateien
woher stammen, was an der Firmware geändert wurde (mit Datum) und wo der Quellcode zu den
Release-Paketen liegt, steht in
[`NOTICE.md`](https://github.com/bli-blu-bla/fpv-laptimer/blob/main/NOTICE.md); jedes
Release-Paket enthält diese Datei und `LICENSE`.
