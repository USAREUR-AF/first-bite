#!/usr/bin/env python3
"""Build the vectors of the ntp dictionary from RFC 5905, without first-bite.

    dictionaries/ntp/vectors.py            write dictionaries/ntp/vectors/*.json
    dictionaries/ntp/vectors.py --check    fail if the files differ from what this writes

A vector's expected values must not come from the decoder being tested. Here
they are the values chosen below, and the bytes are packed from them with
`struct` by the layout of RFC 5905 Figure 8. This script reads nothing from
the dictionary: the labels it expects are typed from Figures 9, 10 and 11.
Only the standard library is used.
"""
import json
import struct
import sys
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
NOTE = "Packed with Python struct by dictionaries/ntp/vectors.py from the layout of RFC 5905 Figure 8; the values are chosen there. "

# Figure 9, Figure 10 and Figure 11, typed from the RFC.
LEAP = {
    0: "no warning",
    1: "last minute of the day has 61 seconds",
    2: "last minute of the day has 59 seconds",
    3: "unknown (clock unsynchronized)",
}
MODE = {
    1: "symmetric active",
    2: "symmetric passive",
    3: "client",
    4: "server",
    5: "broadcast",
    6: "NTP control message",
}
STRATUM = {
    0: "unspecified or invalid",
    1: "primary server (e.g., equipped with a GPS receiver)",
    16: "unsynchronized",
}

# Seconds from the prime epoch of era 0, 0 h 1 January 1900 UTC (section 6),
# to the Unix epoch, 1 January 1970: 70 years with 17 leap days.
ERA_0_TO_UNIX = (70 * 365 + 17) * 86400


def stamp(text: str, fraction: int = 0) -> tuple[int, int]:
    """An NTP timestamp: (seconds since 1900, fraction in units of 2^-32 s)."""
    if not text:
        return (0, 0)
    when = datetime.fromisoformat(text).replace(tzinfo=timezone.utc)
    return (int(when.timestamp()) + ERA_0_TO_UNIX, fraction)


def short(seconds: int, fraction: int) -> int:
    """NTP short format: 16-bit seconds, 16-bit fraction."""
    return seconds << 16 | fraction


def number(value: float) -> str:
    return str(int(value)) if value == int(value) else repr(value)


def packet(leap, version, mode, stratum, poll, precision, delay, dispersion, refid, ref, org, rec, xmt):
    """The 48 bytes of Figure 8 and the value each field holds."""
    payload = struct.pack(
        ">BBbbII4s8I",
        leap << 6 | version << 3 | mode,
        stratum,
        poll,
        precision,
        delay,
        dispersion,
        refid,
        *ref, *org, *rec, *xmt,
    )
    fields = {
        "leap_indicator": LEAP[leap],
        "version_number": str(version),
        "mode": MODE[mode],
        "stratum": STRATUM.get(stratum, str(stratum)),
        "poll": str(poll),
        "precision": str(precision),
        "root_delay": number(delay / 2**16),
        "root_dispersion": number(dispersion / 2**16),
        "reference_id": refid.hex(),
    }
    for name, (seconds, fraction) in (("reference", ref), ("origin", org), ("receive", rec), ("transmit", xmt)):
        fields[f"{name}_timestamp_seconds"] = str(seconds)
        fields[f"{name}_timestamp_fraction"] = number(fraction / 2**32)
    return payload, MODE[mode], fields


