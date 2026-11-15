#!/usr/bin/env bash
# Install the first-bite capture node on Ubuntu 22.04 or later.
#
#   sudo ./install.sh --interface eth0
#   sudo ./install.sh --interface eth0 --endpoint https://storage.example.org --bucket captures --prefix site-a
#   sudo ./install.sh --uninstall
#
# Run it from this directory. It asks no questions and can be run again: the
# units, the mirror script and the logrotate rule are replaced; the settings
# file and the credentials file are written only when they do not exist.
# Capture files and logs are never removed.
set -euo pipefail

# The script runs as root: use the system's tools whatever PATH it was
# started with, and a known umask.
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022

usage() {
    cat <<'EOF'
usage: install.sh --interface <name> [--endpoint <url> --bucket <name> [--prefix <text>] [--region <name>]]
       install.sh --uninstall

  --interface <name>   network interface to capture on
  --endpoint <url>     S3-compatible storage to mirror completed files to;
                       without it files stay on this machine and the mirror
                       timer is left off
  --bucket <name>      bucket the files go to
  --prefix <text>      objects are named <prefix>/<file name> (default: capture)
  --region <name>      region the requests are signed for (default: us-east-1)
  --uninstall          stop and remove the units, the mirror script and the
                       logrotate rule; settings, credentials, capture files
                       and logs are kept
EOF
}

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
user=fbcapture
etc_dir=/etc/first-bite-capture
lib_dir=/usr/local/lib/first-bite-capture
state_dir=/var/lib/first-bite-capture
log_dir=/var/log/first-bite-capture
unit_dir=/etc/systemd/system
logrotate_path=/etc/logrotate.d/first-bite-capture
units=(first-bite-capture.service first-bite-capture-mirror.service first-bite-capture-mirror.timer)

interface=
endpoint=
bucket=
prefix=capture
region=us-east-1
uninstall=no

say() { printf '  %-9s %s\n' "$1" "$2"; }
die() {
    echo "install.sh: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --interface | --endpoint | --bucket | --prefix | --region)
            [ "$#" -ge 2 ] || die "$1 needs a value"
            case "$1" in
                --interface) interface=$2 ;;
                --endpoint) endpoint=$2 ;;
                --bucket) bucket=$2 ;;
                --prefix) prefix=$2 ;;
                --region) region=$2 ;;
            esac
            shift 2
            ;;
        --uninstall)
            uninstall=yes
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            die "unknown argument: $1"
            ;;
    esac
done

[ "$(id -u)" -eq 0 ] || die "run as root (sudo $0 ...)"

if [ "$uninstall" = yes ]; then
    echo "Removing the first-bite capture node:"
    for unit in first-bite-capture-mirror.timer first-bite-capture-mirror.service first-bite-capture.service; do
        if [ -e "$unit_dir/$unit" ]; then
            systemctl disable --now "$unit" >/dev/null 2>&1 || true
            rm -f "$unit_dir/$unit"
            say removed "$unit_dir/$unit"
        fi
    done
    for path in "$logrotate_path" "$lib_dir/mirror.py"; do
        if [ -e "$path" ]; then
            rm -f "$path"
            say removed "$path"
        fi
    done
    rmdir "$lib_dir" 2>/dev/null || true
    systemctl daemon-reload
    for path in "$etc_dir" "$state_dir" "$log_dir"; do
        if [ -e "$path" ]; then
            say kept "$path"
        fi
    done
    if id "$user" >/dev/null 2>&1; then
        say kept "system user $user"
    fi
    exit 0
fi

# Everything is checked before anything is changed.
[ -n "$interface" ] || {
    usage >&2
    die "--interface is required"
}
case "$interface" in
    *[!A-Za-z0-9._:@-]*) die "not an interface name: $interface" ;;
esac
[ -e "/sys/class/net/$interface" ] || die "no network interface named $interface (see: ip -brief link)"
if [ -n "$endpoint" ]; then
    case "$endpoint" in
        http://* | https://*) ;;
        *) die "--endpoint must start with http:// or https://" ;;
    esac
    [ -n "$bucket" ] || die "--endpoint needs --bucket"
elif [ -n "$bucket" ]; then
    die "--bucket needs --endpoint"
