# Conventions

Two people who transcribe the same document should produce the same
dictionary, byte for byte. These conventions settle every choice a transcriber
would otherwise make alone, and `bite lint` and `bite coverage` check them.
`bite fmt` settles layout: key order, field order, indentation and the
spelling of numbers.

One thing is left free: the wording of a `note`. Everything else in a message
file follows from the document and this page.

## Names

| Thing | Rule | Example |
|---|---|---|
| Dictionary | lower case, words joined by hyphens | `glasshouse` |
| Interface (directory) | the document's short name for the interface, upper case with underscores | `ZONE`, `LINK_16A` |
| Message (file name) | the document's name for the message, word for word, without "message" added or removed | `Climate Report.json` |
| Field | the document's name for the field in `snake_case`: lower case, every run of other characters one underscore. Nothing abbreviated, nothing expanded. | "CO2 Concentration" is `co2_concentration`; "Nutrient Solution pH" is `nutrient_solution_ph` |
| Spare or reserved bits | `spare_<start bit>` or `reserved_<start bit>`, whichever word the document uses | `spare_72` |
| The two halves of a sign-magnitude field | `<name>_sign`, `<name>_magnitude` | `roof_vent_offset_sign` |
| Vector (file name) | the message name in lower case with hyphens; a suffix for a second vector of one message | `climate-report.json`, `climate-report-negative.json` |

The header fields have the same names in every message of an interface.

| Rule | Requires | Checked by |
|---|---|---|
| `N001` | An interface name is upper-case letters, digits and underscores, starting with a letter. | `bite lint` |
| `N002` | A message name is the document's name for the message: words of letters, digits and hyphens with single spaces between them. | `bite lint` |
| `N003` | A field name is snake_case: lower-case letters and digits, words joined by single underscores, starting with a letter. | `bite lint` |
| `N004` | Bits with no meaning are named spare_<start bit> or reserved_<start bit>. | `bite lint` |
| `N005` | Two message names in one interface differ by more than letter case. | `bite lint` |

## Units

A unit is written one way. The vocabulary is SI first, with the non-SI units
interface documents use. Convert the document's spelling: "°C" is `degC`,
"m/s²" is `m/s2`, "µs" is `us`. `bite lint --units` prints this table.

| Unit | Meaning |
|---|---|
| `1` | dimensionless: a count, an index, an identifier, a ratio |
| `%` | percent |
| `ppm` | parts per million |
| `dB` | decibel |
| `dBm` | decibel relative to one milliwatt |
| `s` | second |
| `ms` | millisecond |
| `us` | microsecond |
| `ns` | nanosecond |
| `min` | minute |
| `h` | hour |
| `d` | day |
| `m` | metre |
| `km` | kilometre |
| `cm` | centimetre |
| `mm` | millimetre |
| `ft` | foot |
| `nmi` | nautical mile |
| `m/s` | metre per second |
| `km/h` | kilometre per hour |
| `kn` | knot |
| `ft/min` | foot per minute |
| `mm/h` | millimetre per hour |
| `m/s2` | metre per second squared |
| `deg` | degree of angle |
| `rad` | radian |
| `mrad` | milliradian |
| `arcmin` | minute of arc |
| `arcsec` | second of arc |
| `deg/s` | degree per second |
| `rad/s` | radian per second |
| `rpm` | revolution per minute |
| `kg` | kilogram |
| `g` | gram |
| `N` | newton |
| `Nm` | newton metre |
| `Pa` | pascal |
| `hPa` | hectopascal |
| `kPa` | kilopascal |
| `bar` | bar |
| `J` | joule |
| `Wh` | watt hour |
| `kWh` | kilowatt hour |
| `W` | watt |
| `kW` | kilowatt |
| `V` | volt |
| `mV` | millivolt |
| `A` | ampere |
| `mA` | milliampere |
| `ohm` | ohm |
| `Hz` | hertz |
| `kHz` | kilohertz |
| `MHz` | megahertz |
| `GHz` | gigahertz |
| `degC` | degree Celsius |
| `K` | kelvin |
| `L` | litre |
| `L/min` | litre per minute |
| `m3` | cubic metre |
| `bits` | bits |
| `bytes` | bytes of eight bits |
| `words` | words of the interface's word size |
| `bit/s` | bit per second |
| `kbit/s` | kilobit per second |
| `Mbit/s` | megabit per second |

Every numeric field without labels states a unit. A count, an index, an
identifier or a ratio takes `1`. A unit the vocabulary lacks is added for one
dictionary under `lint.units` in `dictionary.json`; the
[worked example](06-worked-example/README.md) adds `W/m2` and `mS/cm`.

