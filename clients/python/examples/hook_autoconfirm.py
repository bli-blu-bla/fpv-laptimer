#!/usr/bin/env python3
"""Hook-Script: bestätigt ein Rennen von selbst, wenn alle Piloten im Ziel
sind, und schreibt den Sieger in `sieger.txt`.

Einrichten auf dem Pi:
1. Dieses Script und `laptimer_client/__init__.py` (umbenannt in
   `laptimer_client.py`) in den Script-Ordner kopieren (Standard
   `server/data/hooks/`), `chmod +x hook_autoconfirm.py`.
2. Einen API-Schlüssel anlegen und in `api_key.txt` daneben legen.
3. Auf der Seite *API & Hooks → Hooks*: Event `session_stopped` → dieses
   Script, und Event `race_confirmed` → dieses Script.

Der Hook läuft im Script-Ordner; das Event steht in `LAPTIMER_JSON`.
"""

from pathlib import Path

from laptimer_client import Event, Laptimer

ev = Event.from_env()

if ev.event == "session_stopped" and ev.data["race"] and ev.data["reason"] == "finished":
    # Alle im Ziel: Ergebnis bestätigen. Von Hand gestoppte Rennen
    # (reason "stopped") bleiben für die Rennleitung offen.
    lt = Laptimer("http://localhost:5800", key=Path("api_key.txt").read_text().strip())
    lt.confirm(ev.data["session_id"])

elif ev.event == "race_confirmed":
    with open("sieger.txt", "a") as f:
        f.write(f"Session {ev.data['session_id']}: {ev.data['winner']}\n")
