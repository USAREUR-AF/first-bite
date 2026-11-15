# Vectors and validation

A vector is a payload and the values it must decode to. It is the only gate
that tests what a field means: the right bits, the right scale, the right
sign. It tests that only if its expected values were not produced by the
decoder it is testing.

## The rule

A vector's expected values are never produced by the decoder being tested.
Running `bite decode` on a payload and pasting the output into `expect` makes
a vector that passes whatever the dictionary says, including when it is wrong.

## Where expected values come from

| Source | Strength | How to use it |
|---|---|---|
| The document's own example messages | best: the authors' statement of what the bytes mean | copy the bytes into `payload_hex` and the stated values into `expect.fields`; name the example in `note` |
| Hand computation | good: independent of any code | choose values, work out each field's bits from the document's table, assemble the words; keep the working in `note` or beside the vector |
| A second, independent implementation | good, if it is independent | the vendor's tool, an older decoder, a few lines of Python `struct` written from the document, not from the dictionary |
| A capture, with values read from the sending or receiving system's own display or log | fair: shows what the system does, which may not be what the document says | record where the values were read; a disagreement with the document goes in the [issues register](03-transcription-workflow.md#errata-and-ambiguities) |
| A capture, with values from `bite decode` | none | not a vector |

Each vector says which of these it is, in `note` or `source` (rule `V002`).

## What a good set of vectors covers

| Cover | Because |
|---|---|
| every defined message, at least once (rule `C010`) | a message with no vector has only its structure checked |
| every field of the message, with a value that is not zero | zero decodes to zero at any bit position and under any scale |
| a negative value in every signed field | sign-magnitude or offset binary read as two's complement is right for positive values |
| a value with the top bit set in every field that crosses a byte or a word | a field one bit too short or shifted by one passes small values |
| a label, and an unlabelled value, in an enumerated field | checks the table and what a reserved value looks like |
| different values in neighbouring fields | equal neighbours hide a boundary in the wrong place |
| a payload cut short, where consumers must cope with one | a field past the end decodes to `null`; expect `null` |

## Making a vector

`bite vectors new` writes a vector with every field of the message listed and
nothing filled in but the message identifier and the spare bits:

```console
$ bite vectors new glasshouse --interface SUPV --message "Vent Command" --name storm
wrote glasshouse/vectors/storm.json
7 expected values are empty: fill them in from the document, then run bite vectors encode glasshouse --vector storm
```

Fill `expect.fields` from the document's example, put the example's bytes in
`payload_hex` and say where they came from in `note`. With everything to hand
it is one command, as in the [worked example](06-worked-example/README.md):

```console
$ bite vectors new glasshouse --interface ZONE --message Alarm --payload 050730040038cefc058000030fa0 --note "GZC-ICD-001 revision B, appendix B.5." --set message_id=5 --set message_length=7 --set zone_number=12 --set sequence_number=4 --set time_of_day=3723004 --set "alarm_code=Pump Fault" --set severity=Warning --set acknowledged=false --set occurrence_count=3 --set minutes_since_raised=125
wrote glasshouse/vectors/alarm.json
```

A value is written as the decoder shows it: the label where the field has one
for the value, `true` or `false` for a `bool`, the engineering value for a
scaled field, hex for `bytes`, the text for `ascii`. A scaled value may be
written as the document writes it: `16.00` matches a decoded `16`.

## Building the bytes

When the document gives values and no bytes, or when you choose the values,
`bite vectors encode` builds the payload: the inverse of decoding, for integer,
float, boolean, bytes and text fields, with scaled fields taking the
engineering value.

