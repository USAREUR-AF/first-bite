#!/bin/bash -eu
# Build the fuzz targets of fuzz/ for ClusterFuzzLite. Run by the `compile`
# command of the builder image, which sets CC, CXX, CFLAGS, RUSTFLAGS, the
# nightly toolchain (RUSTUP_TOOLCHAIN, which takes precedence over
# rust-toolchain.toml), $SRC, $WORK and $OUT.
#
# Each target is left in $OUT with its seed corpus (<target>_seed_corpus.zip,
# written by fuzz/seeds.py) and, where there are any, its libFuzzer dictionary
# and options.

cd "$SRC/first-bite"

# The targets run in an image that has no libpcap, so it is linked statically,
# from the same checksummed source the release binaries are built with.
# shellcheck source=tools/libpcap-static.sh
. tools/libpcap-static.sh
# shellcheck disable=SC2153  # WORK is set by the builder image
build_static_libpcap "$WORK/libpcap"

# ZeroMQ, a dependency of mind, is compiled from C++ by its crate. The builder
# compiles C++ against libc++. Its CFLAGS turn a call to an undeclared function
# into a warning, which makes the crate's test for strlcpy() pass on a C
# library that has none; for the crates' C compiler it is an error again.
export CXXSTDLIB=c++
export CFLAGS_x86_64_unknown_linux_gnu="$CFLAGS -Werror=implicit-function-declaration"

python3 fuzz/seeds.py --out "$WORK/corpus"

cd fuzz
cargo fuzz build -O --debug-assertions
for target in $(cargo fuzz list); do
  cp "target/x86_64-unknown-linux-gnu/release/$target" "$OUT/"
  zip -jq "$OUT/${target}_seed_corpus.zip" "$WORK/corpus/$target"/*
  if [ -f "dicts/$target.dict" ]; then
    cp "dicts/$target.dict" "$OUT/"
  fi
  if [ -f "options/$target.options" ]; then
    cp "options/$target.options" "$OUT/"
  fi
done
