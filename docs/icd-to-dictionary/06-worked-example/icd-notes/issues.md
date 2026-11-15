# Document issues: GZC-ICD-001

Every place the document is wrong or can be read two ways, and what the
`glasshouse` dictionary does about it. The form is that of
[`tools/dictionary-repo-template/icd-notes/issues.md`](../../../../tools/dictionary-repo-template/icd-notes/issues.md).

| Status | Means |
|---|---|
| open | no decision; the dictionary follows the document as written |
| decided | the dictionary follows the decision recorded here |
| confirmed | the document's owner agreed, in the reference given |
| fixed in <revision> | a later revision of the document corrects it |

## ISSUE-001: Table 5-3 gives Leaf Wetness seven bits that overlap Relative Humidity

| | |
|---|---|
| Status | decided; fixed in revision C |
| Where | GZC-ICD-001 revision B, section 5.3, Table 5-3 |
| Affects | `ZONE/Climate Report/leaf_wetness` |
| The document says | Relative Humidity: word 6, bits 15-6. Leaf Wetness: word 6, bits 6-0. |
| The problem | Bit 6 belongs to both fields. Transcribed as printed, `bite check` reports `bits [89, 96) overlap relative_humidity [80, 90)`. |
| Evidence | The description of Leaf Wetness calls it "a 6-bit value", and bits 5-0 are six bits. In appendix B.2, word 6 is `CB6A`: bits 15-6 are 813, which is the stated 81.3 %, and bits 5-0 are 42, the stated index; bits 6-0 would be 106, above the field's stated maximum of 60. |
| Decision | Leaf Wetness is word 6, bits 5-0. Relative Humidity is as printed. |
| Reported to | the document's owner; corrected in revision C, Table 5-3 |

## ISSUE-002: The side on which Zone Label is padded is not stated

| | |
|---|---|
| Status | decided |
| Where | GZC-ICD-001 revision B, section 5.2, Table 5-2 |
| Affects | `ZONE/Zone Status/zone_label` |
| The document says | "Four ASCII characters, padded with spaces." |
| The problem | A label shorter than four characters could be padded on the left or on the right. The decoder removes trailing spaces and NUL bytes only, so a left-padded label would decode with its leading spaces. |
| Evidence | Appendix B.1 gives the label "N3" as words 7 and 8 `4E33 2020`: padded on the right. |
| Decision | Padding is on the right. No change to the transcription; the field is `ascii` and decodes to the label without its padding. |
| Reported to | the document's owner, asking for "padded on the right" in Table 5-2 |