fi
# These are written into the settings file, which systemd reads line by line.
for value in "$endpoint" "$bucket" "$prefix" "$region"; do
    case "$value" in
        *[!A-Za-z0-9._:/@%+=~-]*) die "not usable in the settings file: $value" ;;
    esac
done
for name in "${units[@]}" first-bite-capture.logrotate capture.env.example s3-credentials.example mirror.py; do
    [ -f "$here/$name" ] || die "$here/$name not found: run install.sh from deployment/capture-node"
done
[ -x /usr/bin/dumpcap ] || die "dumpcap is not installed: apt-get install wireshark-common (answer No when asked whether non-superusers may capture; the unit grants the capability itself)"
[ -x /usr/bin/python3 ] || die "python3 is not installed: apt-get install python3"

echo "Installing the first-bite capture node:"

if id "$user" >/dev/null 2>&1; then
    say kept "system user $user"
else
    useradd --system --user-group --no-create-home --home-dir "$state_dir" \
        --shell /usr/sbin/nologin "$user"
    say created "system user $user"
fi

install -d -o root -g "$user" -m 0750 "$etc_dir"
chown root:"$user" "$etc_dir"
chmod 0750 "$etc_dir"
install -d -o "$user" -g "$user" -m 0750 "$state_dir" "$log_dir"
install -d -o root -g root -m 0755 "$lib_dir"
install -o root -g root -m 0755 "$here/mirror.py" "$lib_dir/mirror.py"
say installed "$lib_dir/mirror.py"

env_file="$etc_dir/capture.env"
if [ -e "$env_file" ]; then
    say kept "$env_file"
    current="$(sed -n 's/^CAPTURE_INTERFACE=//p' "$env_file" | tail -n 1)"
    if [ "$current" != "$interface" ]; then
        say note "$env_file has CAPTURE_INTERFACE=$current, not $interface; edit the file to change it"
    fi
else
    staged="$(mktemp "$etc_dir/capture.env.XXXXXX")"
    sed -e "s|^CAPTURE_INTERFACE=.*|CAPTURE_INTERFACE=$interface|" \
        -e "s|^S3_ENDPOINT=.*|S3_ENDPOINT=$endpoint|" \
        -e "s|^S3_BUCKET=.*|S3_BUCKET=$bucket|" \
        -e "s|^S3_PREFIX=.*|S3_PREFIX=$prefix|" \
        -e "s|^S3_REGION=.*|S3_REGION=$region|" \
        "$here/capture.env.example" >"$staged"
    mv "$staged" "$env_file"
    say installed "$env_file (interface $interface)"
fi
chown root:"$user" "$env_file"
chmod 0640 "$env_file"

# The access key: root only. systemd reads it and hands it to the mirror.
credentials="$etc_dir/s3-credentials"
if [ -e "$credentials" ]; then
    say kept "$credentials"
else
    install -o root -g root -m 0600 "$here/s3-credentials.example" "$credentials"
    say installed "$credentials (empty: put the access key in it)"
fi
chown root:root "$credentials"
chmod 0600 "$credentials"

for unit in "${units[@]}"; do
    install -o root -g root -m 0644 "$here/$unit" "$unit_dir/$unit"
    say installed "$unit_dir/$unit"
done
install -o root -g root -m 0644 "$here/first-bite-capture.logrotate" "$logrotate_path"
say installed "$logrotate_path"

systemctl daemon-reload
systemctl enable first-bite-capture.service >/dev/null
systemctl restart first-bite-capture.service
say started "first-bite-capture.service (enabled at boot)"

configured="$(sed -n 's/^S3_ENDPOINT=//p' "$env_file" | tail -n 1)"
if [ -n "$configured" ]; then
    systemctl enable --now first-bite-capture-mirror.timer >/dev/null
    say started "first-bite-capture-mirror.timer (every five minutes, to $configured)"
    if ! grep -q '^access_key_id=..*' "$credentials"; then
        say note "$credentials holds no access key yet; the mirror fails until it does"
    fi
else
    systemctl disable --now first-bite-capture-mirror.timer >/dev/null 2>&1 || true
    say note "no storage endpoint: files stay in $state_dir and the mirror timer is off"
fi

cat <<EOF

Check it:
  systemctl status first-bite-capture
  ls -l $state_dir
  journalctl -u first-bite-capture -n 20
  tail $log_dir/mirror.log
EOF
