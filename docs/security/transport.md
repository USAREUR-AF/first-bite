# Transport security

One `security` block in the configuration states how records may leave the
host. `mind` holds every publisher socket and every sink to it before it reads
a frame, and refuses to start, naming the entry and the requirement, when one
cannot meet it. `mind security show` prints what each path will use.

A configuration with no `security` block makes the connections it made before
the block existed: nothing is required of any path, and startup logs one line
with the effective level.

```text
security: level unenforced (no security block); min TLS 1.2; key exchange classical [X25519, secp256r1, secp384r1]; fips off; provider aws-lc-rs (AWS-LC 5.12.0), FIPS module not active; built with: azure, aws, pqc
```

Contents: [the paths](#the-paths) · [the levels](#the-levels) ·
[choose a level](#choose-a-level) · [the block](#the-security-block) ·
[what is refused](#what-is-refused-at-startup) ·
[the posture command](#the-posture-command) ·
[changing level](#changing-level-later) · [builds](#builds-and-platforms) ·
[FIPS](#fips) · [what each path uses](#what-each-path-uses) ·
[proof](#proof) · [status](#status)

## The paths

Records leave `mind` on a ZeroMQ socket and, from there, through sinks. Only
some of those paths are fit to cross a network.

| Path | On the wire | Use it for |
|---|---|---|
| ZeroMQ PUB socket (`publishers.yml`, default `tcp://127.0.0.1:12000`) | plaintext, no authentication | consumers on the same host only |
| REST sink, Azure Blob sink, S3 sink over `https://` | TLS, optionally with a client certificate | consumers and stores on other hosts |
| Zenoh sink over `tls/` or `quic/` | TLS, optionally with a client certificate | consumers on other hosts |
| local file sinks | no network | this host |

**A consumer on another host reads records through the Zenoh sink or an HTTPS
sink, never from the ZeroMQ socket.** Who reads the records is something the
owner of the host grants: by default the socket binds loopback and no sink
runs. The ZeroMQ socket is plaintext and unauthenticated (encryption and
authentication on it are not built), so it cannot check who connects and the
only control over it is where it binds. Under a `security` block a bind
address other than loopback is refused (built) unless
`allow_plaintext_bind: true` is set, and then startup logs a warning that
names the address.

## The levels

`transport` is the level.

| `transport` | Guarantees | Costs | Needs built in |
|---|---|---|---|
| `unenforced` (also: no block) | nothing; each path is reported as it is | nothing | nothing |
| `loopback-plaintext` | no record leaves the host: every sink endpoint is a loopback address or a local file, and a Zenoh configuration has no off-host endpoint and no multicast scouting | no off-host sink | nothing |
| `tls` | every network sink speaks TLS to a server whose certificate chains to a trusted authority and matches the endpoint's name (a Zenoh file that sets `verify_name_on_connect: false` skips the name, and the report says so); an `http://` or Zenoh `tcp/` endpoint is refused | a certificate on every receiving service; local emulators need a per-sink override | nothing |
| `mtls` | as `tls`, and every network sink presents a client certificate; a sink that cannot is refused | a client certificate and key on this host, and a service that verifies them; the public Azure Blob and AWS S3 services take none, so those sinks need a per-sink override | nothing |

Four more keys tighten any level:

| Key | Values | Guarantees | Needs built in |
|---|---|---|---|
| `min_tls` | `1.3` (default), `1.2` | no sink negotiates an older TLS version | nothing |
| `key_exchange` | `classical` | X25519, P-256 and P-384 are offered | nothing |
| | `hybrid-pqc-preferred` | X25519MLKEM768 is offered first and the classical groups after it; a peer without the hybrid group still connects | `pqc` |
| | `hybrid-pqc-required` | only X25519MLKEM768 is offered; a peer without it is refused at the handshake | `pqc` |
| `fips` | `off` (default), `required` | the crypto provider is the FIPS module and reports FIPS mode, and no sink performs cryptography outside it | `fips` |
| `quic` | `allowed` (default), `required` | `required`: every Zenoh endpoint is `quic/` | `quic` |

"Hybrid" here means the TLS 1.3 group X25519MLKEM768, preferred, with
down-negotiation to a classical group, which is the meaning goat's fabric
gives it (`docs/design/pqc-adoption-pattern.md` sections 3 and 4 in goatcore).
`key_exchange: classical` is the same lever as that fabric's fallback to
classical (`docs/design/pqc-kill-switch.md`, tier 1): the classical groups
only, fixed when the process starts.

## Choose a level

| Situation | Block | Build |
|---|---|---|
| Single host: the consumers run beside `mind` | `transport: loopback-plaintext` | any |
| Trusted enclave: sinks on a network you control, with an internal certificate authority | `transport: tls`, `ca_bundle` naming that authority | any |
| Untrusted network between this host and the consumer | `transport: mtls`, `client_cert`, `client_key`, `ca_bundle`; `min_tls: 1.3` | any |
| A programme that mandates FIPS-validated cryptography | the above plus `fips: required` | the `-fips` archive, or `--features fips` |
| A programme that mandates post-quantum key establishment | the above plus `key_exchange: hybrid-pqc-required`, once every receiving service offers X25519MLKEM768; `hybrid-pqc-preferred` until then | released binaries have `pqc` |
| Local development against the emulators in `mocks/` | no block, or `transport: loopback-plaintext` | any |

`mind security show --probe <subscriber>` tells you whether a receiving
service offers the hybrid group before you require it.

## The security block

The block is the top-level key `security:` of the subscriber file
(`SUBSCRIBER_CONFIG_PATH`), or of a file of its own named by
`SECURITY_CONFIG_PATH` (default `config/security.yml` beside the executable).
Written in both, startup is refused. It covers the publishers, the subscribers
and the subpubs of the process.
[`flow/config/security.example.yml`](../../flow/config/security.example.yml)
has every key.

```yaml
security:
  transport: mtls
  min_tls: 1.3
  key_exchange: hybrid-pqc-preferred
  fips: "off"
  quic: allowed
  client_cert: /etc/first-bite/tls/client.pem
  client_key: /etc/first-bite/tls/client.key
  ca_bundle: /etc/first-bite/tls/ca.pem
  allow_plaintext_bind: false
```

| Key | Default | Meaning |
|---|---|---|
| `transport` | required | the level |
| `min_tls` | `1.3` | lowest TLS version |
| `key_exchange` | `hybrid-pqc-preferred` in a binary with `pqc`, else `classical` | groups offered, in order |
| `fips` | `off` | `required` refuses a binary or a sink that works outside the FIPS module |
| `quic` | `allowed` | `required` refuses a Zenoh endpoint that is not `quic/` |
| `client_cert`, `client_key` | none | PEM files: the certificate chain and private key the REST, Azure and S3 sinks present |
| `ca_bundle` | none | PEM file of the authorities those sinks trust, in place of the public web roots (REST, Azure) or the machine's certificate store (S3) |
| `allow_plaintext_bind` | `false` | permit a ZeroMQ bind address that is not loopback |

An unknown key or value is refused. Certificates and keys are named by path
and never written into the block.

With no block each sink keeps what it did before the block existed: a minimum
TLS version of 1.2, and the classical key-exchange groups, except the S3 sink,
which already offered X25519MLKEM768 first. With a block the defaults are 1.3
and, in a binary with `pqc`, `hybrid-pqc-preferred` for every sink: no sink
needs TLS 1.2, and the Zenoh client already uses 1.3 only whenever it presents
a client certificate or runs over QUIC.

### One subscriber that differs

A subscriber may carry its own `security:` key with `transport`, `min_tls`,
`key_exchange`, `fips`, `quic`, `client_cert`, `client_key` and `ca_bundle`.
A value that differs from the block needs `reason`, which the posture report
prints beside the override. A different certificate or bundle alone needs
none.

```yaml
subscribers:
  - name: ToBlobEmulator
    subscriber_type: objectstorage
    subtype: azureobject
    # ...
    security:
      transport: loopback-plaintext
      reason: "local emulator on the development machine"
```

### The Zenoh sink

The Zenoh sink is configured by its own Zenoh JSON5 file (`zenoh_config`).
`mind` reads that file and holds what it will do against the level:

| In the Zenoh file | Checked |
|---|---|
| `connect.endpoints`, `listen.endpoints` | `tcp/` and other plaintext endpoints are refused under `tls` and `mtls`, including the plaintext listener that `peer` and `router` mode open when `listen` is not written; `mode: "client"` opens none |
| `scouting.multicast.enabled` | must be `false` under `loopback-plaintext`, `tls` and `mtls`, unless the mode is `client` with an endpoint to connect to |
| `transport.link.tls.connect_certificate`, `connect_private_key`, `enable_mtls` | all three are needed under `mtls`; the files are read at startup |
| `transport.link.tls.root_ca_certificate` | read at startup |
| `*_base64` certificate or key values | refused under any level: name a file |

The Zenoh client takes its TLS versions and key-exchange groups from the
process-wide provider, which follows the block, so `min_tls` and
`key_exchange` cannot differ for one Zenoh subscriber; an override that tries
is refused. The block's `client_cert` and `ca_bundle` do not reach the Zenoh
client: it uses the files named in its own configuration.

## What is refused at startup

Every refusal happens before a socket is bound or a frame is read, and
`mind security check` prints the same messages. These are the messages, as
`tools/security-selftest.sh` printed them:

| Case | Message |
|---|---|
| level needs a feature the binary lacks | ``security: `key_exchange: hybrid-pqc-required` needs a binary built with the cargo feature `pqc`; this one was built with: azure, aws, zenoh`` |
| | ``security: `fips: required` needs a binary built with the cargo feature `fips`; this one was built with: azure, aws, zenoh, quic, pqc`` |
| | ``security: `quic: required` needs a binary built with the cargo feature `quic`; this one was built with: azure, aws, pqc`` |
| `http://` endpoint under `tls` or `mtls` | ``subscriber ToCollector: level `tls` needs an https:// endpoint; `endpoint_url` gives http://203.0.113.10:5001/records`` |
| emulator over `http://` under `tls` | ``subscriber ToBlobEmulator: level `tls` needs an https:// endpoint; `cloud_address` gives http://127.0.0.1:10000/devstoreaccount1; for a local emulator give this subscriber `security: { transport: loopback-plaintext, reason: ... }` `` |
| off-host sink under `loopback-plaintext` | ``subscriber ToCollector: level `loopback-plaintext` refuses a sink that leaves this host; `endpoint_url` gives https://collector.example/records`` |
| `mtls` with no identity | ``subscriber ToCollector: level `mtls` needs `client_cert` and `client_key`, in the security block or in this subscriber's `security` `` |
| `mtls` with a service that takes no client certificate | ``subscriber ToBlob: level `mtls` needs a client certificate, and the public Azure Blob service authorises requests by Shared Key and takes no client certificate; to accept server-only TLS for this sink give it `security: { transport: tls, reason: ... }` `` |
| certificate, key or bundle unreadable | ``subscriber ToCollector: `security.client_cert`: cannot read /etc/first-bite/tls/client.pem: I/O error: No such file or directory (os error 2)`` |
| Zenoh `tcp/` endpoint under `tls` or `mtls` | ``subscriber ToZenoh: `zenoh_config`: level `tls` refuses the plaintext endpoint tcp/192.0.2.20:7447; use tls/ or quic/`` |
| Zenoh defaults under `tls` | ``subscriber ToZenoh: `zenoh_config`: level `tls` refuses the plaintext listener tcp/[::]:0 that mode peer opens by default; set `mode: "client"` or list tls/ or quic/ endpoints under `listen.endpoints` `` |
| Zenoh without a client certificate under `mtls` | ``subscriber ToZenoh: `zenoh_config`: level `mtls` needs `transport.link.tls.connect_certificate`, `connect_private_key` and `enable_mtls: true` `` |
| `quic/` endpoint in a binary without `quic` | ``subscriber ToZenoh: `zenoh_config`: endpoint quic/localhost:7447 needs a binary built with the cargo feature `quic`; this one was built with: azure, aws, zenoh`` |
| a requirement one Zenoh sink cannot be set apart for | ``subscriber ToZenoh: `security.min_tls` and `security.key_exchange` cannot differ for a Zenoh sink: the Zenoh client takes both from the process-wide provider, which follows the security block`` |
| `fips: required` with a sink that works outside the module | ``subscriber ToS3: `fips: required`: request signing (SigV4, computed by the AWS SDK with the `hmac` and `sha2` crates) runs outside the FIPS module; to accept that for this sink give it `security: { fips: off, reason: ... }` `` |
| ZeroMQ bind address not loopback | ``publisher records: `address` tcp://0.0.0.0:12000 is not loopback, and the ZeroMQ socket is unauthenticated and unencrypted; bind 127.0.0.1 and send off-host through the Zenoh or HTTPS sinks, or set `allow_plaintext_bind: true` in the security block`` |
| a subscriber reading the feed from another host | ``subscriber Rates: `zmq_url` 192.0.2.10 is not loopback: the ZeroMQ feed is unauthenticated and unencrypted, and level `tls` keeps it on this host`` |
| override without a reason | ``subscriber ToCollector: `security.transport` differs from the security block; say why in `security.reason` `` |
| the block in two files | `security: the block is written in both <file> and <file>; keep one` |

With `allow_plaintext_bind: true` the process starts and logs:

```text
publisher records binds tcp://0.0.0.0:12000, which is not loopback: the socket is unauthenticated and unencrypted, and cannot check which hosts on that network read the records (`allow_plaintext_bind: true`)
```

Two things are decided at the handshake and not at startup, because only the
peer can answer them: whether the server's certificate is trusted, and whether
the peer offers the hybrid group under `hybrid-pqc-required`. A sink whose
handshake is refused delivers nothing and logs the failure; `--probe` shows
the outcome beforehand.

## The posture command

`mind security show` loads the configuration a capture run would load (the
same environment variables, the same `--env-file`), binds nothing, connects to
nothing, and prints every path. `--json` prints the same fields as JSON.
`--activate-subpubs` includes the publishers of the subpub file.

| Command | Does |
|---|---|
| `mind security show` | the report |
| `mind security show --json` | the report as JSON |
| `mind security show --probe <subscriber>` | the report, then one TLS handshake with that subscriber's `https://` endpoint or Zenoh `tls/` endpoints, using the sink's own TLS configuration; prints the negotiated version, cipher suite and group. No record is sent. |
| `mind security check` | exits non-zero and prints the messages startup would stop with; prints one line and exits zero otherwise |

The crypto provider's version and whether the FIPS module is active are asked
of the linked library when the command runs; they are not read from the build
features.

No `security` block, the shipped `flow/config/subscribers.local.yml`:

```text
security level     unenforced (no security block)
minimum TLS        1.2
key exchange       classical [X25519, secp256r1, secp384r1]
fips               off
quic               allowed
crypto provider    aws-lc-rs (AWS-LC 5.12.0)
FIPS module active no
built with         azure, aws, pqc

records  [publisher (ZeroMQ PUB)]
  endpoint         tcp://127.0.0.1:12000
  leaves the host  no
  transport        plaintext
  protocol         ZeroMQ over TCP

All2BlobEmulator  [objectstorage/azureobject]
  endpoint         http://127.0.0.1:10000/devstoreaccount1
  leaves the host  no
  transport        plaintext
  protocol         HTTP/1.1 over TCP

All2S3Mock  [objectstorage/awsobject]
  endpoint         http://127.0.0.1:9090/
  leaves the host  no
  transport        plaintext
  protocol         HTTP/1.1 over TCP
  note             request signing (SigV4, computed by the AWS SDK with the `hmac` and `sha2` crates) runs outside the crypto provider

MessageRates  [stateful]
  endpoint         messagerates.log
  leaves the host  no
  transport        none
  protocol         local file

Write2LocalLog  [local]
  endpoint         first-bite-records.log
  leaves the host  no
  transport        none
  protocol         local file
```

`transport: tls` with `min_tls: 1.2`, a REST sink, an S3 sink and a local
file:

```text
security level     tls (/etc/first-bite/config/subscribers.yml)
minimum TLS        1.2
key exchange       hybrid-pqc-preferred [X25519MLKEM768, X25519, secp256r1, secp384r1]
fips               off
quic               allowed
crypto provider    aws-lc-rs (AWS-LC 5.12.0)
FIPS module active no
built with         azure, aws, pqc

records  [publisher (ZeroMQ PUB)]
  endpoint         tcp://127.0.0.1:12000
  leaves the host  no
  transport        plaintext
  protocol         ZeroMQ over TCP

ToCollector  [rest]
  endpoint         https://collector.example:8443/records
  leaves the host  yes
  transport        TLS
  protocol         HTTP/1.1 over TCP
  minimum TLS      1.2
  key exchange     hybrid-pqc-preferred [X25519MLKEM768, X25519, secp256r1, secp384r1]
  client auth      none
  server trust     built-in web roots
  FIPS mode        no

ToS3  [objectstorage/awsobject]
  endpoint         https://s3.us-east-1.amazonaws.com/
  leaves the host  yes
  transport        TLS
  protocol         HTTP/1.1 over TCP
  minimum TLS      1.2
  key exchange     hybrid-pqc-preferred [X25519MLKEM768, X25519, secp256r1, secp384r1]
  client auth      none
  server trust     the machine's certificate store
  FIPS mode        no
  note             request signing (SigV4, computed by the AWS SDK with the `hmac` and `sha2` crates) runs outside the crypto provider

ToFile  [local]
  endpoint         records.log
  leaves the host  no
  transport        none
  protocol         local file
```

`transport: mtls` with `key_exchange: hybrid-pqc-required`, a Zenoh sink over
QUIC, a REST sink and an Azure sink with an override, from a binary built with
`zenoh` and `quic`:

```text
security level     mtls (/etc/first-bite/config/subscribers.yml)
minimum TLS        1.3
key exchange       hybrid-pqc-required [X25519MLKEM768]
fips               off
quic               allowed
crypto provider    aws-lc-rs (AWS-LC 5.12.0)
FIPS module active no
also compiled in   ring, a dependency of the Zenoh client, which verifies server-certificate signatures with it when `verify_name_on_connect` is false; the QUIC library computes its stateless-reset and address-validation tokens with it
built with         azure, aws, zenoh, quic, pqc

records  [publisher (ZeroMQ PUB)]
  endpoint         tcp://127.0.0.1:12000
  leaves the host  no
  transport        plaintext
  protocol         ZeroMQ over TCP

ToZenoh  [zenoh]
  endpoint         quic/router.example:7447
  leaves the host  yes
  transport        mutual TLS
  protocol         Zenoh (client mode) over QUIC
  minimum TLS      1.3
  key exchange     hybrid-pqc-required [X25519MLKEM768]
  client auth      certificate /etc/first-bite/tls/client.pem
  server trust     /etc/first-bite/tls/ca.pem and the built-in web roots
  FIPS mode        no

ToCollector  [rest]
  endpoint         https://collector.example:8443/records
  leaves the host  yes
  transport        mutual TLS
  protocol         HTTP/1.1 over TCP
  minimum TLS      1.3
  key exchange     hybrid-pqc-required [X25519MLKEM768]
  client auth      certificate /etc/first-bite/tls/client.pem (presented when the server asks)
  server trust     bundle /etc/first-bite/tls/ca.pem
  FIPS mode        no

ToBlob  [objectstorage/azureobject]
  endpoint         https://exampleaccount.blob.core.windows.net/
  leaves the host  yes
  transport        TLS
  protocol         HTTP/1.1 over TCP
  minimum TLS      1.3
  key exchange     hybrid-pqc-required [X25519MLKEM768]
  client auth      none
  server trust     built-in web roots
  FIPS mode        no
  overrides        transport (level tls): the storage service has no client-certificate authentication
```

A probe, against a local server that accepts only the hybrid group:

```text
probe ToPeer https://localhost:49665/records: negotiated TLSv1_3, cipher suite TLS13_AES_256_GCM_SHA384, key exchange group X25519MLKEM768, FIPS mode no
```

`mind <interface> ...`, `mind --pcap-file ...` and `mind --mock` work as
before. `security` is now a subcommand name, so an interface named `security`
has to be captured through a different name.

## Changing level later

| Change | How |
|---|---|
| any value of `transport`, `min_tls`, `allow_plaintext_bind`; certificates and bundles; a per-sink override | edit the configuration, run `mind security check`, restart the service |
| `key_exchange` between `classical`, `hybrid-pqc-preferred` and `hybrid-pqc-required` | configuration only, in a binary with `pqc` (every released binary) |
| `fips: required` | install the `-fips` archive (or rebuild with `--features fips`), then set the key |
| `quic/` endpoints, `quic: required` | rebuild with `--features zenoh,quic`, then edit the Zenoh file |
| a Zenoh sink at all | rebuild with `--features zenoh` |

The level is read once, when the process starts. A running process does not
change level.

## Builds and platforms

| Cargo feature (`mind`) | Adds | Default |
|---|---|---|
| `pqc` | X25519MLKEM768 can be offered; `hybrid-pqc-*` values are accepted | on |
| `quic` | QUIC transport for the Zenoh sink (enables `zenoh`) | off |
| `fips` | the crypto provider is the upstream FIPS module build of AWS-LC | off |

```sh
cargo build --release -p mind                          # azure, aws, pqc
cargo build --release -p mind --features zenoh,quic    # + Zenoh over TLS and QUIC
cargo build --release -p mind --features fips          # FIPS module build
```

| Platform | `pqc` | `quic` | `fips` | Status |
|---|---|---|---|---|
| Linux x86-64, glibc (`x86_64-linux-gnu`) | yes | yes | yes: `make portable-fips`, archive `x86_64-linux-gnu-fips` | measured on the build host: tests pass with each feature; the FIPS binary built in `ubuntu:20.04` runs in bare Ubuntu 20.04 to 26.04 containers and reports the module active |
| Linux x86-64, static musl (`x86_64-linux-musl`) | yes | yes | yes: `make static-fips`; no release archive | measured: built in `alpine:3.24`, static, runs in bare `busybox:musl`, `alpine:3.24` and `ubuntu:20.04` containers and reports the module active |
| Linux 64-bit Arm, glibc and musl | yes | yes | designed: the release workflow builds `aarch64-linux-gnu-fips` | not run |
| macOS | yes | yes | no: `compile_error!` | upstream builds the FIPS module as a shared library there, which no archive of this project carries; not tried |
| Windows (`mind` is built without the object storage sinks) | yes | yes | no: `compile_error!` | not tried |
| WebAssembly decoder (`bite-wasm`), `bite` library on iOS and Android | not applicable | not applicable | not applicable | the decoder has no network code and links no TLS or crypto library (`cargo tree -p bite-wasm`) |

The FIPS module's build needs CMake and Go (1.17.13 or later) besides the C
compiler. `make portable-fips` and `make static-fips` install both in the
build container.

## FIPS

`fips: required` means three things, each checked at startup:

1. the binary was built with `fips`, so the only TLS crypto provider is the
   upstream FIPS module build of AWS-LC (`aws-lc-fips-sys` 0.14.4, which
   vendors AWS-LC FIPS 4.2.0);
2. that library, asked at run time, reports FIPS mode, and so does the TLS
   configuration built for each sink;
3. no sink performs cryptography outside the module. The Azure sink signs its
   requests with the provider's HMAC. The S3 sink's request signing is done
   by the AWS SDK with other crates, so it is refused unless overridden. A
   Zenoh `quic/` endpoint and `verify_name_on_connect: false` are refused,
   because both use `ring`.

In FIPS mode the provider offers X25519MLKEM768, P-256 and P-384; X25519 on
its own is not offered.

**FIPS mode means the FIPS module is the cryptographic library in use. It does
not by itself make a deployment compliant.** Whether a CMVP certificate covers
the module version this build links, and whether your operating environment
is within it, is for the programme to establish: upstream describes the
module `aws-lc-fips-sys` binds as one that has completed validation testing
and been submitted to NIST, and points to the CMVP lists for its status. A
programme that needs a particular certified module version pins `aws-lc-rs`
and `aws-lc-fips-sys` in `Cargo.lock` to the pair that vendors it; the two
move together.

## What each path uses

Every TLS connection goes through rustls 0.23.46 with one crypto provider,
aws-lc-rs 1.19.0. `mind` installs it as the process default at start, built
from the block's `min_tls` and `key_exchange`, before anything can connect.

| Path | Before the security block | Now | Read in |
|---|---|---|---|
| REST and Azure sinks (`reqwest` 0.12.28) | feature `rustls-tls`: the `ring` provider unless a process default exists; TLS 1.2 and 1.3; X25519, P-256, P-384; built-in web roots | handed a rustls configuration on aws-lc-rs through `use_preconfigured_tls`; feature `rustls-tls-manual-roots-no-provider`, so `reqwest` brings no provider | `reqwest` `src/async_impl/client.rs` lines 735 to 776; `rustls` `src/crypto/ring/mod.rs` line 180 |
| S3 sink (`aws-sdk-s3` 1.159.0, `aws-smithy-http-client` 1.5.2) | the SDK's own HTTP client built its own aws-lc-rs provider and did not consult the process default; TLS 1.2 and 1.3; X25519MLKEM768 first (it enables rustls `prefer-post-quantum`); the machine's certificate store; no way to set versions, groups or a client certificate, since a caller-supplied provider is behind the `aws_sdk_unstable` cfg | the SDK is given an HTTP client built on the same `reqwest` configuration as the other sinks (`flow/src/security/s3_client.rs`); `default-https-client` is off and `aws-smithy-http-client` is out of the tree | `aws-smithy-http-client` `src/client/tls/rustls_provider.rs` |
| Azure request signature | HMAC-SHA256 from the `hmac` and `sha2` crates | HMAC-SHA256 from aws-lc-rs | `flow/src/sink/azure_blob.rs` |
| S3 request signature | `hmac` and `sha2`, inside `aws-sigv4` | unchanged; it cannot be redirected, and the posture report says so on every S3 sink | `aws-sigv4` 1.4.3 `Cargo.toml` |
| Zenoh sink over `tls/` (`zenoh` 1.9.0, `zenoh-link-tls`) | Zenoh installs `ring` as the process default if none exists, then builds from the default; TLS 1.2 and 1.3 without a client certificate, 1.3 only with one; X25519, P-256, P-384 | the process default is already aws-lc-rs when Zenoh starts, so Zenoh's install is a no-op (the first provider installed wins; goatcore `docs/design/zenoh-pqc-tls.md` section 1 uses the same seam) | `zenoh-link-tls` `src/utils.rs` lines 360 to 432 |
| Zenoh sink over `quic/` | not compiled | feature `quic` enables `transport_quic` (`zenoh-link-quic`, `quinn` 0.11.12); same process default; TLS 1.3 | `zenoh-link-commons` `src/quic/utils.rs` lines 329 to 452 |
| ZeroMQ sockets | plaintext | plaintext; bind and feed addresses checked | `flow/src/security/mod.rs` |

`ring` 0.17.14 is no longer in the tree of a default build. It remains in a
build with `zenoh`, because Zenoh's crates require it: with
`verify_name_on_connect: false` the Zenoh client verifies the server's
certificate signatures against `ring`'s algorithm table
(`zenoh-link-commons` `src/tls.rs` lines 83 to 102), and the QUIC library
computes stateless-reset and address-validation tokens with it (`quinn-proto`
`src/config/mod.rs`, `src/token.rs`). Key exchange and record protection do
not use it. The posture report names it under `also compiled in`.

| Measured with `cargo tree -p mind -e normal,build` | Before | Now |
|---|---|---|
| crates, default features | 233 | 228 |
| crates, `--no-default-features` | 163 | 166 |
| crates, default plus `zenoh` | 399 | 396 |
| crates, default plus `zenoh,quic` | not available | 415 |
| crates, default plus `fips` | not available | 237 |
| `cargo tree -i ring`, default features | `rustls`, `rustls-webpki` | nothing |
| `cargo tree -i aws-lc-rs`, default features | `rustls`, `rustls-webpki` | `flow`, `rustls`, `rustls-webpki` |

A build with no sink feature now links aws-lc-rs where it linked `ring`.

## Proof

[`tools/security-selftest.sh`](../../tools/security-selftest.sh) (`make
security-selftest`) builds `mind` with four feature sets and runs every case
on this page against local servers with throwaway certificates: an HTTPS
server from Python's standard library, `openssl s_server`, and a stock
`eclipse/zenoh:1.9.0` router in a container with TLS and QUIC listeners that
require a client certificate. It removes what it started. Measured on the
build host (x86-64 Linux, OpenSSL 3.5.5), with `--fips`: 51 passed, 0 failed.

| Shown | Evidence line |
|---|---|
| REST over TLS | `REST over TLS: 5 records posted to https://localhost:44621/records` |
| REST over mutual TLS, accepted | `REST over mutual TLS: 5 records accepted with the client certificate` |
| REST over mutual TLS, refused without the certificate | `probe ToCollector https://localhost:44627/records: the peer refused the connection: received fatal alert: CertificateRequired` |
| Zenoh over `tls/` with mutual TLS | `Zenoh over tls/ with mutual TLS: records stored by the router` |
| Zenoh over `quic/` with mutual TLS | `Zenoh over quic/ with mutual TLS: records stored by the router` |
| hybrid handshake, end to end | `key exchange group X25519MLKEM768` from the probe, and from the peer: `Supported groups: X25519MLKEM768:x25519:secp256r1:secp384r1`, `Peer Temp Key: X25519MLKEM768` |
| `hybrid-pqc-required` against a classical-only peer | `handshake failed: received fatal alert: HandshakeFailure`, with `openssl s_server -groups X25519:P-256` and with the stock Zenoh router |
| `hybrid-pqc-preferred` against the same peer | `key exchange group X25519` |
| `fips: required` on a `fips` build | `FIPS module active yes`; `probe ... key exchange group X25519MLKEM768, FIPS mode yes`; 5 records posted |
| `fips: required` on another build | refused, message above |

The stock Zenoh router uses `ring` and offers classical groups only, so a
Zenoh session to it negotiates X25519; a router built on aws-lc-rs is needed
for a hybrid Zenoh session, and that pairing was not run here.

## Status

| Piece | Status |
|---|---|
| one crypto provider, installed at start; `ring` out of the default build | built; measured with `cargo tree` |
| `security` block, per-sink override, startup refusals | built; measured by unit tests and `tools/security-selftest.sh` |
| client certificate and CA bundle for the REST, Azure and S3 sinks | built; REST measured against a local server; Azure and S3 not run against a server that asks for a certificate |
| Zenoh configuration inspection | built; measured |
| `mind security show`, `--json`, `check`, `--probe` for `https://` and Zenoh `tls/` | built; measured |
| `--probe` for Zenoh `quic/` endpoints | not built |
| features `pqc`, `quic`, `fips` | built; measured on x86-64 Linux |
| `make portable-fips`, `make static-fips` and their checks | built; measured on x86-64 |
| CI job `security-features`; release job `linux-fips` | written; not run |
| hybrid key exchange on a Zenoh session | designed; needs a router that offers the group; not measured |
| changing level without a restart | not built |
