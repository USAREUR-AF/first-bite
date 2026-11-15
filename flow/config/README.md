# flow configuration

flow reads three YAML files. Each is found the same way: the path in its
environment variable, else the named file in `config/` beside the executable.

| File | Environment variable | Beside the executable | With no file |
|---|---|---|---|
| publishers | `PUBLISHER_CONFIG_PATH` | `config/publishers.yml` | the built-in publisher, [`publishers.local.yml`](publishers.local.yml): one PUB socket on `tcp://127.0.0.1:12000` |
| subscribers | `SUBSCRIBER_CONFIG_PATH` | `config/subscribers.yml` | no subscribers |
| subpubs | `SUBPUB_CONFIG_PATH` | `config/subpubs.yml` | no subpubs |

Subpubs also need `mind --activate-subpubs`.

Every entry is checked before anything runs. An unknown type, a sink that is
not compiled in, or a missing key stops startup with an error naming the entry.

## Files in this directory

| File | Use |
|---|---|
| [`publishers.local.yml`](publishers.local.yml) | The built-in publisher. Compiled into the binary. |
| [`subscribers.local.yml`](subscribers.local.yml) | Local development against the emulators in [`mocks/`](../../mocks): Azurite, an S3 mock, a message-rate counter and a local log file. |
| [`subscribers.example.yml`](subscribers.example.yml) | One entry per remote sink (REST, Azure Blob Storage, S3, Zenoh) with placeholder addresses and credentials written as references. |
| [`subscribers.tak.example.yml`](subscribers.tak.example.yml) | Cursor-on-Target events to a TAK server over TLS, and to TAK clients on multicast. |
| [`goat.example.yml`](goat.example.yml) | Optional: the settings for joining a goat data fabric, read only by a build with the `goat` feature. |
| [`subpubs.local.yml`](subpubs.local.yml) | Local development: weather-station `Position` records to Cursor-on-Target on port 12001. |
| [`subpubs.example.yml`](subpubs.example.yml) | The same transform with every key explained. |
| [`security.example.yml`](security.example.yml) | The security block with every key explained. |

Your own files hold addresses. Keep them outside the repository, or name
them `config/<anything>.yml`: `flow/.gitignore` ignores every YAML file here
except `*.local.yml` and `*.example.yml`.

## Credentials and references

A configuration file holds no credential value. It holds a reference, and the
value reaches the process when the file is loaded.

| Form | Example | The value comes from |
|---|---|---|
| reference | `cloud_access_key: "${FIRST_BITE_BLOB_ACCESS_KEY}"` | the environment variable named between `${` and `}` |
| file | `cloud_access_key_file: "blob-access-key"` | the text of the file, without the line break at its end; at most 64 KiB |
| plain value | `cloud_access_key: "…"` | the configuration file. Works, and logs one warning at startup naming the entry and the key |

Rules, each checked before anything runs:

- A reference is the whole value: `${NAME}`, where `NAME` is letters, digits
  and `_` and does not start with a digit. Nothing is substituted inside a
  longer value; a value that contains `${` and is not one whole reference
  stops startup.
- A reference to a variable that is unset or empty stops startup with an
  error naming the entry, the key and the variable:
  ``subscriber AllToS3: `aws_secret_access_key` refers to the environment variable FIRST_BITE_S3_SECRET_ACCESS_KEY, which is not set``.
- References work in these keys of a subscriber (also the subscriber of a
  subpub): `zmq_url`, `zmq_port`, every `endpoint_*`, `cloud_*` and `aws_*`
  key, `connection_string`, `local_logfile`, `zenoh_config`, `key_template`;
  and in the `address` of a publisher.
- The credential keys are `cloud_access_key`, `aws_access_key_id`,
  `aws_secret_access_key` and `connection_string`. The first three have a
  `_file` variant. A key and its `_file` variant together stop startup.
- A relative `_file` path is looked up in `$CREDENTIALS_DIRECTORY` when that
  variable is set, which is where systemd puts the files of
  `LoadCredential=`; otherwise in the working directory.

A credential is never written to the log or to an error message: a
configuration printed for debugging shows `***` in its place, an error about
a credential names the key only, and the S3 sink removes both keys from
whatever the store answers before reporting it. The REST sink names its
endpoint in log lines without user, password, query or fragment.

