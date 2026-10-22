"""Find the passes in a `laptimer record` CSV, per board and with the boards combined.

    tools/.venv-analysis/bin/python tools/detect_passes.py <recording.csv> [--channel R3] [--plot]

The detection we try out before it goes into the server:
1. Envelope: the maximum over `--max-ms` (the raw levels drop to the noise
   floor every few captures), then the median over `--smooth-ms`.
2. Boards combined: the larger envelope of the boards at each moment.
3. Thresholds per pilot, from the recording itself: "course" = the median
   envelope while the transmitter is on, "peak" = the high percentile of it.
   A pass starts above course + enter·(peak − course) and ends below
   course + exit·(peak − course). Its time: the middle of the samples within
   `--top-db` of its maximum.
4. Passes closer than `--min-lap` seconds are one pass (the higher peak wins):
   a dip in the middle of a pass would otherwise count it twice.

GPL-3.0-or-later.
"""
import argparse
import re

import numpy as np
from numpy.lib.stride_tricks import sliding_window_view


def load(path):
    names = {}
    header = 0
    with open(path) as f:
        for line in f:
            header += 1
            if not line.startswith("#"):
                break
            if line.startswith("# channels:"):
                for idx, name in re.findall(r"(\d+)=([^@\s]+)@", line):
                    names[int(idx)] = name
    return np.loadtxt(path, delimiter=",", skiprows=header, dtype=np.int64), names


def rolling(x, n, fn):
    """fn over a centered window of n samples (edges padded)."""
    n = max(1, n)
    padded = np.pad(x, (n // 2, n - 1 - n // 2), mode="edge")
    return fn(sliding_window_view(padded, n), axis=1)


def envelope(t_s, db, max_ms, smooth_ms):
    """Envelope of one board's levels; the samples are ~2 ms apart."""
    dt_ms = np.median(np.diff(t_s)) * 1000
    mx = rolling(db, round(max_ms / dt_ms), np.max)
    return rolling(mx, round(smooth_ms / dt_ms), np.median)


def find_passes(t, e, enter_db, exit_db, top_db, min_lap):
    """Returns [(time, peak dB, seconds above enter)]."""
    passes = []
    start = None
    for i, v in enumerate(e):
        if start is None and v >= enter_db:
            start = i
        elif start is not None and v < exit_db:
            seg_t, seg_e = t[start:i], e[start:i]
            top = seg_t[seg_e >= seg_e.max() - top_db]
            p = ((top[0] + top[-1]) / 2, seg_e.max(), seg_t[-1] - seg_t[0])
            if passes and p[0] - passes[-1][0] < min_lap:
                if p[1] > passes[-1][1]:
                    passes[-1] = p
            else:
                passes.append(p)
            start = None
    return passes


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("csv")
    ap.add_argument("--channel", help="channel name (default: the one with the strongest signal)")
    ap.add_argument("--max-ms", type=float, default=20)
    ap.add_argument("--smooth-ms", type=float, default=100)
    ap.add_argument("--enter", type=float, default=0.6, help="fraction of course→peak")
    ap.add_argument("--exit", type=float, default=0.2, help="fraction of course→peak")
    ap.add_argument("--top-db", type=float, default=1.0)
    ap.add_argument("--min-lap", type=float, default=3.0, help="s")
    ap.add_argument("--plot", action="store_true", help="write <csv>_passes.png")
    a = ap.parse_args()

    d, names = load(a.csv)
    board, ch = d[:, 0], d[:, 1]
    t_all, db_all = d[:, 2] / 1e6, d[:, 4] / 4 - 63.75
    clipped_all = (d[:, 5] & 2) > 0
    if a.channel:
        c = next(k for k, v in names.items() if v == a.channel)
    else:
        c = max(set(ch), key=lambda k: np.percentile(db_all[ch == k], 99))
    print(f"channel {names.get(c, c)}; clipped captures: {clipped_all[ch == c].sum()}")

    envs = {}
    for b in sorted(set(board)):
        m = (board == b) & (ch == c)
        envs[f"board {b}"] = (t_all[m], envelope(t_all[m], db_all[m], a.max_ms, a.smooth_ms))
    t0 = next(iter(envs.values()))[0]
    combined = np.max([np.interp(t0, t, e) for t, e in envs.values()], axis=0)
    envs["combined"] = (t0, combined)

    # Thresholds from the combined envelope while the transmitter is on
    # (more than 15 dB above the quietest part of the recording).
    floor = np.percentile(combined, 2)
    on = combined > floor + 15
    if not on.any():
        print("no transmitter found")
        return
    course = np.median(combined[on])
    peak = np.percentile(combined[on], 99.5)
    enter_db = course + a.enter * (peak - course)
    exit_db = course + a.exit * (peak - course)
    print(f"noise {floor:.1f}, course {course:.1f}, peak {peak:.1f} dBFS -> enter {enter_db:.1f}, exit {exit_db:.1f}")

    results = {}
    for name, (t, e) in envs.items():
        p = find_passes(t, e, enter_db, exit_db, a.top_db, a.min_lap)
        results[name] = p
        laps = np.diff([x[0] for x in p])
        print(f"{name:>9}: {len(p)} passes  " + "  ".join(f"{x[0]:.2f}s({x[1]:.0f}dB,{x[2]:.1f}s)" for x in p))
        if len(laps):
            print(f"{'':>9}  laps: " + " ".join(f"{x:.2f}" for x in laps))

    if a.plot:
        import matplotlib

        matplotlib.use("Agg")
        import matplotlib.pyplot as plt

        fig, ax = plt.subplots(figsize=(14, 4.5))
        for name, (t, e) in envs.items():
            ax.plot(t, e, lw=0.8, label=name, **({"color": "k", "alpha": 0.6} if name == "combined" else {}))
        for y, lab in [(enter_db, "enter"), (exit_db, "exit"), (course, "course")]:
            ax.axhline(y, ls="--", lw=0.7, color="gray")
            ax.text(t0[0], y, lab, va="bottom", fontsize=8, color="gray")
        for x in results["combined"]:
            ax.axvline(x[0], color="red", lw=0.8)
        ax.set_xlabel("Zeit, s")
        ax.set_ylabel(f"{names.get(c, c)} dBFS (Hüllkurve)")
        ax.grid(alpha=0.3)
        ax.legend(loc="upper right")
        fig.tight_layout()
        out = re.sub(r"\.csv$", "", a.csv) + "_passes.png"
        fig.savefig(out, dpi=100)
        print(out)


if __name__ == "__main__":
    main()
