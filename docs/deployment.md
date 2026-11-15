# Deployment

first-bite runs as one process, `mind`, that captures on a network interface
and publishes records on a ZeroMQ socket. This page covers which release
archive to download for which machine, installing it as a systemd service,
running it in a container, and building it on a machine with no internet
access.

## Platforms

A release has one archive per platform, named
`first-bite-<version>-<platform>`, a `SHA256SUMS` file and an SPDX SBOM. `mind`
is the capture process; `bite` is the dictionary tool (`check`, `decode`,
`schema`) and the decoder library.

| Platform | Archive | Needs on the target | Status |
|---|---|---|---|
| Ubuntu 20.04, 22.04, 24.04, 26.04 on x86-64 | `x86_64-linux-gnu.tar.gz` | glibc 2.31 or later; nothing to install | built and run on the build host, in a bare container of each of the four releases |
| the same on 64-bit Arm | `aarch64-linux-gnu.tar.gz` | glibc 2.31 or later; nothing to install | built in CI; not run locally |
| any Linux on x86-64, with or without a C library | `x86_64-linux-musl.tar.gz` | a Linux kernel, 3.2 or later | built and run on the build host, in bare `busybox:musl`, `alpine:3.24` and `ubuntu:20.04` containers |
| any Linux on 64-bit Arm | `aarch64-linux-musl.tar.gz` | a Linux kernel, 3.2 or later | built in CI; not run locally |
| macOS on Apple silicon | `aarch64-apple-darwin.tar.gz` | nothing to install; libpcap is part of macOS | built in CI; not run locally |
| macOS on Intel | `x86_64-apple-darwin.tar.gz` | nothing to install; libpcap is part of macOS | built in CI; not run locally |
| Windows on x86-64, `bite` only | `x86_64-pc-windows-msvc.zip` | nothing to install | built in CI; not run locally |
| Windows on x86-64, `mind` | none. The release workflow compiles `mind.exe` without the object storage sinks and keeps it as a workflow artifact | [Npcap](https://npcap.com), installed by the user | built in CI; never run |
| iOS, iPadOS, Android, a browser | none. The `bite` library compiles for `aarch64-apple-ios`, `aarch64-linux-android` and `wasm32-unknown-unknown` (`make decoder-targets`) | an application that embeds the library | library compile-checked on the build host and in CI; no application built |

iOS, iPadOS and Android do not allow an app to capture from a network
interface without root or a VPN entitlement. On those platforms the decoder
library is what runs: an application hands it payloads it received some other
way, and `mind` is not built for them.

### Which Linux archive

| Archive | Choose it when | Published |
|---|---|---|
| `linux-gnu`, the standard build | the machine runs Ubuntu 20.04 or later, or any distribution with glibc 2.31 or later. This is the default. | with every release |
| `linux-musl`, the self-contained build | the machine has no glibc or an older one: Alpine, a busybox or `scratch` container image, an embedded or cut-down root filesystem | with the first release of each minor version (`vX.Y.0`), and when the release workflow is run by hand with the input `static` |

Both carry libpcap 1.10.5, ZeroMQ and the C++ runtime inside `mind`, so
neither needs a package installed. The standard build uses the machine's glibc
and so gets its fixes from the distribution. The self-contained build is linked
statically against musl: it has no program interpreter and loads no shared
library, and a fix in the C library reaches it only in a new build. It is
position-independent, so the kernel loads it at a random address like any
other program.

The S3 sink checks TLS certificates against the machine's certificate store
(`ca-certificates` on Ubuntu and Alpine); a machine without one cannot send to
S3 over HTTPS with either build. The REST and Azure sinks carry their own root
certificates.

## The release archive

A Linux archive unpacks to:

| Path in the archive | What |
|---|---|
| `deployment/install.sh` | the installer |
| `deployment/mind`, `deployment/bite` | the capture binary and the dictionary tool |
| `deployment/first-bite.service` | systemd unit |
| `deployment/first-bite.env.example` | settings file, installed as `/etc/first-bite/first-bite.env` |
| `deployment/first-bite.logrotate` | logrotate rule |
| `deployment/config/` | example `publishers.yml`, `subscribers.yml`, `subpubs.yml` |
| `deployment/filebeat/`, `deployment/telegraf/` | optional log and metric shipping examples |
| `dictionaries/` | the example dictionaries `weather-station`, `vehicle-can`, `toot` |
| `LICENSE`, `NOTICE`, `INTENT.md`, `INSTALL.md` | licence files and a copy of this page |

A macOS archive holds `bin/mind`, `bin/bite`, `config/`, `dictionaries/` and
the licence files; the Windows archive holds `bin/bite.exe`, `dictionaries/`
and the licence files. Neither has an installer: run the binaries from where
they were unpacked.

Check a download before unpacking it:

```bash
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify first-bite-<version>-<platform>.tar.gz --repo USAREUR-AF/first-bite
```

## Install on Ubuntu

The host needs Ubuntu 20.04 or later and systemd. Nothing is installed from
the package archive.

```bash
tar -xzf first-bite-<version>-x86_64-linux-gnu.tar.gz
cd first-bite-<version>-x86_64-linux-gnu
sudo deployment/install.sh --interface eth0 --dictionary dictionaries/weather-station
```

| Option | Meaning |
|---|---|
| `--interface <name>` | network interface to capture on; required |
| `--dictionary <dir>` | dictionary to decode with; checked with `bite check`, then copied to `/etc/first-bite/dictionary`. Without it, records are published undecoded. |
| `--prefix <dir>` | binaries go in `<dir>/bin`; default `/usr/local` |
| `--uninstall` | remove the service and the binaries |

The installer asks no questions and prints each thing it installed or kept. It
checks the interface and the dictionary before it changes anything.

The installer needs bash and systemd, whichever archive it came from. On a
machine without them, copy `deployment/mind` to the machine and start it the
way that system starts services; the options and settings are the ones in the
[README](../README.md#run-it-on-a-network).

## Where things live

| Path | What | Written by |
|---|---|---|
| `/usr/local/bin/mind`, `/usr/local/bin/bite` | binaries | every install |
| `/etc/first-bite/first-bite.env` | settings: interface, dictionary path, configuration paths, record metadata, log level | first install only |
| `/etc/first-bite/config/*.yml` | publisher, subscriber and subpub configuration | first install only |
| `/etc/first-bite/dictionary/` | the dictionary | every install given `--dictionary` |
| `/etc/systemd/system/first-bite.service` | the unit | every install |
| `/etc/logrotate.d/first-bite` | daily rotation, seven kept | every install |
| `/var/log/first-bite/mind.log` | everything `mind` logs: its log lines and, at level `info`, one line per record | the service |
| `/var/lib/first-bite/` | working directory; relative paths in the configuration resolve here | the service |

Records and logs are both in `/var/log/first-bite/mind.log`. The journal
(`journalctl -u first-bite`) holds only what systemd reports about the
service: starts, stops, exit codes and restarts.

The service runs as the system user `firstbite` with the capabilities
`CAP_NET_RAW` and `CAP_NET_ADMIN` and no others. Files under `/etc/first-bite`
are readable by root and that user only, because a subscriber configuration can
hold storage credentials.

Settings are `KEY=value` lines in `/etc/first-bite/first-bite.env`; the file
documents each one. After editing it or a file in `/etc/first-bite/config`:

```bash
sudo systemctl restart first-bite
```

`SUBSCRIBER_CONFIG_PATH` and `SUBPUB_CONFIG_PATH` are commented out in the
installed settings file. Edit `/etc/first-bite/config/subscribers.yml` to name
your own sinks before enabling it; the example entries point at the local
stand-ins in [`mocks/`](../mocks).

`FIRST_BITE_ARGS` in the settings file passes extra options to `mind`, split
on spaces. A capture filter contains spaces, so it goes in a unit drop-in:

```bash
sudo systemctl edit first-bite
```

```ini
[Service]
ExecStart=
ExecStart=/usr/local/bin/mind ${FIRST_BITE_INTERFACE} --filter "udp port 40100"
```

## Supply a dictionary

A dictionary is a directory of JSON files;
[dictionary-format.md](dictionary-format.md) is the reference. Install or
replace it by running the installer again with `--dictionary`:

```bash
sudo deployment/install.sh --interface eth0 --dictionary /path/to/dictionary
```

The installer runs `bite check` on the directory and installs nothing if the
check fails. The copy in `/etc/first-bite/dictionary` replaces the previous
one, and the service restarts with it. A dictionary kept elsewhere works too:
set `BITE_DICT_PATH` in the settings file to a directory the `firstbite` user
can read.

## Check that it is working

| Command | Shows |
|---|---|
| `systemctl status first-bite` | `active (running)` and the command line |
| `journalctl -u first-bite` | what systemd did with the unit: starts, exit codes, restarts |
| `sudo tail /var/log/first-bite/mind.log` | the log lines of `mind` |
| `sudo bite check /etc/first-bite/dictionary` | that the installed dictionary loads and its vectors pass |
| `sudo grep -c 'mind::run' /var/log/first-bite/mind.log` | the number of records published since the last rotation |

A healthy start logs the dictionary and the source:

```text
[mind::dictionary] --- dictionary /etc/first-bite/dictionary: 1 interfaces, 2 messages
[mind] --- reading from interface eth0
```

`bite check` on the example dictionary prints:

```text
1 interfaces, 3 messages, 0 errors, 0 warnings, 5/5 vectors passed
```

To read the records themselves, connect a ZeroMQ SUB socket on the same host
to `tcp://127.0.0.1:12000`.

## The record socket

The ZeroMQ PUB socket is unauthenticated and unencrypted: anything that can
reach it reads every record. The default publisher binds
`tcp://127.0.0.1:12000`, which only processes on the same host can reach. To
serve consumers on other hosts, set `address` in
`/etc/first-bite/config/publishers.yml` to an address on a network you trust,
for example `tcp://192.0.2.10:12000`, and restart the service. Do not bind it
to an untrusted network.

## Upgrade and uninstall

| Task | Command | Effect |
|---|---|---|
| upgrade | unpack the new tarball, then `sudo deployment/install.sh --interface eth0` | replaces the binaries, the unit and the logrotate rule; keeps the settings file and `/etc/first-bite/config`; restarts the service |
| change the interface | edit `FIRST_BITE_INTERFACE` in `/etc/first-bite/first-bite.env`, then `sudo systemctl restart first-bite` | the installer never rewrites an existing settings file |
| uninstall | `sudo deployment/install.sh --uninstall` | stops and disables the service; removes the binaries, the unit and the logrotate rule |

Uninstalling keeps `/etc/first-bite`, `/var/log/first-bite`,
`/var/lib/first-bite` and the `firstbite` user. Remove them by hand when the
configuration and logs are no longer wanted.

## Log and metric shipping

Both are optional examples with placeholder endpoints; each directory has a
README.

| Directory | Ships | To |
|---|---|---|
| [`deployment/filebeat`](../deployment/filebeat) | every line of `/var/log/first-bite/*.log` | Elasticsearch |
| [`deployment/telegraf`](../deployment/telegraf) | host CPU, memory, disk and network metrics, and the resource use of `first-bite.service` | InfluxDB 2 |

## Container

The [`Dockerfile`](../Dockerfile) at the repository root builds an image with
`mind` as the entry point, `bite` beside it, and the example dictionaries in
`/usr/share/first-bite/dictionaries`. It runs as a non-root user.

```bash
docker build -t first-bite .

# Replay a capture file.
docker run --rm -v "$PWD/examples:/examples:ro" first-bite \
    --pcap-file /examples/weather-station.pcap \
    --dictionary /usr/share/first-bite/dictionaries/weather-station

# Read frames from a UDP socket; then send some from the host.
docker compose -f docker/compose.yml up --build
python3 tools/mockplay.py examples/weather-station.pcap
```

| Mode | What the container needs |
|---|---|
| capture file, `--mock` | nothing extra. To reach the records from outside the container, the publisher must bind all of the container's interfaces: `docker/compose.yml` mounts [`docker/publishers.yml`](../docker/publishers.yml) for that and publishes the port on the host's loopback only |
| live capture | `--network host --cap-add NET_RAW --cap-add NET_ADMIN --user 0:0`; the compose file shows the same as comments. On the host network keep the default publisher, which binds the host's loopback |
| your own dictionary | a read-only volume and `--dictionary <mount point>` |
| settings | environment variables (`-e BITE_DICT_PATH=...`), the same names as the settings file |

## Build the release binaries

The release workflow runs the same make targets and scripts a developer does;
each needs only docker.

| Command | Does | Output |
|---|---|---|
| `make portable` | builds the standard Linux binaries in an `ubuntu:20.04` container ([`tools/build-portable.sh`](../tools/build-portable.sh)) and fails if `mind` needs a shared library outside glibc | `target/ubuntu-20.04/release/` |
| `make portable-check` | runs them in bare `ubuntu:20.04`, `22.04`, `24.04` and `26.04` containers with no network | one line per image and binary |
| `make static` | builds the self-contained binaries in an `alpine:3.24` container ([`tools/build-static.sh`](../tools/build-static.sh)) and fails if either has a program interpreter or needs a shared library | `target/alpine-3.24/<arch>-unknown-linux-musl/release/` |
| `make static-check` | runs them in bare `busybox:musl`, `alpine:3.24` and `ubuntu:20.04` containers with no network | one line per image and binary |
| `tools/package-release.sh <platform> <directory>` | assembles the archive | `dist/first-bite-<version>-<platform>.tar.gz` |

The check ([`tools/smoke-test.sh`](../tools/smoke-test.sh)) runs `bite check
--strict` on the `weather-station` dictionary and replays
`examples/weather-station.pcap` through `mind`. It passes when every frame in
the file comes out as a decoded record:

```text
ubuntu:20.04   bite  0 errors, 0 warnings, 5/5 vectors passed
ubuntu:20.04   mind  done: 4 frames, 4 records, 4 decoded, 0 not UDP or TCP, 0 malformed
```

Both build images are pinned by digest in the [`Makefile`](../Makefile). The
machine that builds sets the architecture: the workflow builds the 64-bit Arm
archives on an Arm runner with the same commands.

| Binary, x86-64 | Size | Shared libraries |
|---|---|---|
| `mind`, standard | 16.3 MB | `libc`, `libm`, `libdl`, `libpthread`, `libgcc_s`; glibc symbols up to 2.30 |
| `bite`, standard | 1.3 MB | `libc`, `libpthread`, `libgcc_s` |
| `mind`, self-contained | 16.6 MB | none |
| `bite`, self-contained | 1.4 MB | none |

## Build without internet access

`make vendor` copies every dependency into `vendor/` and prints the lines that
tell cargo to use it.

| Step | Where | Command |
|---|---|---|
| 1 | connected machine | `make vendor`, and save the printed lines as `.cargo/config.toml` |
| 2 | connected machine | copy the whole tree, with `vendor/` and `.cargo/config.toml`, to the offline machine |
| 3 | offline machine | `cargo build --release --locked --offline -p mind -p bite` |
| 4 | offline machine | `cp target/release/mind target/release/bite deployment/`, then `sudo deployment/install.sh --interface eth0` |

The offline machine needs the Rust toolchain, a C and C++ compiler, CMake and
the libpcap headers (`libpcap-dev`) installed beforehand.

## Status

| Piece | Status |
|---|---|
| standard Linux build, x86-64 | measured: built in `ubuntu:20.04`, run in bare `ubuntu:20.04`, `22.04`, `24.04` and `26.04` containers |
| self-contained Linux build, x86-64 | measured: built in `alpine:3.24`, run in bare `busybox:musl`, `alpine:3.24` and `ubuntu:20.04` containers |
| 64-bit Arm Linux, macOS and Windows archives | built in CI; not run locally |
| `bite` library for iOS, Android, Windows, WebAssembly and 64-bit Arm Linux | measured: `make decoder-targets` passes |
| `install.sh` from the unpacked x86-64 standard archive, on a host with no libpcap | measured in a bare `ubuntu:26.04` container with `systemctl` replaced by a stub |
| `install.sh`: install, second run, `--prefix`, `--uninstall`, refusal of a broken dictionary | measured in an Ubuntu 26.04 container with `systemctl` replaced by a stub |
| `first-bite.service` | built; `systemd-analyze verify` passes; not yet run under systemd |
| container image: capture-file replay, `bite check` | measured |
| container: `--mock` through `docker/compose.yml`, live capture | built |
| `filebeat/`, `telegraf/` | built |