| Rule | Requires | Checked by |
|---|---|---|
| `U001` | A unit is one of the unit vocabulary, or is listed under lint.units in dictionary.json. | `bite lint` |
| `U002` | A field with a scale, an offset or type float states its unit. | `bite lint` |

## Labels

A label is the document's wording for the value, capitalised as the document
has it. Values the document marks reserved, spare or undefined get no label:
they decode to their number. Keys are decimal unless the document lists the
values in hexadecimal, and one `values` object uses one radix.

| Rule | Requires | Checked by |
|---|---|---|
| `L001` | A label is not empty and has no space at either end. | `bite lint` |
| `L002` | Within one field, a label names one value. | `bite lint` |
| `L003` | Values the document leaves reserved, spare or undefined are not given a label. | `bite lint` |
| `L004` | The keys of one values object are all decimal or all hexadecimal. | `bite lint` |

## Descriptions

A description is the document's description of the item, word for word, as one
sentence or more: it starts with a capital letter or a digit and ends with a
full stop. A reference to an enumeration table ("See Table 5-2a.") is left
out, since the values are in the dictionary. Spare fields have none.

| Rule | Requires | Checked by |
|---|---|---|
| `D001` | Every interface and every message has a description. | `bite lint` |
| `D002` | A description is a sentence: it starts with a capital letter or a digit, ends with a full stop and has no space at either end. | `bite lint` |

## Sources

