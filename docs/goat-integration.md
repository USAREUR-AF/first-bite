# Publishing to a goat data fabric

A goat network carries data on [Zenoh](https://zenoh.io). first-bite joins it
as a shim: a service that keeps a non-Zenoh protocol on one side (the binary
messages it captures) and publishes on the fabric on the other. This page says
where its records go on the fabric and what a goatnet operator configures.

The conventions referred to are in goatcore's
[`docs/design/zenoh-data-fabric.md`](https://github.com/USAREUR-AF/goatcore/blob/main/docs/design/zenoh-data-fabric.md).

## What is published where

| Data | Key expression | goat prefix class | Status |
|---|---|---|---|
| one decoded record per packet, as JSON | `telemetry/<system>/<location>/<interface>/<message>` | `telemetry/`: wire-faithful surfaces for non-canonical formats; situational QoS | built |
| Cursor-on-Target events, when the transform is on | `telemetry/cot/<location>/<uid>` | same | designed, not built: the transform publishes on ZeroMQ only, and the key template has no placeholder for the event's `uid` |

`<system>`, `<location>` come from `DATA_SOURCE_SYSTEM_TYPE` and
`DATA_SOURCE_GEO_LOCATION`; `<interface>` and `<message>` from the dictionary.
Each part is lower-cased and every character outside `a-z 0-9 _ -` becomes `_`,
so a value cannot add a level or a wildcard to the key. The template is the
`key_template` of the subscriber; the default is the first row above.

Records are the fabric's `telemetry/` class, not `data/`: they are faithful to
the source message set, not to a canonical goat schema. Mapping a message set
onto a canonical `data/` schema is a separate subscriber on the fabric.

## Configuration

Build with the `zenoh` feature (`cargo build --release -p mind --features
zenoh`); the Zenoh crate is pinned to the version goat pins, `=1.9.0`. Then add
a subscriber:

```yaml
subscribers:
  - name: fabric
    subscriber_type: "zenoh"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics: ["*"]
    zenoh_config: /etc/first-bite/zenoh.json5
    # key_template: "telemetry/{system}/{location}/{interface}/{message}"
```

`zenoh_config` is a standard Zenoh configuration file. Everything about joining
the goatnet lives there and nowhere else: the router endpoints, client mode,
and the TLS identity. A goat router admits a publisher by the certificate it
presents, so the file names the leaf certificate and key the goatnet's offline
CA issued for this peer, and the router's access-control list must grant that
certificate's subject `put` on the key prefix above.

## Declaring the schema

goat asks every publisher of a data-carrying topic to declare what it
publishes. `bite schema <dictionary>` prints a JSON Schema (draft 2020-12) per
message, with bounded integers, labels as enumerations, and units and bit
ranges as `x-unit` and `x-bits`. That schema describes the typed form of a
record's `message`; the published form renders every value as a string.

| Piece | Status |
|---|---|
| Zenoh sink with templated, sanitised keys | built |
| JSON Schema per message from a dictionary | built |
| publishing the typed form so the schema validates the record as sent | designed, not built |
| publishing the schemas at `attest/schema/<topic-pattern>` and the announcement at `services/shims/<format>/<instance>/announce` | designed, not built |

## Beside Open Billie and octo-pod

[Open Billie](https://github.com/USAREUR-AF/open-billie) publishes normalised
tracks (Cursor-on-Target, ASTERIX CAT062, SAPIENT) from a sensor node onto the
same kind of fabric. [octo-pod](https://github.com/USAREUR-AF/octo-pod)
carries feeds from equipment that cannot join the fabric, unchanged, under a
release gate and an audit record. [related-projects.md](related-projects.md)
compares the three, gives the keys each publishes on, and says what to
configure when first-bite runs with either, including publishing into a pod
so that no fabric certificate is held by first-bite.

## Without a native session

A deployment that cannot hold a Zenoh session in `mind` can use goat's
`rest-pub-bridge` sidecar, which accepts `PUT /<key expression>` and republishes
with the bridge's mutual-TLS identity. first-bite's REST sink posts every record
to one configured path, so through the bridge all records land on one key; the
native sink is the one that gives a key per interface and message.
