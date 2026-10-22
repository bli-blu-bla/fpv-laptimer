#!/usr/bin/env bash
# Flash the laptimer firmware onto ESP32-C5 boards (macOS and Linux).
# GPL-3.0-or-later.
#
# Builds the firmware once (skip with --no-build), then waits for a board on
# USB, flashes it, checks that it answers INFO, and waits for it to be
# unplugged before the next one. Ctrl-C ends it.
#
#   tools/flash_c5_many.sh [--no-build] [--all | --once] [--manual]
#
#   --no-build  flash the binaries already in firmware/.pio/build/esp32c5
#   --all       flash every board plugged in now, one after the other, and stop
#   --once      flash a single board and stop
#   --manual    the board is already in download mode (hold BOOT, tap RESET,
#               release BOOT): needed for a rev v1.0 C5 whose other firmware
#               hangs after a USB reset (esp-idf#18089)
#
# esptool and pyserial come from a private venv (tools/.venv-flash), created
# on the first run.
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
build=$here/firmware/.pio/build/esp32c5
venv=$here/tools/.venv-flash

do_build=1 once=0 all=0 manual=0
for arg in "$@"; do
    case $arg in
        --no-build) do_build=0 ;;
        --once) once=1 ;;
        --all) all=1 ;;
        --manual) manual=1 ;;
        -h|--help) sed -n '2,19p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

if [[ ! -x $venv/bin/python ]]; then
    echo "== Setting up esptool in $venv"
    python3 -m venv "$venv"
    "$venv/bin/pip" install -q --upgrade pip
    "$venv/bin/pip" install -q 'esptool>=5' pyserial
fi
py=$venv/bin/python

if (( do_build )); then
    echo "== Building firmware (esp32c5)"
    (cd "$here/firmware" && pio run -e esp32c5)
fi
for f in bootloader.bin partitions.bin firmware.bin; do
    [[ -f $build/$f ]] || { echo "missing $build/$f (build first)" >&2; exit 1; }
done

list_ports() {
    ls /dev/cu.usbmodem* /dev/serial/by-id/usb-Espressif_USB_JTAG_serial_debug_unit_* 2>/dev/null || true
}

count=0 failed=0
# Flash one board and check it; returns non-zero on failure.
flash_port() {
    local port=$1 before=usb-reset info
    echo "== Flashing $port"
    if (( manual )); then
        before=no-reset
    else
        # Stop a running Widscan58 stream so esptool's reset doesn't race it.
        "$py" -c '
import sys, time, serial
try:
    s = serial.Serial(sys.argv[1], 115200, timeout=0.1)
    s.write(b"\nSCAN STOP\nPROTOCOL RH\n"); time.sleep(0.3); s.close()
except Exception:
    pass
' "$port"
    fi

    if ! "$py" -m esptool --chip esp32c5 -p "$port" -b 921600 \
        --before "$before" --after watchdog-reset \
        write-flash --flash-mode dio --flash-freq 80m --flash-size 4MB \
        0x2000 "$build/bootloader.bin" 0x8000 "$build/partitions.bin" \
        0x10000 "$build/firmware.bin"; then
        echo "!! Flashing $port failed. If esptool says the port is busy, close the program" >&2
        echo "!! holding it (a browser tab with a web UI, RotorHazard, a serial monitor)." >&2
        echo "!! Otherwise hold BOOT, tap RESET, release BOOT, then run" >&2
        echo "!!   tools/flash_c5_many.sh --no-build --once --manual" >&2
        return 1
    fi
    # The board resets and comes back under the same name (USB location on
    # macOS, serial number on Linux).
    for _ in {1..50}; do [[ -e $port ]] && break; sleep 0.1; done
    sleep 1
    info=$("$py" -c '
import sys, time, serial
s = serial.Serial(sys.argv[1], 115200, timeout=0.3)
for _ in range(5):
    s.reset_input_buffer(); s.write(b"\nINFO\n"); time.sleep(0.5)
    for line in s.read(4096).decode(errors="replace").splitlines():
        if line.startswith("INFO "):
            print(line[5:]); sys.exit(0)
sys.exit(1)
' "$port" 2>/dev/null) || info=
    # pyserial leaves the tty at VMIN=0, which Chrome's Web Serial reads as a
    # lost device.
    stty -f "$port" min 1 time 0 2>/dev/null || stty -F "$port" min 1 time 0 2>/dev/null || true
    if [[ -z $info ]]; then
        echo "!! $port flashed, but it did not answer INFO. Press RESET and check it." >&2
        return 1
    fi
    count=$((count + 1))
    echo "== OK ($count flashed): $port $info"
}

if (( all )); then
    ports=$(list_ports)
    [[ -n $ports ]] || { echo "no ESP32-C5 found on USB" >&2; exit 1; }
    for port in $ports; do flash_port "$port" || failed=$((failed + 1)); echo; done
    echo "== $count flashed, $failed failed"
    (( failed == 0 ))
    exit
fi

while :; do
    echo
    echo "== Waiting for an ESP32-C5 on USB ..."
    port=
    while [[ -z $port ]]; do
        port=$(list_ports | head -1)
        [[ -n $port ]] || sleep 0.5
    done
    sleep 1 # let the USB device settle
    flash_port "$port" || true
    (( once )) && break
    echo "== Unplug the board (Ctrl-C to stop)"
    while [[ -n $(list_ports) ]]; do sleep 0.5; done
done
