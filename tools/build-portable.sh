#!/usr/bin/env bash
# Build release binaries that need nothing on the target but glibc.
#
#   tools/build-portable.sh [extra cargo build arguments]
#
# libpcap is built from source as a static library (tools/libpcap-static.sh)
# and the C++ runtime that ZeroMQ needs is linked statically. glibc stays
# dynamic: the binaries run on any Linux whose glibc is at least as new as this
# machine's, so build on the oldest release you deploy to, or in its container
# image: `make portable` runs this script in ubuntu:20.04.
#
# Needs: a Rust toolchain, gcc and g++, make, cmake, flex, bison, curl, xz.
# Output: mind and bite in $CARGO_TARGET_DIR/release (default target/release).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$root/target}"

# shellcheck source=tools/libpcap-static.sh
. "$root/tools/libpcap-static.sh"
build_static_libpcap "$target_dir/portable"

# cc-rs emits the C++ standard library named here for crates that compile C++.
export CXXSTDLIB="static=stdc++"
stdcxx_dir="$(dirname "$(g++ -print-file-name=libstdc++.a)")"
export RUSTFLAGS="${RUSTFLAGS:-} -L native=$stdcxx_dir -C link-arg=-static-libgcc"

cd "$root"
cargo build --release --locked -p mind -p bite "$@"

mind="$target_dir/release/mind"
echo
echo "shared libraries needed by $mind:"
needed="$(readelf -d "$mind" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p')"
# shellcheck disable=SC2086  # one library name per word
printf '  %s\n' $needed
unexpected="$(echo "$needed" | grep -Ev '^(libc|libm|libdl|libpthread|librt|ld-linux[-a-z0-9]*|libgcc_s)\.so' || true)"
if [ -n "$unexpected" ]; then
  echo "build-portable: unexpected shared library dependency:" >&2
  # shellcheck disable=SC2086  # one library name per word
  printf '  %s\n' $unexpected >&2
  exit 1
fi
echo "glibc symbols required up to: $(objdump -T "$mind" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"
