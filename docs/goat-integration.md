# Publishing to a goat data fabric

goat support is optional and off by default. first-bite is a general tool: the
default build, the capture path, the decoder and the record format do not
depend on goat. A `mind` built with the cargo feature `goat` can also join a
goat network ("goatnet") and publish decoded records on its Zenoh fabric the
way goat expects.

| Page | For |
|---|---|
| [goat/joining.md](goat/joining.md) | an operator with a goatnet and an onboarding archive: build, configure, check, start, confirm, troubleshoot |
| [goat/contract.md](goat/contract.md) | what goat requires of a publisher, the goat file that states each requirement, and what the public goat repositories leave undefined |

## What exists

| Piece | Where | Status |
|---|---|---|
| Generic Zenoh sink with templated, sanitised keys (any fabric) | `subscriber_type: zenoh`, feature `zenoh` | built |
| JSON Schema per message from a dictionary | `bite schema`, `bite::jsonschema` | built |
| goat configuration file and `mind goat init` from an onboarding archive | `flow/src/goat/config.rs`, `bundle.rs` | built |
| Offline checks of configuration, key, certificate name, dates and root | `mind goat check`, `flow/src/goat/cert.rs` | built |
| Zenoh client configuration generated for a goat router (client mode, mutual TLS, session identifier pinned to the slot UUID) | `mind goat config`, `flow/src/goat/session.rs` | built |
| Records on `<prefix>/telemetry/<format>/<site>/<interface>/<message>` | `subscriber_type: goat`, `flow/src/goat/sink.rs` | built |
| Schema declaration per record key at `attest/schema/<record key>` | same | built |
| Service announcement and 30 s heartbeat under `<prefix>/services/shims/…` and `<prefix>/health/shims/…` | same | built |
| Delivery probe for the router's silent denial | `mind goat probe`, and the sink at startup | built |
| Reading the fabric back | `mind goat watch` | built |
| The whole path against a stock `eclipse/zenoh:1.9.0` router configured as goat configures one | `tools/goat-selftest.sh` | measured |
| A run against a real goatnet | | not built |
| Per-sample time-quality attachment and Zenoh source info | | not built |
| Publishing under the never-persisted `volatile/` class | | not built |
| Mapping a message set onto goat's canonical `data/` schemas | a separate consumer on the fabric | not built |

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
`goat` sink is the one that gives a key per interface and message.