Every definition can be traced to a place in the document. The `source`
object is described in [the dictionary format](../dictionary-format.md#source);
a level inherits what it does not state from the level above.

| Level | States |
|---|---|
| `dictionary.json` | `document` and `revision` |
| Interface | the section that defines the interface |
| Message | the `section` and `table` of its definition; `page` where the document has stable page numbers |
| Field | nothing, when it is defined in its message's table; its own `section` and `table` when it comes from elsewhere, as header fields do |
| Vector | a `note` or a `source` naming the example it was taken from |

Two interfaces from different documents each state their own `document` and
`revision`.

| Rule | Requires | Checked by |
|---|---|---|
| `P001` | Every interface resolves a source document and revision, its own or the dictionary's. | `bite lint` |
| `P002` | Every message has a source with a section, a table or a page. | `bite lint` |

## One fact, one place

| Fact | Its one place |
|---|---|
| Document and revision | `source` in `dictionary.json` |
| The numbering of words and bits | `icd_numbering` in `metadata.json` |
| A message's identifier | `message_type.values` in `metadata.json`; a message file does not repeat it |
| A message's length | `declared_bits` in the message file |
| Signedness | `"type": "int"`; the older `signed` key is not used |
| A default | nowhere: `"type": "uint"`, `"scale": 1`, `"offset": 0` and a `byte_order` equal to the interface's are left out |
| An erratum or an ambiguity | the issues register, [`icd-notes/issues.md`](03-transcription-workflow.md#errata-and-ambiguities); the field's `note` names the entry |

| Rule | Requires | Checked by |
|---|---|---|
| `S001` | A signed integer is written "type": "int"; the signed key is not used. | `bite lint` |
| `S002` | A key that only repeats a default is left out: "type": "uint", a byte_order equal to the interface's, "scale": 1, "offset": 0. | `bite lint` |
| `S003` | dictionary.json gives a name and a version of the form MAJOR.MINOR.PATCH. | `bite lint` |
| `H001` | Every message of an interface has a field on exactly the message_type bits and one on exactly the message_length bits, with the same names throughout the interface. | `bite lint` |
| `V001` | A vector expects something: a message type or at least one field. | `bite lint` |
| `V002` | A vector says where its expected values come from, in a source or a note. | `bite lint` |

## `dictionary.json`

```json
{
    "name": "glasshouse",
    "version": "1.0.0",
    "description": "Synthetic example: a glasshouse zone controller and its supervisor.",
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

## Versions

The dictionary's version and the document's revision are separate facts. The
revision, in `source.revision`, says what was transcribed. The version, in
`version`, says what a consumer of the decoded records can rely on, and is
`MAJOR.MINOR.PATCH`.

| A change that | Example | Raises |
|---|---|---|
| removes or renames a field or message, changes a type or a unit, removes or changes a label | "Motor Current" becomes "Drive Current" | MAJOR |
| changes the value decoded from the same bytes: bits moved, a scale or offset corrected, a length changed | a scale of 0.25 becomes 0.1 | MINOR |
| adds a message, a field or a label | a new word at the end of a message | MINOR |
| changes only descriptions, sources or notes | the revision letter | PATCH |

`bite diff <old> <new>` classifies every change this way and
`bite diff --check-version` fails when the version was raised by less than the
changes ask for. A new revision of the document does not by itself raise
MAJOR: the changes it brings decide. A dictionary starts at `0.1.0` and
becomes `1.0.0` at its first reviewed release.

## Repository layout

A dictionary lives in its own repository, made by `bite new <dir> --repo`
from [`tools/dictionary-repo-template`](../../tools/dictionary-repo-template):

```text
<repository>/
  README.md                         the handling statement, first
  dictionary/                       what BITE_DICT_PATH points at
    dictionary.json
    <INTERFACE>/metadata.json
    <INTERFACE>/messages/<Message Name>.json
    vectors/<name>.json
  tables/                           the four CSV files the dictionary is entered in
  icd-notes/issues.md               errata and ambiguities, and what was decided
  CHANGELOG.md                      one entry per dictionary version
  Makefile                          make check: the gates
  .github/workflows/check.yml       make check on every pull request, with a pinned bite
  .github/pull_request_template.md  the review checklist
  CODEOWNERS                        who must review
```

## Handling

A dictionary transcribes its source document and inherits that document's
handling. If the document is export-controlled, proprietary or otherwise
restricted, so are the dictionary, the tables it was entered in, the issues
register, the vectors and the output of every check, because each of them
restates the document.

| Rule | In practice |
|---|---|
| Keep the dictionary in a repository whose access matches the document's | the repository's access list is the list of people who may hold the document |
| State the handling where it cannot be missed | the first section of the repository's `README.md` carries the document's distribution statement word for word |
| The decoder finds the dictionary by path | `BITE_DICT_PATH=/path/to/repository/dictionary`; nothing is copied into first-bite |
| The public tools check it where it is | `bite fmt`, `check`, `lint`, `coverage` and `diff` take a directory and send nothing anywhere; the workflow in the template runs them inside the private repository |
| Nothing derived from the document goes to an outside service | no field table pasted into a chat, an issue tracker, an online converter, OCR or a language model; [extraction](03-transcription-workflow.md#a-draft-from-the-pdf) runs on your machine |
| A capture is handled as the system it was taken from | captures stay out of the repository; `.gitignore` in the template refuses them |

The first-bite repository holds only synthetic dictionaries. Its
[`tools/check-public-tree.sh`](../../tools/check-public-tree.sh) scans every
file for a list of names that must not appear, which is how a restricted
dictionary is kept from entering by mistake.

## Coverage rules

`bite coverage` measures what the dictionary covers of each message. Three
rules always fail it; a flag turns each of the others into a failure, and
`--strict` turns on all of them.

| Rule | Requires | Checked by |
|---|---|---|
| `C001` | Every bit of a message up to its length belongs to a field. | `bite coverage --require-full-coverage` |
| `C002` | No two fields of a message share a bit. | `bite coverage` |
| `C003` | No field reaches past the message's declared_bits. | `bite coverage` |
| `C004` | Every message named in message_type has a definition. | `bite coverage --require-defined` |
| `C005` | Every message definition is named in message_type. | `bite coverage --require-defined` |
| `C006` | Every message states its length in declared_bits. | `bite coverage --require-declared` |
| `C007` | Every field resolves a complete source: document, revision and a section, table or page. | `bite coverage --require-source` |
| `C008` | Every field that carries meaning has a description. | `bite coverage --require-description` |
| `C009` | Every numeric field without labels states its unit. | `bite coverage --require-unit` |
| `C010` | Every message definition is exercised by at least one vector. | `bite coverage --require-vectors` |
| `C011` | Every field has a start and a stop with stop greater than start. | `bite coverage` |

## Allowing a finding

A finding that is right for this dictionary is allowed in `dictionary.json`,
with the rule, the place and a reason a reviewer can check:

```json
{"rule": "C004", "at": "ZONE/Maintenance Data", "reason": "Defined in GZC-MNT-004, which is not transcribed here."}
```

`at` covers the location and everything under it; without `at` the entry
covers the whole dictionary. An entry with no reason, or one that no longer
matches anything, is itself a finding. `C011` and the two rules below cannot
be allowed.

| Rule | Requires | Checked by |
|---|---|---|
| `X001` | An entry of lint.allow names a rule that exists and gives a reason. | `bite lint` |
| `X002` | An entry of lint.allow matches at least one finding. | `bite lint` |
