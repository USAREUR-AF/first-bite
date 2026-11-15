# Checklists

Three lists to print: one for the person transcribing, one for the person
reviewing, one for the release. Each item is something a tool cannot decide
for you, or the command that decides it.

## Transcription

Once per document:

- [ ] The document's identifier, revision and distribution statement are written down, and the repository's access matches the statement.
- [ ] `README.md` of the repository opens with the distribution statement, word for word.
- [ ] `dictionary.json` has the name, the version and `source.document` and `source.revision`.
- [ ] Every interface has a row in `interfaces.csv` with its transport, port, identifier bits and numbering.
- [ ] The numbering was tested on a known field in the first word and on one in a later word, with `bite range`.
- [ ] Every message the document names has a row in `messages.csv`, defined or not, and the row count equals the document's summary table.

For each message:

- [ ] `declared_bits` is the document's stated length, converted to bits.
- [ ] The message row has the section and table of its definition.
- [ ] Every row of the document's table has a row in `fields.csv`, spares included, with `word` and `bits` as printed.
- [ ] Header fields have the names they have in every other message.
- [ ] Each field's type follows the document's type column; sign-magnitude, offset binary and BCD fields follow [signed encodings](01-reading-an-icd.md#signed-encodings).
- [ ] Each LSB weight is in `scale` as the document writes it; each unit is from the vocabulary.
- [ ] Every enumerated value with a meaning has a row in `values.csv`; reserved values have none.
- [ ] `make import` and `make check` pass; `bite coverage` shows 0 gaps for the message.
- [ ] The message has a vector from the document's example, with a negative value for each signed field where the document gives one.
- [ ] `bite vectors encode <dir> --vector <name>` reports that it matches the document's bytes.
- [ ] Every erratum or ambiguity met has an entry in `icd-notes/issues.md` and a `note` on the field.
- [ ] Anything the format cannot express is described in a `note`.

## Review

The reviewer did not transcribe the message and has the document open.

- [ ] `make check` passes on the branch.
- [ ] The messages in the change are the ones the pull request says, from the stated revision, section and table.
- [ ] For every changed message: the field rows were read against the document's table one by one, in the document's order: name, word, bits, type, LSB weight, unit.
- [ ] The count of field rows equals the count of table rows, plus the header.
- [ ] Every enumeration was read against its table: each value, each label, nothing reserved given a label.
- [ ] `declared_bits` matches the document's length for the message.
- [ ] One vector per changed message was recomputed by hand from the table, at least for one scaled and one signed field.
- [ ] The vectors' expected values come from the document, and each vector says where.
- [ ] Every entry in `lint.allow` has a reason that is true and specific.
- [ ] Every new entry in `icd-notes/issues.md` states the evidence for its decision, and the field's `note` names it.
- [ ] The `bite diff` output against the last release is in the pull request, and every line of it is explained by the change.
- [ ] Nothing in the change has a handling different from the repository's: no capture, no page of a different document.

## Release

- [ ] Every pull request since the last release was reviewed by a second reader.
- [ ] `make check` passes on the commit to be released.
- [ ] `bite coverage --strict` reports no findings, or each remaining one is allowed with a reason.
- [ ] `bite diff --check-version <last release> dictionary` passes.
- [ ] `version` in `dictionary.json` is the version being released; `source.revision` is the revision transcribed.
- [ ] `CHANGELOG.md` has the entry: version, document revision, the diff's lines.
- [ ] Open entries in `icd-notes/issues.md` are listed in the release notes, so consumers know which fields rest on an undecided reading.
- [ ] `bite export-csv` then `bite import-csv` reproduces the dictionary (`tables/` and `dictionary/` agree).
- [ ] The commit is tagged `v<version>`, and consumers that pin the dictionary are told which class of change it carries.
