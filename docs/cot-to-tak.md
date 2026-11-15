# Cursor-on-Target to a TAK server

first-bite can turn a decoded message that carries a position into a
Cursor-on-Target (CoT) event and deliver it to a TAK server, or to TAK clients
on the local network. This page goes from a packet to an event on the wire,
with the weather-station example dictionary.

| Piece | Status |
|---|---|
| CoT 2.0 event from a record and a field mapping | built |
| delivery over UDP, unicast and multicast | built; measured against listeners on one machine |
| delivery over TCP, with reconnection and a bounded queue | built; measured against listeners on one machine |
| delivery over TLS with a client certificate | built; measured against a TLS server that requires a client certificate |
| delivery to a TAK server product | not measured |

## What is sent

One event per record, built by the mapping under `cot:`
([`cotify/README.md`](../cotify/README.md) describes every key). For the
`Position` message of `dictionaries/weather-station`:

```xml
<event version="2.0" uid="KX7" type="a-f-G" time="2026-11-15T15:56:11.709165450+00:00" start="2026-11-15T15:56:11.709165450+00:00" stale="2026-11-15T16:06:11.709165450+00:00" how="m-g"><point lat="64.8378" lon="-147.7164" hae="136.5" ce="9999999" le="9999999"/><detail/></event>
```

On `udp` each event is one datagram. On `tcp` the events follow one another on
the connection with nothing between them, which is the form a TAK server's
streaming input reads.

## Who receives it

The destinations the configuration lists, and no other. A `udp` or `tcp`
subscriber without `destinations` stops `mind` at startup. Each destination is
a host and a port the owner of the data chose: the address of a TAK server
input, or the multicast group the owner's TAK clients listen on.

## To a TAK server over TLS

A TAK server takes streaming input on a TLS port (8089 on a default
installation) and identifies each client by its certificate. The server's
administrator issues the client certificate and gives the certificate of the
authority that signed the server's own. Both arrive as PEM files, or as
PKCS#12 files that `openssl pkcs12` converts to PEM.

```yaml
# subscribers.yml
subscribers:
  - name: PositionsToTakServer
    subscriber_type: "tcp"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics:
      - "WX/Position"
    destinations:
      - address: "tak.example.org:8089"
    tls:
      ca_file: "/etc/first-bite/tak/truststore.pem"
      client_cert_file: "/etc/first-bite/tak/first-bite.pem"
      client_key_file: "/etc/first-bite/tak/first-bite.key"
    cot:
      uid: [station_id]
      type:
        value: "a-f-G"
      position:
        geodetic: [latitude, longitude, altitude]
      stale_minutes: 10
```

```bash
SUBSCRIBER_CONFIG_PATH=subscribers.yml \
    mind eth0 --dictionary dictionaries/weather-station
```

The server's certificate is checked against `ca_file` and against nothing
else, and must carry the destination's host name, or the name in
`tls.server_name` when the server is addressed by IP. A server that fails
either check is not sent anything.

`topics` selects the messages that carry a position. A record the mapping
cannot use is reported in the log and not sent.

Under a [`security` block](security/transport.md) this subscriber satisfies
level `tls`, and `mtls` with the client certificate. The certificate and the
authorities may also come from the block (`client_cert`, `client_key`,
`ca_bundle`), with `tls: {}` on the subscriber. `mind security show` prints
what the connection will use, and `mind security probe` makes one handshake
with the server and sends nothing.

## To TAK clients on the local network

TAK clients listen for events on the multicast group `239.2.3.1`, port 6969,
unless they are set otherwise.

```yaml
subscribers:
  - name: PositionsToTakClients
    subscriber_type: "udp"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics:
      - "WX/Position"
    destinations:
      - address: "239.2.3.1:6969"
        ttl: 1                    # stays on the local network
        interface: "192.0.2.15"   # address of the interface to send from
    cot:
      uid: [station_id]
      type:
        value: "a-f-G"
      position:
        geodetic: [latitude, longitude, altitude]
      stale_minutes: 10
```

A multicast datagram reaches the hosts that joined the group on the networks
it crosses, and it is not encrypted. `interface` and `ttl` set which networks
those are.

