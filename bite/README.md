# bite

The decoder: a library that loads a dictionary and turns a payload into named,
typed fields, and the `bite` command that checks dictionaries and decodes
payloads by hand. It has no network code; [`mind`](../mind) supplies the
payloads. The dictionary format is in
[docs/dictionary-format.md](../docs/dictionary-format.md).

## The `bite` command

Run it as `cargo run -p bite -- <command>`, or `bite <command>` once installed.

`check <dir> [--strict]` validates a dictionary and runs the vectors in its
`vectors/` directory. It exits non-zero on an error or a failed vector, and
with `--strict` on a warning too.

```console
$ bite check dictionaries/weather-station
1 interfaces, 2 messages, 0 errors, 0 warnings, 4/4 vectors passed
```

`list <dir>` prints each interface with its port, and each message with its
identifier, field count and length.

```console
$ bite list dictionaries/weather-station
WX (port 40100)
       1  Heartbeat  (8 fields, 96 bits)
       2  Observation  (9 fields, 184 bits)
```

`decode <dir> (--interface <name> | --port <port>) [--frame-id <id>] <hex>`
decodes one payload and prints the typed fields.

```console
$ bite decode dictionaries/weather-station --port 40100 0200174b583700fb2e18bcda9b4236000004030201beef
{
  "fields": {
    "checksum": "beef",
    "humidity": 45.5,
    "message_length": 23,
    "message_type": "Observation",
    "pressure": 101325,
    "sample_serial": 16909060,
    "station_id": "KX7",
    "temperature": -12.34,
    "wind_direction": 271.5
  },
  "interface": "WX",
  "message_type": "Observation"
}
```

`schema <dir>` prints one JSON Schema (draft 2020-12) per message, keyed
`<interface>/<message>` (`WX/Heartbeat`, `WX/Observation`); each property
carries its bit range as `x-bits` and its unit as `x-unit`.

## Code

| File | What |
|---|---|
| [`src/dictionary.rs`](src/dictionary.rs) | loading, validation and indexing of a dictionary, from a directory (`Dictionary::load`) or from files in memory (`Dictionary::from_files`) |
| [`src/schema.rs`](src/schema.rs) | the types a dictionary file deserialises into |
| [`src/bits.rs`](src/bits.rs), [`src/decode.rs`](src/decode.rs) | bit extraction; payload to fields |
| [`src/vectors.rs`](src/vectors.rs), [`src/jsonschema.rs`](src/jsonschema.rs) | running test vectors; JSON Schema export |
| [`src/main.rs`](src/main.rs) | the command |
| [`tests/`](tests) | loading and decoding tests against [`dictionaries/`](../dictionaries) |
