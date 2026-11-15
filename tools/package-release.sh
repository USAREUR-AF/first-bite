#!/usr/bin/env bash
# Assemble one release archive from built binaries.
#
#   tools/package-release.sh <platform> <directory holding the binaries>
#
#   tools/package-release.sh x86_64-linux-gnu target/ubuntu-20.04/release
#
# Writes dist/first-bite-<version>-<platform>.tar.gz (.zip for a Windows
# platform) and prints its path. <version> is the version of the mind crate;
# under GitHub Actions on a tag, the tag must be v<version>.
#
#   *-linux-*   deployment/ with install.sh, the systemd unit, mind, bite and
#               example configuration in config/; dictionaries/
#   *-windows-* bin/bite.exe (and bin/mind.exe when it was built); dictionaries/
#   others      bin/mind, bin/bite; config/; dictionaries/
# Every archive also holds LICENSE, NOTICE, INTENT.md and docs/deployment.md
# as INSTALL.md.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: tools/package-release.sh <platform> <directory holding the binaries>" >&2
  exit 2
fi
platform="$1"
bin_dir="$(cd "$2" && pwd)"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' mind/Cargo.toml | head -n 1)"
[ -n "$version" ] || {
  echo "package-release: no version in mind/Cargo.toml" >&2
  exit 1
}
if [ "${GITHUB_REF_TYPE:-}" = tag ] && [ "v$version" != "${GITHUB_REF_NAME:-}" ]; then
  echo "::error::mind/Cargo.toml says $version and the tag is $GITHUB_REF_NAME: set the version before tagging"
  exit 1
fi

name="first-bite-$version-$platform"
stage="dist/$name"
rm -rf "$stage"
mkdir -p "$stage"

copy_config() {
  mkdir -p "$1"
  for kind in publishers subscribers subpubs; do
    cp "flow/config/$kind.local.yml" "$1/$kind.yml"
  done
}

case "$platform" in
  *-linux-*)
    cp -R deployment "$stage/deployment"
    install -m 0755 "$bin_dir/mind" "$bin_dir/bite" "$stage/deployment/"
    copy_config "$stage/deployment/config"
    ;;
  *-windows-*)
    mkdir -p "$stage/bin"
    cp "$bin_dir/bite.exe" "$stage/bin/"
    if [ -f "$bin_dir/mind.exe" ]; then
      cp "$bin_dir/mind.exe" "$stage/bin/"
      copy_config "$stage/config"
    fi
    ;;
  *)
    mkdir -p "$stage/bin"
    install -m 0755 "$bin_dir/mind" "$bin_dir/bite" "$stage/bin/"
    copy_config "$stage/config"
    ;;
esac

mkdir -p "$stage/dictionaries"
for dictionary in ntp weather-station vehicle-can glasshouse; do
  cp -R "dictionaries/$dictionary" "$stage/dictionaries/$dictionary"
done
cp LICENSE NOTICE INTENT.md "$stage/"
cp docs/deployment.md "$stage/INSTALL.md"

case "$platform" in
  *-windows-*)
    archive="dist/$name.zip"
    rm -f "$archive"
    if command -v 7z >/dev/null 2>&1; then
      (cd dist && 7z a -tzip -bso0 "$name.zip" "$name")
    else
      (cd dist && zip -qr "$name.zip" "$name")
    fi
    ;;
  *)
    archive="dist/$name.tar.gz"
    # Files in the archive belong to root whoever built it.
    if tar --version | grep -q 'GNU tar'; then
      owner=(--owner=0 --group=0 --numeric-owner)
    else
      owner=(--uid 0 --gid 0 --numeric-owner)
    fi
    tar -C dist "${owner[@]}" -czf "$archive" "$name"
    ;;
esac
rm -rf "$stage"
echo "$archive"
