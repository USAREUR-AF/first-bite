# Security policy

first-bite reads raw network traffic and runs with capture privileges. Security reports are prioritised over feature work.

## Reporting a vulnerability

**Do not open a public issue or pull request to report a vulnerability.**

**[Report a vulnerability privately](https://github.com/USAREUR-AF/first-bite/security/advisories/new)**

You can also open the reporting form from this repository's *Security* tab by selecting *Report a vulnerability*.

Reports are private until a maintainer publishes the advisory. Include: a description of the issue and its impact; reproduction steps or a proof of concept (a capture file or a payload in hex is ideal); the affected versions or commits; whether you have shared it elsewhere.

## What to expect

| Step | Target |
|---|---|
| Acknowledgement | 3 working days |
| Triage and severity | 7 working days |
| Fix or mitigation for a confirmed high or critical issue | 30 days, with a coordinated disclosure date agreed with you |
| Public advisory | when the fix is released, crediting you unless you prefer otherwise |

## Scope

In scope: the code in this repository and the release artifacts built from it, including the decoder's handling of malformed packets and malformed dictionaries. Out of scope: upstream projects (libpcap, ZeroMQ, Zenoh, the cloud SDKs) — report those to their maintainers, and tell us too if first-bite's use of them is affected; any deployment you do not own.

A dictionary describes a message set. Do not attach a dictionary or a capture that you are not permitted to share.

## How this repository is watched

| Control | Where | Runs | What it catches |
|---|---|---|---|
| leak gate | `.github/workflows/leak-gate.yml` running `tools/check-public-tree.sh` | every pull request, forks included, and every push to `main` | private keys, cloud access key ids and tokens; capture files outside `examples/`; SSH public keys, age recipients, MAC addresses, private and CGNAT IPv4 addresses, e-mail addresses outside `example.org`, assistant session links and attribution trailers; names on the maintainers' list (a repository secret). It prints the file, line and class, never the match |
| leak check self-test | `tools/check-public-tree.test.sh`, `scripts` job of `ci.yml` | every pull request and push to `main` | a leak check that has stopped catching one of its classes |
| pre-commit hook | `tools/git-hooks/pre-commit`, installed with `make hooks` | each commit, on the contributor's machine | the same shapes as the leak gate, in the staged files, before they enter history |
| pin check | `tools/check-pins.py`, `scripts` job of `ci.yml` | every pull request and push to `main` | a GitHub Action not pinned to a full commit SHA; a container image in a workflow, `Dockerfile`, compose file or the `Makefile` without a digest; a pinned image no Dependabot entry updates |
| advisory audit | `.github/workflows/rust-audit.yml` (`cargo audit`) | pull requests that change `Cargo.lock` or a `Cargo.toml`; weekly | a dependency with a RustSec vulnerability advisory. Each ignored advisory is listed with its reason in `.cargo/audit.toml` |
| dependency policy | `.github/workflows/cargo-deny.yml` (`deny.toml`) | pull requests that change `Cargo.lock`, a `Cargo.toml` or `deny.toml` | a licence outside the allowed list; a wildcard version; a crate from outside crates.io |
| CodeQL | `.github/workflows/codeql.yml` | pull requests and pushes that change Rust, Python or a workflow; weekly | code-scanning findings in the Rust and Python sources and in the workflows |
| Scorecard | `.github/workflows/scorecard.yml` | pushes to `main`; weekly | regressions in supply-chain posture: unpinned dependencies, workflow token permissions, branch protection |
| Dependabot | `.github/dependabot.yml` | weekly | newer versions of crates, GitHub Actions, the `Dockerfile` base images and the mock images, as pull requests |
| tests of hostile input | `bite/tests/robustness.rs`, `bite/tests/hostile_dictionary.rs`, `test` job of `ci.yml` | pull requests that change code | a decoder or dictionary loader that panics on a malformed payload or dictionary, or exceeds a limit of `bite/src/limits.rs` |
| script lint | `shellcheck` and `yamllint` in the `scripts` job of `ci.yml` | every pull request and push to `main` | mistakes in shell scripts and workflow files |

CodeQL and Scorecard run only while the repository is public; both workflows
skip on a private repository. [docs/security/threat-model.md](docs/security/threat-model.md)
states what the design protects and what it does not;
[docs/security/](docs/security/) holds the assessments.

## Supported versions

Pre-1.0, the latest tagged release is supported. Fixes land on `main` and are released by tag.
