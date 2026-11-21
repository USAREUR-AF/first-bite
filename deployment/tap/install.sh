#!/usr/bin/env bash
# Install one first-bite tap.
#
#   sudo ./install.sh <name>                 write the unit and, if absent, /etc/first-bite/tap-<name>.env
#   sudo ./install.sh <name> --env <file>    use <file> as the settings (replaces the installed ones)
#   sudo ./install.sh <name> --uninstall     stop and disable the tap; settings and slices are kept
#
# mind must already be installed (deployment/install.sh, or /usr/local/bin/mind).
set -euo pipefail
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
die() {
    echo "install.sh: $*" >&2
    exit 1
}

[ "$#" -ge 1 ] || die "usage: install.sh <name> [--env <file>] [--uninstall]"
name="$1"
shift
case "$name" in
    *[!a-z0-9-]* | "" | -*) die "a tap name is lowercase letters, digits and hyphens" ;;
esac
env_source=
uninstall=no
while [ "$#" -gt 0 ]; do
    case "$1" in
        --env)
            [ "$#" -ge 2 ] || die "--env needs a file"
            env_source="$2"
            shift 2
            ;;
        --uninstall)
            uninstall=yes
            shift
            ;;
        *) die "unknown argument $1" ;;
    esac
done
[ "$(id -u)" -eq 0 ] || die "run as root"

unit=/etc/systemd/system/first-bite-tap@.service
env_file="/etc/first-bite/tap-${name}.env"

if [ "$uninstall" = yes ]; then
    systemctl disable --now "first-bite-tap@${name}" 2>/dev/null || true
    echo "  stopped   first-bite-tap@${name}; kept ${env_file} and /var/lib/first-bite-tap/${name}"
    exit 0
fi

[ -x /usr/local/bin/mind ] || die "/usr/local/bin/mind is not installed; run deployment/install.sh first"
id -u firstbite >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin firstbite
install -d -m 0750 -o root -g firstbite /etc/first-bite
install -m 0644 "$here/first-bite-tap@.service" "$unit"
if [ -n "$env_source" ]; then
    install -m 0640 -o root -g firstbite "$env_source" "$env_file"
elif [ ! -e "$env_file" ]; then
    install -m 0640 -o root -g firstbite "$here/tap.env.example" "$env_file"
fi
systemctl daemon-reload
systemctl enable --now "first-bite-tap@${name}"
echo "  installed first-bite-tap@${name}: settings ${env_file}, slices /var/lib/first-bite-tap/${name}"
