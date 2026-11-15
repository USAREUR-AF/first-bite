#!/usr/bin/env bash
# Run one of this repository's build scripts inside a container image, so the
# result depends on the image and not on the machine.
#
#   tools/build-in-container.sh <image> <script> [script arguments]
#
#   tools/build-in-container.sh ubuntu:20.04 tools/build-portable.sh
#   tools/build-in-container.sh alpine:3.23 tools/build-static.sh
#
# `make portable` and `make static` run these two with the images pinned by
# digest; the release workflow calls the same make targets.
#
# The image needs nothing installed: tools/container-bootstrap.sh adds the
# compilers with apt or apk and a Rust toolchain with rustup. The repository
# is mounted at /src. Build output goes to target/<name>/, where <name> is the
# image reference without its digest and with ':' and '/' replaced by '-'
# (target/ubuntu-20.04/), and is handed back to the owner of the repository
# directory. The Rust toolchain and the cargo registry are kept between runs
# in the volume first-bite-build-<name>.
#
# Environment:
#   CONTAINER_ENGINE   docker (default) or podman
#   CARGO_BUILD_JOBS   passed through; limits cargo and make
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: tools/build-in-container.sh <image> <script> [script arguments]" >&2
  exit 2
fi
image="$1"
shift

root="$(cd "$(dirname "$0")/.." && pwd)"
engine="${CONTAINER_ENGINE:-docker}"
name="$(printf '%s' "${image%@*}" | tr ':/' '--')"

args=(run --rm
  --volume "$root:/src"
  --volume "first-bite-build-$name:/cache"
  --workdir /src
  --env "CARGO_TARGET_DIR=/src/target/$name")
if [ -n "${CARGO_BUILD_JOBS:-}" ]; then
  args+=(--env "CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS")
fi

# Created here so that it belongs to the invoking user, not to the container's.
mkdir -p "$root/target/$name"
echo "build-in-container: $* in $image, output in target/$name/"
exec "$engine" "${args[@]}" "$image" sh tools/container-bootstrap.sh "$@"
