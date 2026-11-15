#!/usr/bin/env bash
# Install Telegraf from a .deb in this directory and report host metrics.
#
#   sudo ./install.sh --output https://influxdb.example.org:8086
#
# Put telegraf_<version>-1_<arch>.deb next to this script first; the package
# is not downloaded here. See README.md for where to get it and how the
# InfluxDB token is supplied.
set -euo pipefail

TELEGRAF_VERSION=1.40.3
marker='# installed by first-bite deployment/telegraf/install.sh'

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
output=

die() {
    echo "install.sh: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --output)
            [ "$#" -ge 2 ] || die "--output needs a value"
            output=$2
            shift 2
            ;;
        -h | --help)
            echo "usage: install.sh --output <influxdb url>"
            exit 0
            ;;
        *) die "unknown argument: $1" ;;
    esac
done

[ "$(id -u)" -eq 0 ] || die "run as root"
case "$output" in
    http://* | https://*) ;;
    *) die "--output must be an http:// or https:// URL of an InfluxDB 2 server" ;;
esac
case "$output" in
    *[\"\|\ ]*) die "--output must not contain quotes, spaces or |" ;;
esac

package="$here/telegraf_$TELEGRAF_VERSION-1_$(dpkg --print-architecture).deb"
[ -f "$package" ] || die "$package not found; see README.md"
[ -f "$here/telegraf.conf" ] || die "$here/telegraf.conf not found"

dpkg -i "$package"

config=/etc/telegraf/telegraf.conf
if [ -f "$config" ] && grep -qF "$marker" "$config"; then
    echo "kept $config"
else
    if [ -f "$config" ]; then
        cp -p "$config" "$config.packaged"
    fi
    {
        echo "$marker"
        sed "s|https://influxdb.example.org:8086|$output|" "$here/telegraf.conf"
    } >"$config"
    chmod 0644 "$config"
    echo "installed $config (output $output)"
fi

if ! grep -q '^INFLUX_TOKEN=' /etc/default/telegraf 2>/dev/null; then
    echo "note: set INFLUX_TOKEN=<token> in /etc/default/telegraf, then: systemctl restart telegraf"
fi
systemctl enable telegraf >/dev/null
systemctl restart telegraf
echo "started telegraf.service"
