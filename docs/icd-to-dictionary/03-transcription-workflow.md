# Transcription workflow

Transcription is a loop: enter a message's fields as the document prints them,
build the dictionary from the tables, run the gates, fix what they find. This
page gives the loop, the tables column by column, and what to do when the
document is wrong or unclear.

## The loop

```bash
bite new crane-dictionary --repo --name crane --interface LINK --port 5000   # once
cd crane-dictionary
# enter rows in tables/*.csv from the document
make import     # bite import-csv tables --into dictionary
make check      # bite fmt --check, check --strict, lint, coverage --strict
```

| Step | Do | Until |
|---|---|---|
| 1 | Fill `interfaces.csv`: transport, the message identifier's bits, the document's numbering. | `bite range --dictionary dictionary --interface LINK --word 1 --bits 15-8` draws the header's first field where the document's figure has it |
| 2 | Fill `messages.csv` from the document's message summary: every name and identifier, `defined` set to `no`. | the row count equals the summary table's |
| 3 | Take one message. Set `defined` to `yes`, enter its description, its length as `declared_bits`, its section and table. Enter its fields in `fields.csv` and its enumerations in `values.csv`. | every row of the document's table has a row |
| 4 | `make import` | it writes the message file |
| 5 | `make check` | `bite coverage` shows the message with 0 gaps and its declared length covered |
| 6 | Add a vector from the document's example of the message. | `bite check` passes it |
| 7 | Commit the tables and the dictionary together. Next message. | |

Work a message at a time. A gap or an overlap found while the table is still
open on the desk costs a minute; found a week later it costs a search.

`bite import-csv` writes the dictionary in canonical form, so `bite fmt` has
nothing to change after an import. Run `bite fmt` after editing a JSON file by
hand.

## The gates

| Command | Passes when | Exit status |
|---|---|---|
| `bite fmt --check <dir>` | every file is in canonical form; no key is written twice | 1 if a file would change |
| `bite check --strict <dir>` | no error, no warning that `lint.allow` does not cover, every vector passes | 1 otherwise |
| `bite lint <dir>` | no finding of the [naming, unit, label, description, source, header and vector rules](02-conventions.md) | 1 on a finding |
| `bite coverage --strict <dir>` | no gap, overlap or field past the declared length; every message defined, declared and exercised by a vector; every field with a source, a description and a unit | 1 on a finding |

