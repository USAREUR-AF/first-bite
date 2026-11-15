#!/bin/sh
# Runs inside the container that build-in-container.sh starts: installs the
# build tools, installs a Rust toolchain, then runs the command it was given.
#
# Works in an image that has apt-get (Debian, Ubuntu) or apk (Alpine). The
# Rust toolchain goes in /cache, which build-in-container.sh mounts from a
# named volume, so a second run downloads nothing.
set -eu

RUSTUP_VERSION=1.29.3

if [ ! -d /src ] || [ ! -d /cache ] || [ -z "${CARGO_TARGET_DIR:-}" ]; then
  echo "container-bootstrap: run through tools/build-in-container.sh" >&2
  exit 2
fi

if command -v apt-get >/dev/null 2>&1; then
  libc=gnu
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq
  apt-get install -y -qq --no-install-recommends \
    binutils bison ca-certificates cmake curl file flex g++ gcc libc6-dev make perl xz-utils >/dev/null
elif command -v apk >/dev/null 2>&1; then
  libc=musl
  apk add --no-cache --quiet \
    bash binutils bison ca-certificates cmake curl file flex g++ gcc linux-headers make musl-dev perl xz
else
  echo "container-bootstrap: this image has neither apt-get nor apk" >&2
  exit 1
fi

export RUSTUP_HOME=/cache/rustup
export CARGO_HOME=/cache/cargo
export PATH="$CARGO_HOME/bin:$PATH"
# The stable toolchain alone: without this, rustup also fetches the lint
# components that rust-toolchain.toml names.
export RUSTUP_TOOLCHAIN=stable

if ! command -v rustup >/dev/null 2>&1; then
  triple="$(uname -m)-unknown-linux-$libc"
  case "$triple" in
    x86_64-unknown-linux-gnu) sum=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55 ;;
    aarch64-unknown-linux-gnu) sum=9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282 ;;
    x86_64-unknown-linux-musl) sum=1552169aba2836777136ccff7a381e95c17233b5d8250cd5e10ae3cb971bfcfc ;;
    aarch64-unknown-linux-musl) sum=956deacdc27ab82ae7ac821d5d3e9e95e4795d476d17efe99743bb8531ca1031 ;;
    *)
      echo "container-bootstrap: no rustup-init checksum recorded for $triple" >&2
      exit 1
      ;;
  esac
  curl -fsSL -o /tmp/rustup-init \
    "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/$triple/rustup-init"
  echo "$sum  /tmp/rustup-init" | sha256sum -c - >/dev/null
  chmod +x /tmp/rustup-init
  /tmp/rustup-init -y -q --no-modify-path --profile minimal --default-toolchain none >/dev/null
  rm /tmp/rustup-init
fi
rustup toolchain install stable --profile minimal --no-self-update
rustc --version

# The container runs as root; give the output back to whoever owns the tree.
trap 'chown -R "$(stat -c %u:%g /src)" "$CARGO_TARGET_DIR"' EXIT

"$@"
