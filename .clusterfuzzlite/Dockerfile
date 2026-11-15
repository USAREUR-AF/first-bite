# The image ClusterFuzzLite builds the fuzz targets in (.clusterfuzzlite/build.sh).
# Built with the repository root as its context.
#
# The base image is OSS-Fuzz's Rust builder: clang, libFuzzer, a nightly
# toolchain and cargo-fuzz. `v1` is a moving tag; the digest is the pin, and
# Dependabot moves it.
FROM gcr.io/oss-fuzz-base/base-builder-rust:v1@sha256:3428d0a90b1e7efbe2ba947bd1d0c9a70594986b4b6553804547f1fb24cfc92f
# cmake for ZeroMQ; flex, bison and xz for libpcap (tools/libpcap-static.sh).
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake flex bison xz-utils \
    && rm -rf /var/lib/apt/lists/*
COPY . $SRC/first-bite
WORKDIR $SRC/first-bite
COPY .clusterfuzzlite/build.sh $SRC/build.sh
