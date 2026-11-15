#!/usr/bin/env python3
"""Print the UDP payloads of a capture file, one per line as hex.

    tools/extract-udp-payloads.py capture.pcap --port 40100 > payloads.txt
    bite stats <dictionary> --port 40100 --payloads payloads.txt

Reads a classic pcap file (not pcapng) of Ethernet, Linux cooked or raw IP
frames, with or without VLAN tags, carrying IPv4 or IPv6. With --port, only
datagrams whose source or destination port is that port are printed.
Fragmented datagrams are skipped and counted. Only the standard library is
used, and nothing leaves this machine.

A capture of real traffic is handled as the system it was taken from is: keep
the capture and this output where the interface document may be kept.
"""
import argparse
import struct
import sys

ETHERNET, RAW_IP, LINUX_SLL = 1, 101, 113


def frames(path):
    """Yield (link type, frame bytes) for every record of a classic pcap file."""
    with open(path, "rb") as handle:
        header = handle.read(24)
        if len(header) < 24:
            sys.exit(f"{path}: too short to be a pcap file")
        magic = header[:4]
        if magic in (b"\xd4\xc3\xb2\xa1", b"\x4d\x3c\xb2\xa1"):
            order = "<"
        elif magic in (b"\xa1\xb2\xc3\xd4", b"\xa1\xb2\x3c\x4d"):
            order = ">"
        elif magic == b"\x0a\x0d\x0d\x0a":
            sys.exit(f"{path}: pcapng is not read; convert it with `tcpdump -r in.pcapng -w out.pcap`")
        else:
            sys.exit(f"{path}: not a pcap file")
        link = struct.unpack(order + "I", header[20:24])[0] & 0x0FFFFFFF
        while True:
            record = handle.read(16)
            if len(record) < 16:
                return
            captured = struct.unpack(order + "IIII", record)[2]
            data = handle.read(captured)
            if len(data) < captured:
                return
            yield link, data


def ip_packet(link, frame):
    """The IP packet inside a frame, or None."""
    if link == RAW_IP:
        return frame
    if link == ETHERNET:
        offset, kind = 14, frame[12:14]
    elif link == LINUX_SLL:
        offset, kind = 16, frame[14:16]
    else:
        return None
    while kind in (b"\x81\x00", b"\x88\xa8"):          # VLAN tags
        kind = frame[offset + 2:offset + 4]
        offset += 4
    if kind not in (b"\x08\x00", b"\x86\xdd"):
        return None
    return frame[offset:]


def udp(packet):
    """(source port, destination port, payload) of a UDP datagram, None for
    anything else, or "fragment" for a fragmented IPv4 datagram."""
    if len(packet) < 20:
        return None
    version = packet[0] >> 4
    if version == 4:
        header = (packet[0] & 0x0F) * 4
        if packet[9] != 17:
            return None
        flags_and_offset = struct.unpack(">H", packet[6:8])[0]
        if flags_and_offset & 0x3FFF:                   # more fragments, or an offset
            return "fragment"
        total = struct.unpack(">H", packet[2:4])[0]
        segment = packet[header:total] if total >= header else packet[header:]
    elif version == 6:
        if len(packet) < 40 or packet[6] != 17:         # extension headers are not followed
            return None
        length = struct.unpack(">H", packet[4:6])[0]
        segment = packet[40:40 + length]
    else:
        return None
    if len(segment) < 8:
        return None
    source, destination, length = struct.unpack(">HHH", segment[:6])
    return source, destination, segment[8:max(length, 8)]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("capture", help="classic pcap file")
    parser.add_argument("--port", type=int, help="keep datagrams sent from or to this UDP port")
    args = parser.parse_args()
    kept = other = fragments = 0
    for link, frame in frames(args.capture):
        packet = ip_packet(link, frame)
        datagram = udp(packet) if packet is not None else None
        if datagram == "fragment":
            fragments += 1
        elif datagram is None or (args.port is not None and args.port not in datagram[:2]):
            other += 1
        else:
            kept += 1
            print(datagram[2].hex())
    print(f"{kept} payloads printed, {other} other frames, {fragments} fragments skipped", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
