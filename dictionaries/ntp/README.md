# ntp

The Network Time Protocol packet header, transcribed from
[RFC 5905](https://www.rfc-editor.org/rfc/rfc5905) section 7.3, Figure 8: the
48 bytes on UDP port 123 that almost every network already carries. It is the
example to run against traffic you already have.

```console
$ bite decode dictionaries/ntp --port 123 240206e90000060000000a80c0000201ed135500c0000000ed13554040000000ed13554043000000ed13554043800000
{
  "fields": {
    "leap_indicator": "no warning",
    "mode": "server",
    "origin_timestamp_fraction": 0.25,
    "origin_timestamp_seconds": 3977467200,
    "poll": 6,
    "precision": -23,
    "receive_timestamp_fraction": 0.26171875,
    "receive_timestamp_seconds": 3977467200,
    "reference_id": "c0000201",
    "reference_timestamp_fraction": 0.75,
    "reference_timestamp_seconds": 3977467136,
    "root_delay": 0.0234375,
    "root_dispersion": 0.041015625,
    "stratum": 2,
    "transmit_timestamp_fraction": 0.263671875,
    "transmit_timestamp_seconds": 3977467200,
    "version_number": 4
  },
  "interface": "NTP",
  "message_type": "server"
}
```

## Try it on your own network

Build `mind` (`make build`), find the name of the interface that carries your
traffic (`ip -br link` on Linux, `ifconfig -l` on macOS) and capture on it:

```bash
sudo ./target/debug/mind eth0 --dictionary dictionaries/ntp --filter "udp port 123"
```

Every machine that keeps its clock set sends a request from time to time, so
records appear without further action. To provoke one now, from a second
terminal:

| System | Command |
|---|---|
| macOS | `sntp pool.ntp.org` |
| Linux with chrony | `sudo chronyc burst 1/1` |
| Linux with systemd-timesyncd | `sudo systemctl restart systemd-timesyncd` |
| Linux with ntpdate or sntp installed | `ntpdate -q pool.ntp.org`, `sntp pool.ntp.org` |
| Windows, seen from a tap or mirror port | `w32tm /stripchart /computer:pool.ntp.org /samples:1 /dataonly` |
| Anything with Python | `python3 -c 'import socket; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.sendto(b"\x23" + bytes(47), ("pool.ntp.org", 123))'` |

`mind` logs each packet as one line that holds the record, and publishes the
same record on ZeroMQ at `tcp://127.0.0.1:12000` under the topic `NTP/client`
or `NTP/server`. A query gives two records: the request, with
`"message_type":"client"`, and the reply from port 123, whose `message` object
reads (values from a reply captured this way):

```json
{"leap_indicator":"no warning","mode":"server","origin_timestamp_fraction":"0","origin_timestamp_seconds":"4003728096","poll":"0","precision":"-24","receive_timestamp_fraction":"0.14186147297732532","receive_timestamp_seconds":"4003728096","reference_id":"a2dd2504","reference_timestamp_fraction":"0.6533022378571332","reference_timestamp_seconds":"4003727893","root_delay":"0.0180511474609375","root_dispersion":"0.0196533203125","stratum":"2","transmit_timestamp_fraction":"0.14189041079953313","transmit_timestamp_seconds":"4003728096","version_number":"4"}
```

Ctrl-C stops the capture. With `--output` the records are also written,
pretty-printed, to `completelog_<time>.log`. Without a network,
[`examples/ntp.pcap`](../../examples/ntp.pcap) holds the ten vectors:

```bash
cargo run -p mind -- --pcap-file examples/ntp.pcap --dictionary dictionaries/ntp --output
```

## Reading the values

| Field | Read it as |
|---|---|
| `*_timestamp_seconds` | seconds since 0 h 1 January 1900 UTC. Subtract 2208988800 for Unix time: 3977467200 is 2026-01-15T12:00:00Z. Zero means unknown or unsynchronized. The count wraps in 2036; the era is not in the packet. |
| `*_timestamp_fraction` | the fraction of that second, in seconds; add it to the seconds field. |
| `root_delay`, `root_dispersion` | seconds. |
| `poll`, `precision` | an exponent: the interval or precision is 2 to that power, in seconds. 6 is 64 s; -23 is about 119 ns. |
| `stratum` | 1 is a primary server, 2 to 15 the number of steps from one, 0 and 16 not synchronized. |
| `reference_id` | at stratum 2 and above, the IPv4 address of the server's own source (`c0000201` is 192.0.2.1) or four octets of the MD5 hash of its IPv6 address; at stratum 1, the ASCII name of the reference clock (`47505300` is `GPS`); at stratum 0, an ASCII kiss code (`52415445` is `RATE`: slow down). |

With `jq`, the transmit time of each record of a `--output` file:

```bash
jq -r 'select(.message.transmit_timestamp_seconds) | .message | ((.transmit_timestamp_seconds | tonumber) - 2208988800 | todate) + " +" + .transmit_timestamp_fraction + " s"' completelog_*.log
```

A client that guards against tracking sends a random number as its transmit
timestamp, so a request's timestamp may not be a time; the server returns it
unchanged as the origin timestamp.

## How it is laid out

| Choice | Reason |
|---|---|
| The message is identified by the Mode field, bits 5 to 7 of the first byte. | Mode is what distinguishes a request from a reply. A record is then `NTP/client` or `NTP/server`, and a subscriber takes the one it wants by topic. |
| Modes 1 to 5 each have a message file, and the five hold the same fields. | The RFC has one header for all five. The format gives a message one name, and one definition per name, so the header is written five times; `bite/tests/dictionaries.rs` fails if the five differ. Mapping all five modes to one message would need one file and lose the topic. |
| Mode 6, NTP control message, is named and not defined. | Its layout is not that of Figure 8 (RFC 1305 appendix B, RFC 9327). A packet in mode 6 is identified and no field is decoded; `dictionary.json` allows the finding with that reason. |
| Modes 0 and 7 are reserved and have no name. | A record for one has the number as its message type. |
| Root delay and root dispersion are one field each, with `"scale": 0.0000152587890625`. | NTP short format is 16 bits of seconds and 16 of fraction, which is a 32-bit integer in units of 2^-16 s. A double holds every such value exactly. |
| Each timestamp is two fields, seconds and fraction. | One 64-bit field scaled by 2^-32 would be rounded to the 53 bits of a double, about half a microsecond. Split, the seconds are an exact integer and the fraction, scaled by 2^-32, is exact. |
| The seconds stay as the RFC has them, counted from 1900. | An `offset` of -2208988800 would give Unix time and turn the zero that means "unknown" into a date in 1900. |
| `stratum` labels 0, 1 and 16. | A label names one value. Figure 11 gives 2 to 15 one meaning as a range, so those decode to the number, which is also the useful value. |
| `reference_id` is `bytes`. | Whether the four octets are text or an address depends on `stratum`, and a field cannot depend on another field. |

## What is not here

A packet may continue after byte 48 with extension fields (section 7.5) and a
message authentication code. Their number and length vary, so they are not
transcribed: the 48-byte header of a longer packet decodes and the rest is
left alone. Packets secured with Network Time Security are of this kind.

## Vectors

[`vectors.py`](vectors.py) packs each payload with Python `struct` from values
chosen in that file, by the layout of Figure 8, and writes the values as the
expectation. It reads nothing from the dictionary, so the vectors test the
dictionary against a second reading of the RFC. `vectors.py --check` fails if
a file in `vectors/` differs from what it writes.

| Vector | Shows |
|---|---|
| `client` | the request of a simple client: version, mode and transmit timestamp, the rest zero |
| `server` | the reply of a stratum 2 server to that request |
| `server-primary` | a stratum 1 server on GPS |
| `server-kiss-of-death` | stratum 0 with the kiss code `RATE` (section 7.4) |
| `server-truncated` | the reply cut to 40 bytes: the transmit timestamp is `null` |
| `broadcast` | mode 5, with no origin or receive timestamp |
| `symmetric-active`, `symmetric-passive` | a leap second announced, version 3, a negative poll, values with the top bit set |
| `ntp-control-message` | mode 6: identified, not decoded |
| `private-use` | mode 7: the message type is `7` |

```bash
bite fmt --check dictionaries/ntp
bite check --strict dictionaries/ntp
bite lint dictionaries/ntp
bite coverage --strict dictionaries/ntp
```
