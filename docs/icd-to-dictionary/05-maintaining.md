# Maintaining a dictionary

A new revision of the document arrives. The work is to transcribe what
changed, see exactly what that does to consumers of the decoded records, and
release a version that says so. `bite diff` does the seeing.

## When a revision arrives

| Step | Do | Command |
|---|---|---|
| 1 | Read the revision history and the change bars. List the sections that changed. Do not trust the list to be complete. | |
| 2 | Keep a copy of the released dictionary to compare with. | `git worktree add ../released v1.0.0` |
| 3 | Edit the tables for each changed section, from the new revision. Update `source.revision` in `dictionary.json`. | `make import` |
| 4 | Run the gates. Vectors of changed messages fail; that is step 6. | `make check` |
| 5 | Compare with the release. | `bite diff ../released/dictionary dictionary` |
| 6 | For each failing vector, take the new revision's example of that message and replace the vector. Vectors of messages the revision did not touch are left alone and must still pass. | `bite vectors new` |
| 7 | Read every line of the diff against the revision history. A line the history does not explain is either a transcription slip or an undocumented change in the document. | |
| 8 | Raise the version by what the diff asks for and write the changelog entry. | `bite diff --check-version ../released/dictionary dictionary` |
| 9 | Update the issues register: errata the revision fixed are marked `fixed in <revision>` and their notes removed. | |

Where the revision history is thin, transcribe the whole document again into a
second table directory and let `bite diff` find the changes: with the
conventions and `bite fmt`, an unchanged message comes out byte for byte the
same, so the diff shows only what the document changed.

## Reading the diff

`bite diff <old> <new>` compares what the two dictionaries decode, not their
text. This is revision C of the [worked example](06-worked-example/README.md)
against revision B:

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

| Class | Means for a consumer | Changes in it | Version |
|---|---|---|---|
| breaking | a name, type, unit or label it may rely on is gone or means something else | a message, field or label removed or renamed; a type changed; a unit changed; a scale added to or removed from an integer field | MAJOR |
| value | the same names and types, different values for the same bytes | bits moved or resized; a scale, offset or byte order changed; a length, port or identifier position changed | MINOR |
| additive | something new that it can ignore | a message, field or label added; any change to spare bits | MINOR |
| documentation | nothing | descriptions, sources, notes | PATCH |

| Command | Exit status 0 | Exit status 1 |
|---|---|---|
| `bite diff <old> <new>` | every change is backward compatible | at least one change is breaking |
| `bite diff --check-version <old> <new>` | the version was raised by at least what the changes ask for | it was not, or a version is missing |

`--json` gives the same report for a release script.

## What the reviewer looks at

| Look at | Ask |
|---|---|
| each `breaking` line | Did the document really remove or rename this, or did a row go missing? Who consumes the field? |
| each `moved` or `resized` line | Does the new range come from the new table, and do its neighbours still meet it? `bite coverage` must still show no gap. |
| each `rescaled` line | Is the new weight the document's, and does the new revision's example decode to its stated value? |
| each `added` label | Was the value reserved before? Consumers that treated it as invalid will now see a name. |
| what is absent | A section the history says changed, with no line in the diff. |
| the vectors that changed | Only those of messages in the diff, each replaced from the new revision's examples. |

## Deprecating a field

A consumer may rely on a field the document has dropped.

| The document | Do |
|---|---|
| marks the field's bits spare | Rename the field `spare_<start bit>`. That is a removal: MAJOR. |
| says the field is no longer sent but leaves its bits | Keep the field. Start its description with "Deprecated in revision C: " and what replaces it. PATCH. Remove it at the next MAJOR. |
| replaces the field with another at different bits | Add the new field (MINOR), keep the old one as above while both are sent, remove it when the document does. |

## Keeping old vectors passing

A vector that passed keeps passing unless the document changed the message it
tests. When a vector fails after an edit:

| The message | Then |
|---|---|
| is not in the diff | the edit broke something it should not have touched: fix the edit |
| is in the diff, and the revision gives a new example | replace the vector from the new example |
| is in the diff, and the revision gives no example | work the new expected values by hand from the new table and say so in `note`; do not copy the decoder's output |

Never edit an expected value to match what the decoder now prints.

## Supporting two revisions at once

Systems in the field run different revisions. A dictionary describes one. Keep
each supported revision on its own release branch of the dictionary
repository, with its own version line (`1.x` for revision B, `2.x` for
revision C), and point each decoder at the one its link speaks.

## The changelog

`CHANGELOG.md` has one entry per version: the version, the document revision
it transcribes, and the lines of `bite diff` against the version before, most
severe first.

```text
## 2.0.0

Transcribed from GZC-ICD-001 revision C.

- Breaking: ZONE/Vent Report: motor_current renamed drive_current.
- Value: ZONE/Irrigation Report: flow_rate LSB 0.25 -> 0.1 L/min.
- Value: ZONE/Climate Report: length 10 -> 11 words.
- Added: ZONE/Climate Report: dew_point. Alarm code 8, Low Battery. Operating mode 5, Ventilation Purge.
- Fixed in the document: ISSUE-001 (Leaf Wetness bits).
```
