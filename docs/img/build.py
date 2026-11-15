#!/usr/bin/env python3
"""Write the illustrations in this directory. Standard library only.

    python3 docs/img/build.py            write every figure, light and dark
    python3 docs/img/build.py --check    fail if a committed figure differs from what this script writes

Each figure is written twice: <name>.svg for a light page and <name>-dark.svg
for a dark one. The files stand alone (no external CSS, fonts or scripts), so
they render in an <img> on GitHub, on the site and in the PDF brief.

Every label is measured against font-metrics.json (the widest advance of each
character across the font stack, plus 6 %) before it is placed, and the script
stops if a label does not fit its box or leaves the canvas. README.md in this
directory describes the visual system.
"""
import json
import math
import os
import sys
from html import escape

HERE = os.path.dirname(os.path.abspath(__file__))
METRICS = json.load(open(os.path.join(HERE, "font-metrics.json"), encoding="utf-8"))
W = 760
FONT = '"Helvetica Neue", Helvetica, Arial, "Liberation Sans", sans-serif'
MONO = 'Menlo, Consolas, "Liberation Mono", "Courier New", monospace'
MIN_SIZE = 13

PALETTE = {
    "light": {
        "ink": "#16191f", "quiet": "#515a66", "rule": "#dfe3e8", "line": "#7d8899",
        "paper": "#ffffff", "panel": "#f4f6f8",
        "accent": "#1f4fbf", "accent-soft": "#e6edfb", "on-accent": "#ffffff",
        "good": "#24703a", "good-bg": "#e3f4e7", "warn": "#8a5300", "warn-bg": "#fcefd6",
        "info": "#5b4bb0", "info-bg": "#eeebfa", "mute": "#5a606a", "mute-bg": "#eeeeec",
    },
    "dark": {
        "ink": "#e6e9ee", "quiet": "#a3abb8", "rule": "#2c333d", "line": "#7a8594",
        "paper": "#0f1317", "panel": "#171b21",
        "accent": "#8fb0ff", "accent-soft": "#172544", "on-accent": "#0f1317",
        "good": "#6cc487", "good-bg": "#172b1e", "warn": "#e3b04f", "warn-bg": "#31281a",
        "info": "#b3a6f5", "info-bg": "#221d3d", "mute": "#9aa2ad", "mute-bg": "#23262b",
    },
}

CSS = """
text{font-family:%(font)s}
.m{font-family:%(mono)s}
.t{fill:%(ink)s}.q{fill:%(quiet)s}.a{fill:%(accent)s}.g{fill:%(good)s}.w{fill:%(warn)s}.i{fill:%(info)s}.mu{fill:%(mute)s}.on{fill:%(on-accent)s}
.bg{fill:%(paper)s;stroke:%(rule)s;stroke-width:1}
.bx{fill:%(paper)s;stroke:%(line)s;stroke-width:1.25}
.bxp{fill:%(panel)s;stroke:%(rule)s;stroke-width:1.25}
.bxa{fill:%(accent-soft)s;stroke:%(accent)s;stroke-width:1.25}
.bxg{fill:%(good-bg)s;stroke:%(good)s;stroke-width:1.25}
.bxw{fill:%(warn-bg)s;stroke:%(warn)s;stroke-width:1.25}
.bxm{fill:%(mute-bg)s;stroke:%(mute)s;stroke-width:1.25;stroke-dasharray:2 4}
.bxd{fill:%(paper)s;stroke:%(line)s;stroke-width:1.25;stroke-dasharray:5 4}
.ln{stroke:%(line)s;stroke-width:1.25;fill:none}
.lnk{stroke:%(ink)s;stroke-width:3;fill:none}
.lna{stroke:%(accent)s;stroke-width:2;fill:none}
.lnd{stroke:%(line)s;stroke-width:1.25;fill:none;stroke-dasharray:6 5}
.lnr{stroke:%(rule)s;stroke-width:1;fill:none}
.fa{fill:%(accent)s}.fl{fill:%(line)s}.fk{fill:%(ink)s}.fg{fill:%(good)s}.fp{fill:%(panel)s}.fr{fill:%(rule)s}
.f0{fill:%(accent-soft)s}.f1{fill:%(warn-bg)s}.f2{fill:%(info-bg)s}.f3{fill:%(good-bg)s}
.s0{stroke:%(accent)s}.s1{stroke:%(warn)s}.s2{stroke:%(info)s}.s3{stroke:%(good)s}
.c0{fill:%(accent)s}.c1{fill:%(warn)s}.c2{fill:%(info)s}.c3{fill:%(good)s}
.pp{fill:%(paper)s}
path,line{stroke-linejoin:round}
"""


def tw(s, size, bold=False, mono=False):
    """Width of a label in user units, with headroom."""
    if mono:
        return len(s) * 0.602 * size * 1.04
    table = METRICS["bold" if bold else "regular"]
    return sum(table.get(ch, 600) for ch in s) / 1000 * size * 1.06


def n(v):
    return ("%.2f" % v).rstrip("0").rstrip(".")


def thousands(v):
    return f"{v:,}"


