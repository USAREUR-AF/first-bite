#!/usr/bin/env bash
# Run a built first-bite container image and fail unless it behaves as the
# published image must.
#
#   tools/image-check.sh <image>
#
#   tools/image-check.sh first-bite:local
#
# Checks, each in a fresh container with no network:
#   the default command prints the usage of mind and exits 0
#   the image's user is not root, and is declared by number (uid:gid), which
#   is what Kubernetes needs to honour `runAsNonRoot`
#   bite check --strict passes on the weather-station dictionary in the image
#   mind replays examples/weather-station.pcap and decodes every frame
#   the image carries the OCI labels source, licenses, description, version
#   and revision
#
# Environment:
#   CONTAINER_ENGINE   docker (default) or podman
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: tools/image-check.sh <image>" >&2
  exit 2
fi
image="$1"
root="$(cd "$(dirname "$0")/.." && pwd)"
engine="${CONTAINER_ENGINE:-docker}"
dictionary=/usr/share/first-bite/dictionaries/weather-station
expected_done='done: ([1-9][0-9]*) frames, \1 records, \1 decoded, 0 not UDP or TCP, 0 malformed'

fail() {
  echo "image-check: $*" >&2
  exit 1
}
say() { printf '%-12s %s\n' "$1" "$2"; }

output="$("$engine" run --rm --network none "$image" 2>&1)" || fail "the default command failed: $output"
usage="$(printf '%s\n' "$output" | grep -m 1 '^Usage: mind')" ||
  fail "the default command did not print the usage of mind: $output"
say default "$usage"

uid="$("$engine" run --rm --network none --entrypoint id "$image" -u)"
[ "$uid" != 0 ] || fail "the image runs as root"
declared="$("$engine" image inspect --format '{{.Config.User}}' "$image")"
printf '%s\n' "$declared" | grep -qE '^[1-9][0-9]*:[1-9][0-9]*$' ||
  fail "the image declares the user \"$declared\"; it must be numeric, uid:gid, and not 0"
[ "${declared%%:*}" = "$uid" ] || fail "the image declares the user $declared and runs as uid $uid"
name="$("$engine" run --rm --network none --entrypoint id "$image" -un)"
say user "$declared ($name), declared by number"

output="$("$engine" run --rm --network none --entrypoint bite "$image" check "$dictionary" --strict 2>&1)" ||
  fail "bite check failed: $output"
say bite "$(printf '%s\n' "$output" | tail -n 1)"

output="$("$engine" run --rm --network none --volume "$root/examples:/examples:ro" "$image" \
  --pcap-file /examples/weather-station.pcap --dictionary "$dictionary" 2>&1)" || fail "mind failed: $output"
done_line="$(printf '%s\n' "$output" | grep -oE -- "$expected_done" | tail -n 1)"
[ -n "$done_line" ] || fail "mind did not end with: $expected_done"
say mind "$done_line"

for label in source licenses description version revision; do
  value="$("$engine" image inspect --format "{{index .Config.Labels \"org.opencontainers.image.$label\"}}" "$image")"
  [ -n "$value" ] || fail "no label org.opencontainers.image.$label"
  say "$label" "$value"
done
say platform "$("$engine" image inspect --format '{{.Os}}/{{.Architecture}}' "$image")"
