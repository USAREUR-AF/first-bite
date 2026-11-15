#!/usr/bin/env bash
# Run the capture-node kit in a container that boots systemd, against the S3
# stand-in of mocks/s3: the units pass systemd-analyze verify, install.sh
# installs them, the capture rotates files, the mirror copies the completed
# ones, logrotate accepts the rule, and install.sh --uninstall removes the
# units and keeps the data.
#
#   deployment/capture-node/selftest.sh [directory to copy the capture files to]
#
# Needs docker and network access for the container's packages. The container
# is privileged, because systemd inside it has to manage control groups.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
keep="${1:-}"
name="fb-capture-selftest-$$"
image="first-bite-capture-selftest"
s3_image="$(sed -n 's/^ *image: *//p' "$here/../../mocks/s3/compose.yml")"

cleanup() {
    docker rm -f "$name" "$name-s3" >/dev/null 2>&1 || true
    docker network rm "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }
in_node() { docker exec "$name" "$@"; }

step "build the node image (Ubuntu 24.04 with systemd, dumpcap, python3, logrotate)"
docker build -q -t "$image" - <<'EOF' >/dev/null
FROM ubuntu:24.04
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        systemd systemd-sysv wireshark-common python3 logrotate iproute2 curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
EOF

docker network create "$name" >/dev/null
docker run -d --name "$name-s3" --network "$name" --network-alias storage \
    -e COM_ADOBE_TESTING_S3MOCK_STORE_INITIAL_BUCKETS=first-bite "$s3_image" >/dev/null
# systemd hands a credential to a service through a mount it makes visible
# to that service; a container's root mount has to be shared for that.
docker run -d --name "$name" --network "$name" --privileged --cgroupns=host \
    --tmpfs /run --tmpfs /run/lock -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
    -v "$here":/kit:ro "$image" \
    sh -c 'mount --make-rshared / && exec /sbin/init' >/dev/null
for _ in $(seq 1 30); do
    state="$(in_node systemctl is-system-running 2>/dev/null || true)"
    case "$state" in running | degraded) break ;; esac
    sleep 1
done
in_node systemctl --version | head -n 1
in_node dumpcap --version | head -n 1

step "systemd-analyze verify"
in_node useradd --system --user-group --no-create-home --shell /usr/sbin/nologin fbcapture
in_node systemd-analyze verify /kit/first-bite-capture.service \
    /kit/first-bite-capture-mirror.service /kit/first-bite-capture-mirror.timer
echo "verify: no finding"

step "install.sh refuses a wrong invocation before changing anything"
in_node /kit/install.sh --interface nosuch0 2>&1 | tail -n 1 || true
in_node /kit/install.sh --interface lo --endpoint ftp://storage 2>&1 | tail -n 1 || true
in_node test ! -e /etc/first-bite-capture

step "install.sh"
in_node /kit/install.sh --interface lo --endpoint http://storage:9090 --bucket first-bite --prefix selftest
in_node stat -c '%U:%G %a %n' /etc/first-bite-capture /etc/first-bite-capture/capture.env \
    /etc/first-bite-capture/s3-credentials /var/lib/first-bite-capture

step "capture rotates: 20 kB or 3 s per file, 6 files kept"
in_node sed -i -e 's/^CAPTURE_FILE_KB=.*/CAPTURE_FILE_KB=20/' -e 's/^CAPTURE_FILE_SECONDS=.*/CAPTURE_FILE_SECONDS=3/' \
    -e 's/^CAPTURE_FILES=.*/CAPTURE_FILES=6/' -e 's/^CAPTURE_FILTER=.*/CAPTURE_FILTER=udp port 40100/' \
    /etc/first-bite-capture/capture.env
in_node sh -c 'printf "access_key_id=selftest\nsecret_access_key=selftest-secret\n" > /etc/first-bite-capture/s3-credentials'
in_node systemctl restart first-bite-capture
sleep 2
in_node systemctl is-active first-bite-capture
# Weather-station heartbeats to UDP port 40100 on the loopback interface.
in_node python3 -c '
import socket, time
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
payload = bytes.fromhex("01000c4b5837000001518060")
for i in range(1200):
    s.sendto(payload, ("127.0.0.1", 40100))
    time.sleep(0.01)
