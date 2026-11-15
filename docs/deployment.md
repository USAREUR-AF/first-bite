# Deployment

first-bite runs as one process, `mind`, that captures on a network interface
and publishes records on a ZeroMQ socket. This page covers which release
archive to download for which machine, installing it as a systemd service,
running it in a container, and building it on a machine with no internet
access.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="img/platforms-dark.svg">
  <img src="img/platforms.svg" width="100%" alt="Where first-bite runs. The standard Linux build needs glibc 2.31 or later and is built and run on x86-64 in Ubuntu 20.04, 22.04, 24.04 and 26.04. The self-contained Linux build is fully static, needs only a Linux kernel 3.2 or later, and is built and run on x86-64. Archives for 64-bit Arm Linux, macOS and Windows are built in CI and not run locally. The container image replays a capture file, measured. The decoder library is compile-checked for iOS, Android, Windows, WebAssembly and 64-bit Arm Linux; no application is built. A build with no internet access uses vendored dependencies.">
</picture>

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
| `linux-gnu-fips`, the standard build with the FIPS module | the configuration sets `fips: required`: the crypto provider is the upstream FIPS module build of AWS-LC ([security/transport.md](security/transport.md#fips)) | with the first release of each minor version (`vX.Y.0`), and when the release workflow is run by hand with the input `fips` |

Both carry libpcap 1.10.7, ZeroMQ and the C++ runtime inside `mind`, so
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
| `dictionaries/` | the example dictionaries `ntp`, `weather-station`, `vehicle-can`, `glasshouse` |
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
| `/etc/first-bite/credentials/` | optional credential files for the unit's `LoadCredential=` lines; `0700 root:root` | you |
| `/etc/first-bite/dictionary/` | the dictionary | every install given `--dictionary` |
| `/etc/systemd/system/first-bite.service` | the unit | every install |
| `/etc/logrotate.d/first-bite` | daily rotation, seven kept | every install |
| `/var/log/first-bite/mind.log` | everything `mind` logs: its log lines and, at level `info`, one line per record | the service |
| `/var/lib/first-bite/` | working directory; relative paths in the configuration resolve here | the service |

Records and logs are both in `/var/log/first-bite/mind.log`. The journal
(`journalctl -u first-bite`) holds only what systemd reports about the
service: starts, stops, exit codes and restarts.

The service runs as the system user `firstbite` with the capabilities
`CAP_NET_RAW` and `CAP_NET_ADMIN` and no others. `/etc/first-bite` and
`/etc/first-bite/config` are `0750 root:firstbite`, and the environment file
and every configuration file are `0640 root:firstbite`; `install.sh` sets
this on every run, also for files it keeps. Records written under
`/var/lib/first-bite` are created `0640` in a directory other users cannot
list.

### Credentials

A subscriber that stores records in Azure Blob Storage or S3 needs a
credential. The configuration file does not hold it; it holds a reference:

```yaml
# /etc/first-bite/config/subscribers.yml
    cloud_access_key: "${FIRST_BITE_BLOB_ACCESS_KEY}"
```

```bash
# /etc/first-bite/first-bite.env
FIRST_BITE_BLOB_ACCESS_KEY=<the key>
```

`mind` replaces the reference with the variable's value when it loads the
file. A reference to a variable that is not set stops the service at startup
with an error naming the subscriber, the key and the variable. The value is
not written to the log or to an error message at any log level.

The other way is a file per credential, which keeps values out of the
environment file as well. Put each value in a file under
`/etc/first-bite/credentials/` (`0700 root:root`; make the files `0600`),
uncomment the matching `LoadCredential=` line in the unit, and name the
credential in the subscriber:

```yaml
    cloud_access_key_file: "blob-access-key"
```

systemd hands the service a private copy; the service user cannot read the
original.

A credential written in the configuration file as a plain value still works.
The service logs one warning for each at startup, naming the subscriber and
the key. To move an existing deployment: put the value in
`first-bite.env` under a name of your choice, replace the value in
`subscribers.yml` with `"${THAT_NAME}"`, and restart.
[flow/config/README.md](../flow/config/README.md#credentials-and-references)
has the rules in full.

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
[mind::dictionary] --- dictionary /etc/first-bite/dictionary: 1 interfaces, 3 messages
[mind] --- reading from interface eth0
```

`bite check` on the example dictionary prints:

```text
1 interfaces, 3 messages, 0 errors, 0 warnings, 5/5 vectors passed
```

To read the records themselves, connect a ZeroMQ SUB socket on the same host
to `tcp://127.0.0.1:12000`.

## The record socket

The ZeroMQ PUB socket is plaintext and unauthenticated: it cannot check who
connects, so the only control over it is where it binds. The default
publisher binds `tcp://127.0.0.1:12000`, which only processes on the same
host can reach. To serve consumers on other hosts, send records through the
Zenoh sink or an HTTPS sink ([security/transport.md](security/transport.md)),
where the receiving service decides who reads them. Binding the socket to
another address, for example `tcp://192.0.2.10:12000` in
`/etc/first-bite/config/publishers.yml`, puts the records on that network in
clear; under a `security` block it is refused unless
`allow_plaintext_bind: true` is set.

## Transport security

One `security` block in the configuration states how records may leave the
host: plaintext on loopback only, TLS, or mutual TLS, with a minimum TLS
version, a key-exchange policy and whether the FIPS module is required.
`mind` refuses to start when a publisher or sink cannot meet it.
[security/transport.md](security/transport.md) is the guide: the levels, how
to choose one, and which archive carries what.

| Task | Command |
|---|---|
| see what every publisher and sink will use | `sudo -u firstbite mind security show --env-file /etc/first-bite/first-bite.env` |
| check a configuration before restarting the service | `sudo -u firstbite mind security check --env-file /etc/first-bite/first-bite.env` |
| one handshake with a sink's endpoint | `mind security show --probe <subscriber name>` |

Put the block in `/etc/first-bite/config/security.yml` and set
`SECURITY_CONFIG_PATH` to it in the settings file, or write it at the top of
`subscribers.yml`. Consumers on other hosts are served by the Zenoh sink or an
HTTPS sink; the ZeroMQ socket stays on loopback.

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

Each release publishes an image for `linux/amd64` and `linux/arm64` with
`mind` as the entry point, `bite` beside it, and the example dictionaries in
`/usr/share/first-bite/dictionaries`. It runs as the non-root user `firstbite`,
which the image declares by number, `10001:10001`, so Kubernetes can enforce
`runAsNonRoot: true` without a `runAsUser` of your own. Started with no
arguments, it prints the options of `mind` and exits.

```bash
docker pull ghcr.io/usareur-af/first-bite:<version>
```

| Tag | Is |
|---|---|
| `<version>`, for example `1.1.0` | that release, for both platforms |
| `<version>-amd64`, `<version>-arm64` | that release, for one platform |
| `latest` | the newest release that is not a pre-release |

A pre-release has a version with a hyphen, such as `1.1.0-rc.1`; it is
published under its own tags and never as `latest`.

The image of a release carries two signed attestations, stored in the registry
beside it and by GitHub. Check them before running an image you did not build:

```bash
# Built by this repository's release workflow, from the commit of the tag.
gh attestation verify oci://ghcr.io/usareur-af/first-bite:<version> --repo USAREUR-AF/first-bite

# The SPDX SBOM attested to the same image.
gh attestation verify oci://ghcr.io/usareur-af/first-bite:<version> --repo USAREUR-AF/first-bite \
    --predicate-type https://spdx.dev/Document/v2.3
```

The SBOM is the one published with the release, `first-bite-<version>.spdx.json`:
it lists the Rust crates compiled into `mind` and `bite`, and not the Debian
packages of the base image. The image's labels give the version
(`org.opencontainers.image.version`) and the commit
(`org.opencontainers.image.revision`) it was built from.

The [`Dockerfile`](../Dockerfile) at the repository root builds the same image
from a checkout: `make image` builds it as `first-bite:local` with those
labels, and `make image-check` runs it
([`tools/image-check.sh`](../tools/image-check.sh)). The examples below use an
image built that way; with a published image, put its name in place of
`first-bite`.

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
| `make portable-fips`, `make portable-fips-check` | the standard build with the cargo feature `fips`; the check also requires `mind security show` to report the FIPS module active | `target/ubuntu-20.04-fips/release/` |
| `make static-fips`, `make static-fips-check` | the self-contained build with the cargo feature `fips`, and the same check | `target/alpine-3.24-fips/<arch>-unknown-linux-musl/release/` |
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

## When a run ends

A run ends when a capture file is exhausted (`--pcap-file`) or the process
receives SIGINT or SIGTERM. `mind` then stops reading and gives the sinks up
to `--drain-seconds` (default 30) to deliver the records already published.
Each sink stops as soon as it has delivered what was published for it, so a
run whose sinks are keeping up ends at once. The log then states, per sink:

```text
sink ToCollector: 18 delivered, 0 failed, 0 abandoned
```

| Count | Meaning |
|---|---|
| delivered | the sink handed the record over: the endpoint answered, the object was stored, the line was written |
| failed | the sink tried and the destination refused or did not answer, after the sink's own retries |
| abandoned | published for the sink and not delivered when the time ran out, or lost before it reached the sink |

| Exit status | Meaning |
|---|---|
| 0 | the input was read, and every sink delivered what was published for it, or failed some records and delivered others |
| 3 | the input was read, and a sink abandoned records or delivered none of those it was given |
| 1 | the run could not start, or the source failed |

Set `--drain-seconds` above the longest a sink may take to make its first
delivery: a first name lookup or TLS handshake of several seconds is common in
a cluster. A job that replays a capture file should treat status 3 as a
failure; the records can be replayed again.

During a file replay `mind` reads no faster than the sinks deliver: when a
sink is more than 500 records behind, reading waits for it. A sink that
delivers nothing for five seconds is no longer waited for, and the log says
so. Live capture is never held up by a sink.

The record socket is closed last and waits up to half a second for a consumer
in another process to take what is queued for it.

## A site that allows capture only

Where a site permits a box that records traffic and no live decoder or
uplink, [`deployment/capture-node`](../deployment/capture-node/README.md)
captures to a ring of files bounded by size and age and, where storage is
reachable, copies completed files to an S3-compatible bucket. The files are
decoded afterwards with `mind --pcap-file`. Built; measured in a container,
not on a physical tap.

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
| container image, `linux/amd64`: default command, non-root user, `bite check`, capture-file replay, labels | measured with `make image-check` on the build host |
| container image, `linux/arm64` | built in CI; not run |
| publishing the image: push of each platform, the joined tag, `latest` only for a release that is not a pre-release | measured against a registry on the build host's loopback, with a stand-in for the `linux/arm64` image |
| publishing to `ghcr.io`, and the attestations | built in CI; not run |
| container: `--mock` through `docker/compose.yml`, live capture | built |
| `filebeat/`, `telegraf/` | built |
