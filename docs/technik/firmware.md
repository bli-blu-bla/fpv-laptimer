# Firmware und Protokoll

## widscan58

Die Firmware im Ordner `firmware/` ist [widscan58](https://github.com/CapnBry/widscan58) von
Bryan Mayland, per `git subtree` übernommen (`tools/update_firmware.sh` holt neue Versionen).
Sie nutzt den Funkteil des ESP32-C5 über den I/Q-Debugpfad (aus
[ESPARGOS/esp-sdr](https://github.com/ESPARGOS/esp-sdr)) als Software-Empfänger für 5,8 GHz.
Für den Laptimer sind drei Befehle dazugekommen: `LEVELBATCH`, `TIME?` und `NAME`.

Flashen: `tools/flash_c5_many.sh` (PlatformIO), oder über die Seite unter
**Boards → Firmware**.

## Einstellen beim Verbinden

Die Firmware startet stumm im RotorHazard-Modus. Der Server öffnet den Port und stellt sie ein:

```text
PROTOCOL MONITOR
CHANNELS R1=5658 R3=5732 R6=5843 R8=5917
METRICS RSSI
FFT 64
CAPN 256
SPEC 0
LEVELBATCH 10
```

Danach kommen die Pegel als Binär-Frames; Antworten auf Befehle stehen als Textzeilen
dazwischen.

## Frames

```text
A5 'R' type version  u16 len  payload…  u32 crc32
```

Little Endian, CRC32 (zlib) über alles davor. Der Laptimer braucht Typ 7,
`FRAME_LEVEL_BATCH`: die Pegel aller Kanal-Sprünge der letzten `LEVELBATCH` Millisekunden in
einem Frame, höchstens 128 Einträge.

| Teil | Felder |
|---|---|
| Kopf, 18 Byte | `u32 seq`, `u64 t0_us`, `u16 lost`, `u8 gain`, `u8 mode`, `u16 n` |
| je Eintrag, 8 Byte | `u32 dt_us`, `u8 id` (Kanal), `u8 flags`, `u8 rssi`, `u8 snr` |

Zeit eines Eintrags = `t0_us + dt_us` (Mitte der Messung, Board-Uhr). Pegel in 0,25-dB-Schritten:
dBFS = `rssi` / 4 − 63,75.

Gemessen mit `LEVELBATCH 10` und vier Kanälen: rund 491–493 Pegel pro Sekunde und Kanal (alle
2,0 ms, längste Lücke etwa 3 ms), 96 Frames/s, etwa 18 KB/s pro Board, keine Verluste, auch mit
zwei Boards gleichzeitig.

## Uhrabgleich

Jedes Board zählt seine eigenen Mikrosekunden. Damit Pegel mehrerer Boards zusammenpassen,
rechnet der Server jede Board-Zeit in seine eigene um (`clock.rs`):

1. `TIME?` schicken, Antwort `TIME <us>` abwarten, Umlaufzeit messen. Von je 10 Antworten
   zählt die mit der kürzesten Umlaufzeit; deren Board-Zeit gehört zur Mitte des Umlaufs.
2. Durch diese Schätzungen der letzten 5 Minuten eine Ausgleichsgerade legen. Die Drift
   (gemessen +4 und +8 ppm) wird erst ab 20 s Spanne geschätzt.

Die kürzeste Umlaufzeit über USB lag am Mac bei 0,7–0,9 ms, an einem Port bei 2,4 ms, also
±0,4 bis ±1,2 ms Unsicherheit. Vor dem Scan-Start (ohne Pegelstrom) gelingt der Abgleich auf
±0,1 ms. Gegenüber der Breite eines Durchflug-Gipfels (Zehntelsekunden) ist das klein.

## Board-Kennung und Namen

Jedes Board wird an seiner USB-Seriennummer erkannt (= MAC-Adresse). Die Rollen (Ziel,
Freigabe, Aus) hängen an dieser Kennung, nicht am Port; Umstecken ändert nichts.

`NAME <text>` speichert einen Namen im NVS des Boards (bleibt beim Flashen), `NAME?` liest ihn,
`NAME` allein löscht ihn. Höchstens 32 Byte UTF-8, ohne `|` und Steuerzeichen.

## Ohne `libudev`

Die Crate `serialport` läuft ohne `libudev`, damit der Cross-Build für den Pi keine
C-Bibliothek braucht. Boards werden am Portnamen gefunden (`/dev/ttyACM*`,
`/dev/cu.usbmodem*`), die Kennung unter Linux aus `/dev/serial/by-id`.