Set the variables where the service reads its environment:
`/etc/first-bite/first-bite.env`, mode 0640 `root:firstbite`
([docs/deployment.md](../../docs/deployment.md)).

## Security

One `security` block states how records may leave the host; every publisher,
subscriber and subpub is held to it before anything connects, and an entry
that cannot meet it stops startup with an error naming the entry and the
requirement. [`docs/security/transport.md`](../../docs/security/transport.md)
explains the levels and how to choose one.

The block is the top-level `security:` key of the subscriber file, or of a
file of its own found like the other three: `SECURITY_CONFIG_PATH`, else
`config/security.yml` beside the executable. Written in both, startup is
refused. With no block nothing is required of any path.

```yaml
security:
  transport: tls                      # the level; the only required key
  min_tls: 1.3
  key_exchange: hybrid-pqc-preferred
  fips: "off"
  quic: allowed
  client_cert: /etc/first-bite/tls/client.pem
  client_key: /etc/first-bite/tls/client.key
  ca_bundle: /etc/first-bite/tls/ca.pem
  allow_plaintext_bind: false
```

| Key | Values | Default | Meaning |
|---|---|---|---|
| `transport` | `unenforced`, `loopback-plaintext`, `tls`, `mtls` | required | `loopback-plaintext`: no sink may leave the host. `tls`: every network sink uses TLS and authenticates the server. `mtls`: and presents a client certificate. |
| `min_tls` | `1.2`, `1.3` | `1.3` | lowest TLS version a sink negotiates |
| `key_exchange` | `classical`, `hybrid-pqc-preferred`, `hybrid-pqc-required` | `hybrid-pqc-preferred` in a binary with the cargo feature `pqc`, else `classical` | whether X25519MLKEM768 is offered, and whether anything else is |
| `fips` | `off`, `required` | `off` | `required` needs a binary built with the cargo feature `fips` |
| `quic` | `allowed`, `required` | `allowed` | `required`: every Zenoh endpoint is `quic/`; needs the cargo feature `quic` |
| `client_cert`, `client_key` | paths of PEM files | none | client certificate chain and key of the REST, Azure and S3 sinks |
| `ca_bundle` | path of a PEM file | none | authorities those sinks trust, in place of their usual roots |
| `allow_plaintext_bind` | `true`, `false` | `false` | permit a ZeroMQ socket to bind an address that is not loopback; startup logs a warning naming it |

A subscriber may carry its own `security:` key with any of `transport`,
`min_tls`, `key_exchange`, `fips`, `quic`, `client_cert`, `client_key` and
`ca_bundle`. A value that differs from the block needs `reason`:

```yaml
    security:
      transport: loopback-plaintext
      reason: "local emulator on the development machine"
```

`mind security check` reports what startup would refuse; `mind security show`
prints what every entry will use.

## Publishers

```yaml
publishers:
  - name: records            # used in log lines and key file names
    publisher_type: PUB      # PUB or PUSH
    address: "tcp://127.0.0.1:12000" # address the socket binds
    topic: "subsystem/message_type"
```

`topic` says how the topic of each record is formed:

| `topic` | Topic a record is published on |
|---|---|
| `*` or `catchall` | `catchall` |
| `location` | the record's `data_source_geo_location`, or `unknown_location` |
| `message_type` | the record's `message_type`, or `unknown_message_type` |
| `subsystem/message_type` | `<subsystem>/<message_type>`, e.g. `WX/Observation`; `subsystem` is the dictionary interface the packet matched |

