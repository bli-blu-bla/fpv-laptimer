#!/usr/bin/env bash
# Installs the laptimer, or updates an installed one, for the user who runs
# it (not root). One command on the Raspberry Pi, a Linux PC or a Mac:
#
#   curl -fsSL https://github.com/@REPO@/releases/latest/download/install.sh | bash
#
# Options go after "bash -s --", e.g. "| bash -s -- --service":
#
#   --version X     this version instead of the newest
#   --service       run as a service that starts at boot (Linux: systemd,
#                   Mac: launchd); asked on the first install
#   --no-service    no service (removes one that is there)
#   -y, --yes       ask nothing (random password; Linux with service)
#   --rollback      back to the version installed before
#   --uninstall     remove the service (versions, config and data stay)
#
# From an unpacked package (no internet needed): ./install.sh
#
# The script downloads the package for this system and processor from the
# GitHub release (laptimer-<ver>-<linux|macos>-<aarch64|x86_64>.tar.gz),
# checks its sha256, sets up what needs sudo (Linux: the packages for the
# announcements, the groups dialout and audio, with a service also "linger"
# and the rule that lets the page shut the Pi down) and runs
# "bin/laptimer install" from the package, which does the rest: everything
# to ~/.laptimer, config.toml on the first install, the service.
# GPL-3.0-or-later.
set -euo pipefail

# Filled in by tools/build_release.sh.
repo=@REPO@
version=@VERSION@
home=$HOME/.laptimer
packages=(mpg123 alsa-utils libttspico-utils curl)

say() { printf '\033[1m%s\033[0m\n' "$*"; }
die() { printf 'Fehler: %s\n' "$*" >&2; exit 1; }

# Questions go to the terminal: with "curl … | bash" stdin is the script.
tty=0
if { : </dev/tty; } 2>/dev/null; then tty=1; fi
ask() { # ask <question> <default j|n> → yes?
    local answer
    if [ $tty = 0 ] || [ $yes = 1 ]; then answer=$2; else
        read -rp "$1 " answer </dev/tty || answer=""
        answer=${answer:-$2}
    fi
    case $answer in [jJyY]*) return 0 ;; *) return 1 ;; esac
}

mode=install service="" yes=0
while [ $# -gt 0 ]; do
    case $1 in
        --version) [ $# -ge 2 ] || die "--version braucht eine Version"; version=${2#v}; shift ;;
        --service) service=--service ;;
        --no-service) service=--no-service ;;
        -y|--yes) yes=1 ;;
        --rollback) mode=rollback ;;
        --uninstall) mode=uninstall ;;
        -h|--help) sed -n '2,27p' "${BASH_SOURCE[0]:-$0}" 2>/dev/null | sed 's/^# \{0,1\}//' || true; exit 0 ;;
        *) die "unbekannte Option $1 (--help)" ;;
    esac
    shift
done

[ "$(id -u)" != 0 ] || die "nicht als root ausführen: der Laptimer läuft als normaler Nutzer (sudo fragt das Script selbst, wo nötig)"

if [ $mode != install ]; then
    [ -x "$home/current/bin/laptimer" ] || die "kein Laptimer installiert ($home/current fehlt)"
    exec "$home/current/bin/laptimer" "$mode"
fi

case $(uname -s) in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    *) die "nur Linux und macOS, hier ist $(uname -s)" ;;
esac
case $(uname -m) in
    aarch64|arm64) arch=aarch64 ;;
    x86_64|amd64) arch=x86_64 ;;
    *) die "nur 64-bit ARM und x86, hier ist $(uname -m) (Raspberry Pi: 64-bit Pi OS nehmen)" ;;
esac
platform=$os-$arch

installed=$(cat "$home/current/VERSION" 2>/dev/null || true)

# The package: this script's folder if it is an unpacked one, else downloaded.
here=""
if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "${BASH_SOURCE[0]}" ]; then
    here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
fi
if [ -n "$here" ] && [ -f "$here/VERSION" ] && [ -x "$here/bin/laptimer" ]; then
    pkg=$here
    version=$(cat "$pkg/VERSION")
else
    case $version in @*|"")
        # Not from a release (straight from the repository): the newest.
        case $repo in @*) repo=bli-blu-bla/fpv-laptimer ;; esac
        version=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" |
            sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -1)
        [ -n "$version" ] || die "keine Version auf github.com/$repo gefunden"
        ;;
    esac
    if [ "$installed" = "$version" ] && [ -z "$service" ]; then
        say "Laptimer $version ist schon installiert (neueste Version)."
        echo "  Dienst einrichten oder entfernen: … | bash -s -- --service bzw. --no-service"
        exit 0
    fi
    if [ "$installed" = "$version" ]; then
        # Only the service changes: no download.
        pkg=$home/current
    else
        name=laptimer-$version-$platform.tar.gz
        url=https://github.com/$repo/releases/download/v$version
        tmp=$(mktemp -d)
        trap 'rm -rf "$tmp"' EXIT
        say "Lade $name"
        curl -fL --progress-bar -o "$tmp/$name" "$url/$name" ||
            die "$url/$name nicht ladbar (gibt es ein Paket für $platform?)"
        curl -fsSL -o "$tmp/$name.sha256" "$url/$name.sha256" || die "$name.sha256 nicht ladbar"
        if command -v sha256sum >/dev/null; then
            sum=$(sha256sum "$tmp/$name" | cut -d' ' -f1)
        else
            sum=$(shasum -a 256 "$tmp/$name" | cut -d' ' -f1)
        fi
        [ "$sum" = "$(cut -d' ' -f1 "$tmp/$name.sha256")" ] || die "Prüfsumme falsch: Download beschädigt"
        tar -xzf "$tmp/$name" -C "$tmp"
        pkg=$tmp/laptimer-$version
    fi