Under a `security` block at level `tls` or `mtls`, a `udp` subscriber whose
destination is on another host is refused at startup, as is a `tcp`
subscriber without `tls`. Where plaintext to that destination is what the
owner intends, the subscriber says so and says why:

```yaml
    security:
      transport: unenforced
      reason: "TAK clients on the enclave network listen on multicast only"
```

The reason is printed by `mind security show`. A destination on this machine
needs no override.

## To a plain TCP input, and several destinations

A TAK server can also be given a plain TCP streaming input (8087 on a default
installation). The stream is not encrypted; it suits a server on the same
machine, or a network its owner accepts plaintext on.

```yaml
    subscriber_type: "tcp"
    destinations:
      - address: "127.0.0.1:8087"
      - address: "203.0.113.20:8087"
        source_port: 4242
```

A subscriber sends every event to each of its destinations. `source_port`
fixes the local port the connection or the datagrams come from, for a server
that is configured to tell its inputs apart by it.

## A destination given by name

A destination may be a literal address or a name. A name can resolve to
several addresses, often one IPv4 and one IPv6, and on many machines
`localhost` gives `::1` before `127.0.0.1`.

| | What the sink does with a name |
|---|---|
| `tcp` | resolves the name for each connection attempt and tries the addresses in the order resolved until one connects. The log names the address reached, the others, and any that refused |
| `udp` | resolves the name at start-up and sends to the first address. A datagram gets no answer, so there is nothing to tell the sink that another address would have been the right one. The name is resolved again every five minutes and after a failed send; a change of address is logged |

```text
subscriber PositionsToTakServer: connected to tak.example.org:8089 at 203.0.113.20:8089 (the name also resolves to [2001:db8::20]:8089), with TLS
subscriber PositionsToTakClients: sending UDP to clients.example.org:6969 at [2001:db8::7]:6969 from port 49152 (the name also resolves to 203.0.113.7:6969)
```

**For `udp`, write a literal address.** A listener bound to `127.0.0.1` does
not hear a datagram sent to `::1`, and nothing fails: the line above, logged
at start-up, is where to look when datagrams do not arrive.

## When the server is away

A `tcp` subscriber keeps one connection per destination. If it cannot connect,
or the connection is lost, it tries again after 1 s, then 2 s, 4 s and so on
up to 30 s. Meanwhile up to `queue` events (default 1000) wait for that
destination; an event that finds the queue full is dropped and counted.
Capture, decoding and the other sinks are not held up. The log reports, per
destination, in every minute in which a count changed:

```text
subscriber PositionsToTakServer: tcp tak.example.org:8089: 1204 sent, 0 dropped with the queue full, 0 failed, 1 reconnections
```

A `udp` subscriber holds nothing: a datagram is sent or it is not.

## Try it on one machine

Two listeners stand in for the clients and the server:

```bash
nc -u -l 127.0.0.1 6969 &        # the UDP listener
nc -l 127.0.0.1 8087 &           # the TCP listener
```

```yaml
# subscribers.yml
subscribers:
  - name: ToUdp
    subscriber_type: "udp"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics: ["WX/Position"]
    destinations:
      - address: "127.0.0.1:6969"
    cot: &position
      uid: [station_id]
      type: { value: "a-f-G" }
      position: { geodetic: [latitude, longitude, altitude] }
      stale_minutes: 10
  - name: ToTcp
    subscriber_type: "tcp"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics: ["WX/Position"]
    destinations:
      - address: "127.0.0.1:8087"
    cot: *position
```

```bash
SUBSCRIBER_CONFIG_PATH=subscribers.yml cargo run -p mind -- \
    --pcap-file examples/weather-station.pcap --dictionary dictionaries/weather-station
```

The capture holds one `Position` message; each listener prints the event shown
at the top of this page.

## Other payloads

Without a `cot:` block a `udp` or `tcp` subscriber sends the record itself, in
the subscriber's `encoding`: one record per datagram, or on `tcp` one JSON
record per line. [`flow/config/README.md`](../flow/config/README.md) lists
every key.

## Beside a transform on ZeroMQ

A subpub with `subtype: cot` publishes the same event on a second ZeroMQ
socket, for software on the machine that wants the events as messages
([`flow/config/README.md`](../flow/config/README.md#subpubs)). The two are
independent: a `udp` or `tcp` subscriber needs no subpub and no
`--activate-subpubs`.