A message on the socket is `<topic>|<record>`. The record is JSON text, or
one CBOR item when the publisher has `encoding: cbor`
([docs/record.md](../../docs/record.md#encodings)):

```yaml
publishers:
  - name: records
    publisher_type: PUB
    address: "tcp://127.0.0.1:12000"
    topic: "subsystem/message_type"
    encoding: cbor           # json (default) or cbor
```

A subscriber reads a publisher of either encoding without being told which.

The sockets are unauthenticated and unencrypted. Bound to `127.0.0.1`, as in
the built-in publisher, a socket is reachable only from the same machine,
which is where flow's own subscribers connect. Binding another address
(`tcp://*:12000`, an interface address) exposes every decoded record to that
network. The same holds for the publisher of a subpub. A publisher bound to
anything but a loopback address, `localhost`, `ipc://` or `inproc://` logs a
warning at startup that says so; under a `security` block it is refused
unless `allow_plaintext_bind: true` is set. Consumers on other hosts are
served by the Zenoh and HTTPS sinks.

## Subscribers

A subscriber connects to one publisher socket, keeps the records on its
topics, and hands them to one sink.

Keys every subscriber has:

| Key | Meaning |
|---|---|
| `name` | Used in log lines and errors. |
| `subscriber_type`, `subtype` | Which sink; see the table below. |
| `encoding` | How the sink writes each record: `json` (default) or `cbor`. See [Encoding](#encoding). |
| `zmq_url`, `zmq_port` | Host and port of the publisher socket to read. |
| `topics` | Topics to keep. A topic must match exactly. `*` or `all` keeps everything. |

A subscriber accepts messages of up to 16 MiB; a record is well under 1 MiB.
A publisher that sends a larger message is disconnected, and that subscriber
forwards nothing more until the process restarts.

Sinks:

| `subscriber_type` | `subtype` | Sink | Cargo feature |
|---|---|---|---|
| `objectstorage` | `azureobject` | Azure Blob Storage | `azure` (default) |
| `objectstorage` | `awsobject` | S3 and S3-compatible stores | `aws` (default) |
| `rest` | any; not used | HTTP POST of the record as JSON | always built |
| `local` | unset or `locallog` | append to a local file, one record per line | always built |
| `stateful` | unset or `messagerates` | records per topic, written every ten seconds | always built |
| `udp` | unset | one datagram per record to each destination, unicast or multicast; with a `cot:` block, a Cursor-on-Target event | always built |
| `tcp` | unset | a stream to each destination, optionally inside TLS; with a `cot:` block, Cursor-on-Target events for a TAK server | always built |
| `zenoh` | unset | Zenoh put | `zenoh` |
| `goat` | unset | Zenoh put on a goat data fabric; optional, off by default. Its settings are in the file `GOAT_CONFIG_PATH` names ([`goat.example.yml`](goat.example.yml), [docs/goat/joining.md](../../docs/goat/joining.md)) | `goat` |

The `rest` subtypes all behave the same; the name is a label.

A build without a feature refuses a subscriber that needs it. Features are
chosen when building `mind`:

```sh
cargo build --release -p mind                      # azure + aws
cargo build --release -p mind --features zenoh     # azure + aws + zenoh
cargo build --release -p mind --no-default-features --features zenoh
cargo build --release -p mind --features zenoh,quic   # Zenoh over QUIC as well
```

The features `pqc` (default), `quic` and `fips` belong to the security block;
see [`docs/security/transport.md`](../../docs/security/transport.md).

### `objectstorage` / `azureobject`

| Key | |
|---|---|
| `cloud_account` | required |
| `cloud_access_key` or `cloud_access_key_file` | required: the account key, base64; see [Credentials and references](#credentials-and-references) |
| `cloud_container` | required; created at startup when missing |
| `cloud_location_type` | required: `Public` (`https://<cloud_account>.blob.core.windows.net`), `Custom` or `Emulator` |
| `cloud_address`, `cloud_port` | required for `Emulator`: the service is `http://<cloud_address>:<cloud_port>/<cloud_account>` |
| `cloud_uri` | required for `Custom`: the service URL, e.g. `https://blob.storage.example:10000` |
| `cloud_api_version` | storage service version sent as `x-ms-version`; default `2023-11-03`. Must be `2015-02-21` or later. |

Requests are authorised with the account's Shared Key. Blobs are stored as
block blobs with the content type of the sink's encoding.

### `objectstorage` / `awsobject`

| Key | |
|---|---|
| `aws_access_key_id` or `aws_access_key_id_file` | required; see [Credentials and references](#credentials-and-references) |
| `aws_secret_access_key` or `aws_secret_access_key_file` | required |
| `aws_default_region` | required |
| `aws_s3_endpoint` | required, e.g. `https://s3.amazonaws.com` or `http://127.0.0.1:9090` |
| `aws_bucket_name` | required; created at startup when missing |

### Object names

Both object-storage sinks store each record twice, so that listings by time
and by topic are both prefix scans:

```
all-time-topic/<Y>/<m>/<d>/<H>/<M>/<S>/<topic>/<sha256 of the stored bytes>.json
all-topic-time/<topic>/<Y>/<m>/<d>/<H>/<M>/<S>/<sha256 of the stored bytes>.json
```

With `encoding: cbor` the names end in `.cbor`.

The topic is used as it arrived, with these changes so that it cannot name
an object outside its prefix: empty segments are dropped, a segment `.`
becomes `_` and `..` becomes `__`, control characters and `\` become `_`,
and segments past 512 bytes are cut.

The Azure sink does not follow an HTTP redirect.

### `rest`

| Key | |
|---|---|
| `endpoint_url` | required: a host or address (sent over `http://`), or a URL starting `http://` or `https://` |
| `endpoint_port` | required |
| `endpoint_path` | optional, e.g. `/records` |
| `security.client_cert`, `security.client_key` | optional: client certificate and key (PEM files) for this endpoint, in place of the security block's |
| `security.ca_bundle` | optional: authorities (PEM file) this endpoint's certificate is checked against, in place of the public web roots |

A server error or a failed connection is retried three times, after 2, 4 and
8 seconds. A redirect is not followed: it is reported as an error, and the
record is not posted to the address it names. With an `https://` URL the
server's certificate is verified; there is no setting that turns
verification off.

### `local`

| Key | |
|---|---|
| `local_logfile` | required: file to append to; created with mode 0640 |

### `stateful`

| Key | |
|---|---|
| `local_logfile` | optional: file the counts go to; default `messagerates.log` in the working directory |

### `udp` and `tcp`

Each record on the subscriber's topics is sent to every destination listed.
Nothing is sent anywhere else: `destinations` is required and has no default.

What is sent:

| Subscriber has | Payload | On `udp` | On `tcp` |
|---|---|---|---|
| a `cot:` block | the Cursor-on-Target event built from the record ([`cotify/README.md`](../../cotify/README.md)), as XML | one event per datagram | events one after another, as a TAK server's streaming input reads them |
| no `cot:` block | the record, in the subscriber's `encoding` | one record per datagram | JSON: one record per line. CBOR: one item after another |

| Key | |
|---|---|
| `destinations` | required: a list, each entry an `address` and options |
| `destinations[].address` | `host:port`; an IPv6 address is written `[address]:port` |
| `destinations[].source_port` | optional: the local port to send from. A TAK server can tell inputs apart by it. Unset: the system chooses |
| `destinations[].ttl` | `udp` to a multicast group: routers a datagram may cross. Default 1, the local network |
| `destinations[].interface` | `udp` to a multicast group: the interface to send from, as its IPv4 address (its index for an IPv6 group). Unset: the system's choice |
| `cot` | optional: the mapping from fields to a Cursor-on-Target event |
| `tls` | `tcp` only, optional: run the stream inside TLS |
| `tls.ca_file` | PEM file of the authorities the server's certificate is checked against. No other authority is trusted. Left out: `ca_bundle` of the security block; one of the two is required |
| `tls.client_cert_file`, `tls.client_key_file` | optional, together: PEM certificate and key presented to the server. Left out: `client_cert` and `client_key` of the security block, when it has them |
| `tls.server_name` | optional: the name the server's certificate must carry. Default: the destination's host |
| `queue` | `tcp` only: payloads held per destination while its connection is down. Default 1000 |

A `udp` subscriber sends and keeps nothing. A `tcp` subscriber keeps one
connection per destination. While a connection is down its payloads wait in
the queue, the connection is retried after 1 s, 2 s, 4 s and so on up to 30 s,
and a payload that finds the queue full is dropped and counted. Every minute
in which a count changed, one log line per destination reports it:

```text
subscriber PositionsToTakServer: tcp tak.example.org:8089: 1204 sent, 0 dropped with the queue full, 0 failed, 1 reconnections
```

A record the mapping cannot use (a message without the mapped fields, a
packet that did not decode) is reported and not sent; select the messages
with `topics`.

`udp`, and `tcp` without `tls`, are not encrypted. Under a `security` block
at level `tls` or `mtls` they are refused for a destination on another host
unless the subscriber carries `security: { transport: unenforced, reason: … }`
([docs/security/transport.md](../../docs/security/transport.md)). [`subscribers.tak.example.yml`](subscribers.tak.example.yml)
has both forms, and [docs/cot-to-tak.md](../../docs/cot-to-tak.md) is the
worked example.

### `zenoh`

| Key | |
|---|---|
| `zenoh_config` | optional: path of a standard Zenoh JSON5 configuration file. Endpoints, mode and TLS identity are set there. Without it the session uses Zenoh's defaults: `peer` mode, a plaintext listener on every interface and multicast scouting, which every level but `unenforced` refuses. Under a `security` block the file's endpoints (`tls/`, `quic/`, `tcp/`), scouting and client certificate are checked against the level. |
| `key_template` | optional: the key each record is put on. Default `telemetry/{system}/{location}/{interface}/{message}`. |

Placeholders in `key_template`:

| Placeholder | Record field |
|---|---|
| `{interface}` | `subsystem` |
| `{message}` | `message_type` |
| `{system}` | `data_source_system_type` (environment variable `DATA_SOURCE_SYSTEM_TYPE`) |
| `{location}` | `data_source_geo_location` (`DATA_SOURCE_GEO_LOCATION`) |
| `{network}` | `data_source_network_name` (`DATA_SOURCE_NETWORK_NAME`) |

A value is lower-cased and every character outside `a-z`, `0-9`, `_` and `-`
becomes `_`; a value the record does not have is written `unknown`. So
`Weather Station` at `Hilltop-7` sending `WX` / `Observation` is put on
`telemetry/weather_station/hilltop-7/wx/observation`. The fixed text of a
template may not contain `*`, `$`, `#` or `?`.

The payload is the record as JSON with Zenoh encoding `application/json`, or
with `encoding: cbor` as CBOR with Zenoh encoding `application/cbor`. Each
subscriber opens one session.

## Encoding

`encoding` on a subscriber says how its sink writes each record;
[docs/record.md](../../docs/record.md#encodings) describes the two encodings.

| Sink | `json` (default) | `cbor` |
|---|---|---|
| `objectstorage` | object `….json`, content type `application/json` | object `….cbor`, content type `application/cbor` |
| `rest` | body JSON, `Content-Type: application/json` | body CBOR, `Content-Type: application/cbor` |
| `local` | one record per line | a CBOR sequence (RFC 8742): one item after another |
| `udp`, `tcp` without `cot` | the record as JSON; on `tcp`, one per line | the record as one CBOR item |
| `udp`, `tcp` with `cot` | refused at startup: the event is XML | refused at startup |
| `zenoh` | Zenoh encoding `application/json` | Zenoh encoding `application/cbor` |
| `goat` | JSON | refused at startup: the schema declared for each record is a JSON Schema |
| `stateful` | counts only | refused at startup: no record is stored |

Any other value is refused at startup with the two choices.

## Subpubs

A subpub connects to a publisher socket like a subscriber, transforms each
record on its topics, and publishes the result on a socket of its own. The
output topic is the input topic followed by `/<topic_suffix>`. The publisher
must bind a different port from the one the subscriber reads.

The one transform is Cursor-on-Target: `subscriber_type: "transformer"`,
`subtype: "cot"`, and a `cot:` block naming the fields of your messages.

```yaml
subpubs:
  - name: Position2CoT
    subscriber:
      name: "positions"
      subscriber_type: "transformer"
      subtype: "cot"
      zmq_url: "127.0.0.1"
      zmq_port: 12000
      topics:
        - "WX/Position"
      cot:
        uid: [station_id]                 # field values joined with "_"
        type:
          value: "a-f-G"                  # or: { field: ..., values: {...}, default: ... }
        position:
          geodetic: [latitude, longitude, altitude]   # or: ecef: [x, y, z]
        stale_minutes: 60
        # form: flat                      # the XML of earlier releases; default is event
    publisher:
      name: "cot"
      publisher_type: PUB
      address: "tcp://127.0.0.1:12001"
      topic: "subsystem/message_type"
      topic_suffix: "CoT"
```

The `cot:` keys are described in [`cotify/README.md`](../../cotify/README.md).
Each output message is a JSON object with `cot_datetime` and `cot_xml_string`.
