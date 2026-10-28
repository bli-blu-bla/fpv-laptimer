#!/usr/bin/env python3
"""Writes an invented recording for demos and the documentation's screenshots:
four pilots on R1, R3, R6, R8, two boards at the finish gate and one at the
arming gate, in the format of `laptimer record`. Play it with

    laptimer serve --replay demo.csv

(board 2 then needs the role "Freigabe" on the Boards page).

The signal is a rough model of what the boards see: a course level around
-42 dBFS that wanders slowly, fast fading, about 10 % of the readings in
the video transmitter's pauses (noise floor), and per pass a bump of
16-26 dB lasting about half a second. The arming board sees each pilot
halfway round the lap and the finish gate only faintly. Same seed, same file.

    tools/make_demo_recording.py [OUT] [--minutes 4]
"""

import argparse
import math
import random

CHANNELS = [("R1", 5658), ("R3", 5732), ("R6", 5843), ("R8", 5917)]
# Mean lap per pilot (s): the ranking gets some spread.
LAP_MEAN = [19.5, 21.0, 22.5, 24.5]
HOP_US = 1000  # each board reads one channel per ms, so 250 per s per channel
BOARDS = 3  # 0, 1: finish gate; 2: arming gate


def code(dbfs):
    """dBFS → the firmware's level code (0.25 dB steps)."""
    return max(0, min(255, round((dbfs + 63.75) * 4)))


def passes(rng, mean, seconds):
    """Pass times (s) at the finish gate, and the arming gate's in between."""
    finish, arming = [], []
    t = rng.uniform(2.0, 6.0)
    while t < seconds + 30:
        finish.append(t)
        lap = rng.gauss(mean, 0.9)
        arming.append(t + lap * rng.uniform(0.45, 0.55))
        t += lap
    return finish, arming


def bump(t, times, width, heights):
    """The highest Gaussian bump of the passes near t (dB above the course)."""
    best = 0.0
    for p, h in zip(times, heights):
        d = t - p
        if abs(d) < 4 * width:
            best = max(best, h * math.exp(-0.5 * (d / width) ** 2))
    return best


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("out", nargs="?", default="demo.csv")
    ap.add_argument("--minutes", type=float, default=4.0)
    ap.add_argument("--seed", type=int, default=7)
    args = ap.parse_args()
    rng = random.Random(args.seed)
    seconds = args.minutes * 60

    pilots = []
    for mean in LAP_MEAN:
        finish, arming = passes(rng, mean, seconds)
        pilots.append(
            {
                "finish": finish,
                # Each pass its own height; the two finish boards differ a bit.
                "h": [[rng.uniform(16, 26) for _ in finish] for _ in range(2)],
                "dt": [rng.gauss(0, 0.05) for _ in finish],
                "arming": arming,
                "ha": [rng.uniform(15, 22) for _ in arming],
                "course": rng.uniform(-44, -40),
            }
        )

    with open(args.out, "w") as out:
        out.write("# laptimer recording, invented (tools/make_demo_recording.py)\n")
        for b in range(BOARDS):
            out.write(f"# board {b}: demo-{b} widscan58 demo\n")
        out.write("# channels: " + " ".join(f"{i}={n}@{m}" for i, (n, m) in enumerate(CHANNELS)) + "\n")
        out.write("# gain: 45, batch: 10 ms\n")
        out.write("board,channel,host_us,board_us,rssi,flags\n")
        drift = [0.0] * len(CHANNELS)
        t_us = 0
        hop = 0
        while t_us < seconds * 1e6:
            t = t_us / 1e6
            ch = hop % len(CHANNELS)
            p = pilots[ch]
            drift[ch] = max(-4, min(4, drift[ch] + rng.gauss(0, 0.02)))
            for b in range(BOARDS):
                if rng.random() < 0.10:
                    level = rng.gauss(-70, 1.5)  # transmitter pause
                else:
                    if b < 2:
                        times = [f + d * (1 if b == 0 else -1) for f, d in zip(p["finish"], p["dt"])]
                        rise = bump(t, times, 0.18, p["h"][b])
                    else:
                        rise = max(bump(t, p["arming"], 0.18, p["ha"]), 0.25 * bump(t, p["finish"], 0.18, p["h"][0]))
                    level = p["course"] + drift[ch] + rise + rng.gauss(0, 2.0)
                board_us = t_us + 123_456 * (b + 1)
                out.write(f"{b},{ch},{t_us + b * 300},{board_us},{code(level)},0\n")
            hop += 1
            t_us += HOP_US


if __name__ == "__main__":
    main()
