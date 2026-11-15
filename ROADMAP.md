# Roadmap

What is planned for first-bite, in the order it is intended to happen. Every
row is an issue with acceptance criteria; the live board, with status, is
[first-bite Roadmap](https://github.com/orgs/USAREUR-AF/projects/12).

| Horizon | Means |
|---|---|
| **In progress** | built or being built for release 1.1.0 |
| **Now** | the next milestone, starting when 1.1.0 is out |
| **Next** | planned after that milestone |
| **Later** | wanted; starts when a user or a measurement calls for it |

Priority within a horizon: P0 blocks adoption or release, P1 is the next thing
to pick up, P2 is planned, P3 waits for a reason to start. Size is XS to XL.

## In progress: release 1.1.0

Built and in review; these make the first full release. Milestone
[v1.1.0](https://github.com/USAREUR-AF/first-bite/milestone/1). A release
candidate, `v1.0.1-rc.1`, has proved the release workflow end to end.

| Issue | What | Priority | Area | Size |
|---|---|---|---|---|
| [#24](https://github.com/USAREUR-AF/first-bite/issues/24) | Typed values in the record | P0 | record | M |
| [#49](https://github.com/USAREUR-AF/first-bite/issues/49) | Selectable record encodings beyond JSON (a compact binary encoding first) | P0 | record | L |
| [#12](https://github.com/USAREUR-AF/first-bite/issues/12) | Selectable transport security levels | P0 | security | L |
| [#15](https://github.com/USAREUR-AF/first-bite/issues/15) | Cut the first tagged release and run every artifact | P0 | release | M |

## Now: variable-length records and a published surveillance format

The next milestone,
[Variable-length records and ASTERIX](https://github.com/USAREUR-AF/first-bite/milestone/2):
decode records whose fields vary in presence and length, show it on a
published format against an independent decoder, and compare two decoded
streams.

| Issue | What | Priority | Area | Size |
|---|---|---|---|---|
| [#23](https://github.com/USAREUR-AF/first-bite/issues/23) | Repeated groups, conditional fields and variable-length messages | P0 | decoder | L |
| [#55](https://github.com/USAREUR-AF/first-bite/issues/55) | ASTERIX category 033 as a dictionary, checked against an open decoder | P0 | dictionaries | L |
| [#56](https://github.com/USAREUR-AF/first-bite/issues/56) | Compare two decoded streams | P0 | tools | L |
| [#48](https://github.com/USAREUR-AF/first-bite/issues/48) | Labels on ranges, shared field groups, fixed-point and timestamp types | P1 | dictionaries | M |

## Next

| Issue | What | Priority | Area | Size |
|---|---|---|---|---|
| [#57](https://github.com/USAREUR-AF/first-bite/issues/57) | Publish onto a real goatnet | P1 | integration | M |
| [#58](https://github.com/USAREUR-AF/first-bite/issues/58) | Run the bite-decoded kid on a provisioned Open Billie node | P1 | integration | S |
| [#18](https://github.com/USAREUR-AF/first-bite/issues/18) | Run tunnelled mirrors against real mirror sessions | P1 | capture | M |
| [#19](https://github.com/USAREUR-AF/first-bite/issues/19) | Measure live capture at rate on a physical tap | P1 | capture | M |
| [#39](https://github.com/USAREUR-AF/first-bite/issues/39) | Run the unit under systemd on target hardware | P1 | release | S |
| [#20](https://github.com/USAREUR-AF/first-bite/issues/20) | Keep records when a sink is unreachable | P1 | sinks | L |
| [#21](https://github.com/USAREUR-AF/first-bite/issues/21) | Reassemble fragmented IP datagrams | P1 | capture | M |
| [#22](https://github.com/USAREUR-AF/first-bite/issues/22) | Reassemble TCP streams and frame messages | P1 | capture | L |
| [#50](https://github.com/USAREUR-AF/first-bite/issues/50) | OpenSSF Scorecard 9.5 | P1 | security | M |
| [#25](https://github.com/USAREUR-AF/first-bite/issues/25) | A metrics endpoint | P2 | capture | S |
| [#30](https://github.com/USAREUR-AF/first-bite/issues/30) | Publish schema declarations and a service announcement on a Zenoh fabric | P2 | integration | M |
| [#31](https://github.com/USAREUR-AF/first-bite/issues/31) | Cursor-on-Target events on a Zenoh fabric | P2 | integration | S |
| [#32](https://github.com/USAREUR-AF/first-bite/issues/32) | Run first-bite as a peer of an octo-pod | P2 | integration | S |

## Later

| Issue | What | Priority | Area | Size |
|---|---|---|---|---|
| [#26](https://github.com/USAREUR-AF/first-bite/issues/26) | Capture from SocketCAN | P2 | capture | M |
| [#27](https://github.com/USAREUR-AF/first-bite/issues/27) | Import a DBC file | P2 | dictionaries | M |
| [#37](https://github.com/USAREUR-AF/first-bite/issues/37) | MIL-STD-1553 and ARINC 429 sources | P2 | capture | L |
| [#28](https://github.com/USAREUR-AF/first-bite/issues/28) | A Python package | P2 | decoder | M |
| [#29](https://github.com/USAREUR-AF/first-bite/issues/29) | Swift and Kotlin bindings for the decoder | P2 | decoder | L |
| [#33](https://github.com/USAREUR-AF/first-bite/issues/33) | Decode payloads received from a Zenoh key | P2 | integration | M |
| [#51](https://github.com/USAREUR-AF/first-bite/issues/51) | OpenSSF Scorecard 10 | P2 | security | L |
| [#35](https://github.com/USAREUR-AF/first-bite/issues/35) | Scale the pipeline across cores | P3 | capture | M |
| [#36](https://github.com/USAREUR-AF/first-bite/issues/36) | Hardware timestamps | P3 | capture | S |
| [#38](https://github.com/USAREUR-AF/first-bite/issues/38) | Signed dictionaries | P3 | security | M |

## How the order was chosen

1. **Release what is built.** Typed records, a compact encoding and transport
   security levels are built; release 1.1.0 puts them in users' hands with
   static builds and provenance.
2. **Decode formats that are not fixed-layout.** Most surveillance and many
   bus formats carry optional and repeated items. Until the dictionary can
   express them, first-bite cannot read them, so this comes before breadth.
   A published format decoded against an independent decoder is the proof.
3. **Make agreement measurable.** Reading the same traffic on both sides of a
   changed link is the use people ask about first; a comparison of two
   decoded streams turns it from a method into a command.
4. **Measure what is only built.** A real goatnet, a provisioned Open Billie
   node, a physical tap at rate, real mirror sessions, the service under
   systemd: each row of the documentation that says built, not measured.
5. **Do not lose data.** A sink that is down or slow loses records today.
6. **Then breadth.** Reassembly, machine buses, language bindings and scale,
   each when a user or a measurement calls for it.

## Proposing a change to the roadmap

Open an issue with the user-story template. A maintainer sets its horizon and
priority on the board.
