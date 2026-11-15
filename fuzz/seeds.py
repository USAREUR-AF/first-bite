#!/usr/bin/env python3
"""Write the seed corpus of every fuzz target from what the repository ships.

    fuzz/seeds.py [--out fuzz/corpus]

  decode_payload   one input per test vector of the shipped dictionaries: the
                   frame identifier (8 bytes, little-endian), then the payload.
  load_dictionary  one input per shipped dictionary and one per interface: the
                   files as NUL-separated path and text pairs, then a payload
                   from the dictionary's vectors.
  frame_pipeline   every frame of examples/weather-station.pcap as captured,
                   as a raw IP packet, and wrapped in each tunnel the pipeline
                   unwraps (GRE, ERSPAN types I to III, VXLAN), each after its
                   option byte.

The corpus is generated, not committed: a seed never goes stale against the
dictionaries. Standard library only. Each input is named by its SHA-1, as
libFuzzer names what it finds.
"""
import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VXLAN_PORT = 4789


def write(directory: Path, data: bytes) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / hashlib.sha1(data).hexdigest()).write_bytes(data)


def dictionaries():
    """The shipped dictionaries in the order fuzz/build.rs embeds them."""
    return sorted(p for p in (ROOT / "dictionaries").iterdir() if p.is_dir())


def vectors(dictionary: Path):
    for path in sorted((dictionary / "vectors").glob("*.json")):
        vector = json.loads(path.read_text())
        yield vector, bytes.fromhex("".join(vector["payload_hex"].split()))


def files(dictionary: Path):
    for path in sorted(dictionary.rglob("*.json")):
        yield path.relative_to(dictionary).as_posix(), path.read_text()


def packed(pairs, payload: bytes) -> bytes:
    out = b""
    for path, text in pairs:
        out += path.encode() + b"\0" + text.encode().replace(b"\0", b"") + b"\0"
    return out + payload


def checksum(data: bytes) -> int:
    if len(data) % 2:
        data += b"\0"
    total = sum(struct.unpack(f">{len(data) // 2}H", data))
    while total >> 16:
        total = (total & 0xFFFF) + (total >> 16)
    return ~total & 0xFFFF


ETHERNET = bytes([2, 0, 0, 0, 0, 4]) + bytes([2, 0, 0, 0, 0, 3]) + b"\x08\x00"


def ipv4(protocol: int, payload: bytes) -> bytes:
    """An IPv4 packet between two documentation addresses (RFC 5737)."""
    header = struct.pack(">BBHHHBBH", 0x45, 0, 20 + len(payload), 0, 0, 64, protocol, 0)
    header += bytes([198, 51, 100, 1]) + bytes([198, 51, 100, 2])
    header = header[:10] + struct.pack(">H", checksum(header)) + header[12:]
    return header + payload


def gre(flags: int, protocol: int, extra: bytes, inner: bytes) -> bytes:
    return ETHERNET + ipv4(47, struct.pack(">BBH", flags, 0, protocol) + extra + inner)


def vxlan(inner: bytes) -> bytes:
    body = bytes([0x08, 0, 0, 0, 0, 0, 0x2A, 0]) + inner
    udp = struct.pack(">HHHH", 49152, VXLAN_PORT, 8 + len(body), 0) + body
    return ETHERNET + ipv4(17, udp)


def pcap_frames(path: Path):
    data = path.read_bytes()
    magic, = struct.unpack_from("<I", data)
    if magic != 0xA1B2C3D4:
        sys.exit(f"{path}: not a little-endian microsecond pcap file")
    at = 24
    while at + 16 <= len(data):
        _, _, captured, _ = struct.unpack_from("<IIII", data, at)
        yield data[at + 16 : at + 16 + captured]
        at += 16 + captured


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=ROOT / "fuzz" / "corpus")
    out = parser.parse_args().out

    shipped = dictionaries()
    for dictionary in shipped:
        payload = b""
        for vector, payload in vectors(dictionary):
            frame_id = struct.pack("<Q", vector.get("frame_id") or 0)
            write(out / "decode_payload", frame_id + payload)
        pairs = list(files(dictionary))
        write(out / "load_dictionary", packed(pairs, payload))
        for interface in sorted(p.name for p in dictionary.iterdir() if (p / "metadata.json").exists()):
            only = [(path, text) for path, text in pairs if path.startswith(interface + "/")]
            write(out / "load_dictionary", packed(only, payload))

    # Option byte: bit 0 raw IP, bit 1 keep GRE, bit 2 unwrap VXLAN, bits 4-7
    # the dictionary. The example capture is of the weather-station dictionary.
    index = [d.name for d in shipped].index("weather-station") << 4
    sequence = bytes([0, 0, 0, 1])
    for frame in pcap_frames(ROOT / "examples" / "weather-station.pcap"):
        packet = frame[14:]
        wrapped = [
            (0, frame),
            (1, packet),
            (0, gre(0x00, 0x6558, b"", frame)),
            (0, gre(0x20, 0x0800, bytes([0, 0, 0, 7]), packet)),
            (0, gre(0x00, 0x88BE, b"", frame)),
            (0, gre(0x10, 0x88BE, sequence + bytes([0x10, 0x64, 0, 1, 0, 0, 0, 0]), frame)),
            (0, gre(0x10, 0x22EB, sequence + bytes([0x20, 0x64, 0, 1] + [0] * 8), frame)),
            (2, gre(0x00, 0x6558, b"", frame)),
            (4, vxlan(frame)),
            (4, vxlan(gre(0x00, 0x6558, b"", frame))),
        ]
        for options, data in wrapped:
            write(out / "frame_pipeline", bytes([index | options]) + data)

    for target in sorted(p for p in out.iterdir() if p.is_dir()):
        print(f"{target}: {len(list(target.iterdir()))} inputs")
    return 0


if __name__ == "__main__":
    sys.exit(main())
