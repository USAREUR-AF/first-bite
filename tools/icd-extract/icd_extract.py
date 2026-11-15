#!/usr/bin/env python3
"""Draft dictionary tables from the field tables of an interface document.

    tools/icd-extract/icd_extract.py document.pdf --out draft/ \\
        --interface ZONE=5 --interface SUPV=6 --header-table 4-1 \\
        --word-bits 16 --bit0 lsb --first-word 1

Reads the PDF's text layer with `pdftotext -layout` (or a text file made that
way, with --text) and writes draft/messages.csv, draft/fields.csv and
draft/values.csv in the form `bite import-csv` reads. Every field row is
marked `unreviewed`, and `bite import-csv` refuses such rows: the draft is
checked against the document row by row, and the mark is cleared by the person
who checked.

Everything runs on this machine. Nothing is sent anywhere: no network, no OCR
service, no language model. Only the standard library is used; `pdftotext`
(poppler) does the PDF reading.

It recognises tables laid out like those of the worked example
(docs/icd-to-dictionary/06-worked-example/icd/):

    Table 5-3: Climate Report
     Field              Word  Bits   Type  LSB   Units  Description
     Air Temperature    5     15-0   S     0.01  °C     Air temperature at ...

and value tables titled `Table 5-2a: Operating Mode values` with the columns
Value and Meaning. A document laid out otherwise needs the patterns at the top
of this file changed, or the tables typed by hand.
"""
import argparse
import csv
import re
import subprocess
import sys
from pathlib import Path

# A table title, a field-table header and the columns of a field row.
TITLE = re.compile(r"^\s*Table\s+(?P<table>[0-9A-Z]+-[0-9]+[a-z]?)\s*:\s*(?P<title>.+?)\s*$")
SECTION = re.compile(r"^(?P<number>\d+(?:\.\d+)*)\s+(?P<title>\S.*)$")
FIELD_HEADER = ("Field", "Word", "Bits", "Type", "LSB", "Units", "Description")
VALUE_HEADER = ("Value", "Meaning")
SUMMARY_HEADER = ("ID", "Message")
WORD = re.compile(r"^\d+(-\d+)?$")
BITS = re.compile(r"^\d+(-\d+)?$")
NOTHING = {"", "—", "-", "–"}
TYPES = {"S": "int", "D": "bool", "A": "ascii"}
UNMEANT = {"reserved", "spare", "unused", "not used", "undefined"}
# Units a document prints with characters the unit vocabulary spells out.
UNIT_SPELLINGS = {"°C": "degC", "°": "deg", "°/s": "deg/s", "W/m²": "W/m2", "m/s²": "m/s2", "m³": "m3",
                  "µs": "us", "μs": "us", "Ω": "ohm"}

FIELD_COLUMNS = ["interface", "message", "field", "word", "bits", "type", "scale", "offset", "unit",
                 "description", "source_section", "source_table", "note", "review"]
MESSAGE_COLUMNS = ["interface", "message", "type_id", "defined", "description", "declared_bits",
                   "source_section", "source_table", "note"]
VALUE_COLUMNS = ["interface", "message", "field", "value", "label"]


def snake(name):
    return re.sub(r"[^a-z0-9]+", "_", name.lower()).strip("_")


def cells(line):
    """Split a layout line into cells at runs of two or more spaces."""
    return [cell for cell in re.split(r"\s{2,}", line.strip()) if cell]


def column_starts(line, header):
    """Character offset of each header word in the header line."""
    starts, at = [], 0
    for word in header:
        at = line.index(word, at)
        starts.append(at)
        at += len(word)
    return starts


def split_at(line, starts):
    """Cut a line at the header's column offsets, allowing a cell to begin a little early."""
    out = []
    for index, start in enumerate(starts):
        end = starts[index + 1] if index + 1 < len(starts) else len(line)
        # A cell is left-aligned under its header; step back over non-spaces.
        while start > 0 and start < len(line) and line[start - 1] != " ":
            start -= 1
        while end < len(line) and end > 0 and line[end - 1] != " " and index + 1 < len(starts):
            end -= 1
        out.append(line[start:end].strip())
    return out