```console
$ bite vectors encode glasshouse --vector alarm
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

Encoding with the same tool does not make an independent vector. It makes the
bytes; the expectations still come from the document. A payload built by
`bite vectors encode` and decoded by `bite check` agrees with the dictionary
by construction, whether or not the dictionary agrees with the document.

| Use | What it gives |
|---|---|
| `bite vectors encode <dir> --vector <name>` on a vector that has the document's bytes | a cross-check in the other direction: the bytes built from the stated values must equal the document's bytes. A difference names the byte and the fields on it, and means a field, a spare bit or the example itself is wrong. Exit status 1. |
| `bite vectors encode <dir> --vector <name> --write` on a vector with values and no bytes | bytes for `payload_hex`. Then check them by hand against the table: the `raw` and `bits` columns are the working to compare. |
| `bite vectors encode <dir> --interface <name> --message <name> --set field=value ...` | a payload for an experiment, nothing written |

The encoder refuses a value the field cannot hold, such as 81.35 in a field
whose LSB is 0.1, and gives the nearest values on either side. `raw:<n>`
sets a field's integer directly. Fields given no value are left zero and
listed. An `ascii` field's padding is NUL, or space where the vector's own
payload has a space at that byte.

### By hand

For a message of 16-bit big-endian words, write each word as 16 binary digits,
most significant on the left. Place each field's raw integer at its bits, most
significant bit first, fill spares with zero, and read each word as four hex
digits. Word 5 of the Alarm example above:

| Field | Bits | Raw | Binary |
|---|---|---|---|
| Alarm Code | 15-8 | 5 | `00000101` |
| Severity | 7-6 | 2 | `10` |
| Acknowledged | 5 | 0 | `0` |
| Spare | 4-0 | 0 | `00000` |

Together `0000 0101 1000 0000`, which is `0580`, the fifth word of the
payload.

## Using a capture

A capture shows what a system sends. It is evidence about the document and the
dictionary, and it is not the truth about either: the sender can be wrong, the
capture can miss the cases that matter, and a dictionary bent until it fits
one capture has stopped describing the document.

`bite` has no network code. [`tools/extract-udp-payloads.py`](../../tools/extract-udp-payloads.py)
prints the UDP payloads of a pcap file as hex lines, and `bite stats` counts
what they hold:

```console
$ tools/extract-udp-payloads.py examples/weather-station.pcap --port 40100 > payloads.txt
5 payloads printed, 0 other frames, 0 fragments skipped
$ bite stats dictionaries/weather-station --port 40100 --payloads payloads.txt
WX: 5 payloads
  type 42: 1 payload, not named by the interface
WX/Heartbeat: 1 payload (1 of 12 bytes)
  message_type        1 distinct  Heartbeat x1
  message_length      1 distinct  12 x1
  station_id          1 distinct  KX7 x1
  uptime              1 distinct  86400 x1
  battery_low         1 distinct  false x1
  sensor_fault        1 distinct  true x1
  mode                1 distinct  Calibrating x1
  reserved            1 distinct  0 x1
```

(The output continues with the other messages and ends with the count of
things to look at.) A line of payloads is hex, with an optional frame
identifier before `#`, as `can-utils` writes it: `100#E02E28B4250200`.

| `bite stats` reports | It may mean | Look at |
|---|---|---|
| a field zero in every payload | the field is at the wrong bits; or the capture never exercised it | the field's row in the document; a capture taken while the quantity changes |
| a value with no label in an enumerated field | a label was missed; the document reserves the value and the sender uses it anyway; the field is at the wrong bits | the enumeration table |
| a spare field that is not zero | the bits have a meaning the document's table omits, or a neighbouring field is too narrow | the fields on either side |
| bits that belong to no field, set | a field was skipped | the document's table, row by row |
| payloads whose length differs from `declared_bits` | the length is wrong; the message has a variable part; another message shares the identifier | the message's stated length; the summary table |
| bytes after the last field that are not zero | a repeated group or trailer that is not transcribed | [what the format cannot express](01-reading-an-icd.md#what-the-format-cannot-express) |
| an identifier the interface does not name | a message missing from the inventory; a different interface on the same port | the summary table |
| a value that never leaves a narrow range, or jumps wildly | nothing, or a wrong scale, sign or byte order | the `min` and `max` against what the quantity can physically be |

Every flag is a place to look in the document. A change to the dictionary is
made because the document supports it, and the capture is cited as the reason
for looking. Where the capture and the document disagree and the document is
clear, the dictionary follows the document and the disagreement goes in the
issues register.

A capture of real traffic is handled as the system it was taken from is. Keep
it, and the payload lines made from it, out of the repository.
