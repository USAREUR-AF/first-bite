#!/usr/bin/env python3
"""Send each packet of a capture file to the UDP socket of `mind --mock`.

    tools/mockplay.py examples/weather-station.pcap
    tools/mockplay.py capture.pcap --address 127.0.0.1:12345 --rate 100

Every packet record of the file becomes one UDP datagram that carries the
whole captured frame, which is what `mind --mock` expects: one Ethernet frame
per datagram. Only the standard library is used.

The file must be a classic pcap file with the Ethernet link type, the format
`tools/make-example-pcap.py` and `tcpdump -w` write. A pcapng file needs
converting first:

    tcpdump -r in.pcapng -w out.pcap
"""
import argparse
import socket
import struct
import sys
import time
from pathlib import Path

# Magic number of a classic pcap file, as the four bytes read from disk, and
# the byte order of the headers it implies. The second pair has nanosecond
# timestamps; the layout is the same.
MAGIC = {
    b"\xd4\xc3\xb2\xa1": "<",
    b"\xa1\xb2\xc3\xd4": ">",
    b"\x4d\x3c\xb2\xa1": "<",
    b"\xa1\xb2\x3c\x4d": ">",
}
PCAPNG_MAGIC = b"\x0a\x0d\x0d\x0a"
LINKTYPE_ETHERNET = 1
# The largest payload one UDP datagram over IPv4 can carry.
MAX_DATAGRAM = 65_507


class CaptureError(Exception):
    pass


def frames(path: Path):
    """Yield the captured bytes of each packet record in the file."""
    with path.open("rb") as capture:
        header = capture.read(24)
        if header[:4] == PCAPNG_MAGIC:
            raise CaptureError(
                f"{path} is a pcapng file; convert it first: tcpdump -r {path} -w out.pcap"
            )
        order = MAGIC.get(header[:4])
        if order is None or len(header) < 24:
            raise CaptureError(f"{path} is not a pcap file")
        link_type = struct.unpack(order + "I", header[20:24])[0]
        if link_type != LINKTYPE_ETHERNET:
            raise CaptureError(
                f"{path} has link type {link_type}; only Ethernet (1) can be sent to --mock"
            )
        while True:
            record = capture.read(16)
            if not record:
                return
            if len(record) < 16:
                raise CaptureError(f"{path} ends inside a packet header")
            captured_length = struct.unpack(order + "IIII", record)[2]
            data = capture.read(captured_length)
            if len(data) < captured_length:
                raise CaptureError(f"{path} ends inside a packet")
            yield data


def address(value: str):
    host, separator, port = value.rpartition(":")
    if not separator or not host or not port.isdigit() or not 0 < int(port) < 65536:
        raise argparse.ArgumentTypeError("expected host:port")
    return host, int(port)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Send each packet of a pcap file to the UDP socket of `mind --mock`.",
        epilog="A pcapng file needs converting first: tcpdump -r in.pcapng -w out.pcap",
    )
    parser.add_argument("capture", type=Path, help="classic pcap file, Ethernet link type")
    parser.add_argument(
        "--address",
        type=address,
        default=("127.0.0.1", 12345),
        metavar="HOST:PORT",
        help="where `mind --mock` listens (default 127.0.0.1:12345)",
    )
    parser.add_argument(
        "--rate",
        type=float,
        default=0,
        metavar="PPS",
        help="packets per second (default: as fast as they can be sent)",
    )
    arguments = parser.parse_args()
    if arguments.rate < 0:
        parser.error("--rate must not be negative")

    sent = skipped = 0
    sender = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        for frame in frames(arguments.capture):
            if len(frame) > MAX_DATAGRAM:
                skipped += 1
                continue
            sender.sendto(frame, arguments.address)
            sent += 1
            if arguments.rate:
                time.sleep(1 / arguments.rate)
    except (CaptureError, OSError) as error:
        print(f"mockplay: {error}", file=sys.stderr)
        return 1
    finally:
        sender.close()
    host, port = arguments.address
    print(f"sent {sent} packets to {host}:{port}")
    if skipped:
        print(f"skipped {skipped} packets larger than {MAX_DATAGRAM} bytes", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
