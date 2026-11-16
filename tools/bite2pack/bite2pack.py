#!/usr/bin/env python3
"""bite2pack: turn a first-bite dictionary into goat-transformers packs.

A pack is the data file the goat-transformers engine reads to decode and encode
one format (https://github.com/USAREUR-AF/goat-transformers, spec/engine-spec.md,
written against spec 0.8). This tool writes one pack for each interface of a
dictionary, so that the `xf` transformer of `flow` can convert a record's
payload to any other format that has a pack. flow/README.md describes the
transformer; tools/bite2pack/README.md describes this tool.

Reads   <dictionary>/dictionary.json, <INTERFACE>/metadata.json,
        <INTERFACE>/messages/*.json, vectors/*.json
        --bindings <json>   which fields carry the shared keys uid, lat, lon, hae
Writes  <out>/model/<module>.def            one model module per dictionary
        <out>/packs/<format>/pack.def       one pack per interface
        <out>/packs/<format>/pack.md        what is mapped and what is not
        <out>/packs/<format>/vectors/*.json conformance vectors
        <out>/index.json                    dictionary and interface to format
        <report>/<dictionary>.map.json      with --report: every field's mapping

Python 3 standard library only. Re-runnable and deterministic: every output is
rewritten from the dictionary, and a dictionary that has not changed gives the
same bytes. `--check` writes nothing and exits 1 when <out> differs from what
would be written.

Usage:
  bite2pack.py --out <dir> [--check] [--bindings <json>] [--core-def <core.def>]
               [--report <dir>] <dictionary directory> ...

How a dictionary field becomes a `field` line (`wire bits`, `word_bytes 1`,
locator `0:<bit offset>:<bits>`; every line is `optional`, because bite decodes
a truncated payload up to the cut):

  uint / int                     uint / twos                      model int
  with scale or offset           uscale / sscale                  model float
  float                          f32 / f64                        model float
  ascii                          ascii trim                       model string
  bool, at most 8 bits           uint via a table of every value  model string, `true` / `false`
  values, at most 8 bits, no     uint / twos via a table of       model string: the label, or the
    scale                          every value                      decimal text of a value with none
  bytes, 8 bits                  uint via a table of every value  model string: two hexadecimal digits
  bytes, 16 to 64 bits           uint                             model int (the pack language has
                                                                    no transform to hexadecimal text)
  byte_order little under msb0   the same, with `swap`

An interface that identifies messages by `frame_id` gives a pack whose message
is 4 bytes of frame identifier followed by the payload.

After writing, the tool re-reads its own output from disk and checks it:
grammar, key declarations, type agreement, bit ranges, every vector through a
small codec written here from the specification, and the dictionary's own
vectors against that codec. Any failed check exits non-zero.
"""
import argparse
import filecmp
import glob
import json
import math
import os
import re
import shutil
import struct
import sys
import tempfile
from collections import OrderedDict
from decimal import Decimal
from fractions import Fraction

FRAME_ID_BYTES = 4
# model/core.def of goat-transformers; --core-def reads the file instead
CORE_KEYS = OrderedDict([
    ("uid", "string"), ("type", "string"), ("time_ms", "int"), ("lat", "float"),
    ("lon", "float"), ("hae", "float"), ("ce", "float"), ("le", "float"),
    ("callsign", "string"), ("course", "float"), ("speed_ms", "float"), ("how", "string"),
    ("stale_ms", "int"),
])
# the unit a dictionary field must state to carry a shared key
BINDABLE = OrderedDict([("uid", ("string", None)), ("lat", ("float", "deg")), ("lon", ("float", "deg")),
                        ("hae", ("float", "m")), ("callsign", ("string", None))])
YIELDS = {"uint": "int", "twos": "int", "uscale": "float", "sscale": "float",
          "f32": "float", "f64": "float", "ascii": "string"}
OPTIONS = ("swap", "trim", "optional")
TABLE_MAX_BITS = 8           # a value table lists every value of a field this wide or narrower
MAX_DECIMALS = 15            # bite/src/dictionary.rs
MAX_PAYLOAD_BITS = 65535 * 8  # bite/src/limits.rs


class GenError(Exception):
    pass


def die(msg):
    raise GenError(msg)


# ---------------------------------------------------------------- small helpers

def slug(text):
    s = re.sub(r"[^a-z0-9]+", "_", text.lower()).strip("_")
    return s or "x"


def plain(x):
    """Decimal text of a number in plain notation (no exponent), shortest round trip.
    The same text Rust's `f64` Display writes, without a trailing `.0`."""
    if isinstance(x, int):
        return str(x)
    if x == 0:
        return "0"
    if x == int(x) and abs(x) < 1e15:
        return str(int(x))
    t = format(Decimal(repr(x)), "f")
    if "." in t:
        t = t.rstrip("0").rstrip(".")
    return t


def parse_int(text):
    """bite/src/schema.rs parse_int: decimal or 0x hexadecimal with at most one sign."""
    if not isinstance(text, str):
        return None
    t = text.strip()
    neg = False
    if t.startswith("-"):
        neg, t = True, t[1:]
    elif t.startswith("+"):
        t = t[1:]
    if t[:1] in "+-":
        return None
    try:
        if t[:2] in ("0x", "0X"):
            if t[2:3] in ("+", "-", ""):
                return None
            v = int(t[2:], 16)
        else:
            if not re.fullmatch(r"[0-9]+", t):
                return None
            v = int(t)
    except ValueError:
        return None
    return -v if neg else v


def decimal_places(number):
    """bite/src/dictionary.rs decimal_places for one number: places of the shortest
    decimal that reads back as the same double; None above MAX_DECIMALS."""
    if number is None:
        return 0
    if math.isinf(number) or math.isnan(number):
        return None
    t = plain(float(number))
    places = len(t.split(".")[1]) if "." in t else 0
    return places if places <= MAX_DECIMALS else None


def round_half_away_f(x):
    """f64::round: nearest integer, halves away from zero, exact."""
    r = float(math.trunc(x))
    if abs(x - r) >= 0.5:
        r += math.copysign(1.0, x)
    return r


def bite_scaled(raw, scale, offset):
    """bite Field::scaled for an integer field: raw * scale + offset, rounded to the
    decimal places scale and offset are written with."""
    value = float(raw) * (1.0 if scale is None else scale) + (0.0 if offset is None else offset)
    ps, po = decimal_places(scale), decimal_places(offset)
    if ps is None or po is None:
        return value
    power = 10.0 ** max(ps, po)
    shifted = round_half_away_f(value * power)
    return shifted / power if abs(shifted) < 1e15 else value


def factor_text(scale):
    """Scale factor (engine-spec 0.5 ruling 1). A short decimal, which bite rounds
    to, is written as the fraction in lowest terms so the engine divides and gets
    the nearest double to the exact value. A power of two is written 1/2^n. Any
    other double is written as its shortest decimal, which the engine reads back
    as the same double and multiplies by, as bite does."""
    s = float(scale)
    if s == int(s) and abs(s) < 1e15:
        return str(int(s))
    if s > 0:
        if decimal_places(s) is not None:
            fr = Fraction(Decimal(plain(s)))
            if float(fr.denominator) == fr.denominator and fr.numerator < 2 ** 53:
                return "%d/%d" % (fr.numerator, fr.denominator)
        fr = Fraction(s)
        if fr.numerator < 2 ** 53 and fr.denominator <= 2 ** 64:
            return "%d/%d" % (fr.numerator, fr.denominator)
    return plain(s)


def parse_factor(text):
    if "/" in text:
        a, b = text.split("/")
        return float(int(a)), float(int(b))
    return float(text), None


def scale_decode(v, fac, offset):
    """ruling 1: v * factor (or v * num / den), then + offset, in doubles, in this order."""
    num, den = fac
    x = float(v) * num
    if den is not None:
        x = x / den
    return x + offset


def scale_encode(x, fac, offset):
    num, den = fac
    q = (x - offset) / num if den is None else (x - offset) * den / num
    return int(round_half_away_f(q))


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)


def q(text):
    """A double-quoted pack value: no double quote inside (ruling 6), one line."""
    return '"%s"' % re.sub(r"\s+", " ", str(text).replace('"', "'")).strip()


# ---------------------------------------------------------------- reading a dictionary as bite does

class Field:
    pass


def load_json(path):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh, object_pairs_hook=OrderedDict)