'
in_node sh -c 'ls -l /var/lib/first-bite-capture/*.pcapng'
files="$(in_node sh -c 'ls /var/lib/first-bite-capture/*.pcapng | wc -l')"
# dumpcap opens the next file before it deletes the oldest, so one more than
# the bound can be seen for a moment.
# and the file of the run install.sh started is kept across the restart.
[ "$files" -ge 3 ] && [ "$files" -le 8 ] || {
    echo "expected 3 to 8 files, found $files" >&2
    exit 1
}
echo "$files files on disk (a ring of 6, one more while a new file opens, one from the earlier run)"
in_node sh -c 'ps -o user=,args= -C dumpcap | cut -c1-150'

step "mirror: completed files reach the storage, the open one does not"
in_node systemctl start first-bite-capture-mirror.service || {
    in_node cat /var/log/first-bite-capture/mirror.log
    in_node journalctl -u first-bite-capture-mirror.service -n 15 --no-pager
    exit 1
}
in_node cat /var/log/first-bite-capture/mirror.log
listed="$(in_node curl -s 'http://storage:9090/first-bite?list-type=2&prefix=selftest/' | grep -o '<Key>[^<]*</Key>' | wc -l)"
echo "objects under selftest/: $listed"
[ "$listed" -ge 2 ] || exit 1
in_node systemctl start first-bite-capture-mirror.service
in_node tail -n 1 /var/log/first-bite-capture/mirror.log
in_node systemctl is-enabled first-bite-capture-mirror.timer
in_node sh -c 'systemctl list-timers --no-pager first-bite-capture-mirror.timer | sed -n 1,2p | cut -c1-110'
# The access key is in no unit, no environment and no command line.
if in_node sh -c 'systemctl show first-bite-capture-mirror.service -p Environment -p ExecStart; cat /etc/systemd/system/first-bite-capture*' | grep -q selftest-secret; then
    echo "the secret is visible in a unit" >&2
    exit 1
fi
echo "the secret appears in no unit and no unit environment"

step "a storage that is away: the run fails, nothing is marked, the next run sends"
docker stop "$name-s3" >/dev/null
sleep 4
in_node python3 -c '
import socket, time
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
for i in range(700):
    s.sendto(bytes.fromhex("01000c4b5837000001518060"), ("127.0.0.1", 40100)); time.sleep(0.01)
'
in_node systemctl start first-bite-capture-mirror.service && echo "unexpected success" || echo "mirror unit failed, as it should"
in_node tail -n 2 /var/log/first-bite-capture/mirror.log

step "a restart cuts the files of earlier runs back to the newest 6"
in_node systemctl restart first-bite-capture
sleep 1
in_node sh -c 'ls /var/lib/first-bite-capture/*.pcapng | wc -l'
in_node sh -c 'grep "start of capture" /var/log/first-bite-capture/mirror.log || journalctl -u first-bite-capture --no-pager -o cat | grep "start of capture" | tail -n 1'
after="$(in_node sh -c 'ls /var/lib/first-bite-capture/*.pcapng | wc -l')"
[ "$after" -le 8 ] || exit 1

step "logrotate accepts the rule and rotates the log"
in_node logrotate --force /etc/logrotate.d/first-bite-capture
in_node sh -c 'ls /var/log/first-bite-capture/'

if [ -n "$keep" ]; then
    mkdir -p "$keep"
    docker cp "$name":/var/lib/first-bite-capture/. "$keep"/ >/dev/null
    echo "capture files copied to $keep"
fi

step "install.sh --uninstall"
in_node /kit/install.sh --uninstall
in_node sh -c 'systemctl is-active first-bite-capture || true'
in_node sh -c 'ls /var/lib/first-bite-capture/*.pcapng | wc -l'

printf '\nselftest: ok\n'
