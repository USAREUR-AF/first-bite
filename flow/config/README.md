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
| [`subscribers.example.yml`](subscribers.example.yml) | One entry per remote sink (REST, Azure Blob Storage, S3, Zenoh) with placeholder addresses and credentials. |
| [`subpubs.local.yml`](subpubs.local.yml) | Local development: weather-station `Position` records to Cursor-on-Target on port 12001. |
| [`subpubs.example.yml`](subpubs.example.yml) | The same transform with every key explained. |

Your own files hold addresses and credentials. Keep them outside the
repository, or name them `config/<anything>.yml`: `flow/.gitignore` ignores
every YAML file here except `*.local.yml` and `*.example.yml`.

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

A message on the socket is `<topic>|<record JSON>`.

The sockets are unauthenticated and unencrypted. Bound to `127.0.0.1`, as in
the built-in publisher, a socket is reachable only from the same machine,
which is where flow's own subscribers connect. Binding another address
(`tcp://*:12000`, an interface address) exposes every decoded record to that
network. The same holds for the publisher of a subpub.

## Subscribers

A subscriber connects to one publisher socket, keeps the records on its
topics, and hands them to one sink.

Keys every subscriber has:

| Key | Meaning |
|---|---|
| `name` | Used in log lines and errors. |
| `subscriber_type`, `subtype` | Which sink; see the table below. |
| `zmq_url`, `zmq_port` | Host and port of the publisher socket to read. |
| `topics` | Topics to keep. A topic must match exactly. `*` or `all` keeps everything. |

Sinks:

| `subscriber_type` | `subtype` | Sink | Cargo feature |
|---|---|---|---|
| `objectstorage` | `azureobject` | Azure Blob Storage | `azure` (default) |
| `objectstorage` | `awsobject` | S3 and S3-compatible stores | `aws` (default) |
| `rest` | any; not used | HTTP POST of the record as JSON | always built |
| `local` | unset or `locallog` | append to a local file, one record per line | always built |
| `stateful` | unset or `messagerates` | records per topic, written every ten seconds | always built |
| `zenoh` | unset | Zenoh put | `zenoh` |

The `rest` subtypes all behave the same; the name is a label.

A build without a feature refuses a subscriber that needs it. Features are
chosen when building `mind`:

```sh
cargo build --release -p mind                      # azure + aws
cargo build --release -p mind --features zenoh     # azure + aws + zenoh
cargo build --release -p mind --no-default-features --features zenoh
```

### `objectstorage` / `azureobject`

| Key | |
|---|---|
| `cloud_account`, `cloud_access_key` | required |
| `cloud_container` | required; created at startup when missing |
| `cloud_location_type` | required: `Public` (`https://<cloud_account>.blob.core.windows.net`), `Custom` or `Emulator` |
| `cloud_address`, `cloud_port` | required for `Emulator`: the service is `http://<cloud_address>:<cloud_port>/<cloud_account>` |
| `cloud_uri` | required for `Custom`: the service URL, e.g. `https://blob.storage.example:10000` |
| `cloud_api_version` | storage service version sent as `x-ms-version`; default `2023-11-03`. Must be `2015-02-21` or later. |

Requests are authorised with the account's Shared Key. Blobs are stored as
block blobs with content type `application/json`.

### `objectstorage` / `awsobject`

| Key | |
|---|---|
| `aws_access_key_id`, `aws_secret_access_key` | required |
| `aws_default_region` | required |
| `aws_s3_endpoint` | required, e.g. `https://s3.amazonaws.com` or `http://127.0.0.1:9090` |
| `aws_bucket_name` | required; created at startup when missing |

### Object names

Both object-storage sinks store each record twice, so that listings by time
and by topic are both prefix scans:

```
all-time-topic/<Y>/<m>/<d>/<H>/<M>/<S>/<topic>/<sha256 of the record>.json
all-topic-time/<topic>/<Y>/<m>/<d>/<H>/<M>/<S>/<sha256 of the record>.json
```

### `rest`

| Key | |
|---|---|
| `endpoint_url` | required: a host or address (sent over `http://`), or a URL starting `http://` or `https://` |
| `endpoint_port` | required |
| `endpoint_path` | optional, e.g. `/records` |

A server error or a failed connection is retried three times, after 2, 4 and
8 seconds.

### `local`

| Key | |
|---|---|
| `local_logfile` | required: file to append to |

### `stateful`

| Key | |
|---|---|
| `local_logfile` | optional: file the counts go to; default `messagerates.log` in the working directory |

### `zenoh`

| Key | |
|---|---|
| `zenoh_config` | optional: path of a standard Zenoh JSON5 configuration file. Endpoints, mode and TLS identity are set there. Without it the session uses Zenoh's defaults. |
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

The payload is the record JSON with encoding `application/json`. Each
subscriber opens one session.

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
