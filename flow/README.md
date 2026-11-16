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
| 3 | subpubs read records, transform them and publish the result | `SubPubs`: Cursor-on-Target (`cot`), or another format through the optional [`xf` transform](#the-xf-transform); by default on `tcp://127.0.0.1:12001` |

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
    async fn finish(&self, within: Duration) -> u64 { 0 }
    fn lost(&self) -> u64 { 0 }
}
```

`prepare` runs once before the first record (create the bucket, open the
session). `finish` runs once when the run ends: a sink that queues, such as
`tcp`, sends what it holds within the time given and returns what is left,
and `lost` is what it accepted and could not send. `Subscribers::start`
returns a `Running`; `Running::drain` waits for every subscriber to catch up
with what the publishers sent and returns one `SinkReport` per sink. `flow::build_sink` in [`src/sink/mod.rs`](src/sink/mod.rs) is the one
function that turns a subscriber's configuration into a sink; a new sink is a
module in `src/sink/` and an arm there.

| Module | Sink | Cargo feature |
|---|---|---|
| `sink::azure_blob` | Azure Blob Storage, over its REST API | `azure` |
| `sink::aws_s3` | S3 and S3-compatible stores | `aws` |
| `sink::zenoh` | Zenoh put | `zenoh` |
| `goat::sink` | Zenoh put on a goat data fabric, with goat's identity, key and announcement conventions; optional and off by default ([docs/goat/joining.md](../docs/goat/joining.md)) | `goat` |
| `sink::rest` | HTTP POST | always built |
| `sink::network` | UDP datagrams and TCP streams, optionally inside TLS; Cursor-on-Target to a TAK server with a `cot:` mapping, or with an `xf:` block in a build with `goat-transformers` | always built |
| `sink::local_log` | local file: one JSON record per line, or a CBOR sequence | always built |
| `sink::message_rates` | per-topic counts | always built |

`flow` enables no feature by default. `mind` forwards the four features and
enables `azure` and `aws` by default. A fifth, `goat-transformers`, compiles
the [`xf` transform](#the-xf-transform); `mind` forwards it and leaves it off. Three more features, `pqc` (on by
default in `mind`), `quic` and `fips`, select transport security that has to
be compiled in; [`src/security/`](src/security) holds every sink to the
`security` block of the configuration
([`docs/security/transport.md`](../docs/security/transport.md)).

## The xf transform

Status: built, behind the cargo feature `goat-transformers`, off by default.
The numbers below are measured by `cargo test -p flow --features
goat-transformers`, which fails when one of them moves.

```sh
cargo build --release -p mind --features goat-transformers
```

The `cot` transform reads the decoded `message` of a record and writes one
format, Cursor-on-Target, from a mapping in the configuration. The `xf`
transform reads the record's payload (`data_encoded_inner`) and hands it to
the [goat-transformers](https://github.com/USAREUR-AF/goat-transformers)
engine, which converts between formats described by data files called packs:
it decodes the payload with the pack of the record's dictionary and interface,
and encodes what the payload holds with the pack of the target format. One
pack per format is enough for every pair, so a dictionary that has a pack
reaches every target that has one.

| Part | Where | From |
|---|---|---|
| the engine | crate `goat-transformers`, Rust standard library only | its public repository, at the commit `Cargo.toml` pins; `deny.toml` allows that one git source |
| packs of the targets `cot-xml` and `geojson`, and the shared keys they use | [`xf/packs/`](xf/packs), [`xf/model/core.def`](xf/model/core.def), [`xf/model/track.def`](xf/model/track.def) | copied unchanged from the same commit ([`xf/UPSTREAM`](xf/UPSTREAM)) |
| packs of the dictionaries in `dictionaries/` | [`xf/packs/bite-*`](xf/packs), `xf/model/` | generated by [`tools/bite2pack`](../tools/bite2pack/README.md) from the dictionaries and [`xf/bindings.json`](xf/bindings.json) |
| packs of your own dictionaries | a directory you name in `xf.packs` | generated by the same tool; nothing of them is compiled in |

The files of `xf/` are compiled into the binary
([`src/xf_embedded.rs`](src/xf_embedded.rs)). `tools/bite2pack/refresh.sh`
rewrites them; `make guards` and CI fail when they are not what the
dictionaries give.

### Packs for a dictionary

```sh
tools/bite2pack/bite2pack.py --out /etc/first-bite/packs --bindings bindings.json path/to/dictionary
```

A dictionary says where a field is; the bindings file says which fields are
the identifier and the position (`uid`, `lat`, `lon`, `hae`), which the `cot`
transform takes from its `cot:` mapping. [`tools/bite2pack/README.md`](../tools/bite2pack/README.md)
describes the file and how each kind of field is mapped.

### Switching a subpub from `cot` to `xf`

| `cot` | `xf` |
|---|---|
| `subtype: "cot"` | `subtype: "xf"` |
| `cot.uid`, `cot.position.geodetic` | in the bindings file the pack was generated with |
| `cot.type.value` | `xf.context.type` |
| `how`, always `m-g` | `xf.context.how` |
| `cot.stale_minutes` | `xf.stale_minutes` |
| a record without a field the mapping names is refused | `xf.require: [uid, lat, lon]` |
| (Cursor-on-Target only) | `xf.to`: `cot-xml`, `geojson`, or any other format with a pack |

[`config/subpubs.example.yml`](config/subpubs.example.yml) and
[`config/subpubs.xf.example.yml`](config/subpubs.xf.example.yml) are the same
subpub written both ways. Four things `cot` does have no counterpart yet:

| `cot` | With `xf` |
|---|---|
| `uid` joined from several fields | one field; a join needs a `derive` line in the model module, which the generator does not write |
| `type` looked up from a field (`type.field`, `values`, `default`) | a constant from `context`; a lookup needs a value table in the pack, which the generator does not write |
| `position.ecef` | not built: the engine has no conversion from Earth-centred coordinates |
| `form: flat` | not built |

### What `xf` writes, compared with `cot`

Measured on 502 Position records of the weather-station dictionary (the one in
`examples/weather-station.records.jsonl`, the dictionary's vector, and 500
generated payloads), through the two example files, with the same clock
([`tests/xf_cot.rs`](tests/xf_cot.rs)):

| Part of the event | `xf` against `cot` |
|---|---|
| `version`, `uid`, `type`, `how`; `point` with `ce`, `le` | equal on 502 of 502 |
| `lat`, `lon`, `hae` | equal on 473, 449 and 500 of 502. On the others the two numbers are neighbouring doubles (`46.06388434160976` and `46.063884341609764`): `cot` reads the number from the record's JSON text, which `serde_json` reads to within one unit in the last place, and `xf` reads the payload |
| `time`, `start`, `stale` | the same instant in different text on 502 of 502: `cot` writes `2026-03-04T12:00:00+00:00`, `xf` writes `2026-03-04T12:00:00.000Z` |
| `<detail/>` | `cot` writes the empty element; `xf` writes none |
| the published object | the same keys, `cot_datetime` and `cot_xml_string`; `cot_datetime` differs as `time` does |

`xf` writes the time to the millisecond; `cot` writes the clock's full
precision. Both forms are valid in a Cursor-on-Target event.

A message with no position (a Heartbeat) is refused by `cot`, and by `xf`
with `require: [uid, lat, lon]`. Without `require`, `xf` writes an event whose
`<point>` holds only the "not known" values of `hae`, `ce` and `le`.

### What the engine decodes, compared with `bite`

The transform does not use the record's `message`; the engine decodes the
payload again from the pack. [`tests/xf_packs.rs`](tests/xf_packs.rs) compares
the two decoders field by field on every fixture in the repository: the 27
dictionary vectors, the 5 records of the weather-station capture and the 10
packets of the NTP capture.

| Fields of 42 messages | 462 |
|---|---|
| the same value in the form `bite` writes it (a number, a label, `true` or `false`, text; a field beyond the end of a truncated payload absent in both) | 443 |
| the same bytes, in another form: a `bytes` field of two to eight bytes is hexadecimal text in the record and an integer in the engine (a one-byte field is the same two digits in both; the fixtures hold none) | 18 |
| a scaled value that differs in the last digits of the double (`outside_temperature` of the glasshouse Climate Report: `bite` gives -7.35, the engine -7.3500000000000085) | 1 |

Both decoders pick the same message type for all 42. One difference does not
show in the fixtures: `bite` passes a text byte outside 0x20 to 0x7E, and the
engine refuses the message that holds one.

## Standalone subscribers

The `flow` binary runs the configured subscribers against publishers that are
already running:

```sh
SUBSCRIBER_CONFIG_PATH=flow/config/subscribers.local.yml cargo run -p flow --features azure,aws
```
