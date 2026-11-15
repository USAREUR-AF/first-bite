# cotify

Turns decoded records into Cursor-on-Target (CoT) events, the format TAK and
many other tactical displays read.

The crate knows no message format. A mapping, supplied as configuration, names
the fields of a decoded message that give the event its identifier, its type
and its position:

```yaml
uid: [station_id]                 # field values joined with "_"
type:                             # CoT type; either a constant…
  value: "a-f-G"
# …or looked up from a field:
# type: { field: affiliation, values: { "Hostile": "a-h-A" }, default: "a-u-A" }
position:
  geodetic: [latitude, longitude, altitude]   # degrees, degrees, metres HAE
  # or: ecef: [x, y, z]                        # metres, WGS 84 Earth-centred
stale_minutes: 60                 # optional, default 60
```

| Key | Meaning |
|---|---|
| `uid` | One or more field names. Their values, joined with `_`, become the event `uid`. |
| `type.value` | The CoT type for every event. |
| `type.field`, `type.values`, `type.default` | The CoT type is `values[<value of field>]`, or `default` when the value is not listed. |
| `position.geodetic` | Three field names: latitude and longitude in degrees, altitude in metres above the WGS 84 ellipsoid. |
| `position.ecef` | Three field names: WGS 84 Earth-centred, Earth-fixed x, y, z in metres. Converted to latitude, longitude and height. |
| `stale_minutes` | Minutes after creation at which the event goes stale. |

Field values may be decimal strings (the form `mind` publishes) or JSON
numbers. A field that is missing, null or not a number where a number is needed
is an error that names the field.

## Use

```rust
let mapping: cotify::Mapping = serde_norway::from_str(yaml)?;
let cot_json = cotify::record_to_cot(&mapping, record_json)?;
```

`record_json` is one published record; its `message` object holds the decoded
fields. The result is a JSON object:

```json
{"cot_datetime": "2026-03-04T12:00:00+00:00", "cot_xml_string": "<event version=\"2.0\" uid=\"KX7\" type=\"a-f-G\" …>"}
```

`flow` applies a mapping to a ZeroMQ topic through a subpub with
`subtype: "cot"`; see [`flow/config/subpubs.example.yml`](../flow/config/subpubs.example.yml).
