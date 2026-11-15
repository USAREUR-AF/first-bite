# AGENTS.md

Instructions for AI coding agents, and a short version for anyone working here. [CONTRIBUTING.md](CONTRIBUTING.md) is the full process.

## What this repository is

first-bite reads frames from a network interface or a capture file, decodes fixed-layout binary messages against a dictionary, and publishes the decoded records. The message set is data: a dictionary is a directory of JSON files, loaded at run time. [docs/architecture.md](docs/architecture.md) is the one-read overview.

## Layout

| Path | What |
|---|---|
| `bite/` | decoder library and `bite` CLI (`check`, `list`, `decode`, `schema`; `new`, `range`, `import-csv`, `export-csv`, `fmt`, `coverage`, `lint`, `diff`, `vectors`, `stats` for writing a dictionary). No network code. |
| `bite-wasm/` | the decoder as a WebAssembly module for browsers; `make wasm` builds it into `site/pkg/` |
| `site/decoder/` | the page that runs it: pick a dictionary and a payload, see which bits make which field |
| `mind/` | the `mind` binary: `source.rs` (where frames come from), `pipeline.rs` (frame to record), `run.rs` (the loop), `cli.rs` |
| `flow/` | ZeroMQ publishers; sinks behind cargo features `azure`, `aws`, `zenoh`; transforms |
| `cotify/` | Cursor-on-Target XML from a record and a field mapping |
| `datamodels/`, `logger/` | the shared record type; the log line format |
| `dictionaries/` | example dictionaries; each carries `vectors/` |
| `docs/dictionary-format.md` | the dictionary format and everything `bite check` reports |
| `docs/icd-to-dictionary/` | how to go from an interface control document to a checked, versioned dictionary; `06-worked-example/retrace.sh` redoes the worked example |
| `tools/dictionary-repo-template/`, `tools/icd-extract/` | the skeleton of a private dictionary repository; an optional local helper that drafts tables from a PDF |
| `.github/workflows/` | `ci.yml` (fmt, clippy, test, guards), `leak-gate.yml`, `cargo-deny.yml`, `rust-audit.yml`, `codeql.yml`, `scorecard.yml`, `release.yml` |
| `tools/check-public-tree.sh`, `tools/check-pins.py` | the leak check and the pin check; [SECURITY.md](SECURITY.md) lists what watches the repository |
| `docs/security/` | the threat model and the security assessments |

## Rules

1. **Nothing about a specific message set goes in code.** A field name, a message name, a port or a label belongs in a dictionary. If a change needs `if interface == "…"`, the dictionary format is missing a feature; add the feature.
2. **No restricted material.** No dictionary transcribed from a document that is export-controlled or proprietary, no capture of real traffic, no identifier of a real deployment (hostname, address, account, bucket, key, person). `tools/check-public-tree.sh` scans for these, and for keys, MAC addresses, private and mesh addresses and e-mail addresses by shape; use the documentation ranges (`203.0.113.0/24`) and `example.org`. A configuration file holds credentials as `${NAME}` references, never as values. Tests build their frames in code or use `dictionaries/*/vectors/`.
3. **Existing dictionaries keep loading.** New keys in the dictionary format are optional with a default that preserves current behaviour. A vector that passed keeps passing.
4. **The record's wire format is a contract.** Field names and value rendering of the published JSON record (`datamodels/src/packets.rs`) do not change without bumping the data model version.
5. **Commits.** Conventional Commits subject of 100 characters or fewer with a scope (`fix(bite): …`), signed off (`git commit -s`). One logical change per pull request. No assistant session links or attribution trailers in commits or files.
6. **Documents move with the code**, using the status words built, measured, designed and not built exactly.

## Before you push

```bash
make lint          # rustfmt, clippy -D warnings, actionlint, yamllint, shellcheck, guards
make test
cargo run -p bite -- check dictionaries/weather-station --strict
make guards        # leak check of the published files, pin check, their self-tests
make hooks         # once per clone: the leak check as a pre-commit hook
```

Vectors are computed independently of the decoder (by hand or with a few lines of Python `struct`), never by pasting the decoder's own output.

## Building on another machine

`tools/remote-build.sh <command>` copies the working tree to the ssh host named by `REMOTE_BUILD_HOST` and runs the command there, one directory per branch.
