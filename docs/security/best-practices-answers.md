# OpenSSF Best Practices badge: the answers

The [OpenSSF Best Practices](https://www.bestpractices.dev/) badge is a
questionnaire a maintainer answers about the project. Its *passing* level has
67 criteria; this page gives the answer and the evidence for each, in the
order the form asks, so that filling it in is copying. The form takes a URL as
evidence wherever one is given here. Status of the badge: registered as project 16721, in progress.

## Registering

1. The repository must be public: the form reads it.
2. Sign in at `https://www.bestpractices.dev/` with GitHub and choose
   *Get your badge now*.
3. Repository URL: `https://github.com/USAREUR-AF/first-bite`. Home page URL:
   the same. The form fills in some answers from the repository; check them
   against the tables below.
4. Answer the rest from the tables. An answer marked *maintainer* is a
   statement about the people, which only they can make.
5. The project page then has a number in its address,
   `https://www.bestpractices.dev/projects/<id>`. Put it in the two places of
   the commented badge under the wordmark in `README.md` and remove the comment
   marks:

```
[![OpenSSF Best Practices](https://www.bestpractices.dev/projects/<id>/badge)](https://www.bestpractices.dev/projects/<id>)
```

Two answers below wait for something that has not happened yet: the first
release (`release_notes`), and the first CodeQL run on the public repository
(`static_analysis_fixed`). Until then the badge reads *in progress*, which
Scorecard counts as 2; passing counts as 5.

`R` below stands for `https://github.com/USAREUR-AF/first-bite/blob/main`.

## Basics

| Criterion | Answer | Evidence to paste |
|---|---|---|
| `description_good` | Met | `R/README.md`: the first paragraph says what the software does and where it sits. |
| `interact` | Met | `R/README.md#contributing` links to `R/CONTRIBUTING.md` (how to contribute, report a bug, propose a feature) and `R/SECURITY.md`; releases are at `https://github.com/USAREUR-AF/first-bite/releases`. |
| `contribution` | Met | `R/CONTRIBUTING.md` is the whole process: branch, gates, commit form, pull request, review. |
| `contribution_requirements` | Met | `R/CONTRIBUTING.md`: Conventional Commits, DCO sign-off, signed commits, the gates to run, what does not belong in the repository. |
| `floss_license` | Met | Apache-2.0, `R/LICENSE`. |
| `floss_license_osi` | Met | Apache-2.0 is approved by the OSI. |
| `license_location` | Met | `R/LICENSE`, with `R/NOTICE`. |
| `documentation_basics` | Met | `R/README.md` (install, run, write a dictionary) and `https://usareur-af.github.io/first-bite/`. |
| `documentation_interface` | Met | `R/docs/dictionary-format.md` (every key of a dictionary), `R/docs/architecture.md` (the record that is published), `R/docs/deployment.md` (configuration); `mind --help` and `bite --help`. |
| `sites_https` | Met | The repository, the releases and the documentation site (`github.io`) are served over HTTPS only. |
| `discussion` | Met | GitHub Issues: `https://github.com/USAREUR-AF/first-bite/issues`. Searchable, addressable by URL, open to anyone with a GitHub account. |
| `english` | Met | Documentation and issue tracker are in English. |
| `maintained` | Met | `https://github.com/USAREUR-AF/first-bite/commits/main`. |

## Change control

| Criterion | Answer | Evidence to paste |
|---|---|---|
| `repo_public` | Met | `https://github.com/USAREUR-AF/first-bite`. |
| `repo_track` | Met | Git: every change with its author and date. |
| `repo_interim` | Met | Every change is a commit on `main` between releases; releases are tags on it. |
| `repo_distributed` | Met | Git. |
| `version_unique` | Met | Each release is a tag `vX.Y.Z`, or `vX.Y.Z-<suffix>` for a pre-release of it; `release.yml` refuses any other tag than those of the version of the `mind` crate (`R/tools/release-version.sh`). |
| `version_semver` | Met | `R/GOVERNANCE.md#releases`: `vX.Y.Z`; pre-1.0 minor versions may change interfaces. |
| `version_tags` | Met | Releases are git tags; a ruleset forbids moving or deleting a `v*` tag. |
| `release_notes` | N/A until the first release, then Met | `https://github.com/USAREUR-AF/first-bite/releases`. The release workflow drafts the notes from the merged pull requests; the criterion wants a summary a user can decide an upgrade from, so the maintainer adds one at the top of each release. |
| `release_notes_vulns` | N/A | No release has fixed a publicly known vulnerability. |

## Reporting

| Criterion | Answer | Evidence to paste |
|---|---|---|
| `report_process` | Met | `R/CONTRIBUTING.md#reporting-a-bug-or-proposing-a-feature`; issue templates in `R/.github/ISSUE_TEMPLATE`. |
| `report_tracker` | Met | GitHub Issues. |
| `report_responses` | Met | No bug report has been submitted yet; reports are answered by the maintainers named in `R/GOVERNANCE.md`. |
| `enhancement_responses` | Met | No enhancement request has been submitted yet. |
| `report_archive` | Met | `https://github.com/USAREUR-AF/first-bite/issues?q=is%3Aissue`. |
| `vulnerability_report_process` | Met | `R/SECURITY.md#reporting-a-vulnerability`. |
| `vulnerability_report_private` | Met | GitHub private vulnerability reporting: `https://github.com/USAREUR-AF/first-bite/security/advisories/new`. |
| `vulnerability_report_response` | N/A | No vulnerability report in the last six months. `R/SECURITY.md#what-to-expect` commits to an acknowledgement within 3 working days. |

## Quality

| Criterion | Answer | Evidence to paste |
|---|---|---|
| `build` | Met | `cargo build --workspace --locked`; `make build`, `make release` (`R/Makefile`). |
| `build_common_tools` | Met | Cargo and Make. |
| `build_floss_tools` | Met | The Rust toolchain, GCC or Clang, CMake, Make. |
| `test` | Met | `cargo test --workspace`: unit tests in each crate and integration tests in `bite/tests`, `flow/tests`, `mind/tests`; each shipped dictionary carries test vectors (`R/dictionaries`). |
| `test_invocation` | Met | `make test`. |
| `test_most` | Met | The decoder, the dictionary loader, the frame pipeline, every sink and the Cursor-on-Target transform have tests. Statement coverage is not measured. |
| `test_continuous_integration` | Met | `R/.github/workflows/ci.yml` runs the tests on Linux and macOS on every pull request and push to `main`. |
| `test_policy` | Met | `R/CONTRIBUTING.md#tests`: a change to behaviour comes with a test of it in the same pull request. |
| `tests_are_added` | Met | `R/bite/tests/hostile_dictionary.rs` and `R/bite/tests/robustness.rs` arrived with the limits they test; `R/mind/src/decap.rs` carries the tests of the tunnel unwrapping it added. |
| `tests_documented_added` | Met | `R/CONTRIBUTING.md#tests`. |
| `warnings` | Met | `cargo clippy` and `rustfmt` for Rust, `shellcheck` for shell, `actionlint` and `yamllint` for workflows: the `fmt`, `clippy` and `scripts` jobs of `R/.github/workflows/ci.yml`. |
| `warnings_fixed` | Met | Clippy runs with `-D warnings`: a warning fails CI, so `main` has none. |
| `warnings_strict` | Met | Every default Clippy lint is an error, on all targets of all crates. |

## Security

| Criterion | Answer | Evidence to paste |
|---|---|---|
| `know_secure_design` | Met (*maintainer*) | `R/docs/security/threat-model.md` states the assets, the trust boundaries and what is done at each. |
| `know_common_errors` | Met (*maintainer*) | `R/docs/security/threat-model.md` and `R/docs/security/assessment-2026-11.md`: untrusted frames and dictionaries, resource limits (`R/bite/src/limits.rs`), credential handling, supply chain. |
| `crypto_published` | Met | TLS 1.2 and 1.3 (rustls) to HTTPS and Zenoh endpoints; HMAC-SHA256 to sign Azure Blob requests; SHA-256. All published and reviewed. |
| `crypto_call` | Met | No cryptography is implemented here: it is called from `rustls`, `ring` or `aws-lc`, and the RustCrypto `hmac` and `sha2` crates. |
| `crypto_floss` | Met | All of those are open source. |
| `crypto_keylength` | Met | rustls offers only cipher suites with at least 128-bit security and refuses RSA keys under 2048 bits. |
| `crypto_working` | Met | first-bite's code calls HMAC-SHA256 and SHA-256 only. The `md-5` and `sha1` crates in the lock file belong to the AWS SDK, which uses them for the object checksums the S3 protocol defines, not for security. |
| `crypto_weaknesses` | Met | As above: SHA-1 only as an S3 object checksum inside the AWS SDK. |
| `crypto_pfs` | Met | rustls offers only ECDHE key exchange. |
| `crypto_password_storage` | N/A | The software stores no passwords for authenticating users. |
| `crypto_random` | N/A | The software generates no keys or nonces itself; the TLS libraries take theirs from the operating system. |
| `delivery_mitm` | Met | Releases are downloaded from GitHub over HTTPS; each asset has a signed SLSA provenance statement (`R/SECURITY.md`, the row *release provenance*). |
| `delivery_unsigned` | Met | `SHA256SUMS` is served over HTTPS beside the assets, and the same digests are the subjects of the signed provenance. |
| `vulnerabilities_fixed_60_days` | Met | No vulnerability is known in first-bite's code. One advisory is open in the dependency tree: RUSTSEC-2026-0041 in `lz4_flex`, which only the optional `zenoh` feature builds; the default build and the release binaries do not contain it (`R/.cargo/audit.toml`). |
| `vulnerabilities_critical_fixed` | Met | None has been reported. |
| `no_leaked_credentials` | Met | `R/.github/workflows/leak-gate.yml` scans every pull request and push for credential shapes; GitHub secret scanning with push protection is on. |

## Analysis

| Criterion | Answer | Evidence to paste |
|---|---|---|
| `static_analysis` | Met | CodeQL on Rust, Python, the JavaScript of the site and the workflows (`R/.github/workflows/codeql.yml`); Clippy. |
| `static_analysis_common_vulnerabilities` | Met | CodeQL's `security-and-quality` query suite. |
| `static_analysis_fixed` | Met after the first CodeQL run | `https://github.com/USAREUR-AF/first-bite/security/code-scanning`: answer once the first run on the public repository shows no open finding of medium or higher severity. |
| `static_analysis_often` | Met | CodeQL runs on every pull request and every push to `main`. |
| `dynamic_analysis` | Met | Three libFuzzer targets (`R/fuzz/README.md`) run by ClusterFuzzLite on pull requests and every night (`R/.github/workflows/cflite_pr.yml`, `R/.github/workflows/cflite_batch.yml`). |
| `dynamic_analysis_unsafe` | N/A | Rust is memory-safe and first-party code has no `unsafe` block. The fuzz targets are built with AddressSanitizer all the same, for the C libraries linked in. |
| `dynamic_analysis_enable_assertions` | Met | The fuzz build keeps debug assertions and integer-overflow checks on (`R/fuzz/Cargo.toml`). |
| `dynamic_analysis_fixed` | Met | The fuzz runs have produced no finding (`R/docs/security/scorecard.md#fuzzing`). |
