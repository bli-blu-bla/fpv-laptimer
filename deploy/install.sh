#!/usr/bin/env bash
# Installs this laptimer release for the user who runs it (not root).
# GPL-3.0-or-later.
#
#   ./install.sh               install or update to this release, start it
#   ./install.sh --rollback    back to the release installed before
#   ./install.sh --uninstall   stop and remove the service (data stays)
#
# Everything goes to ~/.laptimer:
#   config.toml        port, password (asked for on the first install)
#   current            -> releases/<version>, the one running
#   previous           -> releases/<version>, the one before (--rollback)
#   releases/<version> bin/, web/, sounds/, … (the three newest are kept)
#   data/              database, levels, backups, hooks/
# and the service to ~/.config/systemd/user/laptimer.service.
#
# Needs sudo once (only for what is missing): the packages for the
# announcements, the groups dialout (boards on USB) and audio, "linger"
# (the service starts at boot without a login), and a sudoers rule that lets
# the web page shut the Pi down (systemctl poweroff/reboot, nothing else).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
home=$HOME/.laptimer
unit_dir=$HOME/.config/systemd/user
keep=3
packages=(mpg123 alsa-utils libttspico-utils curl)

say() { printf '\033[1m%s\033[0m\n' "$*"; }
die() { printf 'Fehler: %s\n' "$*" >&2; exit 1; }

mode=install
case ${1:-} in
    "") ;;
    --rollback) mode=rollback ;;
    --uninstall) mode=uninstall ;;
    -h|--help) sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unbekannte Option $1 (--rollback, --uninstall)" ;;
esac

[ "$(id -u)" != 0 ] || die "nicht als root ausführen: der Laptimer läuft als normaler Nutzer (sudo fragt das Script selbst, wo nötig)"

# The user's systemd needs these to find the user's bus over ssh/sudo -u.
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}

restart_service() {
    systemctl --user daemon-reload
    systemctl --user enable laptimer.service >/dev/null 2>&1
    systemctl --user restart laptimer.service
}

# --- uninstall ---------------------------------------------------------------
if [ $mode = uninstall ]; then
    systemctl --user disable --now laptimer.service 2>/dev/null || true
    rm -f "$unit_dir/laptimer.service"
    systemctl --user daemon-reload || true
    say "Dienst entfernt. Daten und Releases liegen weiter in $home (von Hand löschen, wenn gewünscht)."
    exit 0
fi

# --- rollback ----------------------------------------------------------------
if [ $mode = rollback ]; then
    prev=$(readlink "$home/previous" 2>/dev/null) || die "keine vorige Version ($home/previous fehlt)"
    [ -x "$home/$prev/bin/laptimer" ] || die "$home/$prev ist nicht mehr da"
    cur=$(readlink "$home/current")
    rm -f "$home/update-pending" # a rollback by hand ends a pending update
    ln -sfn "$prev" "$home/current.new" && mv -Tf "$home/current.new" "$home/current"
    ln -sfn "$cur" "$home/previous.new" && mv -Tf "$home/previous.new" "$home/previous"
    restart_service
    say "Zurück auf $(basename "$prev") (vorher $(basename "$cur"))."
    exit 0
fi

# --- install -----------------------------------------------------------------
[ -f "$here/VERSION" ] && [ -x "$here/bin/laptimer" ] || die "install.sh aus dem entpackten Release starten (bin/laptimer, VERSION fehlen)"
version=$(cat "$here/VERSION")
arch=$(uname -m)
[ "$arch" = aarch64 ] || die "dieses Release ist für aarch64 (64-bit Pi OS), hier ist $arch"
say "Laptimer $version"

# System (sudo), only what is missing.
missing=()
for p in "${packages[@]}"; do
    dpkg -s "$p" >/dev/null 2>&1 || missing+=("$p")
done
groups_missing=()
for g in dialout audio; do
    id -nG | tr ' ' '\n' | grep -qx "$g" || groups_missing+=("$g")
done
linger=$(loginctl show-user "$USER" --property=Linger --value 2>/dev/null || echo no)
sudoers=/etc/sudoers.d/laptimer
sudoers_line="$USER ALL=(root) NOPASSWD: /usr/bin/systemctl poweroff, /usr/bin/systemctl reboot"
need_sudoers=0
# sudoers.d is only readable by root; ask sudo whether poweroff works without
# a password (also true if the user may do everything, as Pi OS's first user).
sudo -n -l /usr/bin/systemctl poweroff >/dev/null 2>&1 || need_sudoers=1