def read_tables(text):
    """Yield (section, table, title, kind, rows) for every table recognised."""
    lines = text.replace("\f", "\n").split("\n")
    section = ""
    index = 0
    while index < len(lines):
        line = lines[index]
        match = SECTION.match(line)
        if match and not line.startswith(" "):
            section = match.group("number")
        title = TITLE.match(line)
        if not title:
            index += 1
            continue
        # The header is the next line with text.
        at = index + 1
        while at < len(lines) and not lines[at].strip():
            at += 1
        header = tuple(cells(lines[at])) if at < len(lines) else ()
        if header[:len(FIELD_HEADER)] == FIELD_HEADER:
            kind, columns = "fields", FIELD_HEADER
        elif header[:len(VALUE_HEADER)] == VALUE_HEADER:
            kind, columns = "values", VALUE_HEADER
        elif header[:len(SUMMARY_HEADER)] == SUMMARY_HEADER:
            kind, columns = "summary", SUMMARY_HEADER
        else:
            index += 1
            continue
        starts = column_starts(lines[at], columns)
        rows, at = [], at + 1
        blank = 1           # blank lines since the last line of text
        while at < len(lines):
            row = lines[at]
            if not row.strip():
                blank += 1
                at += 1
                if blank > 2:
                    break
                continue
            if TITLE.match(row) or not row.startswith(" "):
                break       # the next table, or text at the left margin
            parts = split_at(row, starts)
            if blank == 0 and rows:
                # A line directly under a row is that row's cells wrapping.
                rows[-1] = [(a + " " + b).strip() for a, b in zip(rows[-1], parts)]
            else:
                rows.append(parts)
            blank = 0
            at += 1
        yield section, title.group("table"), title.group("title"), kind, rows
        index = at


def start_bit(word, bits, numbering):
    """Start bit of a field, for naming spare bits. Big-endian words."""
    word_bits, bit0, first_word, first_bit = numbering
    words = [int(part) for part in word.split("-")]
    ends = [int(part) for part in bits.split("-")]

    def position(w, b):
        offset = b - first_bit
        if bit0 == "lsb":
            offset = word_bits - 1 - offset
        return (w - first_word) * word_bits + offset

    return min(position(words[0], ends[0]), position(words[-1], ends[-1]))


def draft(text, interfaces, header_table, numbering):
    """Return (messages, fields, values) rows, and a list of remarks."""
    tables = list(read_tables(text))
    remarks = []
    header_rows = []
    summaries = {}     # interface -> {message name: id}
    for section, table, title, kind, rows in tables:
        if kind == "fields" and table == header_table:
            header_rows = [(row, section, table) for row in rows]
        if kind == "summary":
            interface = interface_of(section, interfaces)
            for row in rows:
                parts = cells("  ".join(row))
                if len(parts) >= 2 and parts[0].isdigit():
                    summaries.setdefault(interface, {})[parts[1]] = parts[0]
    if header_table and not header_rows:
        remarks.append(f"no field table numbered {header_table}: no header rows were added")

    messages, fields, values = [], [], []
    current = None      # (interface, message) of the last field table
    names = {}          # (interface, message, field title) -> field name
    for section, table, title, kind, rows in tables:
        if kind == "fields" and table != header_table:
            interface = interface_of(section, interfaces)
            if interface is None:
                remarks.append(f"Table {table} ({title}): section {section} belongs to no --interface; skipped")
                current = None
                continue
            current = (interface, title)
            type_id = summaries.get(interface, {}).get(title, "")
            if not type_id:
                remarks.append(f"Table {table} ({title}): no identifier found in a message summary table")
            messages.append([interface, title, type_id, "yes", "", "", section, table, ""])
            for row, hsection, htable in header_rows:
                add_field(fields, names, interface, title, row, hsection, htable, numbering, remarks)
            for row in rows:
                add_field(fields, names, interface, title, row, "", "", numbering, remarks)
        elif kind == "values" and current:
            subject = re.sub(r"\s+values$", "", title)
            field = names.get((*current, subject))
            if field is None:
                remarks.append(f"Table {table} ({title}): no field named {subject!r} in {current[1]}; skipped")
                continue
            for row in rows:
                value, meaning = row[0], row[1]
                if not value.isdigit() or meaning.lower() in UNMEANT:
                    continue        # a range, or a value with no meaning
                values.append([current[0], current[1], field, value, meaning])
    for interface, summary in summaries.items():
        defined = {row[1] for row in messages if row[0] == interface}
        for name, type_id in summary.items():
            if interface and name not in defined:
                messages.append([interface, name, type_id, "no", "", "", "", "", ""])
    return messages, fields, values, remarks


