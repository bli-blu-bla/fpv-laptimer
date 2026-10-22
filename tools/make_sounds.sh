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

# Start
speak piloten_bereit "Piloten, macht euch bereit"
speak rennen_beginnt "Rennen beginnt in"
speak training_beginnt "Training läuft"
tone start 880 0.8
# Numbers (countdown, lap numbers); higher ones are spoken by the server.
words=(null eins zwei drei vier fünf sechs sieben acht neun zehn elf zwölf dreizehn vierzehn fünfzehn)
for n in $(seq 1 15); do speak "$n" "${words[$n]}"; done
# Laps; the pilot names and lap times are spoken by the server.
tone runde 1320 0.12
speak ist_in_runde "ist in Runde"
speak pilot "Pilot"
speak neue_bestzeit "hat eine neue Bestzeit erzielt"
# End
speak ist_im_ziel "ist im Ziel"
speak rennen_beendet "Rennen beendet"
# Clips of older versions.
rm -f "$out"/pilot[1-8]_fertig.mp3 "$out/bestzeit.mp3"
ls "$out"
