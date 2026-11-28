# Checking the link: hub, clients, follow

`mind lan` answers three questions about the link a capture interface is on,
and like everything in first-bite it only listens: nothing is sent on the
capture interface.

| Command | Question | Status |
|---|---|---|
| `mind lan hub` | Does this port receive unicast frames between other hosts, so their traffic can be captured here? | built; covered by tests with constructed captures |
| `mind lan clients --server ADDR:PORT` | Which hosts are clients of one TCP server port, how often does each connect, and who ends the connections? | built; covered by tests with constructed captures |
| `mind lan follow --server ADDR:PORT` | Give me that server's stream to its client, in order, without connecting to the server | built; covered by tests with constructed captures; run against a live sensor: not yet |

Each reads a capture file or, with `--iface`, captures live for `--seconds`
(default 30; 0 runs until stopped). `--json` prints the result as JSON.

## `mind lan hub`

A switch learns which port each host is on and sends unicast frames only to
that port. A hub repeats every frame on every port, and a mirror port receives
copies on purpose. Only on a hub or a mirror port can first-bite see two other
hosts talking to each other.

```bash
sudo mind lan hub --iface eth0 --seconds 20
```

The command counts frames addressed to or sent from this port, broadcast and
multicast frames, and unicast frames between other hosts. Any of the last kind
means hub or mirror. None, after at least `--min-frames` frames (default 50),
means a switch port: capture needs a mirror port or a tap ([tap.md](tap.md)).
Fewer frames than that gives `undetermined`. On a capture file, give the
capturing port's MAC address with `--local-mac`; on an interface it is read
from the system.

Many small unmanaged boxes sold as hubs are switches. This is how to tell.

## `mind lan clients`

Some sensors serve their data on one TCP port and keep one client, or a few,
at a time. When more programs connect than the sensor keeps, each new
connection pushes another off, every client reconnects, and every consumer
sees its data stop and start.

```bash
sudo mind lan clients --iface eth0 --server 192.0.2.1:7003 --seconds 60
```

For each client host: connection attempts, connections the server ended,
connections the client ended, the number of separate connections (client
ports), and bytes received from the server. `contention` is true when more
than one host is connected and connections keep being ended or restarted.

On a switch port the command sees only this host's own connections; run
`mind lan hub` first.

## `mind lan follow`

When the link is a hub or a mirror port, a program does not need its own
connection to the sensor: the sensor's stream to its existing client already
arrives here. `follow` copies that stream off the wire and serves it on a local
TCP port as a stand-in for the sensor. Point the program at the stand-in
instead of the sensor; the sensor keeps exactly the clients it had.

```bash
sudo mind lan follow --iface eth0 --server 192.0.2.1:7003 --listen 127.0.0.1:17003 --seconds 0
```

How the stream is rebuilt:

- **One client's stream at a time.** It follows the first client the server
  sends to, and moves to another only after the followed stream has been quiet
  for `--switch-idle-ms` (default 5000). This host's own addresses, and any
  given with `--ignore`, are never followed.
- **In order.** Segments that arrive out of order are held until the gap
  before them fills. Retransmissions and overlaps are trimmed, so every byte is
  delivered once.
- **Gaps are skipped, not waited on forever.** A missing segment is waited for
  `--gap-wait-ms` (default 1000), then skipped.
- **Breaks are visible to the consumer.** When a gap is skipped, the server
  ends the stream, or a different client's stream is followed, every local
  client is disconnected. A consumer reconnects and resets its framing, exactly
  as it would after losing the sensor.
- `--output FILE` also writes the stream to a file; with a capture file as
  input, that is how a recorded session is turned back into the sensor's byte
  stream.

Limits:

- Data flows only while some client is connected to the sensor and receiving.
  If the sensor's only client disconnects, the stand-in is silent.
- `follow` copies the server-to-client direction only. A program that must
  send commands to the sensor needs a relay that is the sensor's one client
  and serves the same port to everyone else; that is a different tool.
- A port that drops mirrored frames under load (see [tap.md](tap.md)) shows up
  as skipped gaps.
