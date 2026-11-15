# first-bite, Open Billie and octo-pod

[Open Billie](https://github.com/USAREUR-AF/open-billie),
[octo-pod](https://github.com/USAREUR-AF/octo-pod) and first-bite all stand
between equipment that speaks its own binary format and a
[Zenoh](https://zenoh.io) data fabric. Each does a different part of the job.
first-bite shows what is on a wire: every message, field for field, as the
source defined it. Open Billie turns one sensor's tracks into standard track
formats. octo-pod carries a feed across a site boundary unchanged, under an
identity, a release gate and an audit record. This page says where they
overlap, what each publishes, and what to configure when they run together.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="img/ways-onto-the-fabric-dark.svg">
  <img src="img/ways-onto-the-fabric.svg" width="100%" alt="A sensor on the left and one Zenoh data fabric, such as a goatnet, on the right, with four routes between them. A native Zenoh publisher: the equipment publishes on the fabric itself, when it can speak Zenoh. Open Billie: an endpoint the sensor sends to, publishing its tracks as Cursor-on-Target, ASTERIX CAT062 or SAPIENT, when you want normalised tracks. octo-pod: a boundary appliance the sensor sends to, carrying bytes unchanged under a certificate, a release gate and an audit record, when a feed must leave a site under those controls. first-bite: listens on a tap or mirror port and publishes every field of every message, when you want everything two systems say without touching either.">
</picture>

Statements about the other two name the file in their repository that they
come from: Open Billie as of platform version 0.3.0, octo-pod as of 0.6.0.

## What each one is

**first-bite** listens passively. It reads frames from a tap, a mirror port or
a capture file, decodes every message it has a dictionary for, and publishes
one JSON record per packet. It does not open a connection to the sensor and
the sensor does not know it is there. The message set is a directory of JSON
files, so a new message set needs no code.

**Open Billie** is an endpoint. A sensor sends to a TCP or UDP port on the
node; a sensor-specific package, called a kid, decodes the bytes and maps them
to a canonical track record; a shared translator encodes that record as
Cursor-on-Target, ASTERIX CAT062 or SAPIENT and publishes it
(`README.md`, `docs/architecture/KID_PACKAGE_CONTRACT.md`). It also prepares
the node: the local Zenoh router, the mutual-TLS session to the fabric, and
the lifecycle of each kid.

**octo-pod** is the boundary appliance of a site. A sensor sends to a UDP or
TCP port on the pod, or the pod dials the sensor; an ingress arm publishes
each datagram or frame, unchanged, on the pod's own Zenoh router under a key
that names the arm, the feed and the source address. The pod records what
arrived, and forwards a feed to the fabric only after an operator releases it
(`README.md`, `docs/engineering-principles.md` "System context"). It does not
decode: "Byte arms capture without interpreting sensor payloads"
(`docs/adr/0001-edge-appliance-and-translation-boundary.md`).

| | first-bite | Open Billie | octo-pod |
|---|---|---|---|
| Where the bytes come from | frames captured on an interface, or a capture file | a TCP or UDP listener the sensor sends to (`schemas/kid-manifest.schema.json`, `interfaces.inputs`) | a UDP or TCP listener, a TCP connection the pod dials, or a Zenoh publisher holding a certificate the pod issued (`README.md` "Ingress") |
| Does it read the payload | yes: every field, from a dictionary | yes: the kid's decoder and mapping | no: payloads are carried as `application/octet-stream` (`arms/common/src/ingest.rs`); only the SAPIENT session arm parses an envelope |
| What describes a message set | a dictionary: JSON files giving the bit range, type and labels of each field ([dictionary-format.md](dictionary-format.md)) | a kid: decoder code, byte-offset JSON schemas, mapping files and lookups, built into a container image (`kids/_template/`) | nothing; an arm is declared by port, source address, feed label and TCP framing (`docs/operations.md` "Adding a publisher") |
| What is published | every decoded message, with the source's own field names | tracks only, as Cursor-on-Target XML, ASTERIX CAT062 or SAPIENT protobuf (`runtime/rust/translator/src/dispatch.rs`) | the bytes that arrived |
| Record between decode and output | the published record itself ([architecture.md](architecture.md)) | Canonical Track v2, sent to the translator over a Unix socket (`schemas/CANONICAL_TRACK_V2.md`) | none |
| Key on the fabric | `telemetry/<system>/<location>/<interface>/<message>`, one key per message type; the template is configuration | `<prefix>/billie/<kid>/<output>/<format>`, one key per output of a kid (`docs/architecture/GOLDEN_PATH_ZENOH.md`) | on the pod `raw/<id>/<feed>/<source address>`; on the fabric the same key under the pod's slot anchor (`arms/common/src/ingest.rs`, `arms/uplink-bridge/src/main.rs`) |
| Identity of a source | deployment labels copied into each record (`DATA_SOURCE_*`); the packet's addresses and ports | the kid, authenticated by its process on the translator socket; `sourceId` in the track | a certificate per arm or peer from the pod's own authority, and a source-address allowlist (`docs/engineering-principles.md` "A certificate per publisher") |
| Release and audit | none: every record goes to every configured sink | none beyond the fabric's own access rules | nothing leaves until `octo-pod release`; one audit subscriber per publisher on the pod (`docs/operations.md` "Publishing upstream", `platform/templates/pen/octo-pen-audit-sink.container.j2`) |
| Fabric session and credentials | a Zenoh configuration file named by the subscriber's `zenoh_config` | owned by the platform: `billie zenoh configure` installs the certificate files and writes the client configuration (`docs/architecture/GOLDEN_PATH_ZENOH.md`) | owned by the pod: a slot bundle issued by the fabric's operators, checked by `octo-pod release` (`docs/operations.md`) |
| Unit of deployment | one static binary, a dictionary directory and a systemd unit ([deployment.md](deployment.md)) | a Debian package for the platform; one container image bundle per kid (`docs/architecture/PLATFORM_CONTRACT.md`) | a Debian package and a bundle of digest-pinned container images, one container per arm (`docs/release-pattern.md`) |
| Runs on | Linux, macOS; the decoder also on Windows and in a browser | Ubuntu Server 24.04, `amd64` and `arm64` (`README.md`) | Ubuntu 24.04, `amd64` and `arm64` (`README.md` "Requirements") |
| Zenoh version | crate pinned `=1.9.0` | router image pinned by digest; release not stated | crate pinned `=1.9.0` (`arms/Cargo.toml`) |
| Licence | Apache-2.0 | Apache-2.0 | Apache-2.0 |

## Which to use

| You want | Use |
|---|---|
| to see or record everything two systems say to each other, without touching either | first-bite |
| status, configuration and command messages as well as tracks | first-bite |
| a sensor's tracks in Cursor-on-Target, ASTERIX CAT062 or SAPIENT, with one identity scheme across sensors | Open Billie |
| a node whose fabric connection, certificates and services are installed and checked for you | Open Billie |
| both the full message stream and normalised tracks from one sensor | first-bite and Open Billie, on the same node or on two |
| a feed carried off a site unchanged, attributed to its source, released only by decision, with a local record | octo-pod |
| the decoded messages of a link carried off a site under those same controls | first-bite publishing into an octo-pod |

## first-bite beside Open Billie

The two do not contend for the sensor. Open Billie's kid binds the port the
sensor sends to. first-bite captures the same datagrams from the interface
without binding anything, so it can run beside a kid and see what the kid
receives.

Their keys do not collide. A first-bite record is put on a key starting
`telemetry/`; Open Billie's translator publishes on keys starting `billie/`
and its health record on `ops/<node-handle>/health`
(`docs/architecture/GOLDEN_PATH_ZENOH.md`,
`docs/architecture/PLATFORM_CONTRACT.md`).

### Cursor-on-Target: produce it in one place

Both can make Cursor-on-Target events from the same track message, and the
events differ, so a display fed by both shows two markers for one track.
On a node that runs Open Billie for a sensor, leave first-bite's transform for
that sensor's track message off. It is off unless `mind` is started with
`--activate-subpubs` and a subpub names the message.

| Attribute | first-bite ([`cotify`](../cotify/README.md)) | Open Billie (`runtime/rust/translator/src/cot.rs`, `request.rs`) |
|---|---|---|
| `uid` | the values of the fields listed under `uid`, joined with `_` | `<kid>/<sourceId>/<trackId>/<trackGeneration>`, each part percent-escaped |
| `type` | a constant, or a lookup from one field's label, both set in the mapping | from the canonical `classification`: for example `a-u-A-M-F-Q` for `uas`, `a-u-X` for `unknown` |
| `time`, `start` | when the event was built | the track's `observedAt` |
| `stale` | `stale_minutes` after `time`; default 60 minutes | 180 seconds after `observedAt` by default |
| `how` | `m-g` | `m-p` by default |
| `<point>` | `lat`, `lon`, `hae` from the mapping; `ce` and `le` `9999999` | `lat`, `lon`, `hae`; `ce` and `le` from the track's position uncertainty, else `9999999` |
| `<detail>` | empty | `<track course speed>` when velocity is present; `<remarks>` with status tokens |
| simulated or test data | not marked | `opex="s"` |
| Where it goes | a ZeroMQ PUB socket, as `{"cot_datetime", "cot_xml_string"}` | the fabric, as `application/xml` |

A first-bite mapping can be brought close to Open Billie's timing with
`stale_minutes: 3`. The identifier and the type cannot be made identical by
configuration: first-bite has no kid name or track generation to put in the
`uid`.

### Reaching the fabric through the node's own session

Status: designed, not measured.

An Open Billie node already holds the fabric credentials. Its local Zenoh
router listens on `127.0.0.1:7447`
(`deploy/platform/templates/zenohd.container.j2`, `billie_local_peer_port` in
`deploy/platform/configure.yml`), and a one-way bridge forwards every key put
on that router to the fabric under the node's authorised prefix
(`deploy/bin/billie-island-bridge.py`, `fabric_key`). A first-bite Zenoh sink
that connects to the local router therefore needs no certificate of its own,
and its records arrive on the fabric as
`<prefix>/telemetry/<system>/<location>/<interface>/<message>`.

```json5
// /etc/first-bite/zenoh.json5
{
  mode: "client",
  connect: { endpoints: ["tcp/127.0.0.1:7447"] },
  scouting: { multicast: { enabled: false } },
}
```

```yaml
subscribers:
  - name: island
    subscriber_type: "zenoh"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics: ["WX/Observation"]      # name the messages wanted, not "*"
    zenoh_config: /etc/first-bite/zenoh.json5
```

Three things to know before relying on it:

- Open Billie documents the local router as the place its kids publish. A
  publisher that is not a kid is not part of its contract.
- The bridge makes one HTTP request per sample and the local router keeps the
  last value of every key in memory (`deploy/platform/templates/zenohd.json5.j2`).
  A first-bite record carries the whole frame in base64, so forward the message
  types that are needed, not every packet on the link.
- first-bite pins the Zenoh crate at `=1.9.0`. Open Billie pins its router
  image by digest and does not state the release, so compatibility of the two
  is to be confirmed on the node.

A first-bite on a machine that is not an Open Billie node joins the fabric
with its own certificate, as [goat-integration.md](goat-integration.md)
describes.

## first-bite beside octo-pod

A sensor sends to one endpoint. An octo-pod arm binds that port, as an Open
Billie kid does, so those two take a sensor in turn, not together. first-bite
binds nothing and can observe the same traffic beside either.

octo-pod makes no Cursor-on-Target and reads no fields, so nothing first-bite
publishes duplicates what a pod publishes. The two differ in what the fabric
receives: from an arm, the bytes of each datagram; from first-bite, a JSON
record with the fields named.

### Publishing into a pod: the tap as a source the pod governs

Status: designed, not measured.

Some equipment cannot be told to send to a pod: the feed runs between two
systems that will not be reconfigured. first-bite can tap that link and
publish into the pod as a Zenoh peer. The pod then gives the decoded stream
the same treatment as any feed: its own certificate, one namespace, an audit
subscriber, and no release to the fabric until an operator opens it
(`docs/guide-zenoh-partners.md` "Let a Partner Publish to You", "Release Your
Own Data Up to the Fabric").

On the pod:

```sh
sudo octo-pod add-peer tap1 --from 192.0.2.40
sudo octo-pod apply
sudo octo-pod peer bundle tap1
```

On the first-bite machine, the Zenoh configuration is the one in the bundle's
own instructions (`platform/templates/peer/bundle-README.md.j2`): client mode,
the pod's endpoint on port 7447, the three files of the bundle, multicast
scouting off. The subscriber sets the key, because a peer may write only
`raw/<id>/**`:

```yaml
subscribers:
  - name: pod
    subscriber_type: "zenoh"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics: ["*"]
    zenoh_config: /etc/first-bite/pod-tap1.json5
    key_template: "raw/tap1/{interface}/{message}"
```

After `octo-pod release tap1 …` the records are on the fabric as
`<slot anchor>/raw/tap1/<interface>/<message>`. Both projects pin the Zenoh
crate at `=1.9.0`.

What the pod's controls cover in this arrangement: which machine may publish
(certificate and `--from`), where (the `raw/tap1/**` namespace), whether it
leaves the site, and a record that it did. What they do not cover: the pod's
source-address allowlist applies to the first-bite machine, not to the
systems on the tapped link. Their addresses and ports are inside each record
(`src_ip`, `dst_ip`, `src_port`, `dst_port`), as first-bite observed them.

### Decoding what a pod captured

Status: designed, not built.

octo-pod captures first and leaves decoding to a separate component: "A vendor
who cannot tell you their message format can still be captured today and
decoded later" (`docs/engineering-principles.md` "Capture-first"), and
decoding belongs to a translation gateway outside the pod
(`docs/adr/0001-edge-appliance-and-translation-boundary.md`). For a
fixed-layout binary feed, a dictionary is that later decoding: each sample an
arm publishes is one datagram or frame payload, which is what `bite decode`
takes.

What first-bite lacks to do this as a service is a source that reads payloads
from a Zenoh subscription. `mind` today reads Ethernet frames and finds the
interface by port; a payload from a pod arrives with no frame, so the
dictionary interface would be chosen by configuration, per subscribed key.
The decoder would run where the released feed is readable, publish under a
key that keeps the pod's `<id>/<feed>/<source address>` suffix, and so keep
the pod's attribution on the decoded record. By hand, one captured payload
decodes today with `bite decode <dictionary> --port <port> <hex>`.

## A dictionary as the decoder of a kid

Status: designed, not built.

A kid may be written in Rust and must supply a decoder
(`schemas/kid-manifest.schema.json`, `decoderImplementations`). The
[`bite`](../bite) library is a decoder with no network code: given a
dictionary and a payload it returns the typed fields. A kid whose decoder is
`bite` with a dictionary would need only the mapping from field names to
Canonical Track v2, and one dictionary would then serve the tap and the kid.

Open Billie's own description of a message layout is a byte-offset JSON schema
(`kids/examples/oetf/packages/oetf/shared/schemas/_meta.schema.json`), which
`billie mapper generate --format billie-json` reads
(`docs/architecture/LOCAL_KID_BUILD_AND_MAPPER_GENERATION.md`). It and a
first-bite dictionary describe the same thing:

| | first-bite dictionary | Open Billie byte-offset schema |
|---|---|---|
| Field position | `start` and `stop` in bits | `offset` in bytes and a `type` that fixes the width; `bits` for sub-fields |
| Value labels | `values` | `values` |
| Units | `unit` | `units` |
| Scaling | `scale`, `offset` | `scale` |
| Repeated blocks | not expressed | fields per index, valid when a count field allows (`validIf`) |
| Bit order and non-IP frames | `lsb0` numbering, frame identifiers | not expressed |
| Test vectors | `vectors/`, run by `bite check` | `conformance/vectors/` |

| Piece | Status |
|---|---|
| first-bite beside Open Billie or octo-pod without contending for the sensor or for keys | designed; follows from each one's code, not measured together |
| complete CoT 2.0 events from first-bite (`form: event`) | built |
| first-bite records to the fabric through an Open Billie node's local router | designed, not measured |
| a kid that decodes with `bite` and a dictionary | designed, not built |
| first-bite publishing into an octo-pod as a peer, under the pod's certificate, release gate and audit | designed, not measured |
| a first-bite source that decodes payloads captured by an octo-pod | designed, not built |
| conversion between a dictionary and a byte-offset schema | not built |
