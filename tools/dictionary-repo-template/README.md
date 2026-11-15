# {{NAME}} dictionary

The first-bite dictionary for {{NAME}}: its interface control document
transcribed into JSON that the `bite` decoder loads.

## Handling

**HANDLING: replace this paragraph with the distribution statement of the
source document, word for word, before the first commit.**

A dictionary transcribes its source document and is handled as that document
is. This repository's access list is the list of people who may hold the
document. Nothing in it is copied to a repository, a chat, an issue tracker or
an online service with wider access, and that includes the tables in
`tables/`, the notes in `icd-notes/` and the output of the checks.

## Layout

| Path | Holds |
|---|---|
| `dictionary/` | the dictionary: `dictionary.json`, one directory per interface, `vectors/` |
| `tables/` | the spreadsheet form the dictionary is entered in: four CSV files |
| `icd-notes/issues.md` | every erratum and ambiguity found in the document, and what was decided |
| `CHANGELOG.md` | one entry per dictionary version |
| `Makefile` | `make check`, the gate every pull request passes |

## Working on it

The method is first-bite's `docs/icd-to-dictionary/`. In short:

```bash
make tables     # dictionary/ -> tables/, to edit in a spreadsheet
make import     # tables/ -> dictionary/, in canonical form
make check      # format, structure, vectors, conventions, coverage
make diff OLD=../{{NAME}}-released/dictionary    # what changed against a released version
```

`make check` needs the `bite` command on `PATH`, at the version the workflow
in `.github/workflows/check.yml` pins. To use a dictionary, point the decoder
at it: `BITE_DICT_PATH=/path/to/this/repository/dictionary`.