if [ ${#missing[@]} -gt 0 ] || [ ${#groups_missing[@]} -gt 0 ] || [ "$linger" != yes ] || [ $need_sudoers = 1 ]; then
    say "Einmal mit sudo einrichten:"
    [ ${#missing[@]} -eq 0 ] || echo "  Pakete: ${missing[*]}"
    [ ${#groups_missing[@]} -eq 0 ] || echo "  Gruppen: ${groups_missing[*]}"
    [ "$linger" = yes ] || echo "  Dienst startet beim Booten (loginctl enable-linger)"
    [ $need_sudoers = 0 ] || echo "  Herunterfahren über die Seite ($sudoers)"
    if [ ${#missing[@]} -gt 0 ]; then
        sudo apt-get update -qq
        if ! sudo apt-get install -y -qq "${missing[@]}"; then
            # pico2wave is in non-free; without it the server speaks with espeak-ng.
            echo "  libttspico-utils nicht installierbar, nehme espeak-ng"
            sudo apt-get install -y -qq "${missing[@]/libttspico-utils/espeak-ng}"
        fi
    fi
    if [ ${#groups_missing[@]} -gt 0 ]; then
        sudo usermod -aG "$(IFS=,; echo "${groups_missing[*]}")" "$USER"
    fi
    [ "$linger" = yes ] || sudo loginctl enable-linger "$USER"
    if [ $need_sudoers = 1 ]; then
        tmp=$(mktemp)
        echo "$sudoers_line" > "$tmp"
        sudo visudo -cqf "$tmp" || die "sudoers-Regel ungültig"
        sudo install -m 0440 -o root -g root "$tmp" "$sudoers"
        rm -f "$tmp"
    fi
fi

# The release.
mkdir -p "$home/releases" "$home/data"
dest=$home/releases/$version
if [ "$here" != "$dest" ]; then
    rm -rf "$dest.new"
    cp -a "$here" "$dest.new"
    rm -rf "$dest"
    mv "$dest.new" "$dest"
fi
old=$(readlink "$home/current" 2>/dev/null || true)
if [ -n "$old" ] && [ "$old" != "releases/$version" ]; then
    ln -sfn "$old" "$home/previous.new" && mv -Tf "$home/previous.new" "$home/previous"
fi
ln -sfn "releases/$version" "$home/current.new" && mv -Tf "$home/current.new" "$home/current"
# Installed by hand: a pending update from the web page is over.
rm -f "$home/update-pending" "$home/update-failed"

# The config, on the first install.
config=$home/config.toml
if [ ! -e "$config" ]; then
    password=""
    if [ -t 0 ]; then
        read -rsp "Passwort für die Rennleitung (leer = zufällig): " password
        echo
    fi
    case $password in *\"*|*\\*) die "das Passwort darf kein \" und kein \\ enthalten" ;; esac
    if [ -z "$password" ]; then
        password=$(tr -dc 'abcdefghjkmnpqrstuvwxyz23456789' </dev/urandom | head -c 12 || true)
        say "Passwort für die Rennleitung: $password  (steht in $config)"
    fi
    umask 077
    cat > "$config" <<EOF
# Laptimer: Einstellungen der Installation (alles andere auf der Web-Seite).
# Nach einer Änderung neu starten: systemctl --user restart laptimer

# port = 5800
# data = "~/.laptimer/data"      # Datenbank, Pegel, Backups, hooks/

[auth]
# Passwort für alles, was etwas ändert (Rennleitung, Einstellungen, …).
# Abschalten nur ausdrücklich mit: enabled = false
enabled = true
password = "$password"

[update]
# Updates von Forgejo (Einstellungen → System → Nach Updates suchen).
# Ohne url: nur Hochladen. Token (read:repository) für ein privates Repo.
# url = "https://git.homeserver.local/bli-blu-bla/LapTimer"
# token = ""
EOF
    umask 022
fi

# Old releases: keep the newest $keep, and always current and previous.
cur=$(readlink "$home/current")
prev=$(readlink "$home/previous" 2>/dev/null || true)
n=0
for r in $(ls -1t "$home/releases"); do
    n=$((n + 1))
    [ $n -gt $keep ] || continue
    [ "releases/$r" = "$cur" ] || [ "releases/$r" = "$prev" ] || rm -rf "${home:?}/releases/$r"
done

# The service.
mkdir -p "$unit_dir"
cp "$dest/laptimer.service" "$unit_dir/laptimer.service"
restart_service

port=$(sed -n 's/^[[:space:]]*port[[:space:]]*=[[:space:]]*\([0-9]*\).*/\1/p' "$config" | head -1)
port=${port:-5800}
say "Läuft: http://$(hostname).local:$port"
ip=$(hostname -I 2>/dev/null | awk '{print $1}')
[ -z "$ip" ] || echo "  oder http://$ip:$port"
echo "  Status: systemctl --user status laptimer   Log: journalctl --user -u laptimer -f"
if [ ${#groups_missing[@]} -gt 0 ]; then
    say "Neue Gruppen (${groups_missing[*]}) gelten erst nach einem Neustart: sudo reboot"
fi
