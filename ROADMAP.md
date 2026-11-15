# Roadmap

What is planned for first-bite, in the order it is intended to happen. Every
row is an issue with acceptance criteria; the live board, with status, is
[first-bite Roadmap](https://github.com/orgs/USAREUR-AF/projects/12).

| Horizon | Means |
|---|---|
| **In progress** | being built now |
| **Now** | next to start; needed before or with the first release |
| **Next** | planned after the first release |
| **Later** | wanted; starts when a user or a measurement calls for it |

Priority within a horizon: P0 blocks adoption or release, P1 is the next thing
to pick up, P2 is planned, P3 waits for a reason to start. Size is XS to XL.

## In progress

| Issue | What | Priority | Area | Size |
|---|---|---|---|---|
| [#11](https://github.com/USAREUR-AF/first-bite/issues/11) | Vulnerability scan, hardening and credential references | P0 | security | L |
| [#12](https://github.com/USAREUR-AF/first-bite/issues/12) | Selectable transport security levels | P0 | security | L |
| [#13](https://github.com/USAREUR-AF/first-bite/issues/13) | Optional machinery to join a goat-net | P1 | integration | L |
| [#14](https://github.com/USAREUR-AF/first-bite/issues/14) | A method and tooling to go from an ICD to a dictionary | P1 | dictionaries | L |

## Now

| Issue | What | Priority | Area | Size |
|---|---|---|---|---|
| [#15](https://github.com/USAREUR-AF/first-bite/issues/15) | Cut the first tagged release and run every artifact | P0 | release | M |
| [#16](https://github.com/USAREUR-AF/first-bite/issues/16) | Release review and making the repository public | P0 | release | S |
| [#17](https://github.com/USAREUR-AF/first-bite/issues/17) | Move hmac and sha2 to the releases that share digest 0.11 | P1 | sinks | XS |
| [#18](https://github.com/USAREUR-AF/first-bite/issues/18) | Run tunnelled mirrors against real mirror sessions | P1 | capture | M |
| [#19](https://github.com/USAREUR-AF/first-bite/issues/19) | Measure live capture at rate on a physical tap | P1 | capture | M |
| [#20](https://github.com/USAREUR-AF/first-bite/issues/20) | Keep records when a sink is unreachable | P1 | sinks | L |
| [#39](https://github.com/USAREUR-AF/first-bite/issues/39) | Run the unit under systemd on target hardware | P1 | release | S |

## Next

| Issue | What | Priority | Area | Size |
|---|---|---|---|---|
| [#21](https://github.com/USAREUR-AF/first-bite/issues/21) | Reassemble fragmented IP datagrams | P1 | capture | M |
| [#22](https://github.com/USAREUR-AF/first-bite/issues/22) | Reassemble TCP streams and frame messages | P1 | capture | L |
| [#23](https://github.com/USAREUR-AF/first-bite/issues/23) | Repeated groups, conditional fields and variable-length messages | P1 | decoder | L |
| [#24](https://github.com/USAREUR-AF/first-bite/issues/24) | A second record version with typed values and capture context | P1 | record | M |
| [#25](https://github.com/USAREUR-AF/first-bite/issues/25) | A metrics endpoint | P2 | capture | S |
| [#26](https://github.com/USAREUR-AF/first-bite/issues/26) | Capture from SocketCAN | P2 | capture | M |
| [#27](https://github.com/USAREUR-AF/first-bite/issues/27) | Import a DBC file | P2 | dictionaries | M |
| [#28](https://github.com/USAREUR-AF/first-bite/issues/28) | A Python package | P2 | decoder | M |
| [#30](https://github.com/USAREUR-AF/first-bite/issues/30) | Publish schema declarations and a service announcement on a Zenoh fabric | P2 | integration | M |
| [#31](https://github.com/USAREUR-AF/first-bite/issues/31) | Cursor-on-Target events on a Zenoh fabric | P2 | integration | S |
| [#32](https://github.com/USAREUR-AF/first-bite/issues/32) | Run first-bite as a peer of an octo-pod | P2 | integration | S |

## Later

| Issue | What | Priority | Area | Size |
|---|---|---|---|---|
| [#29](https://github.com/USAREUR-AF/first-bite/issues/29) | Swift and Kotlin bindings for the decoder | P2 | decoder | L |
| [#33](https://github.com/USAREUR-AF/first-bite/issues/33) | Decode payloads received from a Zenoh key | P2 | integration | M |
| [#34](https://github.com/USAREUR-AF/first-bite/issues/34) | Use the bite library as the decoder inside another project's sensor package | P3 | integration | S |
| [#35](https://github.com/USAREUR-AF/first-bite/issues/35) | Scale the pipeline across cores | P3 | capture | M |
| [#36](https://github.com/USAREUR-AF/first-bite/issues/36) | Hardware timestamps | P3 | capture | S |
| [#37](https://github.com/USAREUR-AF/first-bite/issues/37) | MIL-STD-1553 and ARINC 429 sources | P3 | capture | L |
| [#38](https://github.com/USAREUR-AF/first-bite/issues/38) | Signed dictionaries | P3 | security | M |

## How the order was chosen

1. **Prove what is built.** A tagged release run on every platform, the
   service run under systemd on a real host, live capture measured at rate on
   a physical tap, and tunnelled mirrors run against real mirror sessions.
   Until then several rows of the documentation say built, not measured.
2. **Do not lose data.** A sink that is down or slow loses records today.
   Store-and-forward comes before new capabilities.
3. **Decode more of what is on a wire.** Fragmented datagrams, TCP streams, and
   message structures that repeat, are conditional or vary in length. These
   decide how many real interfaces can be transcribed in full.
4. **Give consumers a better record.** Typed values and capture context, as an
   opt-in second version.
5. **Reach more buses and more languages.** SocketCAN, DBC import, Python,
   then mobile bindings and other buses when someone can test them.

## Proposing a change to the roadmap

Open an issue with the user-story template. A maintainer sets its horizon and
priority on the board.
