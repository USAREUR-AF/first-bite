#!/usr/bin/env bash
# Print the version a release is published under.
#
#   tools/release-version.sh
#
# Away from a tag, that is the version of the mind crate. Under GitHub Actions
# on a tag, it is the tag without its `v`, and the tag must be v<version> or
# a pre-release of it, v<version>-<suffix> (v1.1.0-rc.1 for version 1.1.0):
# the script fails on any other tag. A pre-release is published under its own
# name, and its binaries report the version of the crate.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/mind/Cargo.toml" | head -n 1)"
[ -n "$version" ] || {
  echo "release-version: no version in mind/Cargo.toml" >&2
  exit 1
}
if [ "${GITHUB_REF_TYPE:-}" = tag ]; then
  tag="${GITHUB_REF_NAME:-}"
  case "$tag" in
    "v$version" | "v$version"-?*) version="${tag#v}" ;;
    *)
      echo "::error::mind/Cargo.toml says $version and the tag is $tag: the tag must be v$version or v$version-<pre-release>"
      exit 1
      ;;
  esac
  case "$version" in
    *[!0-9A-Za-z.-]*)
      echo "::error::the tag $tag has a character outside 0-9, A-Z, a-z, '.' and '-'"
      exit 1
      ;;
  esac
fi
echo "$version"
