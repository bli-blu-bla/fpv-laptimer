# Boards und Debug

## Einrichtung

**Boards → Einrichtung** zeigt alle angesteckten ESP32-C5 mit Live-Pegel, Rolle (Ziel,
Freigabe, Aus) und Namen. Später eingesteckte Boards findet **Boards neu suchen**. Darunter
stehen beide Gates mit dem Erkennungsmodus jedes Kanals und „Alle automatisch / Alle automatisch
(experimentell) / Alle manuell“.

![Boards einrichten](../assets/screens/boards.png){ .shot }

**Firmware** spielt man ebenfalls hier auf, auch vom Handy aus: Der Server flasht ein Board,
das an ihm steckt, in etwa 10 s, und vergleicht danach die Version. Am Rechner mit Chrome oder
Edge geht es auch direkt aus dem Browser (Web Serial, nur über `localhost` oder HTTPS).
„veraltet“ steht neben einem Board nur, wenn seine Firmware älter ist als die, die der Laptimer
mitbringt.

Die Firmware hat außerdem eine eigene Seite:
**[bli-blu-bla.github.io/fpv-laptimer-firmware](https://bli-blu-bla.github.io/fpv-laptimer-firmware/)**.
Dort spielt man an jedem Rechner mit Chrome oder Edge die neueste Firmware auf, sieht Version
und Pegel eines Boards, gibt ihm einen Namen und schickt Befehle über eine Konsole.
Wie man ein neues Board einrichtet: [Installation → Boards flashen](../INSTALL.md#boards-flashen).

## Debug

Wenn ein Durchflug fehlt oder einer zu viel kommt, will man wissen, warum. **Boards → Debug**
zeigt pro Kanal einen Graph mit mehreren Serien übereinander:

- **Ziel kombiniert**: das, was zählt
- **Freigabe kombiniert**, falls vorhanden
- **jedes Board allein**: Hat ein Board den Piloten nicht gesehen? Liegt eines falsch?

Zu jeder Serie lassen sich Schwellen und erkannte Durchflüge einblenden. Die Tabelle darunter
legt die Durchflüge der Serien nebeneinander (innerhalb 1 s = derselbe) und zeigt den Abstand
zum kombinierten Ziel. So sieht man, ob ein Board immer früher oder später dran ist.

![Debug-Ansicht](../assets/screens/debug.png){ .shot }

Quellen:

- **Live**: die letzten 3 Minuten, mit Anhalten und Zurückblättern
- **Aufnahme**: mit **Aufnehmen** schreibt der Server alle Pegel in eine Datei, die später
  hier oder mit `laptimer replay` neu ausgewertet werden kann
- **Session**: die Pegel-Datei einer gespeicherten Session

Die Debug-Ansicht hat ihre eigenen Detektoren mit denselben Einstellungen; sie ändert nichts
an den Runden.

## Sensortest

Benimmt sich ein Board seltsam, prüft **Boards → Sensortest**, ob es am Board selbst liegt.
Der Test liest ein paar Sekunden lang mit, was alle Boards ohnehin melden (die Boards laufen
weiter, nichts wird umgestellt), und stellt sie pro Kanal nebeneinander. Ein Board lässt sich
nur im Vergleich beurteilen; am besten mit mindestens zwei Boards testen.

- **Ohne Sender:** Alle Sender aus, auch DJI-Brillen. Gemessen wird der Rauschboden. Ein
  beschädigter Eingang zeigt sich als Boden mehr als 3 dB über den anderen Boards oder als
  unruhiger Boden, eine Störlinie als einzelner Kanal deutlich über den übrigen. Empfangen
  mehrere Boards auf einem Kanal ein Signal, war wohl doch ein Sender an; der Kanal wird dann
  nicht bewertet.
- **Mit Referenz-Sender:** Ein Sender (am besten ein analoger VTX mit fester Leistung) an einer
  festen Stelle 2–5 m entfernt, alle Boards gleich ausgerichtet. Ein Board, das mehr als 6 dB
  schwächer empfängt als das beste, ist auffällig. Danach die Boards die Plätze tauschen lassen
  und noch einmal testen: Was am Board liegt, wandert mit, was am Platz liegt, nicht.

Daneben prüft er bei jedem Test Messrate, Lücken, verlorene Werte und fehlerhafte Pakete (CRC,
oft Kabel oder USB-Port). Ein Board, das auf der Strecke lauter ist als die anderen, ist nicht
unbedingt kaputt: Oft liegt es an Platz, Höhe oder Antenne.

## Debug-Paket

Für eine genauere Analyse (oder einen Fehlerbericht) exportiert **Sessions → ⤓ → Debug**
eine Session als `.tar.gz`: Einstellungen, alle Durchflüge auch gelöschte, Rohpegel, den
Verlauf der Erkennung je 10 ms und eine Neuberechnung mit den Session-Einstellungen. Siehe
[Durchflug-Erkennung](../technik/erkennung.md).
