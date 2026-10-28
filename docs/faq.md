# Häufige Fragen

## Hardware

??? question "Was brauche ich?"
    - **2–3 ESP32-C5-Boards** mit externer Antenne (getestet: Seeed XIAO ESP32-C5), zwei am
      Ziel-Gate, optional eines als [Freigabe-Gate](funktionen/gates.md). Ein Board reicht
      zum Ausprobieren, zwei sind deutlich zuverlässiger.
    - **Einen Rechner**, an dem die Boards per USB stecken: Raspberry Pi 3 oder neuer mit
      64-bit Pi OS, oder ein Linux-PC oder Mac.
    - **Einen WLAN-Router** für die Handys (der Pi per LAN daran).
    - Optional einen **Lautsprecher** für die Ansagen.

??? question "Warum ESP32-C5 und nicht RX5808 wie bei RotorHazard?"
    Der ESP32-C5 ist ein günstiges Board, das 5-GHz-WLAN kann. Die Firmware
    [widscan58](https://github.com/CapnBry/widscan58) macht daraus einen Empfänger, der mehrere
    Kanäle reihum misst: ein Board für alle Piloten statt ein Modul pro Pilot. Gelötet werden
    muss nichts, die Boards hängen nur per USB am Rechner.

??? question "Wie viele Piloten gleichzeitig?"
    Bis zu 8 Kanäle. Gedacht und eingestellt ist der Laptimer für 4 (R1, R3, R6, R8).
    Je mehr Kanäle, desto seltener wird jeder gemessen: bei 4 alle 2 ms.
    Vier Piloten gleichzeitig sind bisher nur mit Aufnahmen getestet.

??? question "Welche Videosysteme gehen?"
    Gemessen wird nur der Pegel auf dem Kanal, nicht das Bild. Getestet ist bisher DJI
    mit 25 mW. DJI sendet in Paketen; die Erkennung ist darauf ausgelegt, dass etwa jede
    fünfte Messung in eine Sendepause fällt. Andere Systeme sollten gehen, sind aber noch
    nicht geprüft.

??? question "Geht Bluetooth zu den Boards?"
    Nein. Das Funkmodul ist durchs Scannen belegt; die Boards brauchen USB.

## Betrieb

??? question "Muss ich etwas kalibrieren?"
    Normalerweise nicht. Die Erkennung lernt den Pegel der Strecke und die Höhe der
    Durchflüge aus dem Flug selbst. In den ersten drei Durchflügen eines Piloten schätzt sie
    noch; danach nimmt sie Fehltreffer vom Anfang automatisch zurück. Wer feste Schwellen
    will, stellt pro Kanal auf Manuell, siehe [Rennleitung](funktionen/rennleitung.md#schwellen-wenn-man-will).

??? question "Brauche ich am Platz Internet?"
    Nein. Seite, Schrift und Icons liefert der Pi selbst aus. Internet braucht nur die
    Installation und „Nach Updates suchen“; Updates gehen auch per Datei-Upload vom Handy.

??? question "Können mehrere Handys gleichzeitig zugreifen?"
    Ja, beliebig viele im selben WLAN. Ansehen geht ohne Anmeldung; Starten, Korrigieren und
    Einstellen nur mit dem Passwort der Rennleitung.

??? question "Wie genau ist die Zeit?"
    Für Rundenzeiten unter Freunden gut genug, für Hundertstel-Entscheidungen nicht. Zwei
    Boards lagen für denselben Durchflug bis zu 0,3–0,4 s auseinander, weil der Gipfel ein
    breiter Buckel ist. Mehr im [Konzept](konzept.md#genauigkeit).

??? question "Ein Durchflug fehlt oder einer ist zu viel. Was tun?"
    Nach dem Rennen im Editor korrigieren: an den Gipfel tippen fügt einen ein, einen
    vorhandenen antippen und löschen. Ergebnis und Bestzeiten rechnen sofort neu. Siehe
    [Runden korrigieren](funktionen/korrigieren.md).

??? question "Was ist mit RotorHazard?"
    Der Laptimer ist ein eigenes Programm und braucht RotorHazard nicht. Er übernimmt ein paar
    Ideen (Schwellen wie EnterAt/ExitAt, die Marshal-Seite zum Korrigieren, die IMD-Bewertung
    der Kanäle), verzichtet aber auf Heats, Klassen und Event-Verwaltung.

## Daten

??? question "Wo liegen die Daten, wie sichere ich sie?"
    Alles in `~/.laptimer/data`: die Datenbank (SQLite) und pro Session eine Datei mit den
    Rohpegeln. Unter **Einstellungen → System → Backups** legt man eine gepackte Kopie der
    Datenbank an und lädt sie herunter. Vor Updates und vor „Alle Sessions löschen“ macht der
    Server selbst ein Backup.

??? question "Kann ich Ergebnisse exportieren?"
    Ja, unter **Sessions** eine oder mehrere Sessions als CSV (für Excel, mit Dezimalkomma)
    oder JSON, wahlweise Runden oder Ergebnis, alle oder einzelne Kanäle.

??? question "Unter welcher Lizenz steht das?"
    GPL-3.0-or-later, wie die Firmware widscan58.
