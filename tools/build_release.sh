#!/usr/bin/env bash
# Build a release for the Raspberry Pi (64-bit Pi OS): firmware, web page,
# server (cross-compiled), packed with everything the installation needs.
# The CI runs this (.forgejo/workflows/release.yml); it runs locally too.
# GPL-3.0-or-later.
#
#   tools/build_release.sh [VERSION] [--no-firmware]
#
#   VERSION        default: git describe without the leading "v"
#   --no-firmware  don't build the firmware; a staged one
#                  (frontend/public/firmware) is packed as it is
#
# Result: dist/laptimer-<VERSION>-aarch64.tar.gz and its .sha256. The CI
# signs it afterwards (minisign).
#
# Needs: Rust with the target aarch64-unknown-linux-gnu, cargo-zigbuild and
# zig (`pip install cargo-zigbuild ziglang`), Node/npm, Python 3, and for the
# firmware PlatformIO (`pio`).
#
# The server is linked against glibc 2.36 (Pi OS Bookworm), so it also runs
# on newer ones (Trixie).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
target=aarch64-unknown-linux-gnu
glibc=2.36

version="" firmware=1
for arg in "$@"; do
    case $arg in
        --no-firmware) firmware=0 ;;
        -*) echo "unknown option $arg" >&2; exit 2 ;;
        *) version=$arg ;;
    esac
done
if [ -z "$version" ]; then
    version=$(git -C "$root" describe --tags --always --dirty)
    version=${version#v}
fi
echo "== laptimer $version"

if [ $firmware = 1 ]; then
    echo "== firmware"
    (cd "$root/firmware" && pio run -e esp32c5)
    python3 "$root/tools/stage_firmware.py"
elif [ -f "$root/frontend/public/firmware/manifest.json" ]; then
    echo "== firmware: packing the staged one (--no-firmware)"
else
    echo "== firmware: none (--no-firmware, nothing staged)"
fi

echo "== web page"
(cd "$root/frontend" && npm ci --no-audit --no-fund && npm run build)

echo "== server ($target, glibc $glibc)"
(cd "$root/server" && LAPTIMER_VERSION=$version cargo zigbuild --release --target "$target.$glibc")

echo "== package"
name=laptimer-$version
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
pkg=$stage/$name
mkdir -p "$pkg/bin"
cp "$root/server/target/$target/release/laptimer" "$pkg/bin/"
cp -r "$root/frontend/dist" "$pkg/web"
cp -r "$root/sounds" "$pkg/sounds"
cp -r "$root/tools/hooks" "$pkg/hooks-examples"
# install.sh and the systemd unit (Phase 3: deploy/).
if [ -d "$root/deploy" ]; then
    cp -r "$root/deploy/." "$pkg/"
fi
cp "$root/README.md" "$root/LICENSE" "$root/minisign.pub" "$pkg/"
echo "$version" > "$pkg/VERSION"

mkdir -p "$root/dist"
tarball=$root/dist/$name-aarch64.tar.gz
tar -C "$stage" --owner=0 --group=0 -czf "$tarball" "$name"
(cd "$root/dist" && sha256sum "$(basename "$tarball")" > "$(basename "$tarball").sha256")
echo "== $tarball ($(du -h "$tarball" | cut -f1))"
