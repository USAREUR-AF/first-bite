#!/usr/bin/env bash
# Build release binaries that need nothing on the target but a Linux kernel:
# no libc, no loader, no shared library.
#
#   tools/build-static.sh [extra cargo build arguments]
#
# Run it on a musl system; `make static` runs it in an Alpine container. The
# binaries are linked against musl with libpcap (tools/libpcap-static.sh) and
# the C++ runtime inside them. They are position-independent, so the kernel
# still loads them at a random address.
#
# Needs: a Rust toolchain whose host is *-linux-musl, gcc and g++, make, cmake,
# flex, bison, curl, xz, the kernel headers.
# Output: mind and bite in $CARGO_TARGET_DIR/<host triple>/release.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$root/target}"

host="$(rustc -vV | sed -n 's/^host: //p')"
case "$host" in
  *-linux-musl) ;;
  *)
    echo "build-static: this toolchain's host is $host, not a musl system; run: make static" >&2
    exit 1
    ;;
esac

# shellcheck source=tools/libpcap-static.sh
. "$root/tools/libpcap-static.sh"
build_static_libpcap "$target_dir/static"

# cc-rs emits the C++ standard library named here for crates that compile C++.
export CXXSTDLIB="static=stdc++"
stdcxx_dir="$(dirname "$(g++ -print-file-name=libstdc++.a)")"
# With --target, these flags reach the binaries and not the build scripts and
# procedural macros, which the compiler has to load as shared objects.
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static -L native=$stdcxx_dir"

cd "$root"
cargo build --release --locked --target "$host" -p mind -p bite "$@"

echo
for name in mind bite; do
  bin="$target_dir/$host/release/$name"
  if readelf -l "$bin" | grep -q 'INTERP'; then
    echo "build-static: $bin asks for a program interpreter, so it is not static" >&2
    exit 1
  fi
  needed="$(readelf -d "$bin" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p')"
  if [ -n "$needed" ]; then
    echo "build-static: $bin needs shared libraries:" >&2
    # shellcheck disable=SC2086  # one library name per word
    printf '  %s\n' $needed >&2
    exit 1
  fi
  echo "$bin: no program interpreter, no shared library"
done