A finding that is right for the dictionary is
[allowed with a reason](02-conventions.md#allowing-a-finding), never worked
around by leaving a gate out.

## The tables

`bite import-csv <directory> --into <dictionary>` reads four files and writes
the interface directories; it leaves `dictionary.json` and `vectors/` alone.
`bite export-csv <dictionary> --out <directory>` writes the four files. The
two are inverse: exporting a dictionary and importing the result reproduces
its files byte for byte, and
[the test](../../bite/tests/authoring.rs) `export_then_import_reproduces_every_shipped_dictionary`
holds every shipped dictionary to that.

The files are UTF-8, comma-separated as in RFC 4180, with a header row.
Columns may be in any order and any column but the required ones may be left
out. A column the importer does not know is an error; so is every problem in a
row, reported with its file and row number, and nothing is written until
there are none.

### `interfaces.csv`: one row per interface

| Column | Meaning |
|---|---|
| `interface` | required: the interface name |
| `description` | free text |
| `port` | UDP or TCP port |
| `network`, `transport` | `protocol` in `metadata.json`, such as `ip` and `udp` |
| `bit_order` | `msb0` or `lsb0`; `msb0` when empty |
| `byte_order` | `big` or `little`; `big` when empty |
| `type_source` | `payload` or `frame_id`; `payload` when empty |
| `type_start`, `type_stop` | the bits of the message identifier, as `[start, stop)` |
| `type_mask` | for `frame_id`: the mask |
| `length_start`, `length_stop` | where a message states its own length |
| `framing` | `single` or `concatenated`, see [Framing](../dictionary-format.md#framing); `single` when empty |
| `icd_version`, `icd_date` | the older `icd` key; prefer the `source_` columns |
| `word_bits`, `bit0`, `first_word`, `first_bit`, `word_order` | the document's numbering, [`icd_numbering`](../dictionary-format.md#icd_numbering); needed for the `word` and `bits` columns of `fields.csv` |
| `source_document`, `source_revision`, `source_section`, `source_table`, `source_page` | the interface's `source` |
| `note` | a remark |

### `messages.csv`: one row per message

| Column | Meaning |
|---|---|
| `interface`, `message` | required |
| `type_id` | required: the identifier that selects the message, decimal or `0x` hex; several separated by spaces; empty for a message nothing selects |
| `defined` | required: `yes` writes a message file; `no` only names the message in `message_type` |
| `description` | free text |
| `declared_bits` | the message's length in bits |
| `source_document`, `source_revision`, `source_section`, `source_table`, `source_page` | the message's `source` |
| `note` | a remark |

### `fields.csv`: one row per field

| Column | Meaning |
|---|---|
| `interface`, `message`, `field` | required |
| `word`, `bits` | the field as the document addresses it: `3` and `12-5`; `7-8` and `15-8` for a field across words |
| `start`, `stop` | the field as the dictionary addresses it |
| `type` | `uint`, `int`, `float`, `bool`, `bytes`, `ascii`; empty for `uint` |
| `signed` | the older spelling of `int`; leave empty |
| `byte_order` | only where it differs from the interface's |
| `scale`, `offset` | numbers |
| `unit` | from the [vocabulary](02-conventions.md#units) |
| `description` | free text |
| `source_document`, `source_revision`, `source_section`, `source_table`, `source_page` | the field's `source`, where it differs from the message's |
| `note` | a remark |
| `review` | empty or `reviewed`; `unreviewed` makes the importer refuse the row |

Give `word` and `bits`, or `start` and `stop`. Type what the document prints
and let the importer convert: that removes the arithmetic from the step where
most mistakes are made. When a row has both forms, they must agree, and a
disagreement is an error:

```text
fields.csv: row 62: start 73 and stop 74 disagree with word 5 bits 7-6, which is [72, 74) under msb0
```

`bite export-csv` writes both forms when the interface has a numbering, so a
reviewer can read either.

### `values.csv`: one row per enumerated value

| Column | Meaning |
|---|---|
| `interface`, `message`, `field` | required: a field that has a row in `fields.csv` |
| `value` | required: the raw integer, decimal or `0x` hex |
| `label` | required: the label |

### In a spreadsheet

Open and save the files as UTF-8 CSV. Format the `word`, `bits`, `value` and
`type_id` columns as text before typing: a spreadsheet left to itself turns
`12-5` into a date. Commit `tables/` with `dictionary/` in the same change, so
that either can be reviewed.

## A draft from the PDF

[`tools/icd-extract`](../../tools/icd-extract) reads the text layer of a PDF
and drafts `messages.csv`, `fields.csv` and `values.csv` from tables laid out
like the worked example's. It is optional.

Restricted documents must not be sent to any external service to do this step.
Extraction output is a starting point, never a source of truth.

The helper runs on your machine with `pdftotext` and the Python standard
library; it makes no network connection and calls no OCR service or language
model. Every row it writes is marked `unreviewed`, and `bite import-csv`
refuses such rows:

```text
fields.csv: row 8: the row is marked unreviewed; check it against the document and clear the mark
```

Clear the mark on a row when you have read that row against the document.
`--allow-unreviewed` imports a draft as it stands, to see what the gates make
of it; a dictionary built that way is not committed. What the helper gets
right and wrong on the worked example is in
[its README](../../tools/icd-extract/README.md).

## Two readers

A transcription error that produces a valid dictionary passes every gate: 0.1
typed for 0.01, "High" and "Low" exchanged, a table row skipped where a spare
was. Vectors catch those the document gives an example for. A second person
catches the rest, in one of two ways.

| Way | How | Use when |
|---|---|---|
| Second reader | A person who did not transcribe the message reads every row of `fields.csv` and `values.csv` against the document, with the [review checklist](checklists.md#review). | always; it is the review of the pull request |
| Double entry | Two people transcribe the same messages into separate table directories, each imports and runs `bite fmt`, and `bite diff first second` lists every difference. Each difference is settled against the document. | the messages a wrong value would hurt most |

Double entry works because the conventions and `bite fmt` leave no room for
two correct transcriptions to differ, apart from the wording of notes:
whatever `bite diff` reports, one of the two got wrong.

## Errata and ambiguities

Documents contain mistakes: a table whose bit ranges overlap, an LSB weight
that disagrees with the range beside it, a length that counts the header in
one message and not in another. Do not silently choose.

| Step | Do |
|---|---|
| 1 | Add an entry to `icd-notes/issues.md` the moment you see the problem: where, what the document says, why that cannot be right or the two readings. |
| 2 | Transcribe as the document is written, if that is possible, and run the gates. An overlap or a gap confirms the problem and belongs in the entry. |
| 3 | Look for evidence in the document itself: an example message, the stated width of the field, the total length of the message, another table. |
| 4 | Decide, and record the decision and its evidence in the entry. If the evidence does not settle it, leave the status `open`, keep the document's reading, and ask the document's owner. |
| 5 | Put a `note` on the field that states what was done and names the entry. |
| 6 | When a later revision fixes it, set the entry to `fixed in <revision>` and remove the note. |

The entry form is in
[`tools/dictionary-repo-template/icd-notes/issues.md`](../../tools/dictionary-repo-template/icd-notes/issues.md),
and the worked example has
[one erratum and one ambiguity](06-worked-example/icd-notes/issues.md).

## The pull request

One message, or one group of related messages, per pull request. The template
in the dictionary repository carries the checklist; it is also in
[checklists](checklists.md). The reviewer is the second reader.
