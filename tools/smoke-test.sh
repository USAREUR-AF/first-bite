#!/usr/bin/env bash
# Run built binaries against the example dictionary and capture file, and fail
# unless they give the known result.
#
#   tools/smoke-test.sh [--bite-only] <directory holding mind and bite> [image ...]
#
#   tools/smoke-test.sh target/release
#   tools/smoke-test.sh target/ubuntu-20.04/release ubuntu:20.04 ubuntu:26.04
#
# With no image the binaries run on this machine. With images, each runs them
# in a fresh container of that image with nothing installed and no network:
# that is the test that a binary carries everything it needs. --bite-only
# skips mind, for a platform that has only the dictionary tool.
#
# Environment:
#   CONTAINER_ENGINE   docker (default) or podman
set -euo pipefail

# `bite check --strict` must succeed and print the first; mind must end its
# replay of examples/weather-station.pcap with a log line that matches the
# second: every frame in the file became a decoded record.
expected_check='0 errors, 0 warnings'
expected_done='done: ([1-9][0-9]*) frames, \1 records, \1 decoded, 0 not UDP or TCP, 0 malformed'

bite_only=no
if [ "${1:-}" = --bite-only ]; then
  bite_only=yes
  shift
fi
if [ "$#" -lt 1 ]; then
  echo "usage: tools/smoke-test.sh [--bite-only] <directory holding mind and bite> [image ...]" >&2
  exit 2
fi
bin_dir="$(cd "$1" && pwd)"
shift

root="$(cd "$(dirname "$0")/.." && pwd)"
engine="${CONTAINER_ENGINE:-docker}"

# run <image or empty> <binary> [arguments]: paths are relative to the
# repository root, which is the working directory in both cases.
run() {
  local image="$1" binary="$2"
  shift 2
  if [ -z "$image" ]; then
    (cd "$root" && "$bin_dir/$binary" "$@") 2>&1
  else
    "$engine" run --rm --network none \
      --volume "$bin_dir:/first-bite/bin:ro" \
      --volume "$root/dictionaries:/first-bite/dictionaries:ro" \
      --volume "$root/examples:/first-bite/examples:ro" \
      --workdir /first-bite \
      "$image" "/first-bite/bin/$binary" "$@" 2>&1
  fi
}

# expect <where> <command name> <extended regular expression> <output>: the
# output must have a line that matches; the part from the match on is printed.
expect() {
  local line
  if line="$(printf '%s\n' "$4" | grep -E -- "$3" | tail -n 1)" && [ -n "$line" ]; then
    printf '%-14s %-5s %s\n' "$1" "$2" "$(printf '%s\n' "$line" | grep -oE -- "$3.*")"
  else
    printf '%s\n' "$4" >&2
    echo "smoke-test: $2 in $1 did not print: $3" >&2
    exit 1
  fi
}

[ "$#" -gt 0 ] || set -- ""
for image in "$@"; do
  where="${image:-this machine}"
  output="$(run "$image" bite check dictionaries/weather-station --strict)" || {
    printf '%s\n' "$output" >&2
    echo "smoke-test: bite check failed in $where" >&2
    exit 1
  }
  expect "$where" bite "$expected_check" "$output"
  [ "$bite_only" = no ] || continue
  output="$(run "$image" mind --pcap-file examples/weather-station.pcap \
    --dictionary dictionaries/weather-station)" || {
    printf '%s\n' "$output" >&2
    echo "smoke-test: mind failed in $where" >&2
    exit 1
  }
  expect "$where" mind "$expected_done" "$output"
done
