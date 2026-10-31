#!/usr/bin/env bash
# Build release packages: web page with the firmware, the server for each target,
# packed with everything the installation needs, plus install.sh for
# "curl … | bash". The CI runs this in parts (.github/workflows/release.yml);
# it runs locally too. GPL-3.0-or-later.
#
#   tools/build_release.sh [VERSION] [--target T]... [--no-firmware] [--no-web] [--web-only]
#
#   VERSION        default: git describe without the leading "v"
#   --target T     linux-aarch64 (default, the Pi), linux-x86_64,
#                  macos-aarch64, macos-x86_64; repeat for several
#   --no-firmware  don't fetch the firmware; whatever is in
#                  frontend/public/firmware is packed as it is
#                  (tools/fetch_firmware.sh --from … for an unreleased one)
#   --no-web       don't build the web page, pack frontend/dist as it is
#                  (the CI builds it once for all targets)
#   --web-only     only firmware and web page (frontend/dist), no packages
#
# The firmware is not built here: it is the release of fpv-laptimer-firmware
# that firmware.env pins (version + sha256), fetched by tools/fetch_firmware.sh.
#
# The packages carry LICENSE and NOTICE.md (where the firmware comes from,
# what we changed, where its source is: the GPL asks for it with binaries).
#
# Result: dist/laptimer-<VERSION>-<T>.tar.gz and its .sha256 per target, and
# dist/install.sh (with VERSION and the repository filled in; the repository
# is $LAPTIMER_REPO, default bli-blu-bla/fpv-laptimer). The CI signs the
# packages afterwards (minisign).
#
# Linux targets are cross-compiled with cargo-zigbuild against glibc 2.36
# (Pi OS Bookworm), so they also run on newer systems; that works on Linux
# and on a Mac. macOS targets build only on a Mac (Apple's linker).
#
# Needs: Rust with the targets (rustup target add aarch64-unknown-linux-gnu
# …), for Linux targets cargo-zigbuild and zig (`pip install cargo-zigbuild
# ziglang`), Node/npm, curl.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
glibc=2.36
repo=${LAPTIMER_REPO:-bli-blu-bla/fpv-laptimer}

version="" firmware=1 web=1 packages=1 targets=()
while [ $# -gt 0 ]; do
    case $1 in
        --target) [ $# -ge 2 ] || { echo "--target needs a target" >&2; exit 2; }; targets+=("$2"); shift ;;
        --no-firmware) firmware=0 ;;
        --no-web) web=0 ;;
        --web-only) packages=0 ;;
        -*) echo "unknown option $1" >&2; exit 2 ;;
        *) version=$1 ;;
    esac
    shift
done
[ ${#targets[@]} -gt 0 ] || targets=(linux-aarch64)
if [ -z "$version" ]; then
    version=$(git -C "$root" describe --tags --always --dirty)
    version=${version#v}
fi
echo "== laptimer $version"

# The Rust target of a package target.
triple() {
    case $1 in
        linux-aarch64) echo aarch64-unknown-linux-gnu ;;
        linux-x86_64) echo x86_64-unknown-linux-gnu ;;
        macos-aarch64) echo aarch64-apple-darwin ;;
        macos-x86_64) echo x86_64-apple-darwin ;;
        *) echo "unknown target $1 (linux-aarch64, linux-x86_64, macos-aarch64, macos-x86_64)" >&2; exit 2 ;;
    esac
}
for t in "${targets[@]}"; do triple "$t" >/dev/null; done

if [ $web = 1 ]; then
    if [ $firmware = 1 ]; then
        "$root/tools/fetch_firmware.sh"
    elif [ -f "$root/frontend/public/firmware/manifest.json" ]; then
        echo "== firmware: packing the staged one (--no-firmware)"
    else
        echo "== firmware: none (--no-firmware, nothing staged)"
    fi
    echo "== web page"
    (cd "$root/frontend" && npm ci --no-audit --no-fund && npm run build)
fi
[ $packages = 1 ] || exit 0
[ -f "$root/frontend/dist/index.html" ] || { echo "frontend/dist is missing (--no-web without a built page)" >&2; exit 1; }

sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi
}
# GNU tar (Linux) and bsdtar (Mac) name the owner options differently.
if tar --version 2>/dev/null | grep -q GNU; then
    owner=(--owner=0 --group=0)
else
    owner=(--uid 0 --gid 0)
fi

mkdir -p "$root/dist"
sed -e "s|@VERSION@|$version|g" -e "s|@REPO@|$repo|g" "$root/deploy/install.sh" > "$root/dist/install.sh"
chmod +x "$root/dist/install.sh"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

for t in "${targets[@]}"; do
    rust=$(triple "$t")
    echo "== server $t ($rust)"
    case $t in
        linux-*) (cd "$root/server" && LAPTIMER_VERSION=$version cargo zigbuild --release --target "$rust.$glibc") ;;
        macos-*) (cd "$root/server" && LAPTIMER_VERSION=$version MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --target "$rust") ;;
    esac

    echo "== package $t"
    name=laptimer-$version
    pkg=$stage/$t/$name
    mkdir -p "$pkg/bin"
    cp "$root/server/target/$rust/release/laptimer" "$pkg/bin/"
    cp -r "$root/frontend/dist" "$pkg/web"
    cp -r "$root/sounds" "$pkg/sounds"
    cp -r "$root/tools/hooks" "$pkg/hooks-examples"
    cp "$root/deploy/laptimer.service" "$root/deploy/update-check.sh" "$pkg/"
    cp "$root/dist/install.sh" "$pkg/"
    cp "$root/README.md" "$root/LICENSE" "$root/NOTICE.md" "$root/minisign.pub" "$pkg/"
    echo "$version" > "$pkg/VERSION"

    tarball=$root/dist/$name-$t.tar.gz
    tar -C "$stage/$t" "${owner[@]}" -czf "$tarball" "$name"
    (cd "$root/dist" && sha256 "$(basename "$tarball")" > "$(basename "$tarball").sha256")
    echo "== $tarball ($(du -h "$tarball" | cut -f1))"
done
