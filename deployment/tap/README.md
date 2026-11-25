# tap: capture to pcap slices and publish them

A first-bite tap is `mind --tap-only`: it captures the frames that match a BPF
filter, writes them to rotating pcap files ("slices") and, when configured,
publishes each finished slice to a Zenoh key. It decodes nothing and publishes
no records. One systemd template unit runs one tap per instance name.

| Piece | Status |
|---|---|
| `--pcap-dir`: rotating slices, cut by time and by size, newest N kept | built; tested with frames built in code |
| `--pcap-publish`: HTTP `PUT` of each finished slice to a Zenoh REST port, retried | built; tested against a local HTTP server |
| `first-bite-tap@.service`, `tap.env.example`, `install.sh` | built; not yet run on a node |
| publishing through a native Zenoh session instead of the REST port | designed, not built |

## What it does, and where the data goes

| Data | Where |
|---|---|
| frames that match `TAP_FILTER` on `TAP_INTERFACE` | `/var/lib/first-bite-tap/<name>/<name>-<UTC time>-<n>.pcap`, one file per `TAP_SLICE_SECONDS` |
| the slice being written | the same name with `.part` appended; renamed when it is closed |
| older slices | deleted once more than `TAP_KEEP` are on disk |
| each finished slice, with `--pcap-publish <url> --pcap-key <key>` | `PUT <url>/<key>`, content type `application/vnd.tcpdump.pcap`: on a Zenoh router's REST port this puts the file on the key, where any subscriber to the key receives it |
| what the tap did | the journal: `journalctl -u first-bite-tap@<name>` (one line per closed and per published slice) |

A slice is closed when it covers `TAP_SLICE_SECONDS` of frame time, when it
would pass `--pcap-slice-mb`, or when the link type of the frames changes. A
link with no matching frames writes no file. A `PUT` that fails (connection
refused, an HTTP error status) is retried `--pcap-publish-retries` times
(default 5), 2 s apart and doubling; a slice that still fails stays on disk,
in the ring. `{host}` in the key is replaced by the host name.

The same options work in a normal `mind` run: with `--pcap-dir` and without
`--tap-only`, `mind` decodes and publishes records as usual and also writes
every captured frame to slices.

## Install

`mind` must be installed first ([`deployment/install.sh`](../install.sh)).

```bash
sudo ./install.sh sensor                      # unit + /etc/first-bite/tap-sensor.env from tap.env.example
sudoedit /etc/first-bite/tap-sensor.env       # interface, filter, slice length, publishing
sudo systemctl restart first-bite-tap@sensor
sudo ./install.sh sensor --env my-tap.env     # or install with a prepared settings file
sudo ./install.sh sensor --uninstall          # stop; settings and slices are kept
```

| Setting | Meaning |
|---|---|
| `TAP_INTERFACE` | interface to capture on |
| `TAP_FILTER` | BPF filter; spaces allowed, for example `udp port 40100` |
| `TAP_SLICE_SECONDS` | seconds a slice covers |
| `TAP_KEEP` | finished slices kept on disk |
| `TAP_ARGS` | further `mind` options, for example `--pcap-publish http://127.0.0.1:8000 --pcap-key sensor/pcap/{host}` |

## How this differs from the capture node

[`deployment/capture-node`](../capture-node/README.md) records with `dumpcap`
and copies finished files to an S3-compatible bucket on a timer. The tap is
`mind` itself, with the same capture code and filter handling as a decoding
run, and publishes each finished slice to a Zenoh key as it closes. Use the
capture node where the only way out is a bucket; use the tap where a Zenoh
router (a local island) is the way out, or where `mind` already runs.

## When the machine has to sit in the link

The tap reads an interface it is given and never transmits. To build the
machine itself, with two ports bridged inline or a listen port behind a passive
tap or mirror port, use [goatrope](https://github.com/USAREUR-AF/goatrope). It
prepares the ports, the bridge and the recording ring, and can run a pinned
first-bite as one of its components.
