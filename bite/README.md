# bite

The decoder: a library that loads a dictionary and turns a payload into named,
typed fields, and the `bite` command that checks dictionaries, decodes
payloads by hand and helps write a dictionary from an interface document. It has no network code; [`mind`](../mind) supplies the
payloads. The dictionary format is in
[docs/dictionary-format.md](../docs/dictionary-format.md).

## The `bite` command

Run it as `cargo run -p bite -- <command>`, or `bite <command>` once installed.

`check <dir> [--strict]` validates a dictionary and runs the vectors in its
`vectors/` directory. It exits non-zero on an error or a failed vector, and
with `--strict` on a warning too.

```console
$ bite check dictionaries/weather-station
1 interfaces, 2 messages, 0 errors, 0 warnings, 4/4 vectors passed
```

`list <dir>` prints each interface with its port, and each message with its
identifier, field count and length.

```console
$ bite list dictionaries/weather-station
WX (port 40100)
       1  Heartbeat  (8 fields, 96 bits)
       2  Observation  (9 fields, 184 bits)
```

`decode <dir> (--interface <name> | --port <port>) [--frame-id <id>] <hex>`
decodes one payload and prints the typed fields.

```console
$ bite decode dictionaries/weather-station --port 40100 0200174b583700fb2e18bcda9b4236000004030201beef
{
  "fields": {
    "checksum": "beef",
    "humidity": 45.5,
    "message_length": 23,
    "message_type": "Observation",
    "pressure": 101325,
    "sample_serial": 16909060,
    "station_id": "KX7",
    "temperature": -12.34,
    "wind_direction": 271.5
  },
  "interface": "WX",
  "message_type": "Observation"
}
```

`schema <dir>` prints one JSON Schema (draft 2020-12) per message, keyed
`<interface>/<message>` (`WX/Heartbeat`, `WX/Observation`); each property
carries its bit range as `x-bits` and its unit as `x-unit`.

## Writing and maintaining a dictionary

These commands take a dictionary from an interface document to a checked,
versioned release. The method they serve is
[docs/icd-to-dictionary](../docs/icd-to-dictionary/README.md); the outputs
below are from its worked example, [`dictionaries/glasshouse`](../dictionaries/glasshouse).

| Command | Does | Exit status 1 when |
|---|---|---|
| `new <dir> --interface <name> [--port <n>] [--name <name>] [--repo]` | scaffolds a dictionary; with `--repo`, a dictionary repository from [`tools/dictionary-repo-template`](../tools/dictionary-repo-template) | |
| `range` | converts between a document's word-and-bit addressing and `[start, stop)`, and draws the word | |
| `import-csv <tables> --into <dir>` | builds the interface files from `interfaces.csv`, `messages.csv`, `fields.csv`, `values.csv` | a row is wrong; nothing is written |
| `export-csv <dir> --out <tables>` | writes those four tables | a key has no column |
| `fmt <dir> [--check]` | rewrites every file in canonical form | `--check`: a file is not canonical |
| `coverage <dir> [--strict] [--require-...] [--json]` | reports covered bits, gaps, overlaps, undefined messages, fields without source, description or unit | a required rule has a finding |
| `lint <dir> [--json]`, `lint --rules`, `lint --units` | checks the [conventions](../docs/icd-to-dictionary/02-conventions.md) | a finding is not allowed in `dictionary.json` |
| `diff <old> <new> [--check-version] [--json]` | lists what changed for a consumer of the decoded records | a change is breaking; with `--check-version`, the version is raised by too little |
| `vectors new <dir> --interface <name> --message <name>` | scaffolds a vector with every field to fill in | |
| `vectors encode <dir> --vector <name> [--write]` | builds a payload from field values | the bytes differ from the vector's |
| `stats <dir> (--interface <name> or --port <n>) --payloads <file>` | counts the values each field takes in a set of payloads and flags what to look at | |

Exit status 2 is a usage error or a dictionary that cannot be read.

`range` is the step where a transcription most often goes wrong, so it shows
its working:

```console
$ bite range --word-bits 16 --word 3 --bits 12-5 --bit0 lsb --first-word 1
start 35
stop  43
width 8 bits, under "bit_order": "msb0"
numbering: 16-bit words numbered from 1, sent most significant byte first; bit 0 is the least significant bit of a word
word 3 (payload bytes 4-5)
  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
   .  .  .  #  #  #  #  #  #  #  #  .  .  .  .  .
```

`--start` and `--stop` convert the other way,
`--dictionary <dir> --interface <name>` takes the numbering from the
interface's `icd_numbering`, and `--word-order little` is for words sent least
significant byte first.

`fmt` puts keys in one order, fields by start bit, enumerated values in
numeric order and numbers in one spelling, so that two transcriptions of the
same definitions are the same bytes. It refuses a file with a key written
twice.

```console
$ bite fmt --check dictionaries/glasshouse
23 files, 0 not canonical
```

`coverage` measures each message against the length the document gives it.
Overlaps, fields past the declared length and unusable ranges always fail;
`--require-full-coverage`, `--require-declared`, `--require-defined`,
`--require-source`, `--require-description`, `--require-unit` and
`--require-vectors` add the other rules, and `--strict` is all of them.

