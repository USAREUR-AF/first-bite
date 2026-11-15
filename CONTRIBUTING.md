# Contributing

This page is the whole process; nothing else is required reading before your first pull request.

## The short version

1. Fork, or branch from `main` if you have write access. Branch names: `feat/<topic>`, `fix/<topic>`, `docs/<topic>`.
2. Make the change. Run the gates that apply (below). Update the document that describes what you changed in the same pull request.
3. Commit with a [Conventional Commits](https://www.conventionalcommits.org/) subject and a DCO sign-off: `git commit -s -m "fix(bite): …"`.
4. Open a pull request against `main`. Fill the template. One approving review from a code owner merges it, by squash.

## Commits

- **Conventional Commits.** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`, `build`, with a scope naming the crate or area (`feat(flow): …`). The subject is at most 100 characters and says what changed, in the imperative.
- **DCO sign-off on every commit** (`git commit -s`). By signing off you certify the [Developer Certificate of Origin 1.1](https://developercertificate.org/): you wrote the change or have the right to submit it under this project's licence. There is no CLA. The policy is in [Policy](#policy).
- **Signed commits** are required on `main`; set up [SSH or GPG commit signing](https://docs.github.com/en/authentication/managing-commit-signature-verification) once.
- One logical change per pull request. Refactors and behaviour changes are separate pull requests.

## Gates

Every CI check is a command you can run locally before you push.

| Change touches | Run |
|---|---|
| Rust | `make lint` (rustfmt, clippy with warnings as errors) and `make test` |
| the decoder, the dictionary loader or the frame pipeline (`bite/src/`, `mind/src/pipeline.rs`, `mind/src/decap.rs`) | the fuzz target for it, for a few minutes: [fuzz/README.md](fuzz/README.md). CI fuzzes the pull request too |
| a dictionary under `dictionaries/` | `cargo run -p bite -- check dictionaries/<name> --strict`, and the same with `fmt --check`, `lint` and `coverage` |
| `docs/icd-to-dictionary/06-worked-example/` or `dictionaries/glasshouse/` | `docs/icd-to-dictionary/06-worked-example/retrace.sh` |
| `Cargo.toml` or `Cargo.lock` | `make deny` and `make audit` |
| `.github/workflows/` | `actionlint` and `yamllint .github` (both run by `make lint`) |
| a workflow, a `Dockerfile`, a compose file, an image in the `Makefile` | `python3 tools/check-pins.py` — every action is pinned to a commit SHA and every image to a digest |
| a shell script | `shellcheck` (run by `make lint`) |
| anything | `make guards` — `tools/check-public-tree.sh` over the files that are published: no deployment name, address, credential or controlled message definition enters the tree |

### The leak check and its hook

`tools/check-public-tree.sh` refuses, by shape: private keys, cloud access key
ids and tokens; capture files outside `examples/`; SSH public keys and age
recipients; MAC addresses; private (`10/8`, `172.16/12`, `192.168/16`,
`169.254/16`) and CGNAT (`100.64/10`) IPv4 addresses; e-mail addresses outside
`example.org` and `users.noreply.github.com`; and an AI assistant's session
link or attribution trailer outside `AGENTS.md`, `CLAUDE.md` and `.claude/`.
In CI it also refuses the names on the maintainers' list. It prints the file,
the line and the class, never the text it matched.

Install it as a pre-commit hook, once per clone:

```bash
make hooks        # git config core.hooksPath tools/git-hooks
```

The hook runs the check on the files staged for the commit. Write addresses
from the documentation ranges (`192.0.2.0/24`, `198.51.100.0/24`,
`203.0.113.0/24`) and mail at `example.org`. A line that has to hold one of
these shapes and identifies nothing real (a MAC address in a parser test)
ends with `opsec-allow: <reason>`; the reviewer reads the reason.
`tools/check-public-tree.test.sh` plants one example of every class and
checks that each is caught; CI runs it before relying on the check.

A configuration file in the tree holds no credential value: a credential key
is written as a reference, `${NAME}`
([flow/config/README.md](flow/config/README.md#credentials-and-references)).

Building needs a Rust toolchain (`rust-toolchain.toml` pins the channel), a C and C++ compiler, CMake, and the libpcap headers (`libpcap-dev` on Debian and Ubuntu; part of the SDK on macOS).

## What does not belong in this repository

- **Dictionaries for message sets you may not publish.** A dictionary transcribes an interface document. If that document is export-controlled, proprietary or otherwise restricted, its dictionary is too. Keep it in your own repository and point `BITE_DICT_PATH` at it; the decoder loads any directory.
- **Captures of real traffic.** Tests use frames built in the test or the vectors in `dictionaries/*/vectors/`.
- **Identifiers of a real deployment**: hostnames, addresses, account ids, bucket names, keys, people.

## Documents

A change to behaviour updates the document that states that behaviour, in the same pull request: [docs/dictionary-format.md](docs/dictionary-format.md) for the dictionary format and what `bite check` reports, [docs/architecture.md](docs/architecture.md) for how the crates fit together, the README of the crate for its configuration. Status words mean exactly this:

| Word | Means |
|---|---|
| built | the code exists and runs |
| measured | a command ran and produced the stated result |
| designed | decided, not implemented |
| not built | nothing exists |

## Tests

A change to behaviour comes with a test of that behaviour in the same pull request, and a fix comes with a test that fails without it. Tests live beside the code (`<crate>/src/`, `<crate>/tests/`); a new message or field of a shipped dictionary gets a test vector in its `vectors/` directory. `make test` runs them all, and CI runs them on every pull request that changes code.

## Review

Reviewers look for: the change does what its subject says and nothing else; the gates pass; the document moved with the code; status words are honest; nothing from "What does not belong" entered the tree. Review comments are about the change, not the author.

## Reporting a bug or proposing a feature

Open an issue with the template. For a vulnerability, **do not open an issue** — follow [SECURITY.md](SECURITY.md).

## Licence

Portions written by U.S. Federal government employees within the scope of their employment are not subject to copyright in the U.S.; in countries where copyright is recognized, they and all other contributions are licensed under the [Apache License 2.0](LICENSE). See [INTENT.md](INTENT.md). Contributions are accepted under the [Policy](#policy) below.

**NOTE: This CONTRIBUTING.md is for software contributions. You do not need to follow the Developer's Certificate of Origin (DCO) process for commenting on this repository's documentation, such as CONTRIBUTING.md, INTENT.md, etc. or for submitting issues.**

## Policy

### 1. Introduction

The project maintainer for this Project will only accept contributions using the Developer's Certificate of Origin 1.1 located at [developercertificate.org](https://developercertificate.org) ("DCO"). The DCO is a legally binding statement asserting that you are the creator of your contribution, or that you otherwise have the authority to distribute the contribution, and that you are intentionally making the contribution available under the license associated with the Project ("License").

### 2. Developer Certificate of Origin Process

Before submitting contributing code to this repository for the first time, you'll need to sign a Developer Certificate of Origin (DCO) (see below). To agree to the DCO, add a `Signed-off-by` line to every commit with `git commit -s`. At a high level, signing off tells us that you have the right to submit the work you're contributing and indicates that you consent to our treating the contribution in a way consistent with the license associated with this software (as described in [LICENSE](LICENSE) and [INTENT.md](INTENT.md)) and its documentation ("Project").

### 3. Important Points

Pseudonymous or anonymous contributions are permissible, but you must be reachable at the email address provided in the Signed-off-by line.

If your contribution is significant, you are also welcome to add your name and copyright date to the source file header.

U.S. Federal law prevents the government from accepting gratuitous services unless certain conditions are met. By submitting a pull request, you acknowledge that your services are offered without expectation of payment and that you expressly waive any future pay claims against the U.S. Federal government related to your contribution.

If you are a U.S. Federal government employee and use a `*.mil` or `*.gov` email address, we interpret your Signed-off-by to mean that the contribution was created in whole or in part by you and that your contribution is not subject to copyright protections.

### 4. DCO Text

The full text of the DCO is included below and is available online at [developercertificate.org](https://developercertificate.org):

```txt
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.
1 Letterman Drive
Suite D4700
San Francisco, CA, 94129

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.

Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```
