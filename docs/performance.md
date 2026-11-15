# Performance

Every figure here was measured by a command in this repository, on one core of
an AMD Ryzen 7 8845HS (a 2024 laptop-class processor) under Linux, with a
release build. Run the same commands to get the figures for your hardware.

| What | Result | Command |
|---|---|---|
| Decode one message (identify it, extract every field, render the values) | **0.69 µs**: 1.45 million messages, 9.3 million fields per second | `cargo run --release -p bite --example throughput -- dictionaries/weather-station` |
| The same on CAN-style frames | 0.71 µs: 1.40 million frames per second | `… -- dictionaries/vehicle-can` |
| Whole pipeline: read a frame, parse Ethernet/IP/UDP, decode, build the JSON record, publish on ZeroMQ | **2.9 µs**: 340,000 packets per second | `mind --pcap-file load.pcap --dictionary dictionaries/weather-station` on 2,000,000 packets |
| The same, also writing one log line per record | 145,000 packets per second | as above with `RUST_LOG=warn,mind=info` |
| Memory while replaying a 145 MB capture | 10.7 MB resident | `/usr/bin/time -f %M` on the same run |
| Live capture on two interfaces at once | 500 of 500 packets decoded, none dropped | `mind lo eno1 --filter "udp port 40100"` |
| Binary | 16.5 MB, one file, needs only glibc | `tools/build-portable.sh` |

The load file is the five weather-station vectors repeated:

```bash
tools/make-example-pcap.py dictionaries/weather-station load.pcap --repeat 400000
RUST_LOG=warn,mind=info,mind::run=warn \
    target/release/mind --pcap-file load.pcap --dictionary dictionaries/weather-station
```

`mind` prints the rate when a replay ends:

```text
done: 2000000 frames, 2000000 records, 2000000 decoded, 0 not UDP or TCP, 0 malformed
replayed in 5.851 s: 341844 frames per second
```

## What the numbers mean

At 2.9 µs per packet, a link carrying 1,000 messages per second uses about 0.3%
of one core, and a record is on the bus about three microseconds after its
frame is read. The capture runs beside the systems on the link, on a tap or a
mirror port, so none of that time is taken from them.

The pipeline is one thread. The cost per packet is fixed by the work listed in
the table, not by the size of the dictionary: finding the interface is one hash
lookup on the port, finding the message is one more on the message type.

## What changed from the first implementation

The first decoder in this project looked every field up through a string-keyed
map and copied the whole dictionary for each field it decoded. On the same
1,658 recorded messages:

| | First implementation | Now |
|---|---|---|
| Messages decoded per second, one core | 194 | 1,700,000 |
| Fields that straddle a byte boundary | returned as null | decoded |
| Dictionary | compiled into the binary | a directory loaded at start |
| Faulty dictionary entries | skipped silently | reported by `bite check` with file and field |

## Not measured

Live capture at high packet rates on a physical tap, where the limits are the
network card, the kernel capture buffer and the sinks rather than the decoder;
`docs/tap.md` describes the loss counters that show when one of them is the
limit. Sink throughput to object storage or a Zenoh router, which depends on
the far end.