def build_field(name, spec, iface):
    """bite/src/dictionary.rs build_field. Returns (Field, None) or (None, why bite leaves it out)."""
    start, stop = spec.get("start"), spec.get("stop")
    if not isinstance(start, int) or not isinstance(stop, int):
        return None, "no integer start and stop"
    if stop <= start:
        return None, "bits [%d, %d) are empty or inverted" % (start, stop)
    if stop > MAX_PAYLOAD_BITS:
        return None, "bits reach past the payload bytes considered"
    width = stop - start
    aligned = start % 8 == 0 and stop % 8 == 0
    kind = spec.get("type")
    if kind is None:
        if spec.get("signed") is True:
            kind = "int"
        elif width > 64 and aligned:
            kind = "bytes"
        else:
            kind = "uint"
    if kind not in ("uint", "int", "float", "bool", "bytes", "ascii"):
        die("field %s: unknown type %r" % (name, kind))
    if kind in ("uint", "int", "bool") and width > 64:
        return None, "%d bits is wider than the 64 an integer field can hold" % width
    if kind == "float" and width not in (32, 64):
        return None, "a float field is 32 or 64 bits, not %d" % width
    if kind in ("bytes", "ascii") and not aligned:
        return None, "a bytes or ascii field must start and stop on a byte boundary"
    byte_order = spec.get("byte_order", iface["byte_order"])
    if byte_order == "little" and iface["bit_order"] == "msb0" and not aligned and kind not in ("bytes", "ascii"):
        return None, "little-endian byte order needs a byte-aligned field"
    f = Field()
    f.name, f.start, f.stop, f.width, f.aligned, f.kind = name, start, stop, width, aligned, kind
    f.byte_order = byte_order
    f.scale = float(spec["scale"]) if spec.get("scale") is not None else None
    f.offset = float(spec["offset"]) if spec.get("offset") is not None else None
    f.unit = spec.get("unit")
    f.description = spec.get("description")
    f.labels = OrderedDict()
    if kind in ("uint", "int", "bool"):
        for k, label in (spec.get("values") or {}).items():
            v = parse_int(k)
            if isinstance(label, bool):
                label = "true" if label else "false"
            elif isinstance(label, (int, float)):
                label = plain(label)
            if v is not None and isinstance(label, str):
                f.labels[v] = label
    return f, None


def load_dictionary(root):
    d = OrderedDict(name=os.path.basename(os.path.normpath(root)), root=root, spec={}, interfaces=OrderedDict(),
                    vectors=[], files=[])
    p = os.path.join(root, "dictionary.json")
    if os.path.isfile(p):
        d["spec"] = load_json(p)
        d["files"].append("dictionary.json")
    for iname in sorted(os.listdir(root)):
        meta_path = os.path.join(root, iname, "metadata.json")
        if not os.path.isfile(meta_path):
            continue
        meta = load_json(meta_path)
        d["files"].append("%s/metadata.json" % iname)
        mt = meta["message_type"]
        iface = OrderedDict(
            name=iname, meta=meta, bit_order=meta.get("bit_order", "msb0"),
            byte_order=meta.get("byte_order", "big"), source=mt.get("source", "payload"),
            dstart=mt.get("start", 0), dstop=mt.get("stop", 0),
            mask=parse_int(mt["mask"]) if mt.get("mask") is not None else None,
            names=OrderedDict(), messages=OrderedDict(), left_out=[])
        for k, mname in (mt.get("values") or {}).items():
            code = parse_int(k)
            if code is None or code < 0:
                die("%s: message_type value %r is not an unsigned integer" % (iname, k))
            iface["names"][code] = mname
        for mpath in sorted(glob.glob(os.path.join(glob.escape(os.path.join(root, iname, "messages")), "*.json"))):
            mname = os.path.splitext(os.path.basename(mpath))[0]
            mspec = load_json(mpath)
            d["files"].append("%s/messages/%s" % (iname, os.path.basename(mpath)))
            fields = []
            for fname, fspec in mspec.get("fields", {}).items():
                f, why = build_field(fname, fspec, iface)
                if f is None:
                    iface["left_out"].append(("%s/%s" % (mname, fname), why))
                else:
                    fields.append(f)
            fields.sort(key=lambda f: (f.start, f.name))
            iface["messages"][mname] = OrderedDict(name=mname, description=mspec.get("description"), fields=fields)
        d["interfaces"][iname] = iface
    for vpath in sorted(glob.glob(os.path.join(glob.escape(os.path.join(root, "vectors")), "*.json"))):
        v = load_json(vpath)
        v["_name"] = os.path.splitext(os.path.basename(vpath))[0]
        d["vectors"].append(v)
    if not d["interfaces"]:
        die("%s: no interface (no sub-directory with metadata.json)" % root)
    return d


# ---------------------------------------------------------------- bindings: which fields carry shared keys

def read_bindings(path):
    """{dictionary: {"<INTERFACE>/<message>": {shared key: field name}}}. A shared key
    is one of the `core` keys a field can carry: uid, lat, lon, hae, callsign."""
    if not path:
        return {}
    data = load_json(path)
    for dname, topics in data.items():
        if dname.startswith("_"):
            continue
        for topic, keys in topics.items():
            for core in keys:
                if core not in BINDABLE:
                    die("%s: %s %s binds `%s`; a field can carry %s" % (path, dname, topic, core, ", ".join(BINDABLE)))
    return data


# ---------------------------------------------------------------- planning: fields to transforms and keys

def table_for(f):
    """The value table of a `bool` field or a field with `values`: every raw value the
    field can hold, and the text bite writes for it. None when the field is wider than
    TABLE_MAX_BITS or a label cannot be written in a pack."""
    if f.width > TABLE_MAX_BITS:
        return None
    raws = range(-(1 << (f.width - 1)), 1 << (f.width - 1)) if f.kind == "int" else range(1 << f.width)
    rows = []
    for raw in sorted(raws, key=lambda v: (v < 0, abs(v))):
        if raw in f.labels:
            text = f.labels[raw]
        elif f.kind == "bool":
            text = "true" if raw else "false"
        else:
            text = str(raw)
        if text == "" or text != " ".join(text.split()) or '"' in text:
            return None
        rows.append((raw, text))
    # the engine reads a table whose model side is all numbers as a number table
    if all(re.fullmatch(r"-?[0-9]+(\.[0-9]+)?", t) for _, t in rows):
        return None
    return rows


def plan_field(f, iface):
    """-> dict(transform, yields, model, options, table, repr, reason). `transform` is None
    when the pack language cannot carry the field. `yields` is the type the transform
    gives; `model` the type of the key, which differs when a table stands between."""
    msb0 = iface["bit_order"] == "msb0"
    out = OrderedDict(transform=None, yields=None, model=None, options=[], table=None, repr="same", reason=None)

    def done(transform, yields, **more):
        out.update(transform=transform, yields=yields, model=yields)
        out.update(more)
        if out["table"]:
            out["model"] = "string"
        if f.byte_order == "little" and msb0 and f.width > 8 and f.kind not in ("bytes", "ascii"):
            out["options"].append("swap")
        if transform == "ascii":
            out["options"].append("trim")
        out["options"].append("optional")
        return out

    if f.kind == "ascii":
        if f.width > 2040:
            out["reason"] = "ascii field wider than 2040 bits"
            return out
        return done("ascii", "string")
    if f.kind == "bytes":
        if f.width > 64:
            out["reason"] = "bytes field wider than 64 bits: the pack language has no opaque-bytes value"
            return out
        if f.width == 8:
            # one byte: a table of every value gives the text bite writes
            return done("uint", "int", table=[(raw, "%02x" % raw) for raw in range(256)])
        return done("uint", "int", repr="bytes-as-integer")
    if f.kind == "float":
        if f.scale is not None or f.offset is not None:
            out["reason"] = "float field with scale or offset: no transform scales an f32/f64"
            return out
        return done("f32" if f.width == 32 else "f64", "float")
    scaled = f.scale is not None or f.offset is not None
    if scaled and f.kind != "bool":
        t = ("sscale:" if f.kind == "int" else "uscale:") + factor_text(1.0 if f.scale is None else f.scale)
        if f.offset:
            t += ":" + plain(f.offset)
        return done(t, "float", repr="label-not-carried" if f.labels else "same")
    transform = "twos" if f.kind == "int" else "uint"
    if f.kind == "bool" or f.labels:
        table = table_for(f)
        if table:
            return done(transform, "int", table=table)
        return done(transform, "int", repr="bool-as-integer" if f.kind == "bool" else "label-as-integer")
    return done(transform, "int")


def mask_runs(mask):
    """Contiguous runs of set bits: [(lo, n)], lowest first."""
    runs, bit = [], 0
    while mask >> bit:
        if (mask >> bit) & 1:
            lo = bit
            while (mask >> bit) & 1:
                bit += 1
            runs.append((lo, bit - lo))
        else:
            bit += 1
    return runs


def discriminator_whens(iface, code):
    """The `when` lines that hold exactly when bite selects `code`. None when no input can."""
    lsb = iface["bit_order"] == "lsb0"
    if iface["source"] == "payload":
        n = iface["dstop"] - iface["dstart"]
        if code >> n:
            return None
        return [(iface["dstart"], n, code)]
    bits = FRAME_ID_BYTES * 8
    mask = iface["mask"] if iface["mask"] is not None else (1 << bits) - 1
    if code & ~mask or mask >> bits:
        return None
    out = []
    for lo, n in mask_runs(mask):
        v = (code >> lo) & ((1 << n) - 1)
        out.append((lo, n, v) if lsb else (bits - lo - n, n, v))
    return out