fi
[ -x "$pkg/bin/laptimer" ] || die "$pkg/bin/laptimer fehlt"

# Service: asked on the first install; an update keeps what is there.
if [ -z "$service" ] && [ -z "$installed" ]; then
    if [ $os = linux ]; then
        if ask "Als Dienst einrichten (startet beim Booten, empfohlen auf dem Pi)? [J/n]" j; then
            service=--service
        fi
    elif ask "Als Dienst einrichten (startet bei der Anmeldung am Mac)? [j/N]" n; then
        service=--service
    fi
fi
with_service=0
if [ "$service" = --service ] ||
    { [ -z "$service" ] && { [ -f "$HOME/.config/systemd/user/laptimer.service" ] ||
        [ -f "$HOME/Library/LaunchAgents/local.laptimer.plist" ]; }; }; then
    with_service=1
fi

# Linux: what needs sudo, only what is missing.
groups_missing="" # a string: bash 3.2 (Mac) dislikes empty arrays with set -u
if [ $os = linux ]; then
    missing=()
    if command -v dpkg >/dev/null; then
        for p in "${packages[@]}"; do
            dpkg -s "$p" >/dev/null 2>&1 || missing+=("$p")
        done
    fi
    for g in dialout audio; do
        getent group "$g" >/dev/null || continue
        id -nG | tr ' ' '\n' | grep -qx "$g" || groups_missing="$groups_missing $g"
    done
    linger=yes need_sudoers=0
    sudoers=/etc/sudoers.d/laptimer
    if [ $with_service = 1 ]; then
        linger=$(loginctl show-user "$USER" --property=Linger --value 2>/dev/null || echo no)
        # sudoers.d is only readable by root; ask sudo whether poweroff works
        # without a password (also true if the user may do everything).
        sudo -n -l /usr/bin/systemctl poweroff >/dev/null 2>&1 || need_sudoers=1
    fi
    if [ ${#missing[@]} -gt 0 ] || [ -n "$groups_missing" ] || [ "$linger" != yes ] || [ $need_sudoers = 1 ]; then
        say "Einmal mit sudo einrichten:"
        [ ${#missing[@]} -eq 0 ] || echo "  Pakete: ${missing[*]}"
        [ -z "$groups_missing" ] || echo "  Gruppen:$groups_missing (Boards an USB, Ton)"
        [ "$linger" = yes ] || echo "  Dienst startet beim Booten ohne Anmeldung (loginctl enable-linger)"
        [ $need_sudoers = 0 ] || echo "  Herunterfahren über die Seite ($sudoers)"
        if [ ${#missing[@]} -gt 0 ]; then
            sudo apt-get update -qq
            if ! sudo apt-get install -y -qq "${missing[@]}"; then
                # pico2wave is in non-free; without it the server speaks with espeak-ng.
                echo "  libttspico-utils nicht installierbar, nehme espeak-ng"
                sudo apt-get install -y -qq "${missing[@]/libttspico-utils/espeak-ng}"
            fi
        fi
        if [ -n "$groups_missing" ]; then
            sudo usermod -aG "$(echo $groups_missing | tr ' ' ,)" "$USER"
        fi
        [ "$linger" = yes ] || sudo loginctl enable-linger "$USER"
        if [ $need_sudoers = 1 ]; then
            rule=$(mktemp)
            echo "$USER ALL=(root) NOPASSWD: /usr/bin/systemctl poweroff, /usr/bin/systemctl reboot" > "$rule"
            sudo visudo -cqf "$rule" || die "sudoers-Regel ungültig"
            sudo install -m 0440 -o root -g root "$rule" "$sudoers"
            rm -f "$rule"
        fi
    fi
    if ! command -v dpkg >/dev/null; then
        echo "Kein Debian/Pi OS: für die Ansagen selbst mpg123 und pico2wave oder espeak-ng installieren, außerdem curl."
    fi
fi

# The password, on the first install (config.toml doesn't exist yet).
if [ ! -e "$home/config.toml" ] && [ $tty = 1 ] && [ $yes = 0 ]; then
    read -rsp "Passwort für die Rennleitung (leer = zufällig): " LAPTIMER_PASSWORD </dev/tty || true
    echo
    export LAPTIMER_PASSWORD
fi

"$pkg/bin/laptimer" install $service

if [ -n "$groups_missing" ]; then
    say "Neue Gruppen (${groups_missing# }) gelten erst nach einem Neustart: sudo reboot"
fi
