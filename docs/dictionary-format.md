# Dictionary format

A dictionary tells `bite` how to decode the messages of one or more
interfaces. It is a directory of JSON files; nothing about a message set is
compiled into the decoder. `bite check <dir>` validates a dictionary and runs
its test vectors.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="img/frame-to-record-dark.svg">
  <img src="img/frame-to-record.svg" width="100%" alt="The 23 bytes of a weather-station Observation message, 0200174b583700fb2e18bcda9b4236000004030201beef, with the bit range of each of its nine fields marked. A table gives the dictionary entry for each field (start bit, stop bit, type, options) and the value it decodes to: message_type Observation, message_length 23, station_id KX7, temperature -12.34, pressure 101325, wind_direction 271.5, humidity 45.5, sample_serial 16909060, checksum beef.">
</picture>

Three dictionaries ship in [`dictionaries/`](../dictionaries):

| Dictionary | Shows |
|---|---|
| `weather-station` | UDP messages identified by a payload field; signed, scaled, float, boolean, text, little-endian and non-byte-aligned fields |
| `vehicle-can` | CAN frames identified by the frame identifier; little-endian signals in `lsb0` numbering |
| `toot` | the minimal form: integer fields with value labels |

## Layout

```text
<dictionary>/
  dictionary.json                 optional
  <INTERFACE>/
    metadata.json                 required; makes the directory an interface
    messages/
      <Message Name>.json         one file per message; the file stem is the name
  vectors/
    <any-name>.json               optional test vectors
```

An interface is a set of messages that share a transport and one way of
telling the messages apart: one ICD, one CAN bus, one serial link.

## `dictionary.json`

| Key | Type | Meaning |
|---|---|---|
| `name` | string | dictionary name |
| `version` | string | dictionary version |
| `description` | string | free text |

## `metadata.json`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `message_type` | object | required | how a message is identified, see below |
| `port` | integer | none | UDP or TCP port. A packet whose source or destination port equals it belongs to this interface; the source port is tried first. Two interfaces may not claim one port. |
| `bit_order` | `"msb0"` or `"lsb0"` | `"msb0"` | how bit positions are numbered, see [Bit numbering](#bit-numbering) |
| `byte_order` | `"big"` or `"little"` | `"big"` | byte order of byte-aligned fields under `msb0` |
| `message_length` | `{start, stop}` | none | where a message states its own length |
| `description` (or `mission`) | string | none | free text |
| `icd` | `{version, date}` | none | the document the interface was transcribed from |
| `protocol` | `{network, transport}` | none | informational |

### `message_type`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `source` | `"payload"` or `"frame_id"` | `"payload"` | `payload`: the identifier is bits `[start, stop)` of the payload. `frame_id`: it is the identifier that arrives with the frame, such as a CAN identifier. |
| `start`, `stop` | integer | `0` | bit range, for `payload`; 1 to 64 bits |
| `mask` | string | none | for `frame_id`: ANDed with the identifier before lookup |
| `values` | object | `{}` | identifier to message name |

An identifier with no entry in `values` decodes to a record whose message type
is the identifier in decimal and which has no fields. A name in `values` with no
file in `messages/` is identified but not decoded; `bite check` warns about it.

## `messages/<Message Name>.json`

| Key | Type | Meaning |
|---|---|---|
| `description` | string | free text |
| `fields` | object | field name to field definition |

### Field

| Key | Type | Default | Meaning |
|---|---|---|---|
| `start`, `stop` | integer | required | bit range `[start, stop)` |
| `type` | string | `"uint"` | `uint`, `int`, `float`, `bool`, `bytes`, `ascii` |
| `signed` | boolean | `false` | `true` is shorthand for `"type": "int"` |
| `byte_order` | `"big"` or `"little"` | interface's | overrides the interface byte order |
| `scale`, `offset` | number | `1`, `0` | value is `raw * scale + offset`, as a float |
| `values` | object | none | raw integer to label |
| `unit` | string | none | informational |
| `description` | string | none | informational |

| Type | Width | Decodes to |
|---|---|---|
| `uint` | 1 to 64 bits, any alignment | unsigned integer |
| `int` | 1 to 64 bits, any alignment | two's-complement integer |
| `float` | 32 or 64 bits | IEEE 754 float |
| `bool` | 1 to 64 bits | `false` when all bits are zero |
| `bytes` | whole bytes, byte-aligned, any length | hex string |
| `ascii` | whole bytes, byte-aligned, any length | text with trailing NUL bytes and spaces removed |

A field with no `type` that is byte-aligned and wider than 64 bits is `bytes`.

Keys in `values` (fields and `message_type`) are integers written as strings,
in decimal or `0x` hexadecimal; field value keys may be negative. Labels are
looked up on the raw integer, before `scale` and `offset`.

A field that reaches past the end of the payload decodes to `null`. The other
fields of the message still decode.

## Bit numbering

`msb0` is the numbering protocol documents use: bit 0 is the most significant
bit of byte 0, and a field is read as a big-endian bit string. A field of bits
`[12, 20)` is the low four bits of byte 1 followed by the high four bits of
byte 2. With `"byte_order": "little"` a byte-aligned field has its bytes
reversed; a little-endian field must be byte-aligned.

`lsb0` is the numbering of little-endian ("Intel") signals on CAN: bit 0 is the
least significant bit of byte 0, and the payload is read as one little-endian
integer. A field of bits `[12, 20)` is the high four bits of byte 1 (its low
half) and the low four bits of byte 2 (its high half).

## Test vectors

Each file in `vectors/` is a payload and what it must decode to:

```json
{
    "interface": "WX",
    "payload_hex": "01000c4b58370000015180600000",
    "expect": {
        "message_type": "Heartbeat",
        "fields": {
            "station_id": "KX7",
            "uptime": "86400",
            "mode": "Calibrating"
        }
    }
}
```

| Key | Meaning |
|---|---|
| `interface` | interface name |
| `payload_hex` | payload bytes; whitespace ignored |
| `frame_id` | frame identifier, for `frame_id` interfaces |
| `expect.message_type` | expected message name |
| `expect.fields` | field name to expected value as text: the label where the field has one, the value otherwise, `null` for a field beyond the payload. Fields not listed are not checked. |

## What `bite check` reports

| Severity | Finding | Effect on loading |
|---|---|---|
| error | file is not valid JSON or lacks a required key | file left out |
| error | field range empty or inverted; integer wider than 64 bits; float not 32 or 64 bits; `bytes`/`ascii` not byte-aligned; little-endian field not byte-aligned under `msb0` | field left out |
| error | `message_type` range not 1 to 64 bits; a `values` key or `mask` that is not an integer | value left out |
| error | two interfaces claim one port | the first, by name, keeps it |
| warning | fields overlap | both kept |
| warning | message file that no `message_type` value names | kept, never selected |
| warning | `message_type` value with no message file | identified, not decoded |
| warning | label that is not text, or a label on a `float`, `bytes` or `ascii` field | label ignored |

`bite check` exits non-zero on any error or failed vector; `--strict` also
fails on warnings.
