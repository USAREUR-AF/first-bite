# Worked example: the glasshouse controller

A complete transcription, start to finish, of a synthetic interface document:
`GZC-ICD-001`, for an invented glasshouse zone controller. The document has
what real ones have: two interfaces, a common header, word-and-bit tables, LSB
weights, an offset-binary field, a sign-magnitude field, enumerations with
reserved ranges, fields that cross words, a message defined elsewhere, a
repeated group, an erratum and an ambiguity. The result is
[`dictionaries/glasshouse`](../../../dictionaries/glasshouse).

Everything here can be redone from the document alone.
[`retrace.sh`](retrace.sh) runs the steps below in a temporary directory and
compares what it builds with `dictionaries/glasshouse` byte for byte;
[`retrace.out`](retrace.out) is the output of one run, and the console blocks
on this page are taken from it.

```bash
docs/icd-to-dictionary/06-worked-example/retrace.sh
```

## What is here

| Path | Holds |
|---|---|
| [`icd/GZC-ICD-001-B.md`](icd/GZC-ICD-001-B.md), [`.pdf`](icd/GZC-ICD-001-B.pdf) | the document, revision B: 12 pages, 10 defined messages |
| [`tables/`](tables) | what a reader types from it: 2 interfaces, 11 messages, 116 fields, 30 values |
| [`icd-notes/issues.md`](icd-notes/issues.md) | the erratum and the ambiguity, and what was decided |
| [`revision-c/`](revision-c) | revision C of the document and the tables for it |
| [`retrace.sh`](retrace.sh), [`retrace.out`](retrace.out) | every command below, in order, and its output |

## 1. Scope and scaffold

The document is GZC-ICD-001 revision B; its distribution is unrestricted, so
the dictionary can live in this repository. It defines two interfaces
(section 2): `ZONE`, sent from UDP port 47010, and `SUPV`, sent to UDP port
47011.

```console
$ bite new glasshouse --name glasshouse --interface ZONE --port 47010 --interface SUPV --port 47011
wrote glasshouse/README.md
wrote glasshouse/SUPV/metadata.json
wrote glasshouse/ZONE/metadata.json
wrote glasshouse/dictionary.json
glasshouse: dictionary "glasshouse" with 2 interfaces and an empty vectors/
```

## 2. Inventory

Tables 5-1 and 6-1 name eleven messages. Ten are defined in the document. The
eleventh, Maintenance Data, is "defined in the maintenance manual GZC-MNT-004
and is not part of this document". All eleven get a row in
[`tables/messages.csv`](tables/messages.csv); Maintenance Data has `defined`
set to `no`, so the decoder names it when it sees identifier 7 and decodes
nothing.

## 3. Numbering

Section 3.1: 16-bit words, numbered from 1, sent most significant byte first;
bits numbered 15 to 0, bit 15 the most significant. In
[`tables/interfaces.csv`](tables/interfaces.csv) that is `word_bits` 16,
`bit0` `lsb`, `first_word` 1, `first_bit` 0, `word_order` `big`.

Three fields check it. The message identifier, word 1 bits 15-8, must be the
first byte, since appendix B.1 begins `0108` for message 1:

```console
$ bite range --word-bits 16 --bit0 lsb --first-word 1 --word 1 --bits 15-8
start 0
stop  8
width 8 bits, under "bit_order": "msb0"
numbering: 16-bit words numbered from 1, sent most significant byte first; bit 0 is the least significant bit of a word
word 1 (payload bytes 0-1)
  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
   #  #  #  #  #  #  #  #  .  .  .  .  .  .  .  .
```

Relative Humidity, word 6 bits 15-6, is a field in a later word that does not
end on a byte. Appendix B.2 gives word 6 as `CB6A` and the humidity as 81.3 %:
the top ten bits of `CB6A` are 813.

```console
$ bite range --word-bits 16 --bit0 lsb --first-word 1 --word 6 --bits 15-6
start 80
stop  90
width 10 bits, under "bit_order": "msb0"
numbering: 16-bit words numbered from 1, sent most significant byte first; bit 0 is the least significant bit of a word
word 6 (payload bytes 10-11)
  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
   #  #  #  #  #  #  #  #  #  #  .  .  .  .  .  .
```

