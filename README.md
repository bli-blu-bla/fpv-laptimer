# FPV Laptimer

Einfache Rundenzeitmessung für FPV-Rennen am Stammtisch: ESP32-C5-Boards am Gate
messen die Videosignale der Piloten, ein Raspberry Pi erkennt die Durchflüge,
speichert sie und sagt an. Bedienung im Browser, ohne Rennen und Heats anlegen.

Status: im Aufbau.

## Aufbau

```
 Gate: [ESP links] [ESP rechts]  (+ optional [ESP Freigabe-Gate])
              \        |        /
                    USB
                     |
          Raspberry Pi (Server, Datenbank, Ansagen) --LAN-- Mini-Router --WLAN-- Handys
```

| Ordner | Inhalt |
|---|---|
| `firmware/` | ESP32-C5-Firmware: [widscan58](https://github.com/CapnBry/widscan58) von Bryan Mayland (per `git subtree`) plus unsere Erweiterungen |
| `server/` | Server in Rust (folgt) |
| `frontend/` | Oberfläche in React mit JavaScript + JSDoc (folgt) |
| `tools/` | Flash-Script, Mess- und Auswerte-Scripts |

## Firmware flashen

```sh
tools/flash_c5_many.sh          # baut einmal, dann ein Board nach dem anderen anstecken
tools/flash_c5_many.sh --all    # alle gerade angesteckten Boards
```

Braucht [PlatformIO](https://platformio.org) (`pio`) und Python 3.

## Danksagung

Die Firmware stammt aus [widscan58](https://github.com/CapnBry/widscan58)
(Bryan Mayland), die den Funkteil des ESP32 als SDR nutzt, über den I/Q-Debugpfad
aus [ESPARGOS/esp-sdr](https://github.com/ESPARGOS/esp-sdr).

## Lizenz

GPL-3.0-or-later, wie die Firmware (siehe `LICENSE`).
