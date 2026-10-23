#!/bin/sh
# Run by systemd before each start of the laptimer (ExecStartPre in
# laptimer.service): goes back to the previous version if an update from the
# web page doesn't come up. GPL-3.0-or-later.
#
# The server writes ~/.laptimer/update-pending before switching to the new
# version (to=releases/<new>, from=releases/<old>, previous=<the previous
# link before>, attempts=0) and deletes it once the new version has run for
# 15 s (server/src/update.rs). Each start counts here; the 4th start without
# that switches current back to `from`, previous back to what it was (not to
# the broken version), and writes update-failed, which the page shows.
home=$HOME/.laptimer
marker=$home/update-pending
[ -f "$marker" ] || exit 0

to=$(sed -n 's/^to=//p' "$marker")
from=$(sed -n 's/^from=//p' "$marker")
previous=$(sed -n 's/^previous=//p' "$marker")
attempts=$(sed -n 's/^attempts=//p' "$marker")
attempts=$((${attempts:-0} + 1))

# Installed by hand meanwhile (install.sh): not ours to judge.
if [ "$(readlink "$home/current")" != "$to" ]; then
    rm -f "$marker"
    exit 0
fi

if [ "$attempts" -le 3 ]; then
    printf 'to=%s\nfrom=%s\nprevious=%s\nattempts=%s\n' "$to" "$from" "$previous" "$attempts" > "$marker.new"
    mv -f "$marker.new" "$marker"
    exit 0
fi

if [ -n "$from" ] && [ -x "$home/$from/bin/laptimer" ]; then
    echo "laptimer: $to kommt nicht hoch, zurück auf $from" >&2
    ln -sfn "$from" "$home/current.new" && mv -Tf "$home/current.new" "$home/current"
    if [ -n "$previous" ] && [ -x "$home/$previous/bin/laptimer" ]; then
        ln -sfn "$previous" "$home/previous.new" && mv -Tf "$home/previous.new" "$home/previous"
    else
        rm -f "$home/previous"
    fi
    printf 'to=%s\nfrom=%s\ntime=%s\n' "$to" "$from" "$(date +%s)" > "$home/update-failed"
fi
rm -f "$marker"