def plan_dictionary(d, bindings, core_keys):
    """Kinds, fields and keys for every interface of one dictionary."""
    module = slug(d["name"])
    uses = {}                # field name -> list of (signature, iface, message)
    plans = OrderedDict()
    for iname, iface in d["interfaces"].items():
        base = FRAME_ID_BYTES * 8 if iface["source"] == "frame_id" else 0
        kinds, undefined, never = [], [], []
        used_names = set()
        for code in sorted(iface["names"]):
            mname = iface["names"][code]
            whens = discriminator_whens(iface, code)
            if whens is None:
                never.append((mname, "identifier %d has bits outside the mask or the identifier width" % code))
                continue
            several = sum(1 for n in iface["names"].values() if n == mname) > 1
            kname = slug(mname) + ("_%d" % code if several else "")
            if kname in used_names:
                kname += "_%d" % code
            used_names.add(kname)
            msg = iface["messages"].get(mname)
            kind = OrderedDict(name=kname, code=code, message=mname, whens=whens, defined=msg is not None,
                               description=msg["description"] if msg else None, fields=[], core=[])
            if msg is None:
                undefined.append(mname)
            else:
                for f in msg["fields"]:
                    p = plan_field(f, iface)
                    p.update(field=f, locator="0:%d:%d" % (base + f.start, f.width))
                    kind["fields"].append(p)
                    if p["transform"]:
                        sig = (p["transform"], p["model"], f.unit or "-", tuple(p["table"] or ()), f.kind)
                        uses.setdefault(f.name, []).append((sig, iname, mname, p))
            kinds.append(kind)
        for mname in iface["messages"]:
            if mname not in iface["names"].values():
                never.append((mname, "no message_type value names it; bite never selects it"))
        # the catch-all kind: bite gives an unnamed identifier a record with no fields
        catch = "unidentified"
        while catch in used_names:
            catch += "_x"
        plans[iname] = OrderedDict(format="bite-%s-%s" % (slug(d["name"]).replace("_", "-"), slug(iname).replace("_", "-")),
                                   iface=iface, base=base, kinds=kinds, undefined=undefined, never=never,
                                   catch=catch)
    # key names: a field name is one key when every use agrees; otherwise it carries its message
    keys = OrderedDict()
    for fname, us in uses.items():
        sigs = {u[0] for u in us}
        for sig, iname, mname, p in us:
            kname = slug(fname) if len(sigs) == 1 else "%s_%s" % (slug(mname), slug(fname))
            if kname in keys and keys[kname]["sig"] != sig:
                kname = "%s_%s_%s" % (slug(iname), slug(mname), slug(fname))
            if kname in keys and keys[kname]["sig"] != sig:
                die("key %s: two definitions" % kname)
            k = keys.setdefault(kname, OrderedDict(name=kname, sig=sig, type=p["model"], unit=sig[2],
                                                   field=p["field"], table=p["table"], where=[]))
            k["where"].append("%s/%s" % (iname, mname))
            p["key"] = "%s.%s" % (module, kname)
            p["table_name"] = kname if p["table"] else None
    # the discriminator of the catch-all kind
    for iname, pl in plans.items():
        iface = pl["iface"]
        kname = "%s_%s" % (slug(iname), "message_type_code" if iface["source"] == "payload" else "frame_id")
        if kname in keys:
            die("key %s is taken" % kname)
        if iface["source"] == "payload":
            loc = "0:%d:%d" % (iface["dstart"], iface["dstop"] - iface["dstart"])
            meaning = "the message identifier of interface %s, bits [%d, %d) of the payload" % (iname, iface["dstart"], iface["dstop"])
        else:
            loc = "0:0:%d" % (FRAME_ID_BYTES * 8)
            meaning = "the frame identifier of interface %s as it arrived, before the mask" % iname
        keys[kname] = OrderedDict(name=kname, sig=None, type="int", unit="-", field=None, where=[iname], meaning=meaning)
        pl["code_key"], pl["code_loc"] = "%s.%s" % (module, kname), loc
    # shared keys, as the bindings file states them
    bound = bindings.get(d["name"], {})
    topics = set()
    for iname, pl in plans.items():
        for kind in pl["kinds"]:
            topic = "%s/%s" % (iname, kind["message"])
            topics.add(topic)
            byname = {p["field"].name: p for p in kind["fields"]}
            for core, fname in bound.get(topic, {}).items():
                p = byname.get(fname)
                want_type, want_unit = BINDABLE[core]
                if core not in core_keys or core_keys[core] != want_type:
                    die("the core module does not declare `%s` as a %s" % (core, want_type))
                if p is None or not p["transform"]:
                    die("bindings: %s %s: `%s` names field `%s`, which the pack does not map" % (d["name"], topic, core, fname))
                if p["model"] != want_type:
                    die("bindings: %s %s: `%s` is a %s; field `%s` gives a %s" % (d["name"], topic, core, want_type, fname, p["model"]))
                if want_unit and p["field"].unit != want_unit:
                    die("bindings: %s %s: `%s` is in %s; field `%s` states unit %r" % (
                        d["name"], topic, core, want_unit, fname, p["field"].unit))
                add_core(kind, core, p, "field `%s`%s" % (fname, ", unit %s" % want_unit if want_unit else ""))
    for topic in bound:
        if topic not in topics:
            die("bindings: %s has no message %s" % (d["name"], topic))
    return OrderedDict(module=module, keys=keys, plans=plans)


def add_core(kind, core, p, evidence):
    for c in kind["core"]:
        if c["core"] == core:
            if c["plan"] is not p:
                die("kind %s: two fields for core key %s" % (kind["name"], core))
            c["evidence"].append(evidence)
            return
    kind["core"].append(OrderedDict(core=core, plan=p, evidence=[evidence]))


# ---------------------------------------------------------------- emitting

def source_lines(d, files):
    version = d["spec"].get("version") or "unversioned"
    out = ["source        %s" % q("first-bite dictionary %s, version %s: %s (Apache License 2.0)" % (d["name"], version, rel))
           for rel in files]
    src = d["spec"].get("source") or {}
    if src.get("document"):
        out.append("source        %s" % q("the dictionary states it transcribes %s%s" % (
            src["document"], " revision %s" % src["revision"] if src.get("revision") else "")))
    elif d["spec"].get("description"):
        out.append("source        %s" % q("the dictionary describes itself: %s" % d["spec"]["description"]))
    return out


def emit_module(d, plan):
    L = ["# The %s module: the fields of the first-bite dictionary `%s`, in its units and in" % (plan["module"], d["name"]),
         "# the form first-bite writes them: a field with value labels is the label, a `bool`",
         "# field is `true` or `false`. A `bytes` field of one byte is two hexadecimal digits; a",
         "# wider one is the integer its bytes spell.",
         "# GENERATED by tools/bite2pack/bite2pack.py. Do not edit.",
         "module        %s" % plan["module"],
         "version       %s" % (d["spec"].get("version") or "0.1.0"),
         "distribution  public"]
    L += source_lines(d, d["files"])
    width = max(len(k) for k in plan["keys"]) + 2
    for k in plan["keys"].values():
        f = k["field"]
        if f is None:
            meaning = k["meaning"]
        else:
            parts = []
            if f.description:
                parts.append(f.description.rstrip("."))
            parts.append("dictionary type %s" % f.kind)
            if k.get("table"):
                parts.append("a label, `true` or `false`, or the decimal text of a value with no label")
            elif f.labels:
                parts.append("labels " + ", ".join("%d=%s" % kv for kv in f.labels.items()))
            parts.append("in " + ", ".join(k["where"][:4]) + (" and %d more" % (len(k["where"]) - 4) if len(k["where"]) > 4 else ""))
            meaning = "; ".join(parts)
        unit = re.sub(r"\s+", "", k["unit"]) or "-"
        L.append("key  %-*s %-7s %-9s %s" % (width, k["name"], k["type"], unit, q(meaning)))
    return "\n".join(L) + "\n"


def field_line(kw, key, p):
    parts = ["field     %-*s %-12s %s" % (kw, key, p["locator"], p["transform"])]
    if p["table"]:
        parts.append("via %s" % p["table_name"])
    parts += p["options"]
    return " ".join(parts)