class Fig:
    def __init__(self, name, title, desc, height):
        self.name, self.title, self.desc, self.h = name, title, desc, height
        self.out = []
        self.out.append(f'<rect class="bg" x="0.5" y="0.5" width="{W - 1}" height="{height - 1}" rx="12"/>')
        self.text(24, 38, title, 18, bold=True, role="title")

    # -- text ---------------------------------------------------------------
    def text(self, x, y, s, size=13, bold=False, cls="t", anchor="start", mono=False,
             halo=False, role=None, maxw=None):
        if size < MIN_SIZE:
            raise SystemExit(f"{self.name}: {s!r} is below the minimum size")
        w = tw(s, size, bold, mono)
        x0 = x if anchor == "start" else x - w / 2 if anchor == "middle" else x - w
        if x0 < 8 or x0 + w > W - 8 or y - size < 4 or y + size * 0.3 > self.h - 6:
            raise SystemExit(f"{self.name}: {s!r} leaves the canvas ({x0:.0f}..{x0 + w:.0f}, y {y:.0f})")
        if maxw is not None and w > maxw:
            raise SystemExit(f"{self.name}: {s!r} is {w:.0f} wide, more than the {maxw:.0f} it has")
        if halo:   # a paper patch behind the label, so a line passing under it does not cross the text
            self.out.append(f'<rect x="{n(x0 - 4)}" y="{n(y - size)}" width="{n(w + 8)}" height="{n(size * 1.35)}" class="pp" data-lint="halo"/>')
        c = cls + (" m" if mono else "")
        a = "" if anchor == "start" else f' text-anchor="{anchor}"'
        b = ' font-weight="700"' if bold else ""
        r = f' data-role="{role}"' if role else ""
        if halo:
            r += ' data-lint="halo"'
        self.out.append(f'<text x="{n(x)}" y="{n(y)}" font-size="{n(size)}"{b}{a}{r} class="{c}">{escape(s)}</text>')
        return w

    def rich(self, x, y, parts, size=13, maxw=None, role="note"):
        """One line made of (text, bold, class) runs."""
        w = sum(tw(s, size, b) for s, b, _ in parts)
        if x < 8 or x + w > W - 8:
            raise SystemExit(f"{self.name}: {parts!r} leaves the canvas")
        if maxw is not None and w > maxw:
            raise SystemExit(f"{self.name}: {parts!r} is {w:.0f} wide, more than {maxw:.0f}")
        spans = "".join(
            f'<tspan class="{c}"{" font-weight=" + chr(34) + "700" + chr(34) if b else ""}>{escape(s)}</tspan>'
            for s, b, c in parts)
        self.out.append(f'<text x="{n(x)}" y="{n(y)}" font-size="{n(size)}" data-role="{role}" class="t" xml:space="preserve">{spans}</text>')

    # -- shapes -------------------------------------------------------------
    def rect(self, x, y, w, h, cls="bx", rx=8):
        self.out.append(f'<rect x="{n(x)}" y="{n(y)}" width="{n(w)}" height="{n(h)}" rx="{n(rx)}" class="{cls}"/>')

    def box(self, x, y, w, h, lines, cls="bx", rx=8, align="middle", pad=10):
        """A box with centred lines. lines: (text, size, bold, class[, mono])."""
        self.rect(x, y, w, h, cls, rx)
        lead = [ln[1] + 5 for ln in lines]
        total = sum(lead) - 5
        if total > h - 12:
            raise SystemExit(f"{self.name}: box at {x},{y} is too short for its text")
        cy = y + (h - total) / 2
        for ln, ld in zip(lines, lead):
            s, size, bold, c = ln[:4]
            mono = len(ln) > 4 and ln[4]
            base = cy + size * 0.80
            tx = x + w / 2 if align == "middle" else x + pad
            self.text(tx, base, s, size, bold, c, anchor=align if align == "middle" else "start",
                      mono=mono, maxw=w - 2 * pad)
            cy += ld

    def line(self, pts, cls="ln", head=None, start_head=False, nohead=False, hcls=None):
        """Polyline. head: fill class of an arrowhead at the last point."""
        pts = [tuple(map(float, p)) for p in pts]
        heads = ""

        def arrow(tip, prev):
            (x2, y2), (x1, y1) = tip, prev
            length = math.hypot(x2 - x1, y2 - y1)
            ux, uy = (x2 - x1) / length, (y2 - y1) / length
            hl, hw = 10, 5
            bx, by = x2 - ux * hl, y2 - uy * hl
            p1, p2 = (bx - uy * hw, by + ux * hw), (bx + uy * hw, by - ux * hw)
            path = f'<path d="M{n(x2)} {n(y2)}L{n(p1[0])} {n(p1[1])}L{n(p2[0])} {n(p2[1])}z" class="{head}"/>'
            return (bx + ux, by + uy), path

        draw = list(pts)
        if head:
            draw[-1], h1 = arrow(pts[-1], pts[-2])
            heads += h1
            if start_head:
                draw[0], h0 = arrow(pts[0], pts[1])
                heads += h0
        d = "M" + "L".join(f"{n(x)} {n(y)}" for x, y in draw)
        lint = ' data-lint="nohead"' if (nohead or not head) else ""
        self.out.append(f'<path d="{d}" class="{cls}"{lint}/>' + heads)

    def disc(self, cx, cy, r, cls, label=None, lcls="on"):
        self.out.append(f'<circle cx="{n(cx)}" cy="{n(cy)}" r="{n(r)}" class="{cls}"/>')
        if label is not None:
            self.text(cx, cy + 4.6, str(label), 13, True, lcls, "middle")

    def raw(self, s):
        self.out.append(s)

    # -- output -------------------------------------------------------------
    def render(self, theme):
        pal = dict(PALETTE[theme], font=FONT, mono=MONO)
        css = (CSS % pal).strip().replace("\n", "")
        tid, did = f"{self.name}-title", f"{self.name}-desc"
        return (
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {self.h}" width="{W}" height="{self.h}" '
            f'role="img" aria-labelledby="{tid} {did}" aria-label="{escape(self.title)}">\n'
            f'<title id="{tid}">{escape(self.title)}</title>\n<desc id="{did}">{escape(self.desc)}</desc>\n'
            f'<style>{css}</style>\n' + "\n".join(self.out) + "\n</svg>\n")


