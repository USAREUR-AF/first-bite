# icd-extract

An optional helper that drafts dictionary tables from the field tables in a
PDF. It reads the PDF's text layer with `pdftotext -layout` and writes
`messages.csv`, `fields.csv` and `values.csv` in the form
[`bite import-csv`](../../docs/icd-to-dictionary/03-transcription-workflow.md#the-tables)
reads. Typing the tables by hand needs nothing from this directory.

Restricted documents must not be sent to any external service to do this step.
Extraction output is a starting point, never a source of truth.

| | |
|---|---|
| Runs | on your machine: Python 3 standard library and `pdftotext` (poppler) |
| Sends | nothing: no network, no OCR service, no language model |
| Reads | a PDF with a text layer, or text already made with `pdftotext -layout` (`--text`). A scanned document has no text layer and gives nothing. |
| Writes | a draft in which every field row is marked `unreviewed` |
| Status | built for tables laid out like the worked example's; measured on its PDF |

## Use

```console
$ tools/icd-extract/icd_extract.py docs/icd-to-dictionary/06-worked-example/icd/GZC-ICD-001-B.pdf --out draft --interface ZONE=5 --interface SUPV=6 --header-table 4-1 --word-bits 16 --bit0 lsb --first-word 1
10 messages, 115 field rows (all unreviewed), 28 values written to draft/ as a DRAFT
interfaces.csv is yours to write: the draft does not know the transport or the numbering.
```

| Option | Meaning |
|---|---|
| `--interface NAME=SECTION` | the messages defined under section `SECTION` belong to interface `NAME`; repeat for each interface |
| `--header-table N-M` | the table of header fields; its rows are put at the top of every message |
| `--word-bits`, `--bit0`, `--first-word`, `--first-bit` | the document's numbering, used to name spare fields `spare_<start bit>` |
| `--text FILE` | read text made by `pdftotext -layout` in place of a PDF |

`bite import-csv` refuses a row marked `unreviewed`. Read each row against the
document, correct it, and clear the mark or set it to `reviewed`.
`--allow-unreviewed` imports the draft as it is, to see what `bite check` and
`bite coverage` make of it before review; the result is not a dictionary to
commit.

## What it recognises

A table titled `Table N-M: Title` whose header row is `Field`, `Word`, `Bits`,
`Type`, `LSB`, `Units`, `Description`; a value table titled
`Table N-Ma: <Field> values` with the columns `Value` and `Meaning`; and a
message summary whose first columns are `ID` and `Message`. Cells that wrap
onto a second line are joined. A document laid out differently needs the
patterns at the top of `icd_extract.py` changed; the layouts of interface
documents vary too much for one set of patterns.

## Measured on the worked example

`test_icd_extract.py` runs the helper on
`docs/icd-to-dictionary/06-worked-example/icd/GZC-ICD-001-B.pdf` (12 pages, 11
field tables, 6 value tables) and compares the draft with the reviewed
`tables/`.

| | Count |
|---|---|
| Field rows in the reviewed table | 116 |
| Field rows drafted | 115 |
| Drafted rows with word and bits as the document prints them | 115 |
| Drafted rows identical to the reviewed row | 76 |
| Rows where the reviewer set only the unit, to `1` for a dimensionless field | 36 |
| Rows where the reviewer did more | 3: the offset of the offset-binary field; the erratum of Table 5-3; the sign-magnitude field, split into two rows |
| Enumerated values drafted, all as reviewed | 28 of 30; the other two are the labels of the sign field the reviewer created |
| Messages and identifiers drafted | 10 defined and 1 named, all as reviewed; descriptions and lengths are left empty |

The draft copies the document, including its mistakes: it reproduced the
overlapping bit range of Table 5-3 exactly as printed. That is the reason
every row is reviewed and the gates are run afterwards.

## Tests

```bash
python3 tools/icd-extract/test_icd_extract.py
```

The layout test needs only Python. The test against the worked example is
skipped where `pdftotext` is not installed.
