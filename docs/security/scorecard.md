# OpenSSF Scorecard, check by check

[OpenSSF Scorecard](https://scorecard.dev/) scores a repository from 0 to 10 on
each of eighteen checks and publishes a weighted average.
`.github/workflows/scorecard.yml` runs it on every push to `main` and weekly,
uploads the findings to code scanning and publishes the result to
`api.scorecard.dev`, which the badge in the README reads. It runs only while
the repository is public.

This page states, for each check, what it measures, what the repository has,
the score to expect and what would raise it. The rules are those of Scorecard
v5.6.0 (`checks/evaluation/` in its source); the scores marked *measured* are
the output of `scorecard --local` v5.6.0 on the published files, which runs the
eleven checks that need no GitHub API: 7.0 over those eleven before these
changes, 10.0 after. The others are worked out from the rules
and are *expected*, not measured, until the repository is public.

## Summary

| Check | Weight | Today | With this page's set-up | Highest reachable | What the last step needs |
|---|---|---|---|---|---|
| Binary-Artifacts | high | 10, measured | 10 | 10 | n/a |
| Branch-Protection | high | 0, expected | 8 with one required review; 4 with none | 10 | two required reviewers: three people with write access |
| CI-Tests | low | not scored: no merged pull request | 10 once changes arrive by pull request | 10 | n/a |
| CII-Best-Practices | low | 0 | 5, the passing badge | 10 | the gold badge: several unassociated contributors, two-person review, 90% statement coverage |
| Code-Review | high | 0, expected | rises to 10 as approved pull requests replace the last 30 commits | 10 | a second person who approves every pull request |
| Contributors | low | 3, expected | 3 | 10 | commits from people of three organisations |
| Dangerous-Workflow | critical | 0, measured | 10, measured | 10 | n/a |
| Dependency-Update-Tool | high | 10, measured | 10 | 10 | n/a |
| Fuzzing | medium | 0, measured | 10, measured | 10 | n/a |
| License | low | 10, expected (9 measured offline) | 10 | 10 | n/a |
| Maintained | high | 0 | 0 until 13 February 2027 | 10 | the repository to be 90 days old, then one commit or maintainer issue reply a week |
| Packaging | medium | not scored | 10, expected, once `release.yml` has completed one run | 10 | n/a |
| Pinned-Dependencies | medium | 10, measured | 10, measured | 10 | n/a |
| SAST | medium | 10, measured | 10 | 10 | n/a |
| Security-Policy | medium | 10, measured | 10 | 10 | n/a |
| Signed-Releases | high | not scored: no release | 10 from the first release | 10 | the first release to be cut while the repository is public |
| Token-Permissions | high | 10, measured | 10, measured | 10 | n/a |
| Vulnerabilities | high | 5, measured | 10, measured | 10 | n/a |

A check that is not scored is left out of the average. The average weights a
critical check 10, a high one 7.5, a medium one 5 and a low one 2.5.

| State of the repository | Expected average |
|---|---|
| public as it was before this page's changes | 4.9 |
| public, the changes merged, the ruleset with one required review, the passing badge, one release | 8.1 |
| the same, 90 days old, the last 30 commits all approved pull requests | 9.6 |
| also two required reviewers, the gold badge and three contributing organisations | 10 |

Scorecard also has two experimental checks, SBOM and Webhooks, that are off
unless asked for and are not part of the published score. Releases carry an
SPDX 2.3 SBOM (`first-bite-<version>.spdx.json`), which is what the SBOM check
looks for; the repository defines no webhook.

## The checks

### Binary-Artifacts

Looks for executables and compiled objects committed to the tree. There are
none: the binary files are `docs/brief/first-bite-brief.pdf`,
`icon/first-bite.png` and `examples/weather-station.pcap`, none of which is
executable code. The WebAssembly decoder is built by `make wasm` into
`site/pkg/`, which `.gitignore` excludes.

### Branch-Protection

Reads the rules on the default branch and scores them in five tiers; a tier
counts only when the one before it is complete.

| Tier | Points | Rule |
|---|---|---|
| 1 | 3 | no force push; no deletion |
| 2 | 3 | at least one approving review; pull request required; branch up to date before merging; approval of the last push |
| 3 | 2 | at least one required status check |
| 4 | 1 | two approving reviews; review by a code owner |
| 5 | 1 | stale approvals dismissed on a new push; no one may bypass the rules |

Today the only rule on `main` is the organisation's, which requires signed and
verified commits: 0 points, because tier 1 is not met.

[rulesets/](rulesets/) holds three bodies for a repository ruleset on `main`,
and one for release tags. All three forbid deletion and force pushes, require
a pull request, a linear history, squash merging, resolved review threads and
these status checks on an up-to-date branch: `scripts`, `fmt`, `clippy`,
`test (ubuntu-latest)`, `test (macos-latest)`, `decoder-targets`, `leak-gate`,
`contributor-trust`.
None has a bypass list. They differ in the review they require:

| File | Approving reviews | Score | What it costs |
|---|---|---|---|
| `rulesets/main-no-review.json` | none | 4 | nothing: the maintainer merges their own pull request when the checks pass. Code-Review stays at 0 |
| `rulesets/main-one-review.json` | one, by a code owner, given after the last push | 8 | a pull request cannot be merged by its author alone. With one maintainer, every change waits for a second member of `goat-owners`; nobody can bypass |
| `rulesets/main-two-reviews.json` | two | 10 | three people for every change |

[CONTRIBUTING.md](../../CONTRIBUTING.md) already states the middle one: "One
approving review from a code owner merges it, by squash."

Adding a bypass actor (an administrator who may merge without review) to
`main-one-review.json` keeps the 8, because the bypass rule is in tier 5; it
then leaves Code-Review to fall with every change merged that way.

Apply one of the three, and the tag ruleset:

```
gh api -X POST repos/USAREUR-AF/first-bite/rulesets --input docs/security/rulesets/main-one-review.json
gh api -X POST repos/USAREUR-AF/first-bite/rulesets --input docs/security/rulesets/tags.json
```

`rulesets/tags.json` forbids deleting, moving or force-pushing a `v*` tag. It
is not scored; it keeps a release tag pointing at the commit that was built.
`rulesets/tag-creation.json` lets only the maintainers' team `goat-owners`
create a tag, as the other goat repositories do; it is designed, not applied.

The status checks are named after the jobs of `ci.yml`, `leak-gate.yml` and
`contributor-trust.yml`. A
job that is renamed has to be renamed in the ruleset, or pull requests wait for
a check that never reports.

### CI-Tests

Of the pull requests merged among the last 30 commits, the share whose head
commit had a successful check run. `ci.yml` runs on every pull request. The
history so far was pushed, not merged from pull requests, so the check has
nothing to count and reports no score; with the ruleset every change is a pull
request with required checks, and the score is 10.

### CII-Best-Practices

Reads the project's [OpenSSF Best Practices](https://www.bestpractices.dev/)
badge: 2 points for a questionnaire in progress, 5 for passing, 7 for silver,
10 for gold. The project is not registered. A person has to register it and
answer the questionnaire; [best-practices-answers.md](best-practices-answers.md)
has an answer and its evidence for each of the passing criteria.

Silver also asks for statement coverage of at least 80%, measured, and for
the project to carry on if one person leaves. Gold asks for at least two
unassociated significant contributors, review of half of all changes by a
second person, and 90% statement coverage. Coverage is not measured.

### Code-Review

Of the changes among the last 30 commits, the share that are pull requests
approved by someone other than their author. A change approved and written by
a bot is not counted. Every commit so far was pushed by one maintainer: 0.
The score is whatever share of the last 30 commits were approved pull requests,
so with one required review it rises with each merge and reaches 10 after 30.

### Contributors

Counts the organisations and companies of the authors of the last 30 commits
who have at least five contributions, from the company and the public
organisation memberships on their GitHub profiles: 10 for three or more, in
proportion below. One organisation gives 3.

### Dangerous-Workflow

Looks for a workflow that is triggered by `pull_request_target` or
`workflow_run` and checks out the pull request's code, and for a script that
interpolates text an outsider controls.

`leak-gate.yml` needs `pull_request_target`: it is what lets a pull request
from a fork be checked against the maintainers' list of names, a secret. It
used to check out the pull request beside the gate, as data. It now checks out
the base branch only, fetches the pull request's commit into the object store
and reads it from there with `tools/check-public-tree.sh --tree <commit>`:
every blob is written under a number to a temporary directory and scanned;
nothing of the pull request is checked out, built or run, and the check that
runs is the one on `main`. The check no longer skips its own file, so a pull
request cannot hide text in a changed copy of it.

### Dependency-Update-Tool

Looks for a Dependabot or Renovate configuration. `.github/dependabot.yml`
covers the workspace's crates, the fuzz targets' crates, the GitHub Actions,
the `Dockerfile`, the fuzz builder image and the mock images, weekly.

### Fuzzing

Looks for an OSS-Fuzz project, a ClusterFuzzLite configuration
(`.clusterfuzzlite/Dockerfile`), or fuzz functions in the source (for Rust,
`libfuzzer_sys`).

[fuzz/](../../fuzz/README.md) has three libFuzzer targets for the code that
reads input the project does not control: `decode_payload`, `load_dictionary`
and `frame_pipeline`. ClusterFuzzLite builds them from `.clusterfuzzlite/` and
runs them for 5 minutes on pull requests that change code (`cflite_pr.yml`) and
for 30 minutes every night (`cflite_batch.yml`).

Measured on one core each of an AMD Ryzen 7 8845HS, AddressSanitizer build,
from the seed corpus:

| Target | Time | Executions | Edges covered | Crashes |
|---|---|---|---|---|
| `decode_payload` | 601 s | 2,963,994 | 633 | 0 |
| `load_dictionary` | 601 s | 1,642,518 | 5,562 | 0 |
| `frame_pipeline` | 601 s | 17,989,010 | 2,628 | 0 |

The ClusterFuzzLite build was run in its builder image and each target then
run for a minute in its runner image, on the build host. The two workflows have
not run on GitHub.

### License

Looks for a licence file at the top level that GitHub recognises as an FSF or
OSI licence. `LICENSE` is the Apache License 2.0. Offline the check cannot ask
GitHub which licence it is and gives 9.

### Maintained

0 for a repository created less than 90 days ago, whatever its activity; the
repository was created on 15 November 2026. After that: the commits plus the
issues a maintainer answered in the last 90 days, out of 12, so one a week
gives 10.

### Packaging

Reads every workflow file for a job that publishes a package, and asks GitHub
whether that workflow has a successful run: 10 when both hold, no score
otherwise. A job counts when one of its steps matches a list in
`IsPackagingWorkflow` (`checks/fileparser/github_workflow.go`); for a
container image the two entries are a step that uses
`docker/build-push-action`, and a `run` step in which the text `docker` is
followed on the same line by `push`. Release archives on the releases page
are not on the list, and the crates are not published to crates.io
(`publish = false`).

The `image-publish` job of `release.yml` pushes the container image of the
`Dockerfile` to `ghcr.io/usareur-af/first-bite` with `docker push` in a `run`
step, which is the second entry. The run that the check asks for is any
successful run of `release.yml`
(`ListWorkflowRunsByFileName` with `Status: "success"`, in
`clients/githubrepo/workflows.go`): the dry run counts, so the score does not
wait for a release.

```
gh workflow run release.yml --ref main -f static=true
```

That builds, runs and packages every archive and both images and publishes
nothing. The job has not run on GitHub.

### Pinned-Dependencies

Looks at every GitHub Action, container image and downloaded tool in the
workflows, Dockerfiles and shell scripts for a reference that can change
under the project: a tag where a commit or digest could be given.

| What | How it is pinned |
|---|---|
| GitHub Actions | full commit SHA, checked by `tools/check-pins.py` |
| images in `Dockerfile`, `.clusterfuzzlite/Dockerfile`, the `Makefile`, the mock compose files | tag and `sha256` digest, checked by `tools/check-pins.py` |
| libpcap source (`tools/libpcap-static.sh`), Npcap SDK (`release.yml`) | version and SHA-256 of the download |
| `cargo-sbom` (`release.yml`), `cargo-audit` (`rust-audit.yml`) | exact version; `cargo install --locked` |
| `yamllint` (`ci.yml`) | exact version, run with `pipx` |
| crates | `Cargo.lock` and `fuzz/Cargo.lock`, with `--locked` |

Not pinned: the packages `apt-get` installs in the two Dockerfiles and on the
runners (Debian and Ubuntu remove superseded versions, so a pinned version
stops installing); the two container images inside ClusterFuzzLite's actions,
which the actions name by the tag `v1`; and the `*_RUNS_IN` images of the
`Makefile`, which exist to try the release on each distribution's current
image. Scorecard counts none of these.

### SAST

Looks for CodeQL or another static analyser, and at whether each merged pull
request was analysed. `codeql.yml` analyses the Rust and Python sources and
the workflows. It ran only on pull requests that changed one of those; it now
runs on every pull request and every push to `main`, so that every merged
change has an analysis. `clippy` with warnings as errors runs in `ci.yml`.

### Security-Policy

Looks in `SECURITY.md` for a link or address to report to, text beyond it, and
the words and numbers of a disclosure process. [SECURITY.md](../../SECURITY.md)
has the private reporting form, what to include, and the times to expect.

### Signed-Releases

Looks at the assets of the last five releases for a signature (`.sig`, `.asc`,
`.minisig`, `.sigstore`, `.sigstore.json`: 8 points) or SLSA provenance
(`.intoto.jsonl`: 10 points). It does not verify them.

`release.yml` already attested every asset with
`actions/attest-build-provenance`: a signed in-toto statement with a SLSA
provenance predicate, kept by GitHub. It now also attaches that statement to
the release as `first-bite-<version>.intoto.jsonl`, so the provenance travels
with the files:

```
gh attestation verify first-bite-1.0.1-x86_64-linux-gnu.tar.gz --repo USAREUR-AF/first-bite --bundle first-bite-1.0.1.intoto.jsonl
```

GitHub issues attestations for public repositories; a release cut while the
repository is private has no provenance and would score 0. There is no release
yet.

The check reads the last five releases, pre-releases included, and gives the
average. A pre-release such as `v1.0.1-rc.1` is built by the same workflow and
carries its own `first-bite-1.0.1-rc.1.intoto.jsonl`, so it scores 10 like any
other release.

### Token-Permissions

Looks for workflows whose token can write. Every workflow sets
`permissions: contents: read` at the top; write scopes are on the one job that
needs each: `security-events` for CodeQL and Scorecard, `id-token` for
Scorecard, Pages and the release attestation, `pages` for the deployment,
`contents` and `attestations` for publishing a release, `packages` and
`attestations` for publishing the image. The check leaves `packages: write`
out of account in a workflow that `IsPackagingWorkflow` matches
(`createIgnoredPermissions` in `checks/raw/permissions.go`).

### Vulnerabilities

Asks OSV for open advisories against the lock files: 10, less one for each.
Five were reported. One is a name collision: the advisory is for the crates.io
crate `bite`, and the `bite` in the lock file is this repository's decoder.
The other four are in crates that only the optional `zenoh` feature builds
(one vulnerability in `lz4_flex`, three notices that a crate is no longer
maintained); the default build and the release binaries do not contain them.
`osv-scanner.toml` lists each with its reason and what removes the entry, as
`.cargo/audit.toml` does for `cargo audit`.

## What is left to do, in order

| # | Who | What |
|---|---|---|
| 1 | maintainer | Choose the review requirement: one of the three ruleset files under [Branch-Protection](#branch-protection). |
| 2 | administrator | Create the ruleset on `main` and the tag ruleset with the two `gh api` calls above. |
| 3 | administrator | Make the repository public. CodeQL, Scorecard and Pages start running, and attestations become available. |
| 4 | administrator | Run `cargo-deny` and `cflite-batch` once from the Actions tab, so that their badges have a result on `main`. |
| 5 | maintainer | Register for the Best Practices badge with [best-practices-answers.md](best-practices-answers.md) and put the project number in the README badge. |
| 6 | maintainer | Run the release workflow as a dry run, after step 3: `gh workflow run release.yml --ref main -f static=true`. Nothing is published; Packaging scores from this run. |
| 7 | maintainer | Cut the first release by tag: it carries the provenance and publishes the image. Then add the release badge to the README. |
| 8 | administrator | Set the visibility of the package `first-bite` to public in the organisation's package settings, once, after the first image is pushed: a new package is private. |

## The workflow

`.github/workflows/scorecard.yml` pins `ossf/scorecard-action` v2.5.0 by
commit, publishes the results, uploads the SARIF to code scanning, and gives
`security-events: write` and `id-token: write` to its one job. It reads
rulesets with the workflow token; no personal token is configured.
