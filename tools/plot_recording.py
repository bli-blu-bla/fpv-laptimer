"""Plot a `laptimer record` CSV: the level of each channel over time, per board.

    tools/.venv-analysis/bin/python tools/plot_recording.py <recording.csv> [--channel R3] [--from 10 --to 40] [--out plot.png]

Setup once: python3 -m venv tools/.venv-analysis && tools/.venv-analysis/bin/pip install numpy matplotlib
GPL-3.0-or-later.
"""
import argparse
import re

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np


def load(path):
    """Returns (data, channel names). data columns: board, channel, host_us, board_us, rssi, flags."""
    names = {}
    header = 0  # comment lines and the column names
    with open(path) as f:
        for line in f:
            header += 1
            if not line.startswith("#"):
                break
            if line.startswith("# channels:"):
                for idx, name in re.findall(r"(\d+)=([^@\s]+)@", line):
                    names[int(idx)] = name
    data = np.loadtxt(path, delimiter=",", skiprows=header, dtype=np.int64)
    return data, names


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("csv")
    ap.add_argument("--channel", help="only this channel (name, e.g. R3)")
    ap.add_argument("--from", dest="t_from", type=float, help="start, s")
    ap.add_argument("--to", dest="t_to", type=float, help="end, s")
    ap.add_argument("--out", help="PNG file (default: next to the CSV)")
    a = ap.parse_args()

    d, names = load(a.csv)
    board, ch, host = d[:, 0], d[:, 1], d[:, 2] / 1e6
    db = d[:, 4] / 4 - 63.75
    clipped = (d[:, 5] & 2) > 0
    keep = np.ones(len(d), bool)
    if a.t_from is not None:
        keep &= host >= a.t_from
    if a.t_to is not None:
        keep &= host <= a.t_to
    channels = sorted(set(ch[keep]))
    if a.channel:
        channels = [c for c in channels if names.get(c) == a.channel]
    boards = sorted(set(board[keep]))

    fig, axes = plt.subplots(len(channels), 1, figsize=(14, 3.2 * len(channels)), sharex=True, squeeze=False)
    for ax, c in zip(axes[:, 0], channels):
        for b in boards:
            m = keep & (ch == c) & (board == b)
            ax.plot(host[m], db[m], lw=0.6, label=f"Board {b}")
            mc = m & clipped
            ax.plot(host[mc], db[mc], "x", ms=3, color="red", label="übersteuert" if b == boards[0] else None)
        ax.set_ylabel(f"{names.get(c, c)}  dBFS")
        ax.grid(alpha=0.3)
        ax.legend(loc="upper right")
    axes[-1, 0].set_xlabel("Zeit, s")
    fig.tight_layout()
    out = a.out or re.sub(r"\.csv$", "", a.csv) + (f"_{a.channel}" if a.channel else "") + ".png"
    fig.savefig(out, dpi=110)
    print(out)


if __name__ == "__main__":
    main()