Volume Delivered crosses words: "7-8" and "15-8" in Table 5-4, a 24-bit value.

```console
$ bite range --word-bits 16 --bit0 lsb --first-word 1 --word 7-8 --bits 15-8
start 96
stop  120
width 24 bits, under "bit_order": "msb0"
numbering: 16-bit words numbered from 1, sent most significant byte first; bit 0 is the least significant bit of a word
word 7 (payload bytes 12-13)
  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
   #  #  #  #  #  #  #  #  #  #  #  #  #  #  #  #
word 8 (payload bytes 14-15)
  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
   #  #  #  #  #  #  #  #  .  .  .  .  .  .  .  .
```

The identifier's `[0, 8)` and the length's `[8, 16)` go in `interfaces.csv` as
`type_start`, `type_stop`, `length_start` and `length_stop`.

## 4. `dictionary.json`

Name, version, the source document, two units the vocabulary lacks (the
document's W/m² and mS/cm), and one allowed finding: Maintenance Data is named
and not defined, on purpose.

```json
{
    "name": "glasshouse",
    "version": "1.0.0",
    "description": "Synthetic example: a glasshouse zone controller and its supervisor, transcribed from the invented document GZC-ICD-001 in docs/icd-to-dictionary/06-worked-example.",
    "source": {
        "document": "GZC-ICD-001",
        "revision": "B"
    },
    "lint": {
        "units": [
            "W/m2",
            "mS/cm"
        ],
        "allow": [
            {
                "rule": "C004",
                "at": "ZONE/Maintenance Data",
                "reason": "Defined in GZC-MNT-004, which is not transcribed here."
            }
        ]
    }
}
```

## 5. Transcribe

One row of [`tables/fields.csv`](tables/fields.csv) per row of the document's
tables, with `word` and `bits` as printed. Table 5-3, Climate Report, becomes:

```text
interface,message,field,word,bits,type,scale,offset,unit,description,source_section,source_table,note
ZONE,Climate Report,air_temperature,5,15-0,int,0.01,,degC,Air temperature at canopy height.,,,
ZONE,Climate Report,relative_humidity,6,15-6,,0.1,,%,Relative humidity at canopy height.,,,
ZONE,Climate Report,outside_temperature,7,15-4,,0.05,-102.4,degC,"Outside air temperature, offset binary: zero is 2048.",,,
ZONE,Climate Report,spare_108,7,3-0,,,,,,,,
ZONE,Climate Report,co2_concentration,8,15-0,,,,ppm,Carbon dioxide concentration.,,,
```

How each kind of row in the document was entered:

| In the document | Entered as | Rule |
|---|---|---|
| the header, Table 4-1 | the same five rows at the top of every message, with `source_section` 4 and `source_table` 4-1 | [header fields](../01-reading-an-icd.md#header-fields) |
| "Air Temperature", type S, LSB 0.01, °C | `air_temperature`, `int`, scale `0.01`, `degC` | names, two's complement, units |
| "Outside Temperature", type OB, 12 bits, LSB 0.05 | scale `0.05`, offset `-102.4`: minus 2048 x 0.05 | [offset binary](../01-reading-an-icd.md#signed-encodings) |
| "Roof Vent Offset", type SM, bits 15-6 | two rows: `roof_vent_offset_sign` at bit 15 with the labels Positive and Negative, `roof_vent_offset_magnitude` at bits 14-6 with scale `0.1`; a note on each | sign-magnitude is not built |
| "Volume Delivered", words 7-8, bits 15-8 | `word` `7-8`, `bits` `15-8` | [fields that cross words](../01-reading-an-icd.md#fields-that-cross-words) |
| "Door Open", type D | `bool` | discretes |
| "Zone Label", type A | `ascii` | |
| "Leaf Wetness", no unit; "Year"; "Message ID" | unit `1` | dimensionless fields |
| "Nutrient Solution pH", units "pH" | unit `1` | pH is a number |
| Spare, word 7 bits 3-0 | `spare_108`, no description | `spare_<start bit>` |
| Operating Mode values 0 to 4, and "5-15 Reserved" | five rows in `values.csv`; nothing for 5 to 15 | reserved values get no label |
| Alarm Code "200-255 Manufacturer specific" | no labels; a note on the field | a range with one meaning |
| Event Log: "Record Count event records follow" | the fixed five words only, `declared_bits` 80, a note on the message | a repeated group is not built |
| Message lengths in Tables 5-1 and 6-1, in words | `declared_bits`: words x 16 | |

## 6. The erratum

Table 5-3 of revision B prints Leaf Wetness as word 6, bits 6-0, beside
Relative Humidity at bits 15-6. Entered exactly as printed, the import
succeeds and the gates object:

```console
$ bite check --strict glasshouse
warning: ZONE/Climate Report/leaf_wetness: bits [89, 96) overlap relative_humidity [80, 90)
allowed: ZONE/Maintenance Data: named by message_type but has no file in messages/; it is identified but not decoded (Defined in GZC-MNT-004, which is not transcribed here.)
2 interfaces, 10 messages, 0 errors, 1 warnings (1 allowed in dictionary.json), 0/0 vectors passed
(exit status 1)
```

`bite coverage` names the shared bit: `overlap [89, 90) relative_humidity and
leaf_wetness`. The document's own text and example settle it: the description
says "a 6-bit value", and in appendix B.2 bits 5-0 of word 6 are 42, the
stated index. [ISSUE-001](icd-notes/issues.md) records the evidence and the
decision; the row is entered as `5-0` with a note naming the entry.
[ISSUE-002](icd-notes/issues.md) is an ambiguity settled the same way, from
appendix B.1.

## 7. Import and check

```console
$ bite import-csv tables --into glasshouse
wrote glasshouse/ZONE/messages/Climate Report.json
2 interfaces, 10 messages: 1 file written, 11 unchanged

$ bite fmt glasshouse
13 files, 0 rewritten

$ bite check --strict glasshouse
allowed: ZONE/Maintenance Data: named by message_type but has no file in messages/; it is identified but not decoded (Defined in GZC-MNT-004, which is not transcribed here.)
2 interfaces, 10 messages, 0 errors, 0 warnings (1 allowed in dictionary.json), 0/0 vectors passed

$ bite lint glasshouse
0 findings, 0 allowed in dictionary.json
```

`bite coverage --strict` still fails, on one rule: `C010`, ten messages with
no vector.

## 8. Vectors

Appendix B gives one example per message: the words in hex and the value of
each field. Each becomes a vector with the document's bytes and the document's
values. For B.2:

```console
$ bite vectors new glasshouse --interface ZONE --message "Climate Report" --payload 020a0c12029331e8ff06cb6a76d0019f0dea01a9 --note "GZC-ICD-001 revision B, appendix B.2." --set message_id=2 --set message_length=10 --set zone_number=3 --set sequence_number=18 --set time_of_day=43201000 --set air_temperature=-2.50 --set relative_humidity=81.3 --set leaf_wetness=42 --set outside_temperature=-7.35 --set co2_concentration=415 --set solar_irradiance=356.2 --set wind_speed=4.25
wrote glasshouse/vectors/climate-report.json
```

The values are written as the document gives them; "Set" and "Not set" become
`true` and `false`, and the sign-magnitude example "-12.5 deg" becomes
`roof_vent_offset_sign=Negative` and `roof_vent_offset_magnitude=12.5`.

`bite vectors encode --vector` then builds the bytes from those values and
compares them with the document's:

```console
$ bite vectors encode glasshouse --vector climate-report
payload_hex 020a0c12029331e8ff06cb6a76d0019f0dea01a9
field                bits          raw                 value
message_id           [0, 8)        0x2                 2
message_length       [8, 16)       0xa                 10
zone_number          [16, 22)      0x3                 3
sequence_number      [22, 32)      0x12                18
time_of_day          [32, 64)      0x29331e8           43201000
air_temperature      [64, 80)      0xff06              -2.50
relative_humidity    [80, 90)      0x32d               81.3
leaf_wetness         [90, 96)      0x2a                42
outside_temperature  [96, 108)     0x76d               -7.35
spare_108            [108, 112)    0x0                 0
co2_concentration    [112, 128)    0x19f               415
solar_irradiance     [128, 144)    0xdea               356.2
wind_speed           [144, 160)    0x1a9               4.25
These bytes were built with the dictionary. Check them against the document by hand or with a second implementation.
matches the vector's payload_hex
```

The example exercises what a careless transcription would get wrong: a
negative two's-complement temperature (`ff06`), a negative offset-binary one
(`0x76d` is 1901, below the zero of 2048) and the two fields of the erratum.
The Event Log example carries two records after the fixed part; the encoder
reports `the vector's payload has 8 more bytes after the definition ends; they
are not compared`.

## 9. The gates, and the comparison

```console
$ bite fmt --check glasshouse
23 files, 0 not canonical

$ bite check --strict glasshouse
allowed: ZONE/Maintenance Data: named by message_type but has no file in messages/; it is identified but not decoded (Defined in GZC-MNT-004, which is not transcribed here.)
2 interfaces, 10 messages, 0 errors, 0 warnings (1 allowed in dictionary.json), 10/10 vectors passed

$ bite lint glasshouse
0 findings, 0 allowed in dictionary.json

$ bite coverage --strict glasshouse
SUPV/Irrigation Command: 112 bits declared, 112 covered by 13 fields, 0 gaps, 1 vector
SUPV/Setpoint Command: 128 bits declared, 128 covered by 10 fields, 0 gaps, 1 vector
SUPV/Time Synchronisation: 112 bits declared, 112 covered by 11 fields, 0 gaps, 1 vector
SUPV/Vent Command: 96 bits declared, 96 covered by 9 fields, 0 gaps, 1 vector
ZONE/Alarm: 112 bits declared, 112 covered by 12 fields, 0 gaps, 1 vector
ZONE/Climate Report: 160 bits declared, 160 covered by 13 fields, 0 gaps, 1 vector
ZONE/Event Log: 80 bits declared, 80 covered by 7 fields, 0 gaps, 1 vector
ZONE/Irrigation Report: 144 bits declared, 144 covered by 16 fields, 0 gaps, 1 vector
ZONE/Vent Report: 112 bits declared, 112 covered by 12 fields, 0 gaps, 1 vector
ZONE/Zone Status: 128 bits declared, 128 covered by 13 fields, 0 gaps, 1 vector
ZONE/Maintenance Data: named in message_type, not defined
10 of 10 messages fully covered; 1 named and not defined; 0 defined and not named
103 fields: 0 without a source, 0 without a description; 0 of 81 numeric fields without a unit
0 findings failing, 0 not enforced by the flags given, 1 allowed in dictionary.json

$ diff -r glasshouse committed
identical
```

`committed` is a copy of `dictionaries/glasshouse`. The script then exports
the dictionary to CSV, imports that into an empty directory and compares
again: identical.

## 10. Revision C

[Revision C](revision-c/icd/GZC-ICD-001-C.md) lists five changes in its
revision history. The tables are edited for each
([`revision-c/tables`](revision-c/tables)), imported over a copy of the
dictionary, and `source.revision` is set to C. The vectors of two changed
messages now fail, as they should:

```console
$ bite check --strict glasshouse-c
allowed: ZONE/Maintenance Data: named by message_type but has no file in messages/; it is identified but not decoded (Defined in GZC-MNT-004, which is not transcribed here.)
vector irrigation-report: flow_rate: expected Some("12.50"), got Some("5")
vector vent-report: motor_current: no such field in the decoded message
2 interfaces, 10 messages, 0 errors, 0 warnings (1 allowed in dictionary.json), 8/10 vectors passed
(exit status 1)
```

The vectors of the three messages the revision changed are replaced from
revision C's appendix B, and the other seven are left as they were. `bite
diff` shows what the revision does to a consumer, and that it needs a major
version:

```console
$ bite diff --check-version glasshouse glasshouse-c
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
version 1.0.0 -> 1.0.0; the changes ask for a new major version: NOT sufficient
(exit status 1)
```

With the version set to `2.0.0` the gates pass and `bite diff
--check-version` succeeds. The revision C dictionary is built by the script
and not committed; `dictionaries/glasshouse` stays at revision B, version
1.0.0.

## What the example does not decode

| In the document | In the dictionary |
|---|---|
| the event records of Event Log (Table 5-7b) | not decoded: a repeated group. The message has its fixed five words and a note. |
| Roof Vent Offset as one signed value | two fields; the consumer applies the sign |
| Alarm Code values 200 to 255, "manufacturer specific" | decode to their number; a note states the range |
| Maintenance Data | identified, not decoded: defined in another document |
