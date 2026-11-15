# mind

The `mind` binary: it reads frames from a source, decodes each UDP or TCP
payload against a dictionary with [`bite`](../bite), and publishes one JSON
record per packet through [`flow`](../flow). The root [README](../README.md)
lists every option and environment variable.

## Run

```bash
# A capture file, read once.
cargo run -p mind -- --pcap-file examples/weather-station.pcap \
    --dictionary dictionaries/weather-station

# A network interface; capture needs root or CAP_NET_RAW and CAP_NET_ADMIN.
make run IFACE=eth0

# A UDP socket, one Ethernet frame per datagram: no capture privileges.
make mock        # mind --mock on 127.0.0.1:12345
make mockplay    # in a second terminal: sends examples/weather-station.pcap
```

`make run` and `make mock` start `mind` in this directory with
[`env/default.local.env`](env/default.local.env), which selects the
`weather-station` dictionary and the configuration files in
[`flow/config`](../flow/config). `make replay PCAP=<file> IFACE=<nic>` puts a
capture file on an interface with `tcpreplay`, for `make run` to capture.

Records leave on the ZeroMQ PUB socket `tcp://127.0.0.1:12000` under the topic
`<interface>/<message>`, as JSON unless the publisher is configured with
`encoding: cbor`. [docs/record.md](../docs/record.md) lists the record's
fields. Log lines go to stderr; at level `info` there is one
line per record, and the last line of a capture-file run is a count:

```text
[mind] --- done: 4 frames, 4 records, 4 decoded, 0 not UDP or TCP, 0 malformed
```

## Frame sources

| Source | Selected by | Reads |
|---|---|---|
| `LiveCapture` | `<interface>` | libpcap on a network interface, promiscuous unless `--no-promiscuous`, optional `--filter` |
| `FileCapture` | `--pcap-file <file>` | a pcap or pcapng file, then exits |
| `UdpFrames` | `--mock` | one Ethernet frame per datagram on `--mock-address` |

## Code

| File | What |
|---|---|
| [`src/cli.rs`](src/cli.rs) | command-line options |
| [`src/main.rs`](src/main.rs) | start-up: environment file, dictionary, publishers, sinks, the run loop |
| [`src/source.rs`](src/source.rs) | `FrameSource` and its three implementations |
| [`src/pipeline.rs`](src/pipeline.rs) | one frame to one record: slice headers, find the interface by port, decode |
| [`src/run.rs`](src/run.rs) | the loop from source through pipeline to publish; `--output` file |
| [`src/dictionary.rs`](src/dictionary.rs) | loads the dictionary from `--dictionary` or `BITE_DICT_PATH` |
| [`tests/replay.rs`](tests/replay.rs) | writes a capture file, replays it through the file source and the pipeline, checks the records |
| [`tests/encodings.rs`](tests/encodings.rs) | every vector of the example dictionaries as a record: typed values against the vector, `message` against `bite schema`, CBOR read back against JSON |
| [`examples/encodings.rs`](examples/encodings.rs) | prints record sizes and encode times per encoding for a dictionary's vectors |

At run time the binary needs libpcap (`libpcap0.8` on Debian and Ubuntu) and
glibc. [docs/deployment.md](../docs/deployment.md) covers running it as a
service and in a container.
