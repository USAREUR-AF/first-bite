# first-bite container image: mind as the entry point, bite beside it, and the
# example dictionaries. Build from the repository root:
#
#   docker build -t first-bite .
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
RUN cargo build --release --locked -p mind -p bite

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
# The upgrade installs the fixes Debian has released since the base image was
# built; the digest above still fixes everything else.
RUN apt-get update \
    && apt-get upgrade -y \
    && apt-get install -y --no-install-recommends libpcap0.8 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --user-group --no-create-home --home-dir /var/lib/first-bite \
        --shell /usr/sbin/nologin firstbite \
    && install -d -o firstbite -g firstbite /var/lib/first-bite
COPY --from=builder /src/target/release/mind /src/target/release/bite /usr/local/bin/
COPY dictionaries /usr/share/first-bite/dictionaries
COPY LICENSE NOTICE INTENT.md /usr/share/doc/first-bite/
USER firstbite
# --output and a local-file sink write relative paths here.
WORKDIR /var/lib/first-bite
# ZeroMQ PUB socket of the default publisher; UDP socket of --mock.
EXPOSE 12000/tcp 12345/udp
ENTRYPOINT ["mind"]
