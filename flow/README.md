# flow

Egress for first-bite. Decoded records are published on local ZeroMQ sockets;
from there, subscribers forward them to sinks and subpubs transform and
republish them.

```
mind ──► Publishers ──► tcp://127.0.0.1:12000 ──► Subscribers ──► sink (object storage, HTTP, file, Zenoh)
                                     └──► SubPubs ──► transform ──► tcp://127.0.0.1:12001
```

| Type | Does |
|---|---|
| `Publishers` | Binds the configured sockets; `publish(record)` sends a record on each. |
| `Subscribers` | One thread per subscriber reads a socket and hands the records on its topics to a `Sink`. |
| `SubPubs` | One thread per subpub reads a socket, applies a `Transform`, and publishes the result. |

All three are values owned by the caller. Each is built from its YAML file and
checked in full before anything runs. With no subscriber or subpub file there
are no subscribers or subpubs; with no publisher file there is one PUB socket
on `tcp://127.0.0.1:12000`.

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
| `sink::rest` | HTTP POST | always built |
| `sink::local_log` | local file, one record per line | always built |
| `sink::message_rates` | per-topic counts | always built |

`flow` enables no feature by default. `mind` forwards the three features and
enables `azure` and `aws` by default.

## Standalone subscribers

The `flow` binary runs the configured subscribers against publishers that are
already running:

```sh
SUBSCRIBER_CONFIG_PATH=flow/config/subscribers.local.yml cargo run -p flow --features azure,aws
```
