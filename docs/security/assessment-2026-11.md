# Security assessment, November 2026

Scope: the tree at commit `ff35ab3` and the commits that follow it on
the security branch. Scanners were run on `Cargo.lock`, the published file
set (every tracked file outside `private/`, `docs/plan.md` and
`docs/decisions.md`), the one-commit history of the public repository, the
image the `Dockerfile` builds, and the workflows. The code named in
[Manual review](#manual-review) was read line by line.

Status words: **fixed in** a commit (the change is in the tree and its test
passes), **accepted** (left as it is, with the reason), **open** (not
resolved here; the owner is named).

Severity is reasoned from who must control what, and what they gain.

## Findings

File and line are those of commit `ff35ab3`, where the defect was.

| Id | Severity | Component | File:line | What | Status |
|---|---|---|---|---|---|
| F-01 | Low: needs control of a dictionary file; a panic in builds with overflow checks, a wrong label key in release builds | bite | `bite/src/schema.rs:214` | A `values` key with two signs (`--170141183460469231731687303715884105728`, `-0x-8000…`) reached `-value` with `i128::MIN`: "attempt to negate with overflow". `--5` parsed as 5 | fixed in `d7859ed`; `bite/tests/hostile_dictionary.rs` fails on the old parser |
| F-02 | Medium: a dictionary received from someone else exhausts memory or CPU of the decoder, or of the browser tab | bite | `bite/src/dictionary.rs:771`, `:613`, `:696`, `:517`, `:745` | No limit on the size of a dictionary file, the number of files, the fields of a message, the entries of a `values` object or the bit position a field may name | fixed in `d7859ed`: limits in `bite/src/limits.rs`, listed in `docs/dictionary-format.md` |
| F-03 | Low: CPU, quadratic in the size of one interface | bite | `bite/src/dictionary.rs:537` | Every message file was compared against every `message_type` name | fixed in `d7859ed` |
| F-04 | Low: memory and log volume | bite | `bite/src/dictionary.rs:702`, `:525`, `:712` | A finding quoted a dictionary key or label in full, whatever its length | fixed in `d7859ed`: quoted text is cut to 80 characters |
| F-05 | Low: not reachable through a loaded dictionary; public functions | bite | `bite/src/bits.rs:53`, `:58` | `sign_extend` with width 0 and `swap_bytes` with width 0 or over 64 overflow a shift | fixed in `d7859ed` |
| F-06 | Low: needs write access to the dictionary directory; the loader blocks | bite | `bite/src/dictionary.rs:371` | An entry of `vectors/` that is not a regular file (a FIFO named `x.json`) was opened and read | fixed in `d7859ed` |
| F-07 | Medium: a host on the tapped link sends large frames while the pipeline is behind; up to 4 GiB held by the capture process | mind | `mind/src/source.rs:186`, `:221` | The queue between capture threads and the pipeline was bounded at 65,536 frames and not in bytes | fixed in `f5322a1`: 256 MiB |
| F-08 | Low: a host that can reach UDP 12345 injects records; development mode only | mind | `mind/src/cli.rs:22` | `--mock` bound `0.0.0.0` by default | fixed in `f5322a1` |
| F-09 | Low: another local user reads records, under a permissive umask and outside the systemd unit | mind, flow | `mind/src/run.rs:61`, `flow/src/sink/local_log.rs:30`, `flow/src/sink/message_rates.rs:53`, `:71` | Record files were created with the mode the umask allowed | fixed in `f5322a1`, `4655eb0`: mode 0640 |
| F-10 | Medium: whoever controls a topic on the subscribed socket, an HTTP endpoint's error answer or a dictionary file name writes arbitrary lines and terminal escape sequences into the log | logger, flow | `logger/src/loggers.rs:65`, `:75`; sources `flow/src/subscribe.rs:130`, `flow/src/sink/mod.rs:40` | Log arguments were written as they came, line breaks included | fixed in `f7ec42e` |
| F-11 | Medium: credentials at rest in a configuration file readable by the service group, and one `{:?}` away from the log | flow, deployment | `flow/src/config.rs:46`, `:64`, `:73`, `:74`; `flow/config/subscribers.example.yml:29`, `:43`, `:44` | Storage credentials were plain values in the subscriber YAML, in `String` fields of a struct that derives `Debug`. No log line printed the struct | fixed in `4655eb0`, `ed0b17b`: references, `_file` variants, type `Secret` |
| F-12 | Low: one byte of the key and its offset in an error message | flow | `flow/src/sink/azure_blob.rs:46` | The base64 error of `cloud_access_key` was quoted | fixed in `4655eb0` |
| F-13 | Low: the access key id, not the secret, in a log line | flow | `flow/src/sink/aws_s3.rs:60` | A store's error answer that quotes the access key id was reported whole | fixed in `4655eb0`: both keys are removed from the text |
| F-14 | Medium: whoever answers for the endpoint, or sits on the path of an `http://` one, receives every record at an address of their choice | flow | `flow/src/sink/rest.rs:34`, `flow/src/sink/azure_blob.rs:60` | The HTTP clients followed redirects; a 307 or 308 re-sends the record | fixed in `4655eb0` |
| F-15 | Low: the endpoint makes the process hold and log an answer of any size | flow | `flow/src/sink/rest.rs:60`, `flow/src/sink/azure_blob.rs:131` | An error answer was read whole and quoted whole | fixed in `4655eb0`: 1 MiB read, 512 characters quoted |
| F-16 | Low: a password or token an operator put in the endpoint URL reaches the log | flow | `flow/src/sink/rest.rs:86`, `:93`; `flow/src/sink/mod.rs:39` | The REST URL, and the URL inside a request error, were logged as configured | fixed in `4655eb0` |
| F-17 | Low: needs control of a topic and a store or proxy that normalises paths; an object lands outside its prefix or bucket | flow | `flow/src/sink/object_path.rs:11`, `:20` | A topic was placed in an object name unchanged, `..` segments included | fixed in `4655eb0` |
| F-18 | Low: whatever is at the configured publisher address makes the process hold a message of any size | flow | `flow/src/feed.rs:68` | The SUB socket had no message size limit | fixed in `4655eb0`: 16 MiB |
| F-19 | Low: an operator binds a publisher beyond loopback without seeing what that exposes | flow | `flow/src/publish.rs:108` | A PUB socket bound to a reachable address gave no sign that it serves every record without authentication | fixed in `4655eb0`: a startup warning. The absence of authentication on the socket is accepted; see the threat model |
| F-20 | Medium: a dictionary a visitor was handed freezes or crashes the tab | site | `site/decoder/decoder.js:297`, `:717` | The bit view drew eight elements for every byte up to the last field of the message; any number of files was read | fixed in `177ba08` |
| F-21 | Low: needs a hostile `PATH` at install time or an earlier loosened file | deployment | `deployment/install.sh:13`, `:175`, `:185`, `:209` | The installer ran with the caller's `PATH` and umask, left the mode and owner of kept configuration as found, and wrote `--prefix` into the unit unchecked | fixed in `ed0b17b`; measured in a container: `750 root:firstbite` on both directories, `640 root:firstbite` on the environment file and both configuration files after they had been set to `666`/`777` |
| F-22 | Low: other local users list the names of record files | deployment | `deployment/first-bite.service:22` | `StateDirectory` was created `0755` | fixed in `ed0b17b`: `StateDirectoryMode=0750`. `systemd-analyze security --offline` scores the unit 3.4 |
| F-23 | Low: CVE-2026-0799 needs a crafted filter program and first-bite compiles only the operator's `--filter` text; CVE-2026-6554 is reachable from a crafted packet only when that filter uses `protochain` | release build | `tools/libpcap-static.sh:15` | The release binaries link libpcap 1.10.5, which has CVE-2026-0799, CVE-2026-6554 and CVE-2025-11961 | fixed in `ac21071`: 1.10.7. Measured: the library builds with the script's flags in Alpine 3.24. Not measured: `make portable` and `make static` with it |
| F-24 | Low | container | `Dockerfile:21` | The pinned base image predates three fixed zlib advisories (CVE-2026-76844, CVE-2026-85091, CVE-2026-27171) | fixed in `ac21071`: the image upgrades its packages |
| F-25 | Low: a moved tag changes what a developer runs | mocks | `mocks/azure/compose.yml:7`, `mocks/s3/compose.yml:7`, `mocks/elastic/compose.yml:7`; `.github/dependabot.yml` | Three images were referenced by tag only, and no Dependabot entry followed the pinned `Dockerfile` bases | fixed in `eee1a49` |
| F-26 | Low: only in a build with the `zenoh` feature; a Zenoh peer sends crafted compressed data | dependencies | `Cargo.lock` (`lz4_flex 0.10.0`) | RUSTSEC-2026-0041: decompression can read uninitialised memory | accepted: `zenoh` is pinned to `=1.9.0` to match the fabric it joins; the ignore in `.cargo/audit.toml` goes when that pin moves |
| F-27 | Low: the image's own libpcap | container | `Dockerfile:23` (`libpcap0.8t64 1.10.5-2`) | Debian trixie's libpcap has the CVEs of F-23; Debian marks them minor and has released no fix | accepted: same reachability as F-23; the fix arrives through the base image |
| F-29 | Medium while the repository is private, higher once public | GitHub | repository settings | Secret scanning, push protection, Dependabot alerts and private vulnerability reporting are off, and `main` has no ruleset beyond the organisation's signature rule | open; owner: repository owner. The calls are in [Repository settings](#repository-settings) |

## Scanner summaries

| Scanner | Version | Target | Result |
|---|---|---|---|
| `cargo audit` | 0.23.0, 1,509 advisories | `Cargo.lock`, 481 crates | 0 vulnerabilities reported with `.cargo/audit.toml` (one ignored: RUSTSEC-2026-0041, F-26). 3 warnings, all unmaintained crates reached only through `zenoh`: `paste` (RUSTSEC-2024-0436), `rustls-pemfile` (RUSTSEC-2025-0134), `unsafe-libyaml` (RUSTSEC-2024-0449) |
| `cargo deny check advisories licenses bans sources` | 0.20.2 | all features | `advisories FAILED, bans ok, licenses ok, sources ok`: the same four advisories; `cargo deny` does not read `.cargo/audit.toml`, and the repository's gate is `check licenses bans sources`, which passes |
| `osv-scanner` | 1.4.3 | `Cargo.lock` | the same four, and RUSTSEC-2020-0153 against `bite 0.2.0`: a false match on the name of an unrelated crates.io crate; the workspace crate is not published |
| `gitleaks dir` | 8.30.0 | published file set | before: 4 (the Azurite development key in three files, a base64 placeholder in `subscribers.example.yml`). After: `no leaks found` |
| `gitleaks git` | 8.30.0 | public repository, 1 commit | 4: the same four, in the initial commit. None is a credential |
| `trivy image` | 0.58.1 | image of the `Dockerfile`, Debian 13.7 | before: 0 critical, 50 high, 96 medium, 68 low; 3 with a fix released. After: 0 critical, 48 high, 95 medium, 68 low; 0 with a fix released. All are Debian packages. The Rust binaries carry no dependency list for trivy to read; `cargo audit` covers them |
| `zizmor --offline` | 1.32.0 | `.github/workflows` | 1 high: `pull_request_target` in `leak-gate.yml`. Read and accepted: the job executes only the base branch's script and reads the pull request as data; the script no longer follows symbolic links. Annotated in the workflow |
| `tools/check-pins.py` | this tree | workflows, `Dockerfile`, compose files, `Makefile` | `ok`; every `uses:` is a 40-character SHA |
| `tools/check-public-tree.sh` | this tree | published file set | `ok`. One hit of the new classes was fixed: a test URL that read as an e-mail address |
| `unsafe` count | `grep` | first-party Rust | 0 occurrences in `bite`, `bite-wasm`, `mind`, `flow`, `cotify`, `datamodels`, `logger` |
| `systemd-analyze security --offline` | systemd 259 | `deployment/first-bite.service` | exposure 3.4 (OK) |

## Manual review

Checked, with the result. A row that says none found names what was read.

| Class | Read | Result |
|---|---|---|
| Out-of-bounds, overflow and panics on packet bytes | `bite/src/bits.rs`, `bite/src/decode.rs`, `mind/src/pipeline.rs`, `mind/src/source.rs` | None found. `extract` refuses a range that is empty, over 64 bits or past the payload before it indexes; byte fields use `get`; frames are sliced by `etherparse`, whose error skips the frame; timestamps are clamped. F-05 is outside the decode path |
| Allocation and CPU from a hostile packet | the same, and `mind/src/run.rs` | Decoding costs at most the fields of one message; a record is at most one frame twice in base64. F-07 |
| Allocation and CPU from a hostile dictionary | `bite/src/dictionary.rs`, `schema.rs`, `vectors.rs`, `jsonschema.rs`, `bite-wasm/src/api.rs` | F-01 to F-04, F-06. Deep nesting: refused by the JSON parser at 128 levels |
| Path handling | `bite/src/dictionary.rs`, `flow/src/sink/local_log.rs`, `message_rates.rs`, `object_path.rs`, `mind/src/run.rs` | No message, field or interface name becomes a file path: file paths come only from configuration and the command line. Object names: F-17 |
| Log injection | every `info!`, `warn!`, `error!`, `debug!` in `mind`, `flow`, `logger` | F-10. Records are logged as JSON, which escapes control characters |
| Credential leakage | `flow/src/config.rs`, every sink, every `{:?}` | F-11, F-12, F-13, F-16. No log line printed a configuration struct |
| TLS, redirects, request forgery | `flow/src/sink/rest.rs`, `azure_blob.rs`, `aws_s3.rs`, `zenoh.rs`, `Cargo.toml` | TLS verification is on (`rustls-tls`; no `danger_accept_invalid_certs` anywhere). Endpoints come only from configuration; no record content selects a host. F-14 |
| ZeroMQ exposure | `flow/src/publish.rs`, `feed.rs`, `flow/config/publishers.local.yml`, `docker/` | The built-in publisher binds `127.0.0.1`. F-18, F-19 |
| Cursor-on-Target XML | `cotify/src/event.rs` | None found: every value is written through `quick-xml` attributes, which escape |
| The decoder page | `site/decoder/decoder.js`, `index.html` | No `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write` or `eval`; text goes through `textContent` and `setAttribute`; style strings are built from numbers. F-20 |
| `install.sh` | `deployment/install.sh` | Quoting is sound; the staged dictionary and environment file are created inside the root-owned directory. F-21 |
| systemd unit | `deployment/first-bite.service` | Capabilities are bounded to the two capture ones; the sandbox directives match what the binary uses. F-22 |
| `Dockerfile` | `Dockerfile`, `.dockerignore`, `docker/compose.yml` | Runs as `firstbite` (uid 999, measured); both bases pinned by digest; `private/` excluded from the build context. F-24, F-27 |
| Workflows | `.github/workflows/*.yml` | Every `uses:` pinned by SHA. Top-level `permissions: contents: read` in all seven; write scopes only on the release `publish` job and the code-scanning uploads. The one `pull_request_target` job runs no pull-request code. The only secret, `LEAK_NEEDLES`, is read by that job alone |

## Not scanned, not measured

| What | Why |
|---|---|
| `make portable` and `make static` with libpcap 1.10.7 | the library alone was built; the two release builds were not run |
| The unit under systemd with a live capture | no root on the build host; `systemd-analyze` reads the unit offline |
| The decoder page in a browser | the change was checked with `node --check` only |
| CodeQL and Scorecard results | both workflows skip while the repository is private |

## Repository settings

Read with `GET` on 2026-11-15; nothing was changed. `goatcore` is the
reference.

| Setting | `first-bite` now | `goatcore` | Call that sets it |
|---|---|---|---|
| Secret scanning | disabled | enabled | `gh api -X PATCH repos/USAREUR-AF/first-bite -f 'security_and_analysis[secret_scanning][status]=enabled'` |
| Push protection | disabled | enabled | `gh api -X PATCH repos/USAREUR-AF/first-bite -f 'security_and_analysis[secret_scanning_push_protection][status]=enabled'` |
| Non-provider patterns | disabled | enabled | `gh api -X PATCH repos/USAREUR-AF/first-bite -f 'security_and_analysis[secret_scanning_non_provider_patterns][status]=enabled'` |
| Validity checks | disabled | enabled | `gh api -X PATCH repos/USAREUR-AF/first-bite -f 'security_and_analysis[secret_scanning_validity_checks][status]=enabled'` |
| Dependabot alerts | disabled (404) | enabled (204) | `gh api -X PUT repos/USAREUR-AF/first-bite/vulnerability-alerts` |
| Dependabot security updates | disabled | disabled | `gh api -X PUT repos/USAREUR-AF/first-bite/automated-security-fixes` (version updates already come from `.github/dependabot.yml`) |
| Private vulnerability reporting | off (404) | enabled | `gh api -X PUT repos/USAREUR-AF/first-bite/private-vulnerability-reporting` |
| Commit sign-off on web commits | false | true | `gh api -X PATCH repos/USAREUR-AF/first-bite -F web_commit_signoff_required=true` |
| Ruleset on `main` | the organisation's `signed-and-verified-commits` only | that, and `main-protection`: no deletion, no force push, signatures, pull request with squash only, required checks | `gh api -X POST repos/USAREUR-AF/first-bite/rulesets --input ruleset-main.json`, with the body below |
| Ruleset on release tags | none | `release-tags`: creation, update and deletion restricted | `gh api -X POST repos/USAREUR-AF/first-bite/rulesets --input ruleset-tags.json`, with the body below |
| Actions: default token permissions | read; cannot approve pull requests | the same | no change |
| Actions: allowed actions | all; SHA pinning not required | the same | `gh api -X PUT repos/USAREUR-AF/first-bite/actions/permissions -F enabled=true -f allowed_actions=all -F sha_pinning_required=true` (every action in the tree is pinned) |
| Actions: approval for fork pull requests | not available on a private repository | all external contributors | after publication: `gh api -X PUT repos/USAREUR-AF/first-bite/actions/permissions/fork-pr-contributor-approval -f approval_policy=all_external_contributors` |
| Merge methods | squash only; branches deleted on merge | squash and rebase | no change |
| Code scanning | needs Code Security on a private repository | workflow-based | no call: `codeql.yml` runs once the repository is public |

On a private repository, secret scanning and push protection need GitHub
Secret Protection on the organisation's plan; on a public one they are free.

`ruleset-main.json`:

```json
{
  "name": "main-protection",
  "target": "branch",
  "enforcement": "active",
  "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
  "rules": [
    {"type": "deletion"},
    {"type": "non_fast_forward"},
    {"type": "required_signatures"},
    {"type": "pull_request", "parameters": {
      "allowed_merge_methods": ["squash"],
      "dismiss_stale_reviews_on_push": true,
      "require_code_owner_review": false,
      "require_last_push_approval": false,
      "required_approving_review_count": 0,
      "required_review_thread_resolution": false}},
    {"type": "required_status_checks", "parameters": {
      "strict_required_status_checks_policy": false,
      "do_not_enforce_on_create": false,
      "required_status_checks": [{"context": "leak-gate"}, {"context": "scripts"}]}}
  ]
}
```

`leak-gate` and `scripts` run on every pull request. The Rust jobs of
`ci.yml` are skipped for documentation-only changes and report success when
skipped, so `fmt`, `clippy`, `test (ubuntu-latest)` and `test (macos-latest)`
can be added to the list as well. `goatcore` requires zero approving reviews;
`CONTRIBUTING.md` here says one from a code owner, which is
`"required_approving_review_count": 1` with `"require_code_owner_review": true`.

`ruleset-tags.json`:

```json
{
  "name": "release-tags",
  "target": "tag",
  "enforcement": "active",
  "conditions": {"ref_name": {"include": ["refs/tags/v*"], "exclude": []}},
  "rules": [{"type": "creation"}, {"type": "update"}, {"type": "deletion"}],
  "bypass_actors": [{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"}]
}
```

`actor_id` 5 is the repository admin role: only admins create, move or
delete a release tag.
