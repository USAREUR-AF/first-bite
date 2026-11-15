# The record

`mind` publishes one record per captured UDP or TCP packet. This page is the
contract: the fields, their types, the two record versions, and the encodings
a record can leave in. The type is `PacketInfoWithParsedMessage` in
[`datamodels/src/packets.rs`](../datamodels/src/packets.rs).

## A record

A weather-station heartbeat from
[`examples/weather-station.pcap`](../examples/weather-station.pcap), as
version 2 JSON (line breaks added):

```json
{"timestamp":{"secs_since_epoch":1750000000,"nanos_since_epoch":0},
 "src_port":40100,"dst_port":50000,
 "src_ip":[192,0,2,10],"dst_ip":[192,0,2,20],
 "data_encoded_inner":"AQAMS1g3AAABUYBg",
 "subsystem":"WX","message_type":"Heartbeat",
 "message":{"battery_low":false,"message_length":12,"message_type":"Heartbeat",
            "mode":"Calibrating","reserved":0,"sensor_fault":true,
            "station_id":"KX7","uptime":86400},
 "data_encoded_full":"AgAAAAACAgAAAAABCABFAAAoAAAAAEAR9qbAAAIKwAACFJykw1AAFDR+AQAMS1g3AAABUYBg",
 "data_source_geo_location":null,"data_source_network_name":null,
 "data_source_system_type":null,"data_source_owner":null,
 "data_packet_classification":null,"data_packet_accm":null,
 "data_model_version":"2.0.0","application_version":"1.1.0",
 "dictionary":"weather-station","dictionary_version":"1.0.0"}
```

## Fields

| Field | Type | Meaning |
|---|---|---|
| `timestamp` | object of two integers | capture time: `secs_since_epoch` and `nanos_since_epoch`, from 1970-01-01T00:00:00Z |
| `src_port`, `dst_port` | integer | UDP or TCP ports of the packet |
| `src_ip`, `dst_ip` | array of integers | the address, one integer per byte: 4 for IPv4, 16 for IPv6 |
| `data_encoded_inner` | string | the transport payload, base64 |
| `data_encoded_full` | string | the frame as captured, tunnel included, base64 |
| `subsystem` | string or null | the dictionary interface the port belongs to |
| `message_type` | string or null | the message name; the type code in decimal when the interface has no name for it |
| `message` | object or null | field name to value; see below |
| `data_source_*`, `data_packet_*` | string or null | the deployment's settings of the same names |
| `data_model_version` | string | `"2.0.0"` |
| `application_version` | string | the version of `mind` |
| `capture_interface` | string | the network interface the frame was captured on. Present on live capture |
| `tunnel` | string | `gre`, `erspan` or `vxlan`. Present when the packet was unwrapped from a tunnel |
| `dictionary`, `dictionary_version` | string | the dictionary that decoded the message, from its `dictionary.json`. Present on decoded records |

A packet on a port no interface claims is published with `subsystem`,
`message_type` and `message` null.

### Values in `message`

Each value has the type its field declares in the dictionary.

| Field in the dictionary | Value in the record |
|---|---|
| `uint`, `int` | integer |
| `uint`, `int` with `scale` or `offset` | number: the decimal the scale gives, so raw 445012345 with scale `1e-7` is `44.5012345` ([dictionary-format.md](dictionary-format.md)) |
| `float` | number |
| `bool` | `true` or `false` |
| `ascii` | string |
| `bytes` | string of lower-case hexadecimal digits |
| any field whose raw value has a label in `values` | the label, a string |
| a field beyond the end of a short payload | `null` |

`bite schema <dictionary>` prints a JSON Schema (draft 2020-12) for the
`message` of each message type; every `message` validates against it
(`mind/tests/encodings.rs` checks this for every vector of the example
dictionaries).

A 64-bit integer field is written as a JSON number with every digit, and the
schema declares it `integer` with no upper bound. A JSON reader that keeps
numbers as doubles rounds a value above 2^53; read such a field with a parser
that keeps 64-bit integers, or take the record as CBOR, which carries the
integer exactly.

## Record versions

| Version | Selected by | `message` values and ports | Capture context | `data_model_version` |
|---|---|---|---|---|
| 2 | default | typed, as above | `capture_interface`, `tunnel`, `dictionary`, `dictionary_version` | `"2.0.0"` |
| 1 | `FIRST_BITE_RECORD_VERSION=1` | every value and both ports as text: the label, or the value in decimal | none | `FIRST_BITE_DATA_MODEL_VERSION` if set, else null |

The same heartbeat as version 1, where it differs:

```json
"src_port":"40100","dst_port":"50000",
"message":{"battery_low":"false","message_length":"12","message_type":"Heartbeat",
           "mode":"Calibrating","reserved":"0","sensor_fault":"true",
           "station_id":"KX7","uptime":"86400"},
"data_model_version":null
```

Version 1 is for a consumer written against text values. A process publishes
one version.

## Encodings

A publisher socket and each sink write the record as JSON or as CBOR, chosen
by `encoding` in their configuration
([`flow/config/README.md`](../flow/config/README.md)). JSON is the default.

| `encoding` | Bytes | Media type | Object name ends | Zenoh encoding |
|---|---|---|---|---|
| `json` | the record as JSON text | `application/json` | `.json` | `application/json` |
| `cbor` | the record as one CBOR item ([RFC 8949](https://www.rfc-editor.org/rfc/rfc8949)) | `application/cbor` | `.cbor` | `application/cbor` |

The CBOR record is the JSON record's data model, value for value: an object is
a map with text keys, an array an array, a string a text string, an integer an
integer, any other number a float, and `true`, `false` and `null` themselves.
It holds no tags and no byte strings: `data_encoded_inner`, `data_encoded_full`
and `bytes` fields are the same text as in JSON. Any CBOR library reads it, and
reading it gives the JSON record.

The item is written in the core deterministic encoding of RFC 8949 section
4.2.1: shortest integers and lengths, the shortest float that keeps the value,
definite lengths, map keys in bytewise order of their encodings. One record
always gives the same bytes, so the SHA-256 in an object name identifies it.

Reading a CBOR record:

```python
import cbor2, zmq
socket = zmq.Context().socket(zmq.SUB)
socket.connect("tcp://127.0.0.1:12000")
socket.setsockopt(zmq.SUBSCRIBE, b"")
topic, payload = socket.recv().split(b"|", 1)
record = cbor2.loads(payload)
```

A local file written with `encoding: cbor` is a CBOR sequence
([RFC 8742](https://www.rfc-editor.org/rfc/rfc8742)): one item after another
with nothing between them.

Sizes and encode times are in [performance.md](performance.md#record-size-and-encoding).
The goat sink writes JSON only ([goat/contract.md](goat/contract.md)).
