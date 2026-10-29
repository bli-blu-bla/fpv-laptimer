#!/usr/bin/env python3
"""Ein Rennen fahren: Piloten auf die Kanäle, Rennen starten, jede Runde
ausgeben, am Ende das Ergebnis zeigen und bestätigen oder verwerfen.

    python race.py --url http://pi:5800 --key lt_... --laps 3 NITRO ACE BOB

Braucht `websockets` (pip install "fpv-laptimer-client[events]").
"""

import argparse

from laptimer_client import Laptimer, LaptimerError


def main() -> None:
    parser = argparse.ArgumentParser(description="Ein Rennen fahren und mitschreiben.")
    parser.add_argument("--url", default="http://localhost:5800")
    parser.add_argument("--key", help="API-Schlüssel (sonst LAPTIMER_API_KEY)")
    parser.add_argument("--laps", type=int, default=3)
    parser.add_argument("--countdown", type=int, default=5)
    parser.add_argument("pilots", nargs="*", help="Callsigns, der Reihe nach pro Kanal")
    a = parser.parse_args()

    lt = Laptimer(a.url, key=a.key)
    if a.pilots:
        for ch in lt.assign_channels(a.pilots, create=True):
            print(f"{ch['name']}: {ch['shown']}")

    @lt.on("connected")
    def start(ev):
        # Erst nach dem Verbinden starten, damit kein Event verloren geht.
        # (Nach einem Wiederverbinden läuft das Rennen schon.)
        if ev.data["session"] is None:
            try:
                lt.start_session(race=True, laps=a.laps, countdown_s=a.countdown)
            except LaptimerError as e:
                print("Start abgelehnt:", e)
                lt.stop()

    @lt.on("countdown")
    def countdown(ev):
        print(ev.data["seconds_left"], "…")

    @lt.on("start_signal")
    def go(ev):
        print("Los!")

    @lt.on("pilot_pass")
    def lap(ev):
        d = ev.data
        if d["kind"] == "lap":
            best = "  Bestzeit!" if d["session_best"] else ""
            print(f"{d['pilot']:>10}  Runde {d['lap']}: {d['lap_time_s']:6.2f} s  (Platz {d['position']}){best}")
        elif d["kind"] == "holeshot":
            print(f"{d['pilot']:>10}  Holeshot")

    @lt.on("pilot_finished")
    def finished(ev):
        d = ev.data
        print(f"{d['pilot']:>10}  im Ziel als {d['position']}., {d['total_s']:.2f} s")

    @lt.on("session_stopped")
    def stopped(ev):
        d = ev.data
        print("\nErgebnis:")
        for r in d["results"]:
            total = f"{r['total_us'] / 1e6:.2f} s" if r["total_us"] else ""
            print(f"  {r['position'] or '-'}. {r['pilot']:<10} {r['laps']} Runden  {total}")
        if d["needs_confirmation"]:
            if input("Ergebnis bestätigen? [j/n] ").strip().lower().startswith("j"):
                lt.confirm(d["session_id"])
                print("Bestätigt.")
            else:
                lt.discard(d["session_id"])
                print("Verworfen.")
        lt.stop()

    lt.run_forever()


if __name__ == "__main__":
    main()
