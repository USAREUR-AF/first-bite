# What a goat data fabric requires of a publisher

goat support in first-bite is optional and off by default: it is the cargo
feature `goat`, and nothing here applies to a build without it.

A goat network ("goatnet") carries data on [Zenoh](https://zenoh.io) 1.9.0.
Its routers are the stock `eclipse/zenoh` image; a publisher joins by speaking
standard Zenoh over mutual TLS, presenting a certificate the goatnet's
certificate authority issued, and writing only under the key prefix that
certificate is granted. goat states that there is "no goat profile of Zenoh":
a service that runs against stock `zenohd` can join.

This page lists each requirement, the goat file that states it, whether goat
enforces it (**mandatory**) or recommends it (**conventional**), and what
first-bite does about it. [joining.md](joining.md) is the operator's
procedure. Paths are relative to the public goat repositories
[`goatcore`](https://github.com/USAREUR-AF/goatcore),
[`goatstuff`](https://github.com/USAREUR-AF/goatstuff) and
[`goatenv`](https://github.com/USAREUR-AF/goatenv).

## Terms

| Term | Meaning in goat | Stated in |
|---|---|---|
| goatnet | One goat network: a NetBird/WireGuard mesh, its certificate authority, and one or more Zenoh routers reachable only over the mesh. | `goatcore/docs/design/zenoh-data-fabric.md` §"Deployment topology + ACL contract" |
| identity model | How a goatnet's routers map a certificate to write access. Two exist: `per-uuid-slot-pool` and per-org (the older trial model). The goatnet's administrator says which one applies. | `goatcore/ops/ansible/roles/goat-zenoh/templates/config.json5.j2` (`zenoh_identity_model`); `goatenv/ops/ansible/group_vars/zagros_hosts/vars.yml` |
| slot | A subject, a write rule and a policy written into the router's access-control list before anyone holds the certificate. Onboarding hands out a certificate whose common name is a free slot's identifier, so no router restart is needed. | `goatcore/docs/adr/1025-zenoh-dynamic-tenant-authz-slot-pool.md` §1 |
| UUID, anchor | The slot's identifier: 32 lower-case hexadecimal digits, `sha256("<salt>:<index>")` truncated. It is at once the certificate common name, the access-control subject, and the first chunk of every key the holder may write (`<uuid>/**`). | ADR 1025 §1, §2; `goatcore/docs/design/slot-alias-display-layer.md` §1, §2 |
| alias | A short human name for a slot (`alpha-operator`). Display only: it never appears in a key, a certificate, a rule or a stored record. | `slot-alias-display-layer.md` §2, §3 |
| org prefix | On a per-org goatnet, the registered name whose certificate common name is `<prefix>-trial-peer` and whose write grant is `<prefix>/**`. | `goatcore/docs/design/zenoh-trial-fabric.md` §"Namespace convention" |
| peer name | The name of a machine on the NetBird mesh, `peer-<handle>[-<org_short_tag>]-<site>-<date>[-<hostname>]`. It links a mesh peer to its owner. It is not the certificate common name and is not presented to Zenoh. | `goatcore/ops/identity/peer-name-grammar.yaml`; `goatcore/docs/design/peer-identity-resolution.md` |

## Requirements

| # | Requirement | Stated in | Mandatory or conventional | first-bite |
|---|---|---|---|---|
| 1 | **Reach the router over the mesh.** The router listens on the goatnet's mesh address only; a host that is not a mesh peer cannot open a TCP connection to it. | `zenoh-data-fabric.md` §"Deployment topology + ACL contract" | Mandatory (network) | Not first-bite's job. The host must be enrolled by the goatnet's own tooling; first-bite reports a connection failure. |
| 2 | **Zenoh `=1.9.0`.** Every Rust service that links Zenoh pins the exact version. | `goatcore/docs/design/zenoh-version-pinning.md` | Mandatory for goat's own crates; for an outside publisher, the version the routers are tested against | Built: workspace `Cargo.toml` pins `zenoh = "=1.9.0"`. |
| 3 | **Client mode, TLS endpoint.** Publishers connect as `mode: client` to `tls/<host>:7447`; routers federate among themselves. | `zenoh-data-fabric.md` §"Per-entity mode selection"; `goatstuff/ops/zenoh-shims/rest-pub-bridge/src/main.rs` (`open_session`) | Mandatory in practice: goat's configuration drops publishes from peer-mode sessions | Built: generated configuration sets `mode: "client"` and the configured endpoints; a non-`tls/` endpoint is refused. |
| 4 | **Mutual TLS.** The client presents its leaf certificate (`connect_certificate`, `connect_private_key`, `enable_mtls: true`) and trusts the goatnet root (`root_ca_certificate`). The TLS block is set as one object. | `rest-pub-bridge/src/main.rs`; `goatcore/ops/goat-zenoh-portal/internal/certmint/certmint.go` (`renderFirstPublisher`); `goatcore/docs/operations/zenoh-trial-onboarding.md` §"mTLS gotchas" | Mandatory: the router requires a client certificate that chains to its root | Built: `mind goat config` prints the block; paths are references to files, never key material. |
| 5 | **Host-name verification.** The onboarding bundle's publisher connects by a DNS name in the router certificate and keeps `verify_name_on_connect: true`. A sidecar that reaches the router by a name the certificate does not carry sets it to `false`. | `certmint.go` (`renderFirstPublisher`); `rest-pub-bridge/src/main.rs` | Conventional (on by default) | Built: `goatnet.verify_name_on_connect`, default `true`. |
| 6 | **Certificate issued by the goatnet's authority.** The goatnet's portal (or its `cert-sign` tool) mints the leaf; the publisher never generates its own identity. Leaves are valid one year with extended key usage `clientAuth`. | `certmint.go` package comment and `MintOptions`; `zenoh-data-fabric.md` §"What the fabric does not replace" | Mandatory | Built: `mind goat check` verifies the leaf against the root, its validity window and its common name before any connection. |
| 7 | **Common name is the identity.** Zenoh 1.9.0 access control matches `cert_common_names` only. On a slot-pool goatnet the common name is the 32-hex UUID; on a per-org goatnet it is `<prefix>-trial-peer` (the suffix is a per-goatnet setting). A URI SAN is minted but not consulted. | `zenoh-trial-fabric.md` §"Trust model"; ADR 1025 §1; `certmint.go` (`CNSuffix`, `MintOptions.CommonName`); `config.json5.j2` subjects `slot-<uuid>`, `vendor-<prefix>` | Mandatory | Built: configuration names the slot or the org prefix; a certificate whose common name differs is refused with both values shown. |
| 8 | **Write only under the granted prefix.** A slot may `put` and `delete` on `<uuid>/**`, `volatile/<uuid>/**` and `attest/schema/**`; an org on `<prefix>/**`, `volatile/<prefix>/**` and `attest/schema/**`. Everything else is denied. | `config.json5.j2` rules `slot-w-<uuid>`, `trial-peer-w-<vendor>`, `trial-peer-w-attest-schema` and their policies | Mandatory (router ACL, default deny) | Built: every key first-bite writes starts with the prefix or with `attest/schema/<prefix>/`. |
| 9 | **goat's key taxonomy nests below the prefix.** On a slot-pool goatnet the reserved top-level classes (`data/`, `services/`, `health/`, `telemetry/`, …) keep their shape one level down: `<uuid>/<class>/…`. | ADR 1025 §2 ("nests verbatim below the anchor"); `slot-alias-display-layer.md` §1 (`<uuid>/<class>/<site>/<id>/…`) | Conventional below the prefix: "subtree structure below the first segment is the publisher's choice" | Built: see "Keys first-bite writes". |
| 10 | **Wire-faithful data goes under `telemetry/<format>/<site>/<id>`.** `telemetry/` is for formats that are not goat's canonical schemas; it is format-shaped, and canonical data lives under `data/`. `<site>` is where the source is. | `zenoh-data-fabric.md` §"Top-level key-expression prefixes", §"Data-class structure under `data/`", §"Schema neutrality" | Conventional | Built: `<prefix>/telemetry/<format>/<site>/<interface>/<message>`. |
| 11 | **Quality-of-service class follows the prefix.** `telemetry/` is the situational class. The class comes from the authenticated key, so a producer cannot raise its own traffic by setting a priority. | `zenoh-data-fabric.md` §"QoS class is bound to the prefix"; `goatcore/docs/design/zenoh-qos-and-prioritization.md` §5 (designed, not built in goat) | Conventional | Built: puts use Zenoh's default priority and congestion control; first-bite sets neither. |
| 12 | **A small, fixed set of keys.** What varies per message goes in the payload, not the key; each tenant has a bounded allowance of distinct keys. | `goatcore/docs/design/pubsub-best-practices.md` §1b | Mandatory in effect (measured per tenant) | Built: one key per interface and message of the dictionary, plus one announcement and one health key. Packets that do not decode, and message types the dictionary does not define, are not published on the fabric. |
| 13 | **Version tail.** A trailing `v<N>` chunk is encouraged so a changed message shape gets a new key. | `zenoh-trial-fabric.md` §"Namespace convention"; `goatcore/docs/design/data-dictionary.md` §"Schema evolution" | Conventional | Built: `publish.version: <N>` appends `/v<N>` to record and schema keys; unset by default. |
| 14 | **Declare a schema for every data topic.** A JSON Schema (draft 2020-12) at `attest/schema/<topic-pattern>`, with `unit` on numeric fields. Enforcement is in audit mode: publishes without a declaration are counted, not rejected. | `data-dictionary.md` §"The two-state model", §"Rollout curve"; `zenoh-trial-fabric.md` §"Reserved prefixes" | Conventional now; designed to become mandatory (enforce mode) | Built: one declaration per record key at `attest/schema/<record key>`, from `bite::jsonschema`, describing the record as published: typed values for a version 2 record, text for version 1. |
| 15 | **Announce the service.** A shim publishes at startup to `services/shims/<vendor-class>/<instance-id>/announce` with class, version, instance, site, capabilities, `translates_from`, `translates_to` and `config_digest`. | `goatcore/docs/adr/0209-shim-pattern.md` convention 2; `zenoh-data-fabric.md` prefix table | Conventional | Built: `<prefix>/services/shims/<format>/<instance>/announce`, JSON, at startup. |
| 16 | **Publish health.** A shim heartbeats every 30 s on `health/shims/<vendor>/<instance>`. | ADR 0209 convention 3; `zenoh-data-fabric.md` ("`health/<class>/<instance>`") | Conventional | Built: `<prefix>/health/shims/<format>/<instance>`, JSON, every `publish.health_interval_seconds` (default 30). |
| 17 | **Pin the session identifier to the UUID.** A publisher whose common name is a slot UUID sets the Zenoh session `id` to it (leading zeros removed), so the transport identity, the access-control subject and the key prefix are one value. Read-only sessions keep a random identifier: one transport per identifier is allowed. | `slot-alias-display-layer.md` §9 item 4b(i), §10 | Conventional (display and provenance; not authorization) | Built: on by default for a slot identity (`identity.pin_session_id`); `mind goat watch` and the delivery probe's reader never pin. |
| 18 | **Never subscribe to the whole keyspace.** `**` and spellings made only of wildcards are a grant held by named infrastructure certificates. | `zenoh-data-fabric.md` §"No one subscription may match the whole keyspace" | Mandatory (`**` is refused); all-wildcard spellings are detected and alerted on | Built: the sink subscribes to nothing. `mind goat watch` refuses a key expression with no literal chunk. |
| 19 | **Do not answer for keys that are not yours.** A queryable on `**` makes the process the responder of record for the fabric. | `pubsub-best-practices.md` §3; `zenoh-data-fabric.md` ("edge-queryable lockdown") | Mandatory where the lockdown is on | Built: first-bite declares no queryable. |
| 20 | **A denied write is silent.** The router drops it; `put` returns success. The publisher-side check is to read the sample back. | `zenoh-data-fabric.md` §"Access control"; `goatcore/docs/adr/1022-zenoh-denial-observability.md` | Property of the fabric | Built: `mind goat probe` and the sink at startup put one health sample and wait for it on a second session; a sample that does not arrive is reported with the common name and the grant to ask for. |
| 21 | **Time.** The fabric does not serve time; TLS checks the certificate against the host's clock. | `zenoh-trial-onboarding.md` §1 | Mandatory (TLS) | Built: `mind goat check` reports a certificate that is expired or not yet valid by the host clock, with both times. |

## What a publisher is handed

The goatnet's portal produces one onboarding archive per identity
(`goat-onboarding-<prefix>-<stamp>.tar.gz`). Its layout is fixed by
`goatcore/ops/goat-zenoh-portal/internal/certmint/certmint.go`
(`assembleTarball`):

| File | Content |
|---|---|
| `<prefix>-cert.pem` | the leaf certificate; `<prefix>` is the slot UUID or the org prefix |
| `<prefix>-key.pem` | its private key, mode 0400 |
| `ca-root.pem` (the name is a per-goatnet setting) | the goatnet's root certificate |
| `NOTES.md` | the prefix, the URI SAN and a `Router endpoint:` line |
| `first-publisher.py`, `first-subscriber.py` | Python examples with the same TLS block |
| `<prefix>-goat-cli.cbor`, `<prefix>-goat-cli-ca.pem` | optional: a signed profile for goat's own CLI |
| an onboarding packet (`.md`) | the goatnet's instructions |

`mind goat init --bundle <dir>` reads the unpacked archive and writes the
first-bite configuration. It reads the three PEM files and `NOTES.md`; it does
not read the `.cbor` profile.

## Keys first-bite writes

`<prefix>` is the slot UUID or the org prefix. `<format>`, `<site>` and
`<instance>` come from the configuration; `<interface>` and `<message>` from
the dictionary, lower-cased with every character outside `a-z 0-9 _ -`
replaced by `_`, as the generic Zenoh sink does.

| Key | Payload | When |
|---|---|---|
| `<prefix>/telemetry/<format>/<site>/<interface>/<message>[/v<N>]` | the record, unchanged: the JSON of `datamodels/src/packets.rs` ([record.md](../record.md)), encoding `application/json` | each decoded packet |
| `attest/schema/<prefix>/telemetry/<format>/<site>/<interface>/<message>[/v<N>]` | JSON Schema of that record | startup |
| `<prefix>/services/shims/<format>/<instance>/announce` | JSON: the ADR 0209 fields | startup |
| `<prefix>/health/shims/<format>/<instance>` | JSON: status, counts, times | startup, then every 30 s |

The record carries no goat field. Identity travels as goat defines it: in the
certificate, in the first key chunk, and in the pinned session identifier.

## Not defined in the public repositories

Each item below is something first-bite would need a definition for and the
public goat repositories do not give one. Where first-bite still does
something, the choice is first-bite's and is marked so.

| Item | What is missing | first-bite |
|---|---|---|
| Announcement payload | ADR 0209 lists the field names and refers to `docs/design/shim-pattern.md` §2 for the rest; that file is not in the public repositories. No encoding and no schema are given. | JSON with exactly the named fields; `capabilities` is `["publish"]`; `config_digest` is the SHA-256 of the configuration file and the dictionary's name and version. First-bite's choice. |
| Health payload | The key and the 30 s period are given; the body is not. | A small JSON object (status, start time, counters). First-bite's choice. |
| Where shim keys sit on a slot-pool goatnet | ADR 0209 predates the slot pool and writes `services/shims/…` and `health/shims/…` at the top level, which a slot's write rule denies. ADR 1025 says the taxonomy nests below the UUID but gives no shim example. | Nested below the prefix. Inferred from ADR 1025 §2. |
| Schema extensions | `data-dictionary.md` requires `unit` on numeric fields and offers `range`, `entity_ref`, `freshness_target_seconds`; the meta-schema URL in its example is a placeholder. | Declarations carry `unit` (and keep `x-unit`, `x-bits`); the standard draft 2020-12 `$schema`. No `entity_ref`. |
| Binary declarations | `attest/schema-declaration/<topic-pattern>` is for formats with no machine-readable schema. | Not used: every published topic has a JSON Schema. |
| Encodings other than JSON | A declaration is a JSON Schema; no document says how a CBOR payload is declared. | Records on a goat fabric are JSON. `encoding: cbor` on a `goat` subscriber is refused at startup. |
| Per-sample attachment | goat's own producers attach 5 bytes (`ntp_class`, `producer_epoch`) written by an `ntp-health-agent`; a producer without the agent is the `unattested-publisher` role. The agent and its enrolment are not described for an outside publisher. | Not built. Samples carry no attachment. |
| Source info | goat's own plain publishers attach Zenoh `SourceInfo`; it is behind Zenoh's `unstable` feature. | Not built: first-bite does not enable `unstable`. |
| Peer name for a service | The grammar covers people (`peer-<handle>-…`) and portal-enrolled machines (`machine-<handle>[-<hostname>]`); the machine form has no single-source grammar file. Nothing ties a mesh peer name to a Zenoh session. | `identity.mesh_peer_name` is optional and only checked against the `peer-` form; it is not transmitted. |
| The `.cbor` profile | Its layout is defined by goat's CLI, not documented as an interface. | Not read. |
| Mesh enrolment | Done with NetBird and goat's tooling. | Not built, by design. |
| `volatile/` streams | A never-persisted class for high-rate data exists (`zenoh-volatile-class.md`). | Not built: records go under `telemetry/` only. |
