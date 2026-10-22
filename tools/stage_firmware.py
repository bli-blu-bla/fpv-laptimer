#!/usr/bin/env python3
"""Stage the built firmware for the web page's flasher. GPL-3.0-or-later.

Derived from widscan58's tools/pages_firmware.py (CapnBry/widscan58).

Copies the bootloader, partition table and app of firmware/.pio/build/esp32c5
into frontend/public/firmware/esp32c5/ and writes manifest.json (ESP Web
Tools' format plus size and sha256 per part). Vite copies public/ into
dist/, so both flash paths find it: the page itself (Web Serial) and the
server (`laptimer serve` reads <web>/firmware/).

    (cd firmware && pio run -e esp32c5)
    tools/stage_firmware.py            # then: cd frontend && npm run build

"version" is the one the firmware's INFO reports (project_description.json
of the build), so the page can tell which boards are up to date. Offsets
come from flasher_args.json (the C5's bootloader is at 0x2000).
"""
import argparse
import hashlib
import json
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUILD = ROOT / "firmware" / ".pio" / "build" / "esp32c5"
# flasher_args.json section -> file name in the PlatformIO build directory.
PARTS = {"bootloader": "bootloader.bin", "partition-table": "partitions.bin", "app": "firmware.bin"}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out", type=Path, nargs="?", default=ROOT / "frontend" / "public" / "firmware")
    a = ap.parse_args()

    args = json.loads((BUILD / "flasher_args.json").read_text())
    desc = json.loads((BUILD / "project_description.json").read_text())
    (a.out / "esp32c5").mkdir(parents=True, exist_ok=True)
    parts = []
    for section, name in PARTS.items():
        data = (BUILD / name).read_bytes()
        shutil.copyfile(BUILD / name, a.out / "esp32c5" / name)
        parts.append({
            "path": f"esp32c5/{name}",
            "offset": int(args[section]["offset"], 16),
            "size": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        })
    manifest = {
        "name": "Laptimer (widscan58)",
        "version": desc["project_version"],
        "date": None,
        "builds": [{"chipFamily": "ESP32-C5", "parts": parts}],
    }
    (a.out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"{manifest['version']}: " + ", ".join(f"{p['path']} @ {p['offset']:#x}" for p in parts))
    print(f"-> {a.out}")


if __name__ == "__main__":
    main()