def emit_pack(d, plan, pl):
    iface = pl["iface"]
    files = ["dictionary.json"] if "dictionary.json" in d["files"] else []
    files += [f for f in d["files"] if f.startswith(iface["name"] + "/")]
    L = ["# first-bite dictionary `%s`, interface %s." % (d["name"], iface["name"])]
    if iface["source"] == "frame_id":
        L.append("# Message = %d bytes of frame identifier (%s-endian), then the payload." % (
            FRAME_ID_BYTES, "little" if iface["bit_order"] == "lsb0" else "big"))
        L.append("# Payload bit offsets are the dictionary's start + %d. See pack.md." % pl["base"])
    else:
        L.append("# Message = the payload. Bit offsets are the dictionary's. See pack.md.")
    L += ["# GENERATED by tools/bite2pack/bite2pack.py. Do not edit.",
          "format        %s" % pl["format"],
          "version       %s" % (d["spec"].get("version") or "0.1.0"),
          "distribution  public"]
    L += source_lines(d, files)
    L += ["uses          %s" % plan["module"], "wire          bits",
          "bit_order     %s" % ("lsb" if iface["bit_order"] == "lsb0" else "msb"), "word_bytes    1"]
    tables = OrderedDict()
    for kind in pl["kinds"]:
        for p in kind["fields"]:
            if p["transform"] and p["table"]:
                tables.setdefault(p["table_name"], p["table"])
    for name, rows in tables.items():
        L.append("")
        L += ["map       %s %d %s" % (name, raw, q(text) if " " in text else text) for raw, text in rows]
    for kind in pl["kinds"]:
        L.append("")
        L.append("# %s/%s%s" % (iface["name"], kind["message"],
                                ": " + re.sub(r"\s+", " ", kind["description"]) if kind["description"] else ""))
        if not kind["defined"]:
            L.append("# named by message_type, no message file: identified, no field decoded")
        L.append("kind      %s" % kind["name"])
        for start, n, v in kind["whens"]:
            L.append("when      0:%d:%d %d" % (start, n, v))
        mapped = [p for p in kind["fields"] if p["transform"]]
        req = [c["core"] for c in kind["core"]] + [p["key"] for p in mapped]
        if req:
            L.append("requires  " + " ".join(OrderedDict.fromkeys(req)))
        kw = max([len(p["key"]) for p in mapped] + [3]) + 2
        if iface["source"] == "frame_id":      # the whole identifier, so the bits the mask drops survive a round trip
            L.append("field     %-*s %-12s uint" % (kw, pl["code_key"], pl["code_loc"]))
        for p in kind["fields"]:
            if not p["transform"]:
                L.append("# not mapped: %s [%d, %d): %s" % (p["field"].name, p["field"].start, p["field"].stop, p["reason"]))
                continue
            for c in kind["core"]:
                if c["plan"] is p:     # the shared key first: a consumer that changes it sees it on the wire
                    L.append(field_line(kw, c["core"], p))
            L.append(field_line(kw, p["key"], p))
    L += ["", "# an identifier the dictionary does not name: bite gives a record with no fields",
          "kind      %s" % pl["catch"],
          "field     %s %s uint" % (pl["code_key"], pl["code_loc"])]
    return "\n".join(L) + "\n"


# ---------------------------------------------------------------- a small codec, from spec/engine-spec.md

def tokenize(line):
    toks, cur, in_q, started = [], "", False, False
    for ch in line:
        if in_q:
            if ch == '"':
                in_q = False
            else:
                cur += ch
        elif ch == '"':
            in_q, started = True, True
        elif ch.isspace():
            if started:
                toks.append(cur)
                cur, started = "", False
        elif ch == "#" and not started:
            break
        else:
            cur += ch
            started = True
    if in_q:
        die("unterminated quote: %s" % line)
    if started:
        toks.append(cur)
    return toks


def parse_module(text):
    m = OrderedDict(name=None, distribution=None, sources=[], keys=OrderedDict())
    for line in text.splitlines():
        t = tokenize(line)
        if not t:
            continue
        if t[0] == "module" and len(t) == 2:
            m["name"] = t[1]
        elif t[0] == "version" and len(t) == 2:
            pass
        elif t[0] == "distribution" and len(t) == 2 and t[1] in ("public", "controlled"):
            m["distribution"] = t[1]
        elif t[0] == "source" and len(t) == 2:
            m["sources"].append(t[1])
        elif t[0] == "key" and len(t) in (4, 5):
            if "." in t[1] or t[2] not in ("string", "float", "int"):
                die("module: bad key line: %s" % line)
            if t[1] in m["keys"]:
                die("duplicate-key: %s" % t[1])
            m["keys"][t[1]] = t[2]
        else:
            die("module: bad directive: %s" % line)
    if not m["name"] or not m["distribution"] or not m["sources"]:
        die("module: missing name, distribution or source")
    return m


def parse_locator(loc, word_bytes):
    m = re.fullmatch(r"(\d+):(\d+):(\d+)", loc)
    if not m:
        die("bad locator %s" % loc)
    word, pos, ln = (int(x) for x in m.groups())
    if not 1 <= ln <= 2040:
        die("bad length in %s" % loc)
    return word * word_bytes * 8 + pos, ln


def canon(text, kind):
    """A table entry or a value as canonical text of its type (spec 0.6, ruling 4)."""
    if kind == "int":
        return str(int(text))
    if kind == "float":
        return plain(float(text))
    return text


def parse_pack(text, modules):
    p = OrderedDict(format=None, distribution=None, sources=[], uses=["core"], wire=None, msb=False,
                    word_bytes=None, maps=OrderedDict(),
                    common=OrderedDict(name=None, whens=[], fields=[], requires=[]), kinds=[])
    cur = p["common"]
    raw_lines = []
    for line in text.splitlines():
        t = tokenize(line)
        if not t:
            continue
        d = t[0]
        if d == "format" and len(t) == 2:
            p["format"] = t[1]
        elif d == "version" and len(t) == 2:
            pass
        elif d == "distribution" and len(t) == 2 and t[1] in ("public", "controlled"):
            p["distribution"] = t[1]
        elif d == "source" and len(t) == 2:
            p["sources"].append(t[1])
        elif d == "uses" and len(t) == 2:
            p["uses"].append(t[1])
        elif d == "wire" and len(t) == 2:
            p["wire"] = t[1]
        elif d == "bit_order" and len(t) == 2 and t[1] in ("lsb", "msb"):
            p["msb"] = t[1] == "msb"
        elif d == "word_bytes" and len(t) == 2:
            p["word_bytes"] = int(t[1])
        elif d == "map" and len(t) == 4:
            p["maps"].setdefault(t[1], []).append((t[2], t[3]))
        elif d == "kind" and len(t) == 2:
            if any(k["name"] == t[1] for k in p["kinds"]):
                die("duplicate-kind: %s" % t[1])
            cur = OrderedDict(name=t[1], whens=[], fields=[], requires=[])
            p["kinds"].append(cur)
        elif d == "when" and len(t) == 3:
            raw_lines.append((cur, "when", t[1], t[2]))
        elif d == "requires" and len(t) >= 2:
            if cur is p["common"]:
                die("bad-directive: requires before the first kind")
            cur["requires"] += t[1:]
        elif d == "field" and len(t) >= 4:
            raw_lines.append((cur, "field", t[1], t[2], t[3], t[4:]))
        else:
            die("pack: bad directive: %s" % line)
    if p["wire"] != "bits" or not p["word_bytes"]:
        die("pack %s: not a bits pack with word_bytes" % p["format"])
    if not p["distribution"]:
        die("missing-distribution")
    if not p["sources"]:
        die("missing-source")
    for u in p["uses"]:
        if u not in modules:
            die("unknown-module: %s" % u)
        if p["distribution"] == "public" and modules[u]["distribution"] != "public":
            die("distribution-violation: %s" % u)

    def key_type(key):
        mod, name = (key.split(".", 1) if "." in key else ("core", key))
        if mod not in p["uses"]:
            die("module-not-used: %s" % key)
        if name not in modules[mod]["keys"]:
            die("undeclared-key: %s" % key)
        return modules[mod]["keys"][name]

    def table_type(name):
        models = [m for _, m in p["maps"][name]]
        if all(re.fullmatch(r"-?[0-9]+", m) for m in models):
            return "int"
        if all(re.fullmatch(r"-?[0-9]+(\.[0-9]+)?", m) for m in models):
            return "float"
        return "string"

    for rl in raw_lines:
        if rl[1] == "when":
            start, ln = parse_locator(rl[2], p["word_bytes"])
            if not re.fullmatch(r"\d+", rl[3]) or ln > 64 or int(rl[3]) >> ln:
                die("bad-directive: when %s %s" % (rl[2], rl[3]))
            rl[0]["whens"].append((start, ln, int(rl[3])))
            continue
        start, ln = parse_locator(rl[3], p["word_bytes"])
        name = rl[4].split(":")[0]
        if name not in YIELDS:
            die("unknown transform %s" % rl[4])
        if name == "ascii":
            if ln % 8:
                die("bad-directive: ascii length %d" % ln)
        elif ln > 64 or (name == "f32" and ln != 32) or (name == "f64" and ln != 64):
            die("bad-directive: %s length %d" % (name, ln))
        tr = OrderedDict(name=name)
        if name in ("uscale", "sscale"):
            parts = rl[4].split(":")
            if len(parts) not in (2, 3):
                die("bad transform %s" % rl[4])
            tr["fac"] = parse_factor(parts[1])
            tr["offset"] = float(parts[2]) if len(parts) == 3 else 0.0
        elif ":" in rl[4]:
            die("bad transform %s" % rl[4])
        f = OrderedDict(key=rl[2], start=start, len=ln, tr=tr, via=None, swap=False, trim=False, optional=False)
        opts = list(rl[5])
        while opts:
            o = opts.pop(0)
            if o == "via" and opts and f["via"] is None:
                f["via"] = opts.pop(0)
                if f["via"] not in p["maps"]:
                    die("bad-directive: via %s names a table with no map line" % f["via"])
            elif o in OPTIONS and not f[o]:
                f[o] = True
            else:
                die("bad-directive: field option %s in %s" % (o, " ".join(rl[2:5])))
        if f["swap"] and ln % 8:
            die("bad-directive: swap on %d bits" % ln)
        if f["trim"] and name != "ascii":
            die("bad-directive: trim on %s" % name)
        f["wire_type"] = YIELDS[name]
        f["key_type"] = key_type(rl[2])
        if f["key_type"] != (table_type(f["via"]) if f["via"] else YIELDS[name]):
            die("key-type-mismatch: %s with %s" % (rl[2], " ".join([rl[4]] + list(rl[5]))))
        rl[0]["fields"].append(f)
    for k in p["kinds"]:
        for r in k["requires"]:
            key_type(r)
    if not p["kinds"]:
        die("pack %s has no kind" % p["format"])
    return p