def vectors() -> dict:
    made = {}

    def add(name, note, payload, message_type, fields, keep=None):
        if keep is not None:
            # A payload cut short: fields that reach past its end decode to null.
            payload = payload[:keep]
            ends = {"reference_id": 16}
            for index, stem in enumerate(("reference", "origin", "receive", "transmit")):
                ends[f"{stem}_timestamp_seconds"] = 20 + 8 * index
                ends[f"{stem}_timestamp_fraction"] = 24 + 8 * index
            fields = {key: (None if ends.get(key, 0) > keep else value) for key, value in fields.items()}
        made[name] = {
            "interface": "NTP",
            "payload_hex": payload.hex(),
            "expect": {"message_type": message_type, "fields": fields},
            "note": NOTE + note,
        }

    request_sent = stamp("2026-01-15T12:00:00", 0x40000000)  # .25 s

    # What a simple client sends: version 4, mode 3, its own clock in the
    # transmit timestamp, everything else zero. The first byte is 0x23.
    add(
        "client",
        "A request as an SNTP client sends it: only the version, the mode and the transmit timestamp are set.",
        *packet(0, 4, 3, 0, 0, 0, 0, 0, bytes(4), (0, 0), (0, 0), (0, 0), request_sent),
    )
    # The reply of a secondary server (Figure 31): the origin timestamp is the
    # request's transmit timestamp, and the reference ID is the IPv4 address
    # of the server it follows, here the documentation address 192.0.2.1.
    reply = packet(
        0, 4, 4, 2, 6, -23,
        short(0, 0x0600), short(0, 0x0A80),
        bytes([192, 0, 2, 1]),
        stamp("2026-01-15T11:58:56", 0xC0000000),
        request_sent,
        stamp("2026-01-15T12:00:00", 0x43000000),
        stamp("2026-01-15T12:00:00", 0x43800000),
    )
    add("server", "The reply of a stratum 2 server to the request in client.json; the reference ID is the address 192.0.2.1.", *reply)
    add(
        "server-truncated",
        "The reply in server.json cut to its first 40 bytes: the transmit timestamp is past the end.",
        *reply,
        keep=40,
    )
    add(
        "server-primary",
        "The reply of a stratum 1 server whose reference clock is GPS (Figure 12).",
        *packet(
            0, 4, 4, 1, 6, -20,
            0, short(0, 0x0010),
            b"GPS\0",
            stamp("2026-01-15T11:59:59", 0x80000000),
            request_sent,
            stamp("2026-01-15T12:00:00", 0x41000000),
            stamp("2026-01-15T12:00:00", 0x41400000),
        ),
    )
    # Section 7.4: stratum 0 and the kiss code RATE in the reference ID. The
    # leap indicator says the server's clock is not synchronized.
    add(
        "server-kiss-of-death",
        "A kiss-o'-death packet (section 7.4): stratum 0 with the kiss code RATE of Figure 13 in the reference ID.",
        *packet(3, 4, 4, 0, 6, -23, 0, 0, b"RATE", (0, 0), request_sent, (0, 0), (0, 0)),
    )
    # A broadcast server has no request to answer, so origin and receive are
    # zero. The transmit fraction is one unit: 2^-32 s, about 232 picoseconds.
    add(
        "broadcast",
        "A broadcast from a stratum 3 server; the transmit fraction is one unit, 2^-32 s.",
        *packet(
            0, 4, 5, 3, 6, -22,
            short(0, 0x1234), short(0, 0x2345),
            bytes([198, 51, 100, 7]),
            stamp("2026-01-15T11:50:00", 0x12345678),
            (0, 0),
            (0, 0),
            stamp("2026-01-15T12:00:16", 0x00000001),
        ),
    )
    # The two symmetric modes, with values chosen to exercise the fields
    # rather than to look like a running peer: a leap second announced, the
    # largest poll interval, a root delay with a seconds part.
    add(
        "symmetric-active",
        "Values chosen to exercise the fields: a leap second announced, poll 10, a root delay of 1.5 s.",
        *packet(
            1, 4, 1, 2, 10, -18,
            short(1, 0x8000), short(0, 0xFFFF),
            bytes([203, 0, 113, 9]),
            stamp("2026-06-30T23:42:40", 0xFFFFFFFF),
            stamp("2026-06-30T23:59:01", 0x00010000),
            stamp("2026-06-30T23:59:02", 0x80000001),
            stamp("2026-06-30T23:59:03", 0x7FFFFFFF),
        ),
    )
    # Version 3 has the same header (section 7.3). Stratum 16, a negative poll
    # and a root dispersion with its top bit set are not what a peer sends in
    # normal running; they show the sign of poll and the width of the fields.
    add(
        "symmetric-passive",
        "Values chosen to exercise the fields: version 3, stratum 16, a negative poll, a root dispersion with its top bit set.",
        *packet(
            2, 3, 2, 16, -6, -1,
            short(0xFFFF, 0xFFFF), short(0x8000, 0x0001),
            bytes(4),
            (0, 0),
            stamp("2026-06-30T23:59:03", 0x7FFFFFFF),
            stamp("2026-06-30T23:59:04", 0x00000002),
            stamp("2026-06-30T23:59:05", 0xFFFFFFFE),
        ),
    )
    # Mode 6 is named by Figure 10 and laid out elsewhere: identified, not
    # decoded. The twelve bytes are a control header with operation code 2.
    made["ntp-control-message"] = {
        "interface": "NTP",
        "payload_hex": struct.pack(">BBHHHHH", 0 << 6 | 2 << 3 | 6, 2, 1, 0, 0, 0, 0).hex(),
        "expect": {"message_type": "NTP control message", "fields": {}},
        "note": "First byte packed by dictionaries/ntp/vectors.py as version 2, mode 6 (RFC 5905 Figure 10). The message is named and has no definition here, so no field is decoded.",
    }
    # Mode 7 is reserved for private use and has no name: the record's message
    # type is the number.
    made["private-use"] = {
        "interface": "NTP",
        "payload_hex": struct.pack(">B7x", 0 << 6 | 2 << 3 | 7).hex(),
        "expect": {"message_type": "7", "fields": {}},
        "note": "First byte packed by dictionaries/ntp/vectors.py as version 2, mode 7, reserved for private use (RFC 5905 Figure 10): the message type is the number and no field is decoded.",
    }
    return made


def main() -> int:
    directory = HERE / "vectors"
    stale = []
    for name, vector in vectors().items():
        path = directory / f"{name}.json"
        text = json.dumps(vector, indent=4) + "\n"
        if "--check" in sys.argv[1:]:
            if not path.is_file() or path.read_text(encoding="utf-8") != text:
                stale.append(path.name)
        else:
            directory.mkdir(exist_ok=True)
            path.write_text(text, encoding="utf-8")
    if stale:
        print("differ from vectors.py: " + ", ".join(stale), file=sys.stderr)
        return 1
    print(f"{len(vectors())} vectors")
    return 0


if __name__ == "__main__":
    sys.exit(main())
