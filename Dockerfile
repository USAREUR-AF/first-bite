# first-bite container image: mind as the entry point, bite beside it, and the
# example dictionaries. Build from the repository root:
#
#   docker build -t first-bite .
#
# `make image` does the same and records the version and the commit in the
# image's labels; the release workflow publishes that image to ghcr.io.
#
# Base images are pinned by tag and digest; change both together. The builder
# and the runtime are the same Debian release so the binaries find the glibc
# and libpcap they were linked against.

FROM rust:1.100.0-trixie@sha256:e19840879045c951c905d0eaa9887f3439c62000e429963371b376bac8d38c31 AS builder
# The image's toolchain, not the channel named in rust-toolchain.toml, so the
# build downloads no second toolchain.
ENV RUSTUP_TOOLCHAIN=1.100.0
RUN apt-get update \
    && apt-get install -y --no-install-recommends libpcap-dev cmake clang \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# Unset, cargo uses every processor.
ARG CARGO_BUILD_JOBS
RUN cargo build --release --locked -p mind -p bite

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
# The upgrade installs the fixes Debian has released since the base image was
# built; the digest above still fixes everything else.
RUN apt-get update \
    && apt-get upgrade -y \
    && apt-get install -y --no-install-recommends libpcap0.8 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 firstbite \
    && useradd --system --uid 10001 --gid 10001 --no-create-home --home-dir /var/lib/first-bite \
        --shell /usr/sbin/nologin firstbite \
    && install -d -o firstbite -g firstbite /var/lib/first-bite
COPY --from=builder /src/target/release/mind /src/target/release/bite /usr/local/bin/
COPY dictionaries /usr/share/first-bite/dictionaries
COPY LICENSE NOTICE INTENT.md /usr/share/doc/first-bite/
# The user `firstbite`, by number: a runtime that must check the user is not
# root before it starts the container (Kubernetes `runAsNonRoot`) can do so
# only for a numeric user.
USER 10001:10001
# --output and a local-file sink write relative paths here.
WORKDIR /var/lib/first-bite
# ZeroMQ PUB socket of the default publisher; UDP socket of --mock.
EXPOSE 12000/tcp 12345/udp
ENTRYPOINT ["mind"]
# With no arguments the container prints the options and exits.
CMD ["--help"]
# Last, so that a new version or commit rebuilds nothing above.
ARG VERSION=0.0.0-local
ARG REVISION=unknown
LABEL org.opencontainers.image.title="first-bite" \
      org.opencontainers.image.description="Reads frames from a network interface or a capture file, decodes fixed-layout binary messages against a dictionary, and publishes each as a JSON record" \
      org.opencontainers.image.source="https://github.com/USAREUR-AF/first-bite" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="$VERSION" \
      org.opencontainers.image.revision="$REVISION"