def interface_of(section, interfaces):
    for prefix, name in interfaces:
        if section == prefix or section.startswith(prefix + "."):
            return name
    return None


def add_field(fields, names, interface, message, row, section, table, numbering, remarks):
    title, word, bits, kind, lsb, unit, description = (row + [""] * 7)[:7]
    # A cell that wrapped at its hyphen reads "15- 12".
    word, bits = word.replace(" ", ""), bits.replace(" ", "")
    where = f"{interface}/{message}/{title}"
    if not WORD.match(word) or not BITS.match(bits):
        remarks.append(f"{where}: word {word!r} or bits {bits!r} was not read as numbers; row skipped")
        return
    note = ""
    if kind in NOTHING and title.lower() in UNMEANT:
        name = f"spare_{start_bit(word, bits, numbering)}"
        description = ""
    else:
        name = snake(title)
        if kind not in TYPES and kind not in ("U", "E"):
            note = f"Type {kind} in the document: not a type the dictionary has; see 01-reading-an-icd.md."
    names[(interface, message, title)] = name
    scale = "" if lsb in NOTHING or lsb == "1" else lsb
    fields.append([interface, message, name, word, bits, TYPES.get(kind, ""), scale, "",
                   "" if unit in NOTHING else UNIT_SPELLINGS.get(unit, unit), re.sub(r"\s+See Table \S+\.$", "", description),
                   section, table, note, "unreviewed"])


def pdf_text(path):
    try:
        done = subprocess.run(["pdftotext", "-layout", str(path), "-"], check=True, capture_output=True)
    except FileNotFoundError:
        sys.exit("icd-extract: pdftotext is not installed (poppler-utils on Debian and Ubuntu, poppler on Homebrew); "
                 "or make the text yourself and pass it with --text")
    except subprocess.CalledProcessError as error:
        sys.exit(f"icd-extract: pdftotext failed: {error.stderr.decode(errors='replace').strip()}")
    return done.stdout.decode("utf-8", errors="replace")


def write(path, header, rows):
    with open(path, "w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, lineterminator="\n")
        writer.writerow(header)
        writer.writerows(rows)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("document", nargs="?", type=Path, help="the PDF")
    parser.add_argument("--text", type=Path, help="a text file made with pdftotext -layout, instead of a PDF")
    parser.add_argument("--out", type=Path, required=True, help="directory to write the draft tables into")
    parser.add_argument("--interface", action="append", default=[], metavar="NAME=SECTION",
                        help="the section whose messages belong to an interface; repeat for more")
    parser.add_argument("--header-table", default="", metavar="N-M",
                        help="number of the table of header fields common to every message")
    parser.add_argument("--word-bits", type=int, required=True)
    parser.add_argument("--bit0", choices=["msb", "lsb"], required=True)
    parser.add_argument("--first-word", type=int, choices=[0, 1], required=True)
    parser.add_argument("--first-bit", type=int, choices=[0, 1], default=0)
    args = parser.parse_args(argv)
    if bool(args.document) == bool(args.text):
        parser.error("give a PDF or --text, not both")
    interfaces = []
    for pair in args.interface:
        name, _, section = pair.partition("=")
        if not name or not section:
            parser.error(f"--interface {pair!r} is not NAME=SECTION")
        interfaces.append((section, name))
    if not interfaces:
        parser.error("give at least one --interface NAME=SECTION")
    text = args.text.read_text(encoding="utf-8") if args.text else pdf_text(args.document)
    numbering = (args.word_bits, args.bit0, args.first_word, args.first_bit)
    messages, fields, values, remarks = draft(text, interfaces, args.header_table, numbering)
    args.out.mkdir(parents=True, exist_ok=True)
    write(args.out / "messages.csv", MESSAGE_COLUMNS, messages)
    write(args.out / "fields.csv", FIELD_COLUMNS, fields)
    write(args.out / "values.csv", VALUE_COLUMNS, values)
    for remark in remarks:
        print(f"remark: {remark}")
    defined = sum(1 for row in messages if row[3] == "yes")
    print(f"{defined} messages, {len(fields)} field rows (all unreviewed), {len(values)} values "
          f"written to {args.out}/ as a DRAFT")
    print("interfaces.csv is yours to write: the draft does not know the transport or the numbering.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
