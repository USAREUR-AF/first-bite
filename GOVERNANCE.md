# Governance

How decisions are made in the goat repositories, including this one. [`goatcore`'s GOVERNANCE.md](https://github.com/USAREUR-AF/goatcore/blob/main/GOVERNANCE.md) is the reference copy.

## Roles

| Role | Who | What they can do |
|---|---|---|
| **Maintainers** | the members of the GitHub team `goat-owners` | administer the repositories, approve and merge pull requests, cut releases, decide project-level questions |
| **Code owners** | listed in `CODEOWNERS` | their approval is required on pull requests touching the paths they own |
| **Contributors** | anyone | open issues and pull requests under [CONTRIBUTING.md](CONTRIBUTING.md) |

## Decisions

- **Day-to-day technical decisions** are made by the contributor doing the work, subject to pull-request review.
- **Load-bearing architectural decisions** that span the goat repositories are recorded as ADRs under `docs/adr/` in [`goatcore`](https://github.com/USAREUR-AF/goatcore/blob/main/docs/adr/INDEX.md), each with a plain-language summary. ADRs are not rewritten; a later ADR supersedes an earlier one and both say so.
- **Changes to the dictionary format** ([docs/dictionary-format.md](docs/dictionary-format.md)) keep existing dictionaries loading. A change that cannot is a project-level decision.
- **Project-level decisions** (licence, scope, governance, releases, new repositories) are made by consensus of the maintainers. Where consensus is not reached after discussion, the maintainers decide by simple majority.

## Becoming a maintainer

Sustained, high-quality contribution over several pull requests; demonstrated judgment in review and design discussion; nomination by a maintainer and no objection from another maintainer within seven days. Maintainers who are inactive for six months are asked whether they wish to remain; those who step down are thanked in the release notes.

## Releases

Releases are tagged `vX.Y.Z` from `main` by a maintainer after the release gates pass. Version numbers follow semantic versioning. Pre-1.0, minor versions may change interfaces; the release notes say so.

## Code of conduct

[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) applies in every project space. Reports go to the maintainers through the private channel in [SECURITY.md](SECURITY.md).