class WireError(Exception):
    def __init__(self, kind, msg):
        Exception.__init__(self, "%s: %s" % (kind, msg))
        self.kind = kind


def get_bits(data, start, ln, msb):
    if start + ln > len(data) * 8:
        return None
    v = 0
    for k in range(ln):
        bit = start + k
        byte = data[bit // 8]
        if msb:
            if byte & (0x80 >> (bit % 8)):
                v |= 1 << (ln - 1 - k)
        elif byte & (1 << (bit % 8)):
            v |= 1 << k
    return v


def set_bits(buf, start, ln, raw, msb):
    for k in range(ln):
        bit = start + k
        if bit // 8 >= len(buf):
            continue
        if msb:
            on, mask = raw & (1 << (ln - 1 - k)), 0x80 >> (bit % 8)
        else:
            on, mask = raw & (1 << k), 1 << (bit % 8)
        if on:
            buf[bit // 8] |= mask
        else:
            buf[bit // 8] &= ~mask & 0xFF


def signed(raw, ln):
    return raw - (1 << ln) if raw & (1 << (ln - 1)) else raw


def swap_bytes(raw, ln):
    return int.from_bytes(raw.to_bytes(ln // 8, "big"), "little")


def message_bytes(p, k, every):
    """Whole words up to the furthest bit: of every line on encode; on decode, of the
    lines that are not `optional` (spec 0.6)."""
    end = 0
    for f in p["common"]["fields"] + k["fields"]:
        if every or not f["optional"]:
            end = max(end, f["start"] + f["len"])
    for w in p["common"]["whens"] + k["whens"]:
        end = max(end, w[0] + w[1])
    wb = p["word_bytes"] * 8
    return max(1, -(-end // wb)) * p["word_bytes"]


def through_table(p, f, v, decoding):
    """A value through the field's table: after the transform on decode, before it on encode."""
    rows = p["maps"][f["via"]]
    if decoding:
        text = canon(model_text(v), f["wire_type"])
        for native, model in rows:
            if canon(native, f["wire_type"]) == text:
                return read_as(model, f["key_type"])
    else:
        text = canon(model_text(v), f["key_type"])
        for native, model in rows:
            if canon(model, f["key_type"]) == text:
                return read_as(native, f["wire_type"])
    if f["wire_type"] == f["key_type"]:
        return v
    raise WireError("transform-decode" if decoding else "transform-encode", "%s: no entry for %s in table %s" % (f["key"], text, f["via"]))


def read_as(text, kind):
    return int(text) if kind == "int" else float(text) if kind == "float" else text


def decode(p, data):
    """-> (kind, entity as OrderedDict key -> value). Raises WireError."""
    holds = lambda w: get_bits(data, w[0], w[1], p["msb"]) == w[2]
    if not all(holds(w) for w in p["common"]["whens"]):
        raise WireError("no-kind", "a common when does not hold")
    for k in p["kinds"]:
        if all(holds(w) for w in k["whens"]):
            break
    else:
        raise WireError("no-kind", "no kind holds")
    need = message_bytes(p, k, False)
    if len(data) < need:
        raise WireError("malformed-wire", "input is %d bytes, need %d" % (len(data), need))
    ent = OrderedDict()
    for f in p["common"]["fields"] + k["fields"]:
        name = f["tr"]["name"]
        if f["start"] + f["len"] > len(data) * 8:
            continue                    # an `optional` field beyond the input is absent
        if name == "ascii":
            bs = bytes(get_bits(data, f["start"] + i * 8, 8, p["msb"]) for i in range(f["len"] // 8))
            if f["swap"]:
                bs = bs[::-1]
            bs = bs.rstrip(b"\0")
            if f["trim"]:
                bs = bs.rstrip(b" ")
            if any(b < 0x20 or b > 0x7E for b in bs):
                raise WireError("transform-decode", "%s: not printable ASCII" % f["key"])
            v = bs.decode("ascii")
        else:
            raw = get_bits(data, f["start"], f["len"], p["msb"])
            if f["swap"]:
                raw = swap_bytes(raw, f["len"])
            if name == "uint":
                v = signed(raw, 64) if f["len"] == 64 else raw      # ruling 2 of 0.5
            elif name == "twos":
                v = signed(raw, f["len"])
            elif name == "uscale":
                v = scale_decode(raw, f["tr"]["fac"], f["tr"]["offset"])
            elif name == "sscale":
                v = scale_decode(signed(raw, f["len"]), f["tr"]["fac"], f["tr"]["offset"])
            else:
                v = struct.unpack(">f" if name == "f32" else ">d", raw.to_bytes(f["len"] // 8, "big"))[0]
                if math.isnan(v) or math.isinf(v):
                    raise WireError("transform-decode", "%s: not a finite number" % f["key"])
        if f["via"]:
            v = through_table(p, f, v, True)
        if f["key"] not in ent:         # the earliest line of a key
            ent[f["key"]] = v
    return k, ent


def encode(p, k, ent):
    buf = bytearray(message_bytes(p, k, True))
    for f in reversed(p["common"]["fields"] + k["fields"]):     # the earlier line wins (ruling 5 of 0.5)
        if f["key"] not in ent:
            continue
        v, name = ent[f["key"]], f["tr"]["name"]
        if f["via"]:
            v = through_table(p, f, v, False)
        if name == "ascii":
            bs = v.encode("ascii")
            if len(bs) > f["len"] // 8:
                raise WireError("transform-encode", "%s: text too long" % f["key"])
            bs = bs.ljust(f["len"] // 8, b"\0")
            if f["swap"]:
                bs = bs[::-1]
            for i, b in enumerate(bs):
                set_bits(buf, f["start"] + i * 8, 8, b, p["msb"])
            continue
        if name in ("uint", "twos"):
            raw = v & ((1 << f["len"]) - 1)
        elif name in ("uscale", "sscale"):
            r = scale_encode(v, f["tr"]["fac"], f["tr"]["offset"])
            lo, hi = (0, 1 << f["len"]) if name == "uscale" else (-(1 << (f["len"] - 1)), 1 << (f["len"] - 1))
            if not lo <= r < hi:
                raise WireError("transform-encode", "%s: out of range" % f["key"])
            raw = r & ((1 << f["len"]) - 1)
        else:
            raw = int.from_bytes(struct.pack(">f" if name == "f32" else ">d", v), "big")
        if f["swap"]:
            raw = swap_bytes(raw, f["len"])
        set_bits(buf, f["start"], f["len"], raw, p["msb"])
    for w in p["common"]["whens"] + k["whens"]:
        set_bits(buf, w[0], w[1], w[2], p["msb"])
    return bytes(buf)


def model_text(x):
    return x if isinstance(x, str) else plain(x)


# ---------------------------------------------------------------- vectors

def wire_input(pl, payload, frame_id):
    """The pack's message for a payload (and frame identifier) as bite receives them."""
    if pl["iface"]["source"] != "frame_id":
        return payload
    if frame_id is None:
        die("interface %s needs a frame identifier" % pl["iface"]["name"])
    if frame_id >> (FRAME_ID_BYTES * 8):
        die("frame identifier %d is wider than %d bytes" % (frame_id, FRAME_ID_BYTES))
    return frame_id.to_bytes(FRAME_ID_BYTES, "little" if pl["iface"]["bit_order"] == "lsb0" else "big") + payload


def synthetic_message(pack, k):
    """A message of kind k whose set bits all lie in mapped fields, and whose table
    values are the ones an encoder writes back."""
    buf = bytearray(message_bytes(pack, k, True))
    for i, f in enumerate(reversed(k["fields"])):
        name, ln = f["tr"]["name"], f["len"]
        if name == "ascii":
            text = "AB12" * (ln // 32 + 1)
            for j in range(ln // 8):
                set_bits(buf, f["start"] + j * 8, 8, ord(text[j]), pack["msb"])
            continue
        if name == "f32":
            raw = int.from_bytes(struct.pack(">f", 45.5), "big")
        elif name == "f64":
            raw = int.from_bytes(struct.pack(">d", 64.8378), "big")
        else:
            raw = (0xA5A5A5A5A5A5A5A5 >> (i % 5)) & ((1 << ln) - 1)
        if f["via"]:        # the first raw value of its text: what encode writes for that text
            mine = through_table(pack, f, signed(raw, ln) if name == "twos" else raw, True)
            raw = through_table(pack, f, mine, False) & ((1 << ln) - 1)
        if f["swap"]:
            raw = swap_bytes(raw, ln)
        set_bits(buf, f["start"], ln, raw, pack["msb"])
    for w in k["whens"]:
        set_bits(buf, w[0], w[1], w[2], pack["msb"])
    return bytes(buf)


def write_vectors(out_dir, d, plan, pl, pack):
    vdir = os.path.join(out_dir, "packs", pl["format"], "vectors")
    os.makedirs(vdir, exist_ok=True)
    for fn in os.listdir(vdir):
        if fn.endswith(".json"):
            os.remove(os.path.join(vdir, fn))
    fmt, counts = pl["format"], OrderedDict(dictionary=0, dictionary_identical=0, refused=0, synthetic=0)

    def put(name, vec):
        write(os.path.join(vdir, "%s.json" % name), json.dumps(vec, indent=2) + "\n")

    def model_of(k, ent):
        m = OrderedDict([("@kind", k["name"])])
        for key, x in ent.items():
            m[key] = model_text(x)
        return m

    for v in d["vectors"]:
        if v.get("interface") != pl["iface"]["name"]:
            continue
        payload = bytes.fromhex("".join(v["payload_hex"].split()))
        data = wire_input(pl, payload, v.get("frame_id"))
        name = "dict-%s" % slug(v["_name"]).replace("_", "-")
        try:
            k, ent = decode(pack, data)
        except WireError as e:
            put(name, OrderedDict([("name", name), ("from", fmt), ("to", fmt), ("input", data.hex()),
                                   ("expect_error", e.kind)]))
            counts["refused"] += 1
            continue
        out = encode(pack, k, ent)
        put(name, OrderedDict([("name", name), ("from", fmt), ("to", fmt), ("input", data.hex()),
                               ("expect", out.hex()), ("model", model_of(k, ent))]))
        counts["dictionary"] += 1
        counts["dictionary_identical"] += out == data
    for k in pack["kinds"]:
        data = synthetic_message(pack, k)
        if not k["whens"]:
            # the catch-all kind: an identifier no other kind's `when` lines hold for
            f = k["fields"][0]
            for raw in [(1 << f["len"]) - 1] + list(range(min(1 << f["len"], 4096))):
                buf = bytearray(len(data))
                set_bits(buf, f["start"], f["len"], raw, pack["msb"])
                try:
                    if decode(pack, bytes(buf))[0] is k:
                        data = bytes(buf)
                        break
                except WireError:
                    pass
            else:
                continue          # every identifier is named: no message reaches this kind
        k2, ent = decode(pack, data)
        if k2 is not k:
            die("%s: synthetic vector for %s decodes as %s" % (fmt, k["name"], k2["name"]))
        if encode(pack, k, ent) != data:
            die("%s: synthetic vector for %s does not round-trip" % (fmt, k["name"]))
        name = "rt-%s" % k["name"].replace("_", "-")
        put(name, OrderedDict([("name", name), ("from", fmt), ("to", fmt), ("input", data.hex()),
                               ("expect", data.hex()), ("model", model_of(k, ent))]))
        counts["synthetic"] += 1
    return counts


# ---------------------------------------------------------------- measuring the scale arithmetic

def measure_scales(plan):
    """For every scaled field: over raw values, how often ruling 1's arithmetic gives a
    double other than the one bite's rounding gives. Exhaustive up to 20 bits."""
    rows = []
    done = set()
    for pl in plan["plans"].values():
        for kind in pl["kinds"]:
            for p in kind["fields"]:
                f = p["field"]
                if not p["transform"] or p["transform"].split(":")[0] not in ("uscale", "sscale"):
                    continue
                sig = (p["transform"], f.width)
                if sig in done:
                    p["scale_measure"] = next(r for r in rows if r["sig"] == sig)
                    continue
                done.add(sig)
                parts = p["transform"].split(":")
                fac, off = parse_factor(parts[1]), float(parts[2]) if len(parts) == 3 else 0.0
                if f.width <= 20:
                    raws, how = range(1 << f.width), "all %d raw values" % (1 << f.width)
                else:
                    x, raws = 0x9E3779B97F4A7C15, [0, 1, (1 << f.width) - 1, 1 << (f.width - 1), (1 << (f.width - 1)) - 1]
                    for _ in range(65536):
                        x = (x * 6364136223846793005 + 1442695040888963407) & (2 ** 64 - 1)
                        raws.append(x >> (64 - f.width))
                    how = "%d raw values (edges and a fixed pseudo-random sample)" % len(raws)
                diff, worst, ex, n = 0, 0.0, None, 0
                for raw in raws:
                    v = signed(raw, f.width) if parts[0] == "sscale" else raw
                    a, b = scale_decode(v, fac, off), bite_scaled(v, f.scale, f.offset)
                    n += 1
                    if a != b:
                        diff += 1
                        if abs(a - b) >= worst:
                            worst, ex = abs(a - b), (v, plain(a), plain(b))
                row = OrderedDict(sig=sig, transform=p["transform"], width=f.width, scale=f.scale, offset=f.offset,
                                  sample=how, n=n, differ=diff, worst=worst, example=ex,
                                  fields=["%s/%s/%s" % (pl["iface"]["name"], kind["message"], f.name)])
                rows.append(row)
                p["scale_measure"] = row
    return rows


# ---------------------------------------------------------------- pack.md, the index and the map

def unmapped_rows(pl):
    return [(kind, p) for kind in pl["kinds"] for p in kind["fields"] if not p["transform"]]


def emit_pack_md(d, plan, pl, counts, scale_rows):
    iface = pl["iface"]
    defined = [k for k in pl["kinds"] if k["defined"]]
    fields = [p for k in pl["kinds"] for p in k["fields"]]
    nf, nm = len(fields), sum(1 for p in fields if p["transform"])
    L = ["# Pack: %s" % pl["format"], "",
         "Interface `%s` of the first-bite dictionary `%s`, version %s, generated by" % (
             iface["name"], d["name"], d["spec"].get("version") or "unversioned"),
         "`tools/bite2pack/bite2pack.py`. Do not edit `pack.def`, `model/%s.def` or the vectors by hand; change the" % plan["module"],
         "dictionary or the generator and run it again.", ""]
    L += ["## Message convention", ""]
    if iface["source"] == "frame_id":
        L += ["first-bite identifies a message of this interface by the identifier that arrives with the frame"
              + (", ANDed with `0x%X`" % iface["mask"] if iface["mask"] is not None else "") + ", not by a payload field.",
              "A message of this pack is therefore %d bytes of frame identifier (%s-endian) followed by the payload."
              % (FRAME_ID_BYTES, "little" if iface["bit_order"] == "lsb0" else "big"),
              "A payload field `[start, stop)` is `0:<start + %d>:<stop - start>`. The `when` lines read only the bits" % pl["base"],
              "the mask keeps; the whole identifier is the key `%s`." % pl["code_key"], ""]
    else:
        L += ["A message is the payload first-bite decodes: the transport payload of one packet. A field",
              "`[start, stop)` is `0:<start>:<stop - start>`; the message type is bits [%d, %d)." % (iface["dstart"], iface["dstop"]), ""]
    L += ["`wire bits`, `word_bytes 1`, `bit_order %s` (the dictionary's `%s`). Input and output are hex text."
          % ("lsb" if iface["bit_order"] == "lsb0" else "msb", iface["bit_order"]),
          "Every field is `optional`: a payload cut short decodes up to the cut, as in first-bite.", "",
          "## What is mapped", "", "| | Count |", "|---|---|",
          "| message types the interface names | %d |" % len(iface["names"]),
          "| kinds in `pack.def` | %d (%d with fields, %d named and not defined, 1 for an unnamed identifier) |"
          % (len(pl["kinds"]) + 1, len(defined), len(pl["kinds"]) - len(defined)),
          "| fields in the message files | %d |" % nf,
          "| fields mapped | %d |" % nm,
          "| of those, in the form first-bite writes | %d |" % sum(1 for p in fields if p["transform"] and p["repr"] == "same"),
          "| fields not mapped | %d |" % (nf - nm),
          "| shared keys mapped | %d |" % sum(len(k["core"]) for k in pl["kinds"]),
          "| vectors | %d (%d from the dictionary's own vectors, %d of those byte-identical round trips; %d the engine must refuse; %d synthetic round trips) |"
          % (sum(v for k, v in counts.items() if k != "dictionary_identical"), counts["dictionary"],
             counts["dictionary_identical"], counts["refused"], counts["synthetic"]), ""]
    other = [(k, p) for k in pl["kinds"] for p in k["fields"] if p["transform"] and p["repr"] != "same"]
    if other:
        why = {"bytes-as-integer": "a `bytes` field wider than one byte is the integer its bytes spell; first-bite writes hexadecimal text",
               "label-as-integer": "wider than %d bits: the wire integer; first-bite writes the label" % TABLE_MAX_BITS,
               "bool-as-integer": "wider than %d bits: the wire integer; first-bite writes `true` or `false`" % TABLE_MAX_BITS,
               "label-not-carried": "scaled: the number; first-bite writes the label of a raw value that has one"}
        L += ["### Fields whose value is equal and whose form differs", "", "| Message | Field | Form |", "|---|---|---|"]
        L += ["| %s | `%s` | %s |" % (k["message"], p["field"].name, why[p["repr"]]) for k, p in other] + [""]
    rows = unmapped_rows(pl)
    if rows:
        L += ["### Fields not mapped", "", "| Message | Field | Bits | Reason |", "|---|---|---|---|"]
        L += ["| %s | `%s` | [%d, %d) | %s |" % (k["message"], p["field"].name, p["field"].start, p["field"].stop, p["reason"]) for k, p in rows]
        L.append("")
    if pl["undefined"] or pl["never"]:
        L += ["### Messages", ""]
        for m in pl["undefined"]:
            L.append("- `%s` is named by `message_type` and has no message file. Its kind has a `when` and no field, as first-bite identifies it and decodes nothing." % m)
        for m, why in pl["never"]:
            L.append("- `%s` has no kind: %s." % (m, why))
        L.append("")
    cores = [(k, c) for k in pl["kinds"] for c in k["core"]]
    L += ["## Shared keys", ""]
    if cores:
        L += ["| Kind | Key | From |", "|---|---|---|"]
        L += ["| %s | `%s` | %s |" % (k["name"], c["core"], "; ".join(c["evidence"])) for k, c in cores]
        L.append("")
    else:
        L += ["None. The bindings file names no message of this interface, so a conversion to another format carries",
              "only what the caller supplies as context.", ""]
    mine = [r for r in scale_rows if any(p.get("scale_measure") is r for k in pl["kinds"] for p in k["fields"])]
    if mine:
        L += ["## Scale arithmetic", "",
              "first-bite rounds `raw * scale + offset` to the decimal places `scale` and `offset` are written with. The engine",
              "computes `raw * num / den`, then `+ offset`. The generator compares the two doubles:", "",
              "| Transform | Bits | Raw values compared | Doubles that differ | Largest difference |", "|---|---|---|---|---|"]
        for r in mine:
            L.append("| `%s` | %d | %s | %d | %s |" % (r["transform"], r["width"], r["sample"], r["differ"],
                     "0" if not r["differ"] else "%.3g (raw %d: engine %s, first-bite %s)" % ((r["worst"],) + tuple(r["example"]))))
        L.append("")
    L += ["## What the pack does not state", "",
          "- first-bite passes a text byte outside 0x20 to 0x7E; the engine refuses the message.",
          "- The dictionary's `port`, `message_length` range, `declared_bits`, `icd_numbering`, `source` and `note` keys have no directive; `port` is how first-bite picks the interface.", ""]
    return "\n".join(L)


def index_rows(d, plan):
    return [OrderedDict(format=pl["format"], dictionary=d["name"], interface=iname,
                        frame_id_bytes=FRAME_ID_BYTES if pl["iface"]["source"] == "frame_id" else 0,
                        module=plan["module"])
            for iname, pl in plan["plans"].items()]


def build_map(d, plan):
    out = OrderedDict(dictionary=d["name"], module=plan["module"], interfaces=OrderedDict())
    for iname, pl in plan["plans"].items():
        iface = pl["iface"]
        ifo = OrderedDict(format=pl["format"], bit_order=iface["bit_order"], discriminator=iface["source"],
                          frame_id_bytes=FRAME_ID_BYTES if iface["source"] == "frame_id" else 0,
                          catch_kind=pl["catch"], code_key=pl["code_key"], names=OrderedDict((str(c), n) for c, n in iface["names"].items()),
                          undefined=pl["undefined"], never=[list(x) for x in pl["never"]],
                          left_out=[list(x) for x in iface["left_out"]], kinds=OrderedDict())
        for kind in pl["kinds"]:
            ko = OrderedDict(message=kind["message"], code=kind["code"], defined=kind["defined"], fields=OrderedDict(),
                             core=OrderedDict((c["core"], c["plan"]["field"].name) for c in kind["core"]))
            for p in kind["fields"]:
                f = p["field"]
                ko["fields"][f.name] = OrderedDict(
                    start=f.start, stop=f.stop, type=f.kind, byte_order=f.byte_order, scale=f.scale, offset=f.offset,
                    unit=f.unit, labels=OrderedDict((str(v), l) for v, l in f.labels.items()),
                    key=p.get("key"), locator=p["locator"], transform=p["transform"], yields=p["yields"],
                    model=p["model"], options=p["options"], table=bool(p["table"]), repr=p["repr"], reason=p["reason"])
            ifo["kinds"][kind["name"]] = ko
        out["interfaces"][iname] = ifo
    return out


# ---------------------------------------------------------------- self-check of the written files

def same_number(want, got):
    """bite/src/vectors.rs same_number: nine significant digits."""
    try:
        a, b = float(want.strip()), float(got)
    except ValueError:
        return False
    return abs(a - b) <= 1e-9 * max(abs(a), abs(b), 1e-300)


def check_dictionary_vectors(d, plan, pl, pack):
    """The dictionary's own vectors against the codec, by bite's comparison rule
    (bite/src/vectors.rs): the text bite renders, or nine significant digits for a
    number. -> (fields compared, equal in bite's form, equal in value only, differences)."""
    kinds = {k["name"]: k for k in pl["kinds"]}
    compared, equal, value_only, diffs = 0, 0, 0, []
    for v in d["vectors"]:
        if v.get("interface") != pl["iface"]["name"]:
            continue
        data = wire_input(pl, bytes.fromhex("".join(v["payload_hex"].split())), v.get("frame_id"))
        exp = v["expect"]
        try:
            k, ent = decode(pack, data)
        except WireError as e:
            for fname, want in (exp.get("fields") or {}).items():
                compared += 1
                diffs.append((v["_name"], fname, "the codec refuses the message (%s); the dictionary expects %r" % (e.kind, want)))
            continue
        if k["name"] == pl["catch"]:
            want_type = str(ent[pl["code_key"]] & ((pl["iface"]["mask"] if pl["iface"]["mask"] is not None else -1)))
        else:
            want_type = kinds[k["name"]]["message"]
        if exp.get("message_type") is not None and exp["message_type"] != want_type:
            die("%s vector %s: kind %s is message %r, the dictionary expects %r" % (pl["format"], v["_name"], k["name"], want_type, exp["message_type"]))
        if k["name"] == pl["catch"]:
            if exp.get("fields"):
                die("%s vector %s: fields expected of an unnamed identifier" % (pl["format"], v["_name"]))
            continue
        plans = {p["field"].name: p for p in kinds[k["name"]]["fields"]}
        for fname, want in (exp.get("fields") or {}).items():
            compared += 1
            p = plans.get(fname)
            if p is None:
                die("%s vector %s: no field %s" % (pl["format"], v["_name"], fname))
            if not p["transform"]:
                diffs.append((v["_name"], fname, "not mapped: " + p["reason"]))
                continue
            got, f = ent.get(p["key"]), p["field"]
            if want is None or got is None:
                if want is None and got is None:
                    equal += 1
                else:
                    diffs.append((v["_name"], fname, "codec %r, dictionary %r" % (got, want)))
            elif f.kind == "bytes" and p["table"]:
                if got == want:
                    equal += 1
                else:
                    diffs.append((v["_name"], fname, "codec %r, dictionary %r" % (got, want)))
            elif f.kind == "bytes":
                if "%0*x" % (f.width // 4, got & ((1 << f.width) - 1)) == want:
                    value_only += 1
                else:
                    diffs.append((v["_name"], fname, "codec %x, dictionary %r" % (got, want)))
            elif isinstance(got, float):
                if same_number(want, got):
                    equal += 1
                else:
                    diffs.append((v["_name"], fname, "codec %s, dictionary %s" % (plain(got), want)))
            elif model_text(got) == want:
                equal += 1
            elif p["repr"] != "same" and str(got) in (want, {"true": "1", "false": "0"}.get(want)):
                value_only += 1
            else:
                diffs.append((v["_name"], fname, "codec %r, dictionary %r" % (got, want)))
    return compared, equal, value_only, diffs


def self_check(out_dir, d, plan, core_keys):
    modules = {"core": OrderedDict(name="core", distribution="public", keys=core_keys)}
    with open(os.path.join(out_dir, "model", "%s.def" % plan["module"]), encoding="utf-8") as fh:
        mod = parse_module(fh.read())
    if mod["name"] != plan["module"] or mod["distribution"] != "public":
        die("module file does not declare %s public" % plan["module"])
    modules[mod["name"]] = mod
    used = set()
    result = OrderedDict()
    for iname, pl in plan["plans"].items():
        pdir = os.path.join(out_dir, "packs", pl["format"])
        with open(os.path.join(pdir, "pack.def"), encoding="utf-8") as fh:
            pack = parse_pack(fh.read(), modules)
        if pack["format"] != pl["format"] or pack["distribution"] != "public":
            die("%s: pack file does not declare the format public" % pl["format"])
        if len(pack["kinds"]) != len(pl["kinds"]) + 1:
            die("%s: %d kinds on disk, %d planned" % (pl["format"], len(pack["kinds"]), len(pl["kinds"]) + 1))
        for k in pack["kinds"]:
            used.update(f["key"].split(".", 1)[1] for f in k["fields"] if "." in f["key"])
        nvec = 0
        for vpath in sorted(glob.glob(os.path.join(pdir, "vectors", "*.json"))):
            v = load_json(vpath)
            nvec += 1
            data = bytes.fromhex(v["input"])
            if "expect_error" in v:
                try:
                    decode(pack, data)
                except WireError as e:
                    if e.kind != v["expect_error"]:
                        die("%s: wants %s, codec gives %s" % (vpath, v["expect_error"], e.kind))
                    continue
                die("%s: codec decodes a vector that must fail" % vpath)
            k, ent = decode(pack, data)
            if v["model"]["@kind"] != k["name"]:
                die("%s: kind %s, vector says %s" % (vpath, k["name"], v["model"]["@kind"]))
            if set(v["model"]) - {"@kind"} != set(ent):
                die("%s: keys decoded and keys in the vector differ" % vpath)
            for key, want in v["model"].items():
                if key != "@kind" and model_text(ent[key]) != want:
                    die("%s: %s is %s, vector says %s" % (vpath, key, model_text(ent[key]), want))
            got = encode(pack, k, ent).hex()
            if got != v["expect"]:
                die("%s: codec gives %s" % (vpath, got))
        compared, equal, value_only, diffs = check_dictionary_vectors(d, plan, pl, pack)
        # a dictionary's vectors pass `bite check`; the pack must give the same values
        hard = [x for x in diffs if not x[2].startswith("not mapped")]
        if hard:
            die("%s: the pack and the dictionary's vectors disagree:\n  %s" % (
                pl["format"], "\n  ".join("%s %s: %s" % x for x in hard)))
        result[iname] = OrderedDict(vectors=nvec, dictionary_vectors=(compared, equal, value_only, diffs))
    if used != set(mod["keys"]):
        die("module %s: keys declared and keys mapped differ: %s" % (plan["module"], sorted(used ^ set(mod["keys"]))))
    return result


# ---------------------------------------------------------------- main

def convert(root, out, bindings, core_keys, report):
    d = load_dictionary(root)
    plan = plan_dictionary(d, bindings, core_keys)
    scale_rows = measure_scales(plan)
    module_text = emit_module(d, plan)
    write(os.path.join(out, "model", "%s.def" % plan["module"]), module_text)
    modules = {"core": OrderedDict(name="core", distribution="public", keys=core_keys),
               plan["module"]: parse_module(module_text)}
    summary = OrderedDict()
    for iname, pl in plan["plans"].items():
        text = emit_pack(d, plan, pl)
        pdir = os.path.join(out, "packs", pl["format"])
        write(os.path.join(pdir, "pack.def"), text)
        counts = write_vectors(out, d, plan, pl, parse_pack(text, modules))
        write(os.path.join(pdir, "pack.md"), emit_pack_md(d, plan, pl, counts, scale_rows))
        summary[iname] = counts
    checked = self_check(out, d, plan, core_keys)
    m = build_map(d, plan)
    m["scale_arithmetic"] = [OrderedDict((k, v) for k, v in r.items() if k != "sig") for r in scale_rows]
    lines = []
    for iname, pl in plan["plans"].items():
        c, e, vo, diffs = checked[iname]["dictionary_vectors"]
        m["interfaces"][iname]["vectors"] = summary[iname]
        m["interfaces"][iname]["dictionary_vectors_vs_codec"] = OrderedDict(
            fields_compared=c, fields_equal=e, fields_equal_in_value_only=vo, differences=[list(x) for x in diffs])
        nf = sum(len(k["fields"]) for k in pl["kinds"])
        nm = sum(1 for k in pl["kinds"] for p in k["fields"] if p["transform"])
        lines.append("%-28s kinds %2d  fields %3d mapped %3d  vectors %3d  dictionary vector fields %3d: %3d equal, %2d equal in value only" % (
            pl["format"], len(pl["kinds"]) + 1, nf, nm, checked[iname]["vectors"], c, e, vo))
    if report:
        write(os.path.join(report, "%s.map.json" % d["name"]), json.dumps(m, indent=1) + "\n")
    return index_rows(d, plan), lines


def generate(args, out):
    core_keys = CORE_KEYS
    if args.core_def:
        with open(args.core_def, encoding="utf-8") as fh:
            core_keys = parse_module(fh.read())["keys"]
    bindings = read_bindings(args.bindings)
    index, lines, seen = [], [], set()
    for root in sorted(args.dictionary, key=lambda r: os.path.basename(os.path.normpath(r))):
        rows, ls = convert(root, out, bindings, core_keys, args.report)
        for r in rows:
            if r["format"] in seen:
                die("two interfaces give the format %s" % r["format"])
            seen.add(r["format"])
        index += rows
        lines += ls
    for name in bindings:
        if not name.startswith("_") and name not in {r["dictionary"] for r in index}:
            die("bindings: no dictionary named %s was given" % name)
    write(os.path.join(out, "index.json"), json.dumps(index, indent=2) + "\n")
    return index, lines


def generated_files(root, index):
    """Every file under `root` that this tool owns: the index, the modules and the packs it names."""
    out = ["index.json"]
    for module in sorted({r["module"] for r in index}):
        out.append(os.path.join("model", "%s.def" % module))
    for r in index:
        base = os.path.join("packs", r["format"])
        for dirpath, _, names in os.walk(os.path.join(root, base)):
            out += [os.path.relpath(os.path.join(dirpath, n), root) for n in names]
    return sorted(out)


def check(args):
    """--check: what would be written, against what is there."""
    with tempfile.TemporaryDirectory() as tmp:
        index, _ = generate(args, tmp)
        want = generated_files(tmp, index)
        stale = []
        try:
            old = load_json(os.path.join(args.out, "index.json"))
        except (OSError, ValueError):
            old = []
        have = set(generated_files(args.out, old)) | set(generated_files(args.out, index))
        for rel in sorted(have | set(want)):
            a, b = os.path.join(tmp, rel), os.path.join(args.out, rel)
            if not os.path.isfile(b):
                stale.append("missing: %s" % rel)
            elif not os.path.isfile(a):
                stale.append("no longer generated: %s" % rel)
            elif not filecmp.cmp(a, b, shallow=False):
                stale.append("differs: %s" % rel)
    if stale:
        print("bite2pack: %s is out of date; run tools/bite2pack/refresh.sh\n  %s" % (args.out, "\n  ".join(stale)), file=sys.stderr)
        return 1
    print("check: %s is what the dictionaries give (%d files)" % (args.out, len(want)))
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("dictionary", nargs="+", help="a dictionary directory")
    ap.add_argument("--out", required=True, help="directory that receives model/, packs/ and index.json")
    ap.add_argument("--bindings", help="JSON file: which fields carry the shared keys")
    ap.add_argument("--core-def", help="model/core.def of goat-transformers, read for the shared keys and their types")
    ap.add_argument("--report", help="directory that receives <dictionary>.map.json")
    ap.add_argument("--check", action="store_true", help="write nothing; exit 1 when --out is out of date")
    args = ap.parse_args()
    try:
        if args.check:
            return check(args)
        # packs of an interface that no longer exists are removed with the index that named them
        try:
            old = load_json(os.path.join(args.out, "index.json"))
        except (OSError, ValueError):
            old = []
        index, lines = generate(args, args.out)
        for r in old:
            if r["format"] not in {x["format"] for x in index}:
                shutil.rmtree(os.path.join(args.out, "packs", r["format"]), ignore_errors=True)
        print("\n".join(lines))
    except GenError as e:
        print("bite2pack: %s" % e, file=sys.stderr)
        return 1
    print("self-check: passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
