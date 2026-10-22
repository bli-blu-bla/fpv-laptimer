#!/usr/bin/env bash
# Makes placeholder announcements in sounds/ with macOS speech synthesis
# (`say`, German voice) and ffmpeg. Replace any of them with your own MP3s
# of the same name. GPL-3.0-or-later.
#
#   tools/make_sounds.sh [voice]     (default: Anna; `say -v '?'` lists them)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
out=$here/sounds
voice=${1:-Anna}
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

speak() { # name text
    say -v "$voice" -o "$tmp/$1.aiff" "$2"
    ffmpeg -loglevel error -y -i "$tmp/$1.aiff" -codec:a libmp3lame -q:a 4 "$out/$1.mp3"
}
tone() { # name frequency seconds
    ffmpeg -loglevel error -y -f lavfi -i "sine=frequency=$2:duration=$3" \
        -af "afade=t=out:st=$(echo "$3 - 0.05" | bc | sed "s|^\.|0.|"):d=0.05" -codec:a libmp3lame -q:a 4 "$out/$1.mp3"
}

speak rennen_beginnt "Rennen beginnt in"
speak 3 "drei"
speak 2 "zwei"
speak 1 "eins"
tone start 880 0.8
tone runde 1320 0.12
for n in 1 2 3 4 5 6 7 8; do speak "pilot${n}_fertig" "Pilot $n fertig"; done
speak rennen_beendet "Rennen beendet"
speak bestzeit "Neue Bestzeit"
ls "$out"
