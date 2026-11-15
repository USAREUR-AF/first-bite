# flow

Egress for first-bite. Decoded records are published on local ZeroMQ sockets;
from there, subscribers forward them to sinks and subpubs transform and
republish them.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../docs/img/record-flow-dark.svg">
  <img src="../docs/img/record-flow.svg" width="100%" alt="mind hands each record to the publishers, a ZeroMQ PUB socket on tcp://127.0.0.1:12000 by default. A subscriber reads the socket, keeps the records on its topics and forwards each to one sink: object storage, HTTP, a local file or Zenoh. A subpub reads the socket the same way, transforms each record into Cursor-on-Target and publishes the result on a socket of its own, tcp://127.0.0.1:12001 by default. Both sockets bind this host only by default; a ZeroMQ SUB socket of your own can read either.">
</picture>

| Step | What | Where |
|---|---|---|
| 1 | `mind` hands each record to the publishers | `Publishers`, a ZeroMQ PUB socket, by default `tcp://127.0.0.1:12000` |
| 2 | subscribers read records by topic and forward each to a sink | `Subscribers`: object storage, HTTP, a local file, Zenoh |
| 3 | subpubs read records, transform them and publish the result | `SubPubs`: Cursor-on-Target, by default on `tcp://127.0.0.1:12001` |

| Type | Does |
|---|---|
| `Publishers` | Binds the configured sockets; `publish(record)` sends a record on each. |
| `Subscribers` | One thread per subscriber reads a socket and hands the records on its topics to a `Sink`. |
| `SubPubs` | One thread per subpub reads a socket, applies a `Transform`, and publishes the result. |

All three are values owned by the caller. Each is built from its YAML file and
checked in full before anything runs. With no subscriber or subpub file there
are no subscribers or subpubs; with no publisher file there is one PUB socket
on `tcp://127.0.0.1:12000`.

A record crosses `flow` as JSON text. Each publisher and each sink writes it
as JSON or, with `encoding: cbor` in its configuration, as CBOR;
[`src/encoding.rs`](src/encoding.rs) holds both directions and
[docs/record.md](../docs/record.md#encodings) describes the result.

Configuration is described in [`config/README.md`](config/README.md).

## Sinks

A sink implements `flow::Sink`:

```rust
#[async_trait]
pub trait Sink: Send + Sync {
    async fn prepare(&self) -> Result<(), SinkError> { Ok(()) }
    async fn send(&self, topic: &str, record: &str) -> Result<(), SinkError>;
}
```

`prepare` runs once before the first record (create the bucket, open the
session). `flow::build_sink` in [`src/sink/mod.rs`](src/sink/mod.rs) is the one
function that turns a subscriber's configuration into a sink; a new sink is a
module in `src/sink/` and an arm there.

| Module | Sink | Cargo feature |
|---|---|---|
| `sink::azure_blob` | Azure Blob Storage, over its REST API | `azure` |
| `sink::aws_s3` | S3 and S3-compatible stores | `aws` |
| `sink::zenoh` | Zenoh put | `zenoh` |
| `goat::sink` | Zenoh put on a goat data fabric, with goat's identity, key and announcement conventions; optional and off by default ([docs/goat/joining.md](../docs/goat/joining.md)) | `goat` |
| `sink::rest` | HTTP POST | always built |
| `sink::local_log` | local file: one JSON record per line, or a CBOR sequence | always built |
| `sink::message_rates` | per-topic counts | always built |

`flow` enables no feature by default. `mind` forwards the four features and
enables `azure` and `aws` by default. Three more features, `pqc` (on by
default in `mind`), `quic` and `fips`, select transport security that has to
be compiled in; [`src/security/`](src/security) holds every sink to the
`security` block of the configuration
([`docs/security/transport.md`](../docs/security/transport.md)).

## Standalone subscribers

The `flow` binary runs the configured subscribers against publishers that are
already running:

```sh
SUBSCRIBER_CONFIG_PATH=flow/config/subscribers.local.yml cargo run -p flow --features azure,aws
```
