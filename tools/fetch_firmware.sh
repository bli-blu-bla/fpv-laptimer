#!/usr/bin/env bash
# Put the firmware for the boards into frontend/public/firmware (manifest.json
# + esp32c5/*.bin), where the page's flasher and the server (<web>/firmware)
# find it. The firmware has its own repository and releases
# (github.com/bli-blu-bla/fpv-laptimer-firmware); firmware.env pins the
# version this laptimer brings along, with the sha256 of its tarball.
# GPL-3.0-or-later.
#
#   tools/fetch_firmware.sh                 the pinned version (build_release.sh does this)
#   tools/fetch_firmware.sh --update [VER]  pin VER (default: the latest release), then fetch it
#   tools/fetch_firmware.sh --from PATH     a tarball or a staged folder instead, e.g.
#                                           ../firmware/web/public/firmware (unpinned, for testing)
#
# $LAPTIMER_FIRMWARE_REPO: another repository (default bli-blu-bla/fpv-laptimer-firmware).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
repo=${LAPTIMER_FIRMWARE_REPO:-bli-blu-bla/fpv-laptimer-firmware}
out=$root/frontend/public/firmware
env_file=$root/firmware.env

mode=pinned arg=""
case ${1:-} in
    "") ;;
    --update) mode=update; arg=${2:-} ;;
    --from) mode=from; arg=${2:?--from needs a tarball or folder} ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
esac

sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Unpacks a tarball (or copies a folder) into $out, replacing what was there.
install_from() {
    local src=$1
    rm -rf "$out"
    mkdir -p "$out"
    if [ -d "$src" ]; then
        cp -R "$src"/. "$out"/
    else
        tar -xzf "$src" -C "$out"
    fi
    [ -f "$out/manifest.json" ] || { echo "no manifest.json in $src" >&2; exit 1; }
    echo "== firmware $(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$out/manifest.json") -> $out"
}

if [ $mode = from ]; then
    install_from "$arg"
    exit 0
fi

if [ $mode = update ]; then
    if [ -z "$arg" ]; then
        arg=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" |
            sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -1)
        [ -n "$arg" ] || { echo "no release found in $repo" >&2; exit 1; }
    fi
    version=${arg#v}
else
    # shellcheck source=/dev/null
    . "$env_file"
    version=$FIRMWARE_VERSION
    [ -n "$FIRMWARE_SHA256" ] || { echo "firmware.env has no FIRMWARE_SHA256 yet: tools/fetch_firmware.sh --update $version" >&2; exit 1; }
fi

name=fpv-laptimer-firmware-$version.tar.gz
echo "== downloading $repo v$version"
curl -fsSL -o "$tmp/$name" "https://github.com/$repo/releases/download/v$version/$name"
got=$(sha256 "$tmp/$name")

if [ $mode = update ]; then
    # The release's own .sha256, against a broken download.
    curl -fsSL -o "$tmp/$name.sha256" "https://github.com/$repo/releases/download/v$version/$name.sha256"
    want=$(cut -d' ' -f1 "$tmp/$name.sha256")
    [ "$got" = "$want" ] || { echo "sha256 mismatch: $got, release says $want" >&2; exit 1; }
    cat > "$env_file" <<PIN
# The firmware this laptimer brings along (web/firmware/ in the packages):
# a release of github.com/$repo, checked by the
# sha256 of its tarball. Change it with tools/fetch_firmware.sh --update [VERSION].
FIRMWARE_VERSION=$version
FIRMWARE_SHA256=$got
PIN
    echo "== pinned $version in firmware.env"
elif [ "$got" != "$FIRMWARE_SHA256" ]; then
    echo "sha256 mismatch for $name: $got, firmware.env says $FIRMWARE_SHA256" >&2
    exit 1
fi
install_from "$tmp/$name"