```console
$ bite coverage dictionaries/vehicle-can --require-full-coverage
POWERTRAIN/Engine Status: 56 bits to the last field, none declared, 55 covered by 6 fields, 1 gap, 1 vector
  gap [39, 40) 1 bit
POWERTRAIN/Wheel Speeds: 52 bits to the last field, none declared, 52 covered by 4 fields, 0 gaps, 1 vector
1 of 2 messages fully covered; 0 named and not defined; 0 defined and not named
10 fields: 10 without a source, 10 without a description; 0 of 8 numeric fields without a unit
C001 POWERTRAIN/Engine Status: bits [39, 40) belong to no field (1 bit)
1 finding failing, 22 not enforced by the flags given, 0 allowed in dictionary.json
```

`lint` checks names, units, labels, descriptions, sources, the header fields
and the vectors. A finding that is right for the dictionary is allowed under
`lint.allow` in `dictionary.json`, with a reason.

```console
$ bite lint dictionaries/glasshouse
0 findings, 0 allowed in dictionary.json
$ bite lint --rules
N001  An interface name is upper-case letters, digits and underscores, starting with a letter.
N002  A message name is the document's name for the message: words of letters, digits and hyphens with single spaces between them.
...
```

`diff` compares two versions by what they decode. Classes, from most to least
severe: breaking, value, additive, documentation.

```console
$ bite diff glasshouse glasshouse-c
breaking (1)
  renamed    ZONE/Vent Report/motor_current: now drive_current at the same bits [100, 112)
value (2)
  resized    ZONE/Climate Report: declared_bits: 160 -> 176
  rescaled   ZONE/Irrigation Report/flow_rate: scale 0.25 offset 0 -> scale 0.1 offset 0
additive (3)
  relabelled ZONE/Alarm/alarm_code: value 8: label "Low Battery" added
  added      ZONE/Climate Report/dew_point: field added at [160, 176)
  relabelled ZONE/Zone Status/operating_mode: value 5: label "Ventilation Purge" added
documentation (2)
  documented dictionary.json: source: {"document": "GZC-ICD-001", "revision": "B"} -> {"document": "GZC-ICD-001", "revision": "C"}
  documented ZONE/Climate Report/leaf_wetness: note: "Table 5-3 prints bits 6-0, which overlap Relative Humidity; transcribed as bits 5-0. See ISSUE-001 in icd-notes/issues.md." -> none
version 1.0.0 -> 2.0.0; the changes ask for a new major version: sufficient
```

`vectors encode` is the inverse of `decode`, for integer, float, boolean,
bytes and text fields; a scaled field takes the engineering value and a
labelled field its label. It builds the bytes. It does not make a vector
independent: the expected values still come from the document.

```console
$ bite vectors encode dictionaries/glasshouse --vector alarm
payload_hex 050730040038cefc058000030fa0
field                 bits          raw                 value
message_id            [0, 8)        0x5                 5
message_length        [8, 16)       0x7                 7
zone_number           [16, 22)      0xc                 12
sequence_number       [22, 32)      0x4                 4
time_of_day           [32, 64)      0x38cefc            3723004
alarm_code            [64, 72)      0x5                 Pump Fault
severity              [72, 74)      0x2                 Warning
acknowledged          [74, 75)      0x0                 false
spare_75              [75, 80)      0x0                 0
occurrence_count      [80, 96)      0x3                 3
minutes_since_raised  [96, 107)     0x7d                125
spare_107             [107, 112)    0x0                 0
These bytes were built with the dictionary. Check them against the document by hand or with a second implementation.
matches the vector's payload_hex
```

`stats` reads payloads as hex, one per line, from a file or `-` for standard
input; [`tools/extract-udp-payloads.py`](../tools/extract-udp-payloads.py)
makes such a file from a pcap. It flags a field that is zero in every
payload, a value with no label, spare bits that are set, bits no field covers
that are set, and lengths that differ from the declared one.

`new`, `import-csv`, `export-csv` and `vectors new` are shown step by step in
the [worked example](../docs/icd-to-dictionary/06-worked-example/README.md).

## Code

| File | What |
|---|---|
| [`src/dictionary.rs`](src/dictionary.rs) | loading, validation and indexing of a dictionary, from a directory (`Dictionary::load`) or from files in memory (`Dictionary::from_files`) |
| [`src/schema.rs`](src/schema.rs) | the types a dictionary file deserialises into |
| [`src/bits.rs`](src/bits.rs), [`src/decode.rs`](src/decode.rs) | bit extraction; payload to fields |
| [`src/vectors.rs`](src/vectors.rs), [`src/jsonschema.rs`](src/jsonschema.rs) | running test vectors; JSON Schema export |
| [`src/authoring/`](src/authoring) | the library behind the commands for writing a dictionary: `canon` (fmt), `range`, `table` and `csv`, `coverage`, `lint` and `rules`, `diff`, `encode`, `stats`, `scaffold`; `json` and `model` read a dictionary's files without dropping anything |
| [`src/main.rs`](src/main.rs), [`src/authoring_cli.rs`](src/authoring_cli.rs) | the command |
| [`tests/`](tests) | loading and decoding tests against [`dictionaries/`](../dictionaries) |
