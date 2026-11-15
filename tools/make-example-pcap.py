#!/usr/bin/env python3
"""Write a pcap file of UDP packets carrying a dictionary's test vectors.

    tools/make-example-pcap.py dictionaries/weather-station examples/weather-station.pcap
    tools/make-example-pcap.py dictionaries/weather-station /tmp/load.pcap --repeat 200000

Each vector of an interface that has a port becomes one Ethernet/IPv4/UDP
frame from 192.0.2.10:<port> to 192.0.2.20:50000, one second apart. With
--repeat N the set is written N times, for load tests. Only the standard
library is used.
"""
import json
import struct
import sys
from pathlib import Path


def checksum(data: bytes) -> int:
    if len(data) % 2:
        data += b"\0"
    total = sum(struct.unpack(f">{len(data) // 2}H", data))
    while total >> 16:
        total = (total & 0xFFFF) + (total >> 16)
    return ~total & 0xFFFF


def udp_frame(source_port: int, destination_port: int, payload: bytes) -> bytes:
    source, destination = bytes([192, 0, 2, 10]), bytes([192, 0, 2, 20])
    udp_length = 8 + len(payload)
    pseudo = source + destination + struct.pack(">BBH", 0, 17, udp_length)
    udp = struct.pack(">HHHH", source_port, destination_port, udp_length, 0) + payload
    udp = udp[:6] + struct.pack(">H", checksum(pseudo + udp) or 0xFFFF) + udp[8:]
    ip = struct.pack(">BBHHHBBH", 0x45, 0, 20 + udp_length, 0, 0, 64, 17, 0) + source + destination
    ip = ip[:10] + struct.pack(">H", checksum(ip)) + ip[12:]
    ethernet = bytes([2, 0, 0, 0, 0, 2]) + bytes([2, 0, 0, 0, 0, 1]) + b"\x08\x00"
    return ethernet + ip + udp


def main() -> int:
    args = sys.argv[1:]
    repeat = 1
    if "--repeat" in args:
        at = args.index("--repeat")
        repeat = int(args[at + 1])
        del args[at : at + 2]
    if len(args) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    dictionary, output = Path(args[0]), Path(args[1])
    ports = {}
    for metadata in sorted(dictionary.glob("*/metadata.json")):
        port = json.loads(metadata.read_text()).get("port")
        if port is not None:
            ports[metadata.parent.name] = port
    frames = []
    for path in sorted((dictionary / "vectors").glob("*.json")):
        vector = json.loads(path.read_text())
        port = ports.get(vector["interface"])
        if port is None:
            continue
        payload = bytes.fromhex("".join(vector["payload_hex"].split()))
        frames.append(udp_frame(port, 50000, payload))
    with output.open("wb") as out:
        # pcap global header: microsecond timestamps, Ethernet link type.
        out.write(struct.pack("<IHHiIII", 0xA1B2C3D4, 2, 4, 0, 0, 65535, 1))
        index = 0
        for _ in range(repeat):
            for frame in frames:
                out.write(struct.pack("<IIII", 1_750_000_000 + index, 0, len(frame), len(frame)))
                out.write(frame)
                index += 1
    print(f"{output}: {index} packets")
    return 0


if __name__ == "__main__":
    sys.exit(main())
