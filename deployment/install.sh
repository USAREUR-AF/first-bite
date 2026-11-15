#!/usr/bin/env bash
# Install first-bite as a systemd service on Ubuntu 20.04 or later.
#
#   sudo ./install.sh --interface eth0 --dictionary ../dictionaries/weather-station
#   sudo ./install.sh --uninstall
#
# Run it from an unpacked release: the binaries (mind, bite),
# first-bite.service, first-bite.logrotate, first-bite.env.example and config/
# are read from the directory this script is in. It asks no questions and can
# be run again: binaries, the unit and the logrotate rule are replaced; the
# environment file and the files in /etc/first-bite/config are written only
# when they do not exist. Ownership and mode of /etc/first-bite, its config
# directory, the environment file and the configuration files are set on every
# run: they hold or name credentials, and end up root:firstbite, directories
# 0750 and files 0640.
set -euo pipefail

# The script runs as root: use the system's tools whatever PATH it was
# started with, and a known umask. Every file that holds settings is given
# its mode explicitly below.
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022

usage() {
    cat <<'EOF'
usage: install.sh --interface <name> [--dictionary <dir>] [--prefix <dir>]
       install.sh --uninstall [--prefix <dir>]

  --interface <name>   network interface to capture on
  --dictionary <dir>   dictionary directory; copied to /etc/first-bite/dictionary,
                       replacing the one installed there
  --prefix <dir>       binaries go in <dir>/bin (default /usr/local)
  --uninstall          stop the service and remove the binaries, the unit and
                       the logrotate rule; configuration and logs are kept
EOF
}

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
service=first-bite
user=firstbite
etc_dir=/etc/first-bite
log_dir=/var/log/first-bite
unit_path=/etc/systemd/system/first-bite.service
logrotate_path=/etc/logrotate.d/first-bite
binaries=(mind bite)
required_binaries=(mind)

interface=
dictionary=
prefix=/usr/local
uninstall=no

