# Changelog

This page says where the record of what changed is. It holds no entries itself.

| Name | What it is | Use it when |
|---|---|---|
| `vX.Y.Z` | A release: a tag on `main`, cut by a maintainer after the release gates pass. `X.Y.Z` is the version of the `mind` crate | You want a fixed reference. Pin this tag |
| `main` | The one development branch. Every change lands here | You are contributing |

What changed in a release is on its release page, at <https://github.com/USAREUR-AF/first-bite/releases>. `.github/workflows/release.yml` publishes the page when the tag is pushed and lists the pull requests merged since the release before; the title of each is its Conventional Commits subject. [GOVERNANCE.md](GOVERNANCE.md#releases) says who cuts a release.

No release has been published yet.
