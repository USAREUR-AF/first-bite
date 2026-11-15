## What changed

<!-- Messages and fields touched, and the document, revision and section they come from. -->

## Checks

- [ ] `make check` passes.
- [ ] `make diff OLD=<last release>` output is pasted below, and the version in `dictionary/dictionary.json` is raised by at least what it asks for.
- [ ] `CHANGELOG.md` has an entry for the new version.

## Transcription

- [ ] Every new or changed field was entered from the document, not from another dictionary or from a capture.
- [ ] Bit positions were entered as the document writes them (`word`, `bits`) and converted by `bite`, not by hand.
- [ ] Every new or changed message has `declared_bits` from the document's stated length.
- [ ] Every new or changed message has a vector whose expected values come from the document, and whose payload was checked without `bite`.
- [ ] Every erratum or ambiguity met is in `icd-notes/issues.md`, and the field carries a `note` naming the entry.
- [ ] Nothing was allowed in `lint.allow` without a reason a reviewer can check.

## Review (second reader)

- [ ] I read each changed field against the document myself: name, word and bits, type, scale and unit, every enumerated value.
- [ ] I recomputed one vector per changed message by hand.
- [ ] This change adds nothing whose handling differs from this repository's.

```
<paste the output of make diff here>
```