# ---------------------------------------------------------------------------
# 1. how-it-works
# ---------------------------------------------------------------------------
def how_it_works():
    f = Fig("how-it-works", "A tap copies the link; first-bite decodes the copy and publishes it",
            "Two native systems exchange binary messages on a link. A tap on the link sends a copy of every "
            "frame one way to first-bite, which captures it, decodes it against a dictionary of JSON files and "
            "publishes one JSON record per message on a ZeroMQ PUB socket. Object storage, an HTTP endpoint, a "
            "Zenoh data fabric, a Cursor-on-Target transform and any other subscriber read the records. "
            "Nothing is sent back to the link, so the native systems are not delayed, blocked or altered.", 500)
    # native link
    f.box(40, 64, 150, 60, [("System A", 15, True, "t"), ("native system", 13, False, "q")])
    f.box(570, 64, 150, 60, [("System B", 15, True, "t"), ("native system", 13, False, "q")])
    f.line([(190, 94), (236, 94)], "lnk")
    f.line([(296, 94), (570, 94)], "lnk")
    for x in (318, 350, 382, 480, 512, 544):
        f.rect(x - 9, 89, 18, 10, "fk", 2)
    f.box(236, 78, 60, 32, [("tap", 13, True, "t")], rx=6)
    f.text(431, 82, "binary messages", 13, False, "q", "middle")
    f.text(431, 118, "not delayed, blocked or altered", 13, True, "g", "middle")
    # first-bite
    f.rect(40, 180, 680, 162, "bxa", 12)
    # the copy
    f.line([(266, 110), (266, 196)], "lna", head="fa")
    f.text(280, 148, "copy of every frame", 13, True, "a")
    f.text(280, 166, "one way: nothing is sent back", 13, False, "q")
    f.text(56, 206, "first-bite", 15, True, "a")
    f.text(56, 224, "never transmits", 13, False, "q")
    f.box(196, 196, 140, 58, [("Capture", 15, True, "t"), ("interface or file", 13, False, "q")])
    f.box(380, 196, 152, 58, [("Decode", 15, True, "t"), ("every field, 0.69 µs", 13, False, "q")])
    f.box(576, 196, 128, 58, [("Publish", 15, True, "t"), ("JSON record", 13, False, "q")])
    f.line([(336, 225), (380, 225)], "lna", head="fa")
    f.line([(532, 225), (576, 225)], "lna", head="fa")
    f.box(380, 280, 152, 50, [("Dictionary", 14, True, "t"), ("JSON files, not code", 13, False, "q")], "bxd")
    f.line([(456, 280), (456, 254)], "lnd", head="fl")
    f.text(56, 296, "340,000", 26, True, "a")
    f.text(56, 316, "packets per second on one core", 13, False, "q")
    # bus
    f.line([(640, 254), (640, 370)], "lna", head="fa")
    f.box(40, 370, 680, 34, [("ZeroMQ PUB socket: one JSON record per message", 14, True, "t")], "bxp")
    xs = [40, 177.5, 315, 452.5, 590]
    labels = [("Object storage", "S3, Azure Blob"), ("HTTP endpoint", "POST per record"),
              ("Zenoh", "data fabric"), ("Cursor-on-Target", "transform"), ("Your subscriber", "any ZeroMQ SUB")]
    for x, (head, sub) in zip(xs, labels):
        f.line([(x + 65, 404), (x + 65, 432)], "lna", head="fa")
        f.box(x, 432, 130, 52, [(head, 13, True, "t"), (sub, 13, False, "q")], pad=6)
    return f


# ---------------------------------------------------------------------------
# 2. tap-placement
# ---------------------------------------------------------------------------
def tap_placement():
    f = Fig("tap-placement", "Where the copy is taken decides what can be lost",
            "Three ways to give first-bite a copy of a link. A breakout tap has one monitor port per direction, "
            "so first-bite uses two capture interfaces and loses nothing at the tap. An aggregating tap merges "
            "both directions into one port and loses nothing at the tap. A switch mirror port carries both "
            "directions on one port, and the switch drops mirrored frames first when it is busy.", 414)
    cols = [
        ("Breakout tap", "tap", 2, [
            [("Link: ", True, "q"), ("no effect", False, "t")],
            [("Loss: ", True, "q"), ("none at the tap", False, "t")],
            [("Ports: ", True, "q"), ("one per direction", False, "t")],
            [("Use: ", True, "q"), ("if every frame matters", False, "t")]]),
        ("Aggregating tap", "tap", 1, [
            [("Link: ", True, "q"), ("no effect", False, "t")],
            [("Loss: ", True, "q"), ("none at the tap", False, "t")],
            [("Ports: ", True, "q"), ("one for both directions", False, "t")],
            [("Use: ", True, "q"), ("if every frame matters", False, "t")]]),
        ("Mirror port (SPAN)", "switch", 1, [
            [("Link: ", True, "q"), ("no effect on forwarding", False, "t")],
            [("Loss: ", True, "q"), ("when the switch is busy", False, "w")],
            [("Ports: ", True, "q"), ("one for both directions", False, "t")],
            [("Use: ", True, "q"), ("if no tap can be fitted", False, "t")]]),
    ]
    for k, (head, dev, ports, facts) in enumerate(cols):
        x0 = 24 + k * 246
        f.rect(x0, 60, 220, 336, "bxp", 10)
        f.text(x0 + 16, 86, head, 15, True, "t")
        # link A - device - B
        f.box(x0 + 16, 104, 44, 34, [("A", 14, True, "t")], rx=6)
        f.box(x0 + 160, 104, 44, 34, [("B", 14, True, "t")], rx=6)
        dw = 60
        f.line([(x0 + 60, 121), (x0 + 110 - dw / 2, 121)], "lnk")
        f.line([(x0 + 110 + dw / 2, 121), (x0 + 160, 121)], "lnk")
        f.box(x0 + 110 - dw / 2, 104, dw, 34, [(dev, 13, True, "t")], rx=6, pad=6)
        # monitor ports down to first-bite
        if ports == 2:
            f.line([(x0 + 96, 138), (x0 + 96, 196)], "lna", head="fa")
            f.line([(x0 + 124, 138), (x0 + 124, 196)], "lna", head="fa")
            f.text(x0 + 86, 170, "A to B", 13, False, "q", "end")
            f.text(x0 + 134, 170, "B to A", 13, False, "q")
            sub = "2 capture interfaces"
        else:
            f.line([(x0 + 110, 138), (x0 + 110, 196)], "lna", head="fa")
            f.text(x0 + 120, 170, "A to B, B to A", 13, False, "q")
            sub = "1 capture interface"
        f.box(x0 + 30, 196, 160, 52, [("first-bite", 14, True, "a"), (sub, 13, False, "q")], "bxa")
        f.line([(x0 + 16, 268), (x0 + 204, 268)], "lnr")
        for j, parts in enumerate(facts):
            f.rich(x0 + 14, 294 + j * 26, parts, 13, maxw=194)
    return f


