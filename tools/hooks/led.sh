#!/bin/sh
# Hook-Script: in den Script-Ordner kopieren (Standard server/data/hooks/, ausführbar).
# LED an einem GPIO des Raspberry Pi: led.sh <gpio> on|off|blink
# Braucht `pinctrl` (Raspberry Pi OS ab Bookworm; der Server-Nutzer muss in der
# Gruppe gpio sein). Pin-Nummern sind BCM-Nummern (GPIO17 = Pin 11).
set -e
pin=$1
case "$2" in
  on) pinctrl set "$pin" op dh ;;
  off) pinctrl set "$pin" op dl ;;
  blink)
    # 3× kurz; den Hook dafür „im Hintergrund“ eintragen, damit er nicht wartet
    for _ in 1 2 3; do
      pinctrl set "$pin" op dl; sleep 0.1
      pinctrl set "$pin" op dh; sleep 0.1
    done ;;
  *) echo "led.sh <gpio> on|off|blink" >&2; exit 2 ;;
esac
