#!/usr/bin/env bash
# Install Filebeat from a .deb in this directory and ship the first-bite log.
#
#   sudo ./install.sh --output https://elasticsearch.example.org:9200
#
# Put filebeat-<version>-<arch>.deb next to this script first; the package is
# not downloaded here, so the script also works on a machine with no route to
# the internet. See README.md for where to get it.
set -euo pipefail

FILEBEAT_VERSION=8.19.25
marker='# installed by first-bite deployment/filebeat/install.sh'

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
            echo "usage: install.sh --output <elasticsearch url>"
            exit 0
            ;;
        *) die "unknown argument: $1" ;;
    esac
done

[ "$(id -u)" -eq 0 ] || die "run as root"
case "$output" in
    http://* | https://*) ;;
    *) die "--output must be an http:// or https:// URL of an Elasticsearch node" ;;
esac
case "$output" in
    *[\"\|\ ]*) die "--output must not contain quotes, spaces or |" ;;
esac

package="$here/filebeat-$FILEBEAT_VERSION-$(dpkg --print-architecture).deb"
[ -f "$package" ] || die "$package not found; see README.md"
[ -f "$here/filebeat.yml" ] || die "$here/filebeat.yml not found"

dpkg -i "$package"

config=/etc/filebeat/filebeat.yml
if [ -f "$config" ] && grep -qF "$marker" "$config"; then
    echo "kept $config"
else
    if [ -f "$config" ]; then
        cp -p "$config" "$config.packaged"
    fi
    {
        echo "$marker"
        sed "s|https://elasticsearch.example.org:9200|$output|" "$here/filebeat.yml"
    } >"$config"
    chown root:root "$config"
    chmod 0600 "$config"
    echo "installed $config (output $output)"
fi

filebeat test config -c "$config"
systemctl enable filebeat >/dev/null
systemctl restart filebeat
echo "started filebeat.service"