say() { printf '  %-9s %s\n' "$1" "$2"; }
die() {
    echo "install.sh: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --interface | --dictionary | --prefix)
            [ "$#" -ge 2 ] || die "$1 needs a value"
            case "$1" in
                --interface) interface=$2 ;;
                --dictionary) dictionary=$2 ;;
                --prefix) prefix=$2 ;;
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
case "$prefix" in
    /*) ;;
    *) die "--prefix must be an absolute path" ;;
esac
# The prefix is written into the unit's ExecStart line.
case "$prefix" in
    *[!A-Za-z0-9._/-]*) die "--prefix may hold only letters, digits and . _ - /" ;;
esac
bin_dir="$prefix/bin"

if [ "$uninstall" = yes ]; then
    echo "Removing first-bite:"
    if [ -e "$unit_path" ]; then
        systemctl disable --now "$service" >/dev/null 2>&1 || true
        say stopped "$service.service"
    fi
    for path in "$unit_path" "$logrotate_path" "${binaries[@]/#/$bin_dir/}"; do
        if [ -e "$path" ]; then
            rm -f "$path"
            say removed "$path"
        fi
    done
    systemctl daemon-reload
    for path in "$etc_dir" "$log_dir" /var/lib/first-bite; do
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
for name in "${required_binaries[@]}"; do
    [ -f "$here/$name" ] || die "$here/$name not found: run install.sh from an unpacked release"
done
for name in first-bite.service first-bite.logrotate first-bite.env.example; do
    [ -f "$here/$name" ] || die "$here/$name not found"
done
# A release binary carries libpcap; one built from source with `make release`
# uses the system's.
if ldd "$here/mind" 2>/dev/null | grep -q 'libpcap.*not found'; then
    die "this mind needs the system libpcap, which is not installed: apt-get install libpcap0.8"
fi
if [ -n "$dictionary" ]; then
    [ -d "$dictionary" ] || die "--dictionary $dictionary is not a directory"
    dictionary="$(cd "$dictionary" && pwd)"
    if [ -x "$here/bite" ]; then
        "$here/bite" check "$dictionary" || die "bite check failed for $dictionary; nothing was installed"
    fi
fi

echo "Installing first-bite:"

for name in "${binaries[@]}"; do
    if [ -f "$here/$name" ]; then
        install -D -o root -g root -m 0755 "$here/$name" "$bin_dir/$name"
        say installed "$bin_dir/$name"
    fi
done

if id "$user" >/dev/null 2>&1; then
    say kept "system user $user"
else
    useradd --system --user-group --no-create-home --home-dir /var/lib/first-bite \
        --shell /usr/sbin/nologin "$user"
    say created "system user $user"
fi

install -d -o root -g "$user" -m 0750 "$etc_dir" "$etc_dir/config"
# install -d leaves a directory that already exists as it finds it.
chown root:"$user" "$etc_dir" "$etc_dir/config"
chmod 0750 "$etc_dir" "$etc_dir/config"
# Files for the unit's LoadCredential= lines; systemd reads them as root.
install -d -o root -g root -m 0700 "$etc_dir/credentials"
install -d -o "$user" -g "$user" -m 0750 "$log_dir"
if [ ! -e "$log_dir/mind.log" ]; then
    install -o "$user" -g "$user" -m 0640 /dev/null "$log_dir/mind.log"
fi
say ready "$log_dir/mind.log"

if [ -n "$dictionary" ]; then
    staged="$etc_dir/dictionary.new"
    rm -rf "$staged"
    cp -R "$dictionary" "$staged"
    chown -R root:"$user" "$staged"
    chmod -R u=rwX,g=rX,o= "$staged"
    rm -rf "$etc_dir/dictionary"
    mv "$staged" "$etc_dir/dictionary"
    say installed "$etc_dir/dictionary (from $dictionary)"
fi

if [ -d "$here/config" ]; then
    for source in "$here"/config/*.yml; do
        [ -f "$source" ] || continue
        target="$etc_dir/config/$(basename "$source")"
        if [ -e "$target" ]; then
            say kept "$target"
        elif [ -L "$target" ]; then
            die "$target is a dangling symbolic link; remove it"
        else
            install -o root -g "$user" -m 0640 "$source" "$target"
            say installed "$target"
        fi
    done
fi

# Configuration kept from an earlier install, or put there by hand, gets the
# same ownership and mode as a file installed now. A symbolic link is left
# alone and reported: chown and chmod would act on what it points to.
for kept in "$etc_dir"/config/*.yml "$etc_dir/first-bite.env"; do
    [ -e "$kept" ] || continue
    if [ -L "$kept" ] || [ ! -f "$kept" ]; then
        say note "$kept is not a regular file; its ownership and mode were not set"
        continue
    fi
    chown root:"$user" "$kept"
    chmod 0640 "$kept"
done

env_file="$etc_dir/first-bite.env"
if [ -e "$env_file" ]; then
    say kept "$env_file"
    current="$(sed -n 's/^FIRST_BITE_INTERFACE=//p' "$env_file" | tail -n 1)"
    if [ "$current" != "$interface" ]; then
        say note "$env_file has FIRST_BITE_INTERFACE=$current, not $interface; edit the file to change it"
    fi
    if [ -d "$etc_dir/dictionary" ] && ! grep -q '^BITE_DICT_PATH=' "$env_file"; then
        say note "$env_file does not set BITE_DICT_PATH; add BITE_DICT_PATH=$etc_dir/dictionary to decode"
    fi
else
    staged="$(mktemp "$etc_dir/first-bite.env.XXXXXX")"
    sed "s|^FIRST_BITE_INTERFACE=.*|FIRST_BITE_INTERFACE=$interface|" "$here/first-bite.env.example" >"$staged"
    if [ -d "$etc_dir/dictionary" ]; then
        sed -i "s|^#BITE_DICT_PATH=.*|BITE_DICT_PATH=$etc_dir/dictionary|" "$staged"
    fi
    if [ ! -f "$etc_dir/config/publishers.yml" ]; then
        # No publisher file was installed: mind then uses its built-in default.
        sed -i "s|^PUBLISHER_CONFIG_PATH=|#PUBLISHER_CONFIG_PATH=|" "$staged"
    fi
    chown root:"$user" "$staged"
    chmod 0640 "$staged"
    mv "$staged" "$env_file"
    say installed "$env_file (interface $interface)"
fi

staged="$(mktemp "$unit_path.XXXXXX")"
sed "s|^ExecStart=/usr/local/bin/mind |ExecStart=$bin_dir/mind |" "$here/first-bite.service" >"$staged"
chown root:root "$staged"
chmod 0644 "$staged"
mv "$staged" "$unit_path"
say installed "$unit_path"
install -o root -g root -m 0644 "$here/first-bite.logrotate" "$logrotate_path"
say installed "$logrotate_path"

systemctl daemon-reload
systemctl enable "$service" >/dev/null
systemctl restart "$service"
say started "$service.service (enabled at boot)"

cat <<EOF

Check it:
  systemctl status $service
  tail $log_dir/mind.log
Settings are in $env_file; run 'systemctl restart $service' after editing.
EOF