# ---------------------------------------------------------------------------
# 3. frame-to-record
# ---------------------------------------------------------------------------
PAYLOAD = "0200174b583700fb2e18bcda9b4236000004030201beef"


def load_observation():
    path = os.path.join(HERE, "..", "..", "dictionaries", "weather-station", "WX")
    msg = json.load(open(os.path.join(path, "messages", "Observation.json"), encoding="utf-8"))
    meta = json.load(open(os.path.join(path, "metadata.json"), encoding="utf-8"))
    return msg["fields"], meta


def decode_field(bits, spec, type_names):
    """Decode one field from a bit string, independently of the decoder, to label the figure."""
    import struct
    raw = bits[spec["start"]:spec["stop"]]
    kind = spec.get("type", "uint")
    width = spec["stop"] - spec["start"]
    if kind in ("ascii", "bytes"):
        data = int(raw, 2).to_bytes(width // 8, "big")
        return '"%s"' % (data.rstrip(b"\x00 ").decode("ascii") if kind == "ascii" else data.hex())
    if kind == "float":
        return repr(struct.unpack(">f" if width == 32 else ">d", int(raw, 2).to_bytes(width // 8, "big"))[0])
    value = int(raw, 2)
    if spec.get("byte_order") == "little":
        value = int.from_bytes(value.to_bytes(width // 8, "big"), "little")
    if kind == "int" and value >= 1 << (width - 1):
        value -= 1 << width
    if "values" in spec and str(value) in spec["values"]:
        return '"%s"' % spec["values"][str(value)]
    if "scale" in spec:
        return repr(round(value * spec["scale"] + spec.get("offset", 0), 6))
    return str(value)


def frame_to_record():
    fields, _ = load_observation()
    data = bytes.fromhex(PAYLOAD)
    bits = "".join(f"{b:08b}" for b in data)
    order = sorted(fields, key=lambda k: fields[k]["start"])
    assert fields[order[-1]]["stop"] == len(bits) == 184
    f = Fig("frame-to-record", "A dictionary entry turns a bit range into a named value",
            "The 23 bytes of a weather-station Observation message, 0200174b583700fb2e18bcda9b4236000004030201beef, "
            "with the bit range of each of its nine fields marked. A table gives the dictionary entry for each "
            "field (start bit, stop bit, type, options) and the value it decodes to: message_type Observation, "
            "message_length 23, station_id KX7, temperature -12.34, pressure 101325, wind_direction 271.5, "
            "humidity 45.5, sample_serial 16909060, checksum beef.", 500)
    x0, cw, y0, ch = 35, 30, 92, 34
    per_bit = cw / 8
    f.text(x0, 64, "Observation payload: 23 bytes in hex, with the start bit of each field above", 13, False, "q", role="note")
    # nibble cells, tinted by field
    def field_at(bit):
        for i, name in enumerate(order):
            if fields[name]["start"] <= bit < fields[name]["stop"]:
                return i
        raise AssertionError(bit)
    for nib in range(len(data) * 2):
        i = field_at(nib * 4)
        x = x0 + nib * (cw / 2)
        f.rect(x, y0, cw / 2, ch, f"f{i % 4}", 0)
        f.text(x + cw / 4, y0 + 22, PAYLOAD[nib], 14, False, "t", "middle", mono=True)
    for b in range(1, len(data)):
        x = x0 + b * cw
        f.line([(x, y0), (x, y0 + ch)], "lnr")
    # field boundaries, start bits, brackets and numbers
    for i, name in enumerate(order):
        s, e = fields[name]["start"], fields[name]["stop"]
        xs, xe = x0 + s * per_bit, x0 + e * per_bit
        f.text(xs, y0 - 8, str(s), 13, False, "q", "middle" if s else "start")
        f.raw(f'<path d="M{n(xs + 3)} {n(y0 + ch + 6)}v5H{n(xe - 3)}v-5" class="ln s{i % 4}" style="stroke-width:1.5" data-lint="nohead"/>')
        f.disc((xs + xe) / 2, y0 + ch + 28, 10, f"c{i % 4}", i + 1)
    f.text(x0 + 184 * per_bit, y0 - 8, "184", 13, False, "q", "end")
    # the table
    top = 210
    cols = {"num": 47, "field": 68, "start": 232, "stop": 282, "type": 304, "opt": 366, "val": 560, "unit": 672}
    f.text(cols["field"], top - 22, "Dictionary entry: WX/messages/Observation.json", 13, True, "t", role="note")
    f.text(cols["val"], top - 22, "Decoded record", 13, True, "t")
    f.rect(cols["val"] - 14, top - 8, 725 - cols["val"] + 14, 9 * 28 + 36, "bxa", 8)
    for key, label, anchor in (("field", "field", "start"), ("start", "start", "end"), ("stop", "stop", "end"),
                               ("type", "type", "start"), ("opt", "options", "start"),
                               ("val", "value", "start"), ("unit", "unit", "start")):
        f.text(cols[key], top + 12, label, 13, False, "q", anchor)
    f.line([(35, top + 22), (cols["val"] - 26, top + 22)], "lnr")
    for i, name in enumerate(order):
        spec = fields[name]
        y = top + 44 + i * 28
        f.disc(cols["num"], y - 4.6, 10, f"c{i % 4}", i + 1)
        f.text(cols["field"], y, name, 13, True, "t", mono=True)
        f.text(cols["start"], y, str(spec["start"]), 13, False, "t", "end", mono=True)
        f.text(cols["stop"], y, str(spec["stop"]), 13, False, "t", "end", mono=True)
        kind = spec.get("type", "uint")
        f.text(cols["type"], y, kind, 13, False, "t", mono=True)
        opts = []
        if "scale" in spec:
            opts.append(f"scale {spec['scale']}")
        if "byte_order" in spec:
            opts.append(f"byte_order {spec['byte_order']}")
        if "values" in spec:
            raw = int(bits[spec["start"]:spec["stop"]], 2)
            opts.append(f"values: {raw} is {spec['values'][str(raw)]}")
        if (spec["stop"] - spec["start"]) % 8:
            opts.append("not byte-aligned")
        f.text(cols["opt"], y, ", ".join(opts), 13, False, "t", maxw=cols["val"] - 20 - cols["opt"])
        f.text(cols["val"], y, decode_field(bits, spec, None), 13, True, "t", mono=True, maxw=cols["unit"] - cols["val"] - 6)
        unit = spec.get("unit", "")
        if unit:
            f.text(cols["unit"], y, unit, 13, False, "q")
        if i < len(order) - 1:
            f.line([(35, y + 10), (cols["val"] - 26, y + 10)], "lnr")
    return f


# ---------------------------------------------------------------------------
# 4. components
# ---------------------------------------------------------------------------
def components():
    f = Fig("components", "Six crates: frames go in on the left, records come out on the right",
            "Frames come from a network interface, a capture file or a UDP socket into the mind binary. Its "
            "source hands each frame to its pipeline, which passes the payload to the bite library; bite "
            "decodes it against the dictionary and returns typed fields. The pipeline hands a record to the "
            "flow library, whose ZeroMQ publisher feeds sinks and transforms; the Cursor-on-Target transform "
            "uses the cotify library. The datamodels crate holds the record type and logger the log format.", 444)
    # sources
    f.text(24, 76, "Frame sources", 13, True, "q")
    srcs = [("Network interface", "libpcap"), ("Capture file", "pcap, pcapng"), ("UDP socket", "needs no privileges")]
    for k, (head, sub) in enumerate(srcs):
        y = 88 + k * 56
        f.box(24, y, 134, 50, [(head, 13, True, "t"), (sub, 13, False, "q")], pad=6)
    # mind
    f.rect(190, 60, 236, 220, "bxa", 12)
    for k in range(3):
        f.line([(158, 113 + k * 56), (174, 113 + k * 56)], "lna", nohead=True)
    f.line([(174, 113), (174, 225)], "lna", nohead=True)
    f.line([(174, 126), (206, 126)], "lna", head="fa")
    f.text(206, 86, "mind", 15, True, "a")
    f.text(246, 86, "binary", 13, False, "q")
    f.box(206, 100, 204, 52, [("Source", 14, True, "t"), ("the next frame", 13, False, "q")])
    f.box(206, 196, 204, 52, [("Pipeline", 14, True, "t"), ("frame to record", 13, False, "q")])
    f.line([(308, 152), (308, 196)], "lna", head="fa")
    f.text(320, 179, "frame", 13, False, "q")
    # bite and dictionary
    f.box(24, 322, 134, 56, [("Dictionary", 14, True, "t"), ("JSON, read at start", 13, False, "q")], "bxd", pad=6)
    f.box(190, 322, 236, 56, [("bite", 15, True, "a"), ("library and CLI: the decoder", 13, False, "q")], "bxa")
    f.line([(158, 350), (190, 350)], "lnd", head="fl")
    f.line([(308, 248), (308, 322)], "lna", head="fa", start_head=True)
    f.text(320, 305, "payload in, fields out", 13, False, "q")
    # flow
    f.rect(480, 60, 256, 220, "bxa", 12)
    f.text(496, 86, "flow", 15, True, "a")
    f.text(532, 86, "library", 13, False, "q")
    f.box(496, 94, 224, 50, [("Sinks", 14, True, "t"), ("storage, HTTP, Zenoh, file", 13, False, "q")])
    f.box(496, 166, 224, 50, [("ZeroMQ PUB", 14, True, "t"), ("topic interface/message", 13, False, "q")])
    f.box(496, 236, 224, 34, [("Transforms", 14, True, "t")])
    f.line([(608, 166), (608, 144)], "lna", head="fa")
    f.line([(608, 216), (608, 236)], "lna", head="fa")
    f.line([(410, 206), (496, 206)], "lna", head="fa")
    f.text(453, 196, "record", 13, True, "a", "middle")
    # cotify
    f.box(480, 322, 256, 56, [("cotify", 15, True, "a"), ("library: Cursor-on-Target XML", 13, False, "q")], "bxa")
    f.line([(608, 270), (608, 322)], "lna", head="fa", start_head=True)
    f.text(620, 305, "record, mapping", 13, False, "q")
    # shared crates
    f.box(24, 394, 402, 34, [("datamodels: the record type that crosses crate boundaries", 13, False, "t")], "bxp")
    f.box(480, 394, 256, 34, [("logger: the log line format", 13, False, "t")], "bxp")
    return f


# ---------------------------------------------------------------------------
# 5. performance
# ---------------------------------------------------------------------------
DECODE_US = 0.69          # docs/performance.md: decode one message
PIPELINE_US = 2.9         # docs/performance.md: whole pipeline per packet
FIRST_RATE = 194          # docs/performance.md: first implementation, messages per second
NOW_RATE = 1_700_000      # docs/performance.md: now, on the same recorded messages


def performance():
    f = Fig("performance", "A link carrying 1,000 messages a second uses 0.3% of one core",
            "Three measurements on one core. Decoder throughput on a logarithmic scale: the project's first "
            "decoder managed 194 messages per second and the current one 1,700,000 on the same recorded "
            "messages. Time per packet on a linear scale: 2.9 microseconds for the whole pipeline, of which "
            "decoding is 0.69. Share of one core by message rate, computed from 2.9 microseconds per packet: "
            "0.3 percent at 1,000 messages per second, 2.9 percent at 10,000 and 29 percent at 100,000.", 508)
    left, right = 176, 700
    # A: decoder throughput, log axis
    f.text(24, 72, "Decoder throughput, messages per second", 14, True, "t", role="note")
    f.text(736, 72, "logarithmic scale", 13, False, "q", "end")
    lo, hi = 2, 7
    xa = lambda v: left + (math.log10(v) - lo) / (hi - lo) * (right - left)
    for e, label in zip(range(lo, hi + 1), ("100", "1,000", "10,000", "100,000", "1 million", "10 million")):
        x = left + (e - lo) / (hi - lo) * (right - left)
        f.line([(x, 86), (x, 150)], "lnr")
        f.text(x, 168, label, 13, False, "q", "middle")
    f.text(24, 108, "First decoder", 13, False, "t")
    f.text(24, 138, "Current decoder", 13, True, "t")
    f.line([(xa(FIRST_RATE), 104), (xa(FIRST_RATE), 134)], "lnd", nohead=True)
    f.disc(xa(FIRST_RATE), 104, 6, "fl")
    f.text(xa(FIRST_RATE) + 14, 108, thousands(FIRST_RATE), 13, True, "t")
    f.line([(xa(FIRST_RATE), 134), (xa(NOW_RATE) - 7, 134)], "lna", nohead=True)
    f.disc(xa(NOW_RATE), 134, 7, "fa")
    f.text(xa(NOW_RATE) + 14, 138, thousands(NOW_RATE), 13, True, "a")
    ratio = round(NOW_RATE / FIRST_RATE, -2)
    f.text((xa(FIRST_RATE) + xa(NOW_RATE)) / 2, 126, f"about {thousands(int(ratio))} times faster", 13, False, "a", "middle", halo=True)
    # B: per-packet time, linear
    f.line([(24, 186), (736, 186)], "lnr")
    f.text(24, 214, "Time per packet, microseconds", 14, True, "t", role="note")
    f.text(736, 214, "linear scale", 13, False, "q", "end")
    xb = lambda us: left + us / 3.0 * (right - left)
    f.rect(left, 230, xb(PIPELINE_US) - left, 34, "bxp", 4)
    f.rect(left, 230, xb(DECODE_US) - left, 34, "fa", 4)
    f.text((left + xb(DECODE_US)) / 2, 252, f"decode {DECODE_US}", 13, True, "on", "middle")
    f.text(xb(DECODE_US) + 14, 252, "read, parse headers, build JSON, publish", 13, False, "t",
           maxw=xb(PIPELINE_US) - xb(DECODE_US) - 24)
    f.text(24, 244, "Whole pipeline", 13, True, "t")
    f.text(24, 262, f"{PIPELINE_US} µs", 13, False, "q")
    for us in range(0, 4):
        f.line([(xb(us), 264), (xb(us), 272)], "ln")
        f.text(xb(us), 288, str(us), 13, False, "q", "middle")
    # C: share of one core
    f.line([(24, 304), (736, 304)], "lnr")
    f.text(24, 332, "Share of one core, by message rate", 14, True, "t", role="note")
    f.text(736, 332, f"rate × {PIPELINE_US} µs, linear scale", 13, False, "q", "end")
    xc = lambda pct: left + pct / 100 * (right - left)
    for k, rate in enumerate((1_000, 10_000, 100_000)):
        y = 346 + k * 34
        pct = rate * PIPELINE_US / 1e6 * 100
        f.text(24, y + 19, f"{thousands(rate)} per second", 13, k == 0, "t")
        f.rect(left, y, right - left, 28, "bxp", 4)
        f.rect(left, y, max(xc(pct) - left, 1.5), 28, "fa", 0 if pct < 1 else 4)
        shown = f"{pct:.1f}%" if pct < 10 else f"{pct:.0f}%"
        f.text(xc(pct) + 10, y + 19, shown, 13, True, "a")
    for pct in (0, 25, 50, 75, 100):
        f.line([(xc(pct), 442), (xc(pct), 450)], "ln")
        f.text(xc(pct), 466, f"{pct}%", 13, False, "q", "middle")
    f.text(24, 492, "Measured on one core of an AMD Ryzen 7 8845HS, release build. Commands: docs/performance.md",
           13, False, "q", role="note")
    return f


# ---------------------------------------------------------------------------
# 6. one-decode-many-consumers
# ---------------------------------------------------------------------------
def one_decode():
    count = 4
    f = Fig("one-decode-many-consumers", "One decode serves every consumer, and none of them touches the link",
            "Left, point to point: each of four consumers implements the binary interface and holds its own "
            "connection to native system A, so there are four parsers and four connections for A to serve. "
            "Right, with a tap: first-bite decodes once and publishes, the four consumers subscribe and read "
            "JSON, and there is one decoder and no connection to A or B.", 406)
    ys = [84 + k * 62 for k in range(count)]
    # left
    f.text(24, 70, "Point to point", 14, True, "t")
    f.box(24, 84, 86, 46, [("System A", 13, True, "t")], pad=6)
    f.box(24, 274, 86, 46, [("System B", 13, True, "t")], pad=6)
    f.line([(67, 130), (67, 274)], "lnk")
    for k, y in enumerate(ys):
        f.box(232, y, 128, 50, [(f"Consumer {k + 1}", 13, True, "t"), ("own binary parser", 13, False, "w")], pad=6)
        f.line([(110, 98 + k * 6), (232, y + 25)], "ln", head="fl")
    f.text(171, 92, "binary", 13, False, "q", "middle")
    f.rich(24, 362, [(f"{count} parsers", True, "w"), (" to write and keep in step", False, "t")], 13)
    f.rich(24, 382, [(f"{count} connections", True, "w"), (" for System A to serve", False, "t")], 13)
    f.line([(380, 60), (380, 388)], "lnr")
    # right
    f.text(400, 70, "Tap, decode once, publish", 14, True, "t")
    f.box(400, 84, 86, 46, [("System A", 13, True, "t")], pad=6)
    f.box(400, 274, 86, 46, [("System B", 13, True, "t")], pad=6)
    f.line([(443, 130), (443, 186)], "lnk")
    f.line([(443, 218), (443, 274)], "lnk")
    f.box(420, 186, 46, 32, [("tap", 13, True, "t")], rx=6, pad=4)
    f.box(500, 174, 92, 56, [("first-bite", 14, True, "a"), ("one decode", 13, False, "q")], "bxa", pad=6)
    f.line([(466, 202), (500, 202)], "lna", head="fa")
    f.line([(592, 202), (610, 202)], "lna", nohead=True)
    f.line([(610, ys[0] + 25), (610, ys[-1] + 25)], "lna", nohead=True)
    for k, y in enumerate(ys):
        f.line([(610, y + 25), (636, y + 25)], "lna", head="fa")
        f.box(636, y, 100, 50, [(f"Consumer {k + 1}", 13, True, "t"), ("reads JSON", 13, False, "q")], pad=6)
    f.rich(400, 362, [("1 decoder", True, "g"), (", driven by a dictionary", False, "t")], 13)
    f.rich(400, 382, [("0 connections", True, "g"), (" to System A or System B", False, "t")], 13)
    return f


# ---------------------------------------------------------------------------
# 7. beyond-ip
# ---------------------------------------------------------------------------
def beyond_ip():
    f = Fig("beyond-ip", "The decoder that reads IP messages also reads CAN frames",
            "A table of link types against decoding and capture. IP networks: decoding is built and measured "
            "at 1.45 million messages per second, and capture is built for an interface, a capture file and a "
            "UDP socket. CAN bus: decoding is built and measured at 1.40 million frames per second with the "
            "same decoder, and a SocketCAN capture source is designed, not built. MIL-STD-1553 and ARINC 429 "
            "sources are not built, and neither is importing a DBC file as a dictionary.", 348)
    c1, c2, c3 = 24, 232, 494
    f.text(c1, 76, "Link", 13, True, "q")
    f.text(c2, 76, "Decode: bite and a dictionary", 13, True, "q")
    f.text(c3, 76, "Capture: a source in mind", 13, True, "q")
    rows = [90, 162, 244]
    f.text(c1, 116, "IP networks", 15, True, "t")
    f.text(c1, 135, "UDP and TCP payloads", 13, False, "q")
    f.text(c1, 188, "CAN bus", 15, True, "t")
    f.text(c1, 207, "frame identifier, lsb0 signals", 13, False, "q")
    f.text(c1, 270, "MIL-STD-1553, ARINC 429", 15, True, "t")
    f.text(c1, 289, "avionics data buses", 13, False, "q")
    # one decoder across the first two rows
    f.rect(c2, rows[0], 246, 132, "bxa", 8)
    f.line([(c2 + 14, 156), (c2 + 232, 156)], "lnr")
    f.rich(c2 + 14, 116, [("Built", True, "g"), (": weather-station, toot", False, "t")], 13, maxw=218)
    f.text(c2 + 14, 135, "measured 1.45 million messages/s", 13, False, "q", maxw=218)
    f.rich(c2 + 14, 188, [("Built", True, "g"), (": vehicle-can", False, "t")], 13, maxw=218)
    f.text(c2 + 14, 207, "measured 1.40 million frames/s", 13, False, "q", maxw=218)
    # capture
    f.rect(c3, rows[0], 242, 60, "bx", 8)
    f.text(c3 + 14, 116, "Built", 13, True, "g")
    f.text(c3 + 14, 135, "interface, capture file, UDP socket", 13, False, "q", maxw=218)
    f.rect(c3, rows[1], 242, 60, "bxd", 8)
    f.text(c3 + 14, 188, "Designed, not built", 13, True, "w")
    f.text(c3 + 14, 207, "SocketCAN source", 13, False, "q")
    f.rect(c2, rows[2], 504, 60, "bxm", 8)
    f.text(c2 + 14, 270, "Not built", 13, True, "mu")
    f.text(c2 + 14, 289, "capture sources for these buses", 13, False, "q", maxw=472)
    f.text(c1, 328, "Also not built: importing a CAN database (DBC) file as a dictionary.", 13, False, "q", role="note")
    return f


# ---------------------------------------------------------------------------
# 8. platforms
# ---------------------------------------------------------------------------
def platforms():
    f = Fig("platforms", "One archive per platform, and nothing to install on the target",
            "Where first-bite runs. The standard Linux build needs glibc 2.31 or later and is built and run on "
            "x86-64 in Ubuntu 20.04, 22.04, 24.04 and 26.04. The self-contained Linux build is fully static, "
            "needs only a Linux kernel 3.2 or later, and is built and run on x86-64. Archives for 64-bit Arm "
            "Linux, macOS and Windows are built in CI and not run locally. The container image replays a "
            "capture file, measured. The decoder library is compile-checked for iOS, Android, Windows, "
            "WebAssembly and 64-bit Arm Linux; no application is built. A build with no internet access "
            "uses vendored dependencies.", 364)
    cards = [
        ("Standard Linux", "bx", "g", "built and run",
         ["Ubuntu 20.04 to 26.04, x86-64", "glibc 2.31 or later", "nothing to install"]),
        ("Self-contained Linux", "bx", "g", "built and run",
         ["fully static (musl), x86-64", "Linux kernel 3.2 or later", "Alpine, busybox, scratch"]),
        ("Arm, macOS, Windows", "bxd", "w", "built in CI; not run locally",
         ["64-bit Arm Linux, both builds", "macOS: Apple silicon and Intel", "Windows: the bite tool only"]),
        ("Container image", "bx", "g", "measured",
         ["mind as the entry point", "runs as a non-root user", "replays a capture file"]),
        ("Decoder library", "bxd", "w", "compile-checked",
         ["iOS, Android, WebAssembly", "Windows, 64-bit Arm Linux", "no application built"]),
        ("No internet access", "bx", "g", "built",
         ["make vendor, copy the tree", "cargo build --offline", "on the offline machine"]),
    ]
    for k, (head, cls, scls, status, lines) in enumerate(cards):
        x, y, w, h = 24 + (k % 3) * 242, 60 + (k // 3) * 146, 228, 132
        f.rect(x, y, w, h, cls, 8)
        f.text(x + 16, y + 30, head, 15, True, "t", maxw=w - 32)
        f.line([(x + 16, y + 42), (x + w - 16, y + 42)], "lnr")
        for j, ln in enumerate(lines):
            f.text(x + 16, y + 63 + j * 18, ln, 13, False, "q" if j else "t", maxw=w - 32)
        f.text(x + 16, y + 119, status, 13, True, scls, maxw=w - 32)
    return f


# ---------------------------------------------------------------------------
# 9. record-flow
# ---------------------------------------------------------------------------
def record_flow():
    f = Fig("record-flow", "A record is published once; each reader takes the topics it filters for",
            "mind hands each record to the publishers, a ZeroMQ PUB socket on tcp://127.0.0.1:12000 by "
            "default. A subscriber reads the socket, keeps the records on its topics and forwards each to "
            "one sink: object storage, HTTP, a local file or Zenoh. A subpub reads the socket the same way, "
            "transforms each record into Cursor-on-Target and publishes the result on a socket of its own, "
            "tcp://127.0.0.1:12001 by default. Any other ZeroMQ SUB socket can read either socket.", 392)
    rows = [60, 152, 244]
    f.box(24, 152, 90, 72, [("mind", 15, True, "a"), ("a record", 13, False, "q"), ("per packet", 13, False, "q")], "bxa", pad=6)
    f.box(150, 152, 160, 72, [("Publishers", 14, True, "t"), ("ZeroMQ PUB socket", 13, False, "q"),
                              ("tcp://127.0.0.1:12000", 13, False, "q")], pad=6)
    f.line([(114, 188), (150, 188)], "lna", head="fa")
    f.line([(310, 188), (330, 188)], "lna", nohead=True)
    f.line([(330, rows[0] + 36), (330, rows[2] + 36)], "lna", nohead=True)
    for y in rows:
        f.line([(330, y + 36), (350, y + 36)], "lna", head="fa")
    f.box(350, rows[0], 170, 72, [("Subscriber", 14, True, "t"), ("keeps its topics, such as", 13, False, "q"),
                                  ("WX/Observation", 13, False, "t", True)], pad=6)
    f.box(556, rows[0], 180, 72, [("Sink", 14, True, "t"), ("object storage, HTTP,", 13, False, "q"),
                                  ("local file, Zenoh", 13, False, "q")], pad=6)
    f.line([(520, rows[0] + 36), (556, rows[0] + 36)], "lna", head="fa")
    f.box(350, rows[1], 170, 72, [("Subpub", 14, True, "t"), ("keeps its topics, such as", 13, False, "q"),
                                  ("WX/Position", 13, False, "t", True)], pad=6)
    f.box(556, rows[1], 180, 72, [("Transform", 14, True, "t"), ("record to", 13, False, "q"),
                                  ("Cursor-on-Target", 13, False, "q")], pad=6)
    f.line([(520, rows[1] + 36), (556, rows[1] + 36)], "lna", head="fa")
    f.line([(646, rows[1] + 72), (646, rows[2])], "lna", head="fa")
    f.box(556, rows[2], 180, 72, [("Its own socket", 14, True, "t"), ("ZeroMQ PUB socket", 13, False, "q"),
                                  ("tcp://127.0.0.1:12001", 13, False, "q")], pad=6)
    f.box(350, rows[2], 170, 72, [("Your subscriber", 14, True, "t"), ("any ZeroMQ SUB socket", 13, False, "q"),
                                  ("on either PUB socket", 13, False, "q")], "bxd", pad=6)
    f.text(24, 348, "A message on a socket is <topic>|<record JSON>, and the topic is interface/message.",
           13, False, "q", role="note")
    f.text(24, 368, "A filter matches a topic exactly; * keeps everything. A subpub publishes on the input topic plus a suffix.",
           13, False, "q", role="note")
    return f


# ---------------------------------------------------------------------------
# 10. loss-counters
# ---------------------------------------------------------------------------
def loss_counters():
    f = Fig("loss-counters", "Three counters say where a frame was lost",
            "A frame passes from the tap to the network card, the kernel capture buffer, the queue inside mind "
            "and the pipeline. mind counts losses at three of them. By interface: the card or driver discarded "
            "frames; raise the receive ring. Dropped by kernel: the capture buffer was full; raise --buffer-mb "
            "or add a filter. In queue: the pipeline was behind because a sink is blocking; check the "
            "subscriber log lines.", 290)
    stages = [("Tap", "monitor port"), ("Network card", "receive ring"), ("Kernel buffer", "32 MiB by default"),
              ("Queue in mind", "65,536 frames"), ("Pipeline", "decode, publish")]
    for k, (head, sub) in enumerate(stages):
        x = 20 + k * 148
        f.box(x, 64, 128, 56, [(head, 14, True, "t"), (sub, 13, False, "q")], "bxa" if k >= 3 else "bx", pad=6)
        if k:
            f.line([(x - 20, 92), (x, 92)], "lna", head="fa")
    cards = [(24, 232, "by interface", "card or driver dropped frames", "raise the receive ring"),
             (266, 380, "dropped by kernel", "the capture buffer was full", "raise --buffer-mb, add a filter"),
             (508, 528, "in queue", "pipeline behind: a sink blocks", "check the subscriber log lines")]
    for x, src, name, why, do in cards:
        cx = x + 114
        f.line([(src, 120), (src, 140), (cx, 140), (cx, 160)] if src != cx else [(src, 120), (src, 160)], "lnd", nohead=True)
        f.rect(x, 160, 228, 82, "bxw", 8)
        f.text(x + 16, 186, name, 14, True, "w", maxw=196)
        f.text(x + 16, 208, why, 13, False, "t", maxw=196)
        f.text(x + 16, 228, do, 13, False, "q", maxw=196)
    f.text(24, 270, "mind logs the three counts for each interface every --stats-interval seconds (default 60) and at shutdown.",
           13, False, "q", role="note")
    return f


FIGURES = [how_it_works, tap_placement, frame_to_record, components, performance, one_decode, beyond_ip, platforms,
           record_flow, loss_counters]


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for make in FIGURES:
        fig = make()
        for theme, suffix in (("light", ""), ("dark", "-dark")):
            path = os.path.join(HERE, f"{fig.name}{suffix}.svg")
            svg = fig.render(theme)
            if check:
                if not os.path.exists(path) or open(path, encoding="utf-8").read() != svg:
                    stale.append(os.path.basename(path))
            else:
                with open(path, "w", encoding="utf-8") as out:
                    out.write(svg)
    if check and stale:
        raise SystemExit("out of date, run docs/img/build.py: " + ", ".join(stale))
    print(("checked" if check else "wrote") + f" {len(FIGURES) * 2} files")


if __name__ == "__main__":
    main()
