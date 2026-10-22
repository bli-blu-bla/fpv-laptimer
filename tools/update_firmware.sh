#!/usr/bin/env bash
# Pull the upstream widscan58 firmware (CapnBry/widscan58, its firmware/ folder)
# into firmware/ with git subtree, merging it with our changes.
# GPL-3.0-or-later.
#
#   tools/update_firmware.sh [branch]     (default: main)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
branch=${1:-main}
[[ -z $(git -C "$here" status --porcelain) ]] || { echo "commit or stash your changes first" >&2; exit 1; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
git clone -q --branch "$branch" https://github.com/CapnBry/widscan58 "$tmp"
rev=$(git -C "$tmp" rev-parse --short HEAD)
# Only firmware/, as its own history (the same commits as the earlier splits).
git -C "$tmp" subtree split -q --prefix=firmware -b firmware-split
git -C "$here" subtree pull --prefix=firmware "$tmp" firmware-split \
    -m "Update the widscan58 firmware to CapnBry/widscan58 $rev"
