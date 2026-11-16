#!/bin/sh
# Rewrite flow/xf, the packs compiled into the `xf` transform, and the list of
# them in flow/src/xf_embedded.rs.
#
#   tools/bite2pack/refresh.sh                       packs of dictionaries/*, from the dictionaries
#   tools/bite2pack/refresh.sh --upstream <checkout> also the files copied from goat-transformers
#   tools/bite2pack/refresh.sh --check               write nothing; fail when anything is out of date
#
# <checkout> is a clone of https://github.com/USAREUR-AF/goat-transformers at
# the commit Cargo.toml pins. The files copied from it are listed in
# flow/xf/UPSTREAM with that commit; --check compares the two commits.
set -eu
cd "$(dirname "$0")/../.."

pinned=$(sed -n 's/^goat-transformers = .*rev = "\([0-9a-f]\{40\}\)".*/\1/p' Cargo.toml)
[ -n "$pinned" ] || { echo "refresh: Cargo.toml pins no goat-transformers commit" >&2; exit 1; }
copied="model/core.def model/track.def packs/cot-xml/pack.def packs/geojson/pack.def"

generate() {
    python3 tools/bite2pack/bite2pack.py --out flow/xf --bindings flow/xf/bindings.json \
        --core-def flow/xf/model/core.def "$@" dictionaries/*/
}

case "${1:-}" in
--check)
    recorded=$(sed -n 's/^commit: //p' flow/xf/UPSTREAM)
    if [ "$recorded" != "$pinned" ]; then
        echo "refresh: flow/xf/UPSTREAM is at $recorded and Cargo.toml pins $pinned;" \
            "run tools/bite2pack/refresh.sh --upstream <checkout>" >&2
        exit 1
    fi
    generate --check
    python3 tools/bite2pack/embed.py --check
    exit 0
    ;;
--upstream)
    upstream=${2:?"--upstream needs the path of a goat-transformers checkout"}
    head=$(git -C "$upstream" rev-parse HEAD)
    if [ "$head" != "$pinned" ]; then
        echo "refresh: $upstream is at $head; Cargo.toml pins $pinned" >&2
        exit 1
    fi
    for file in $copied; do
        mkdir -p "flow/xf/$(dirname "$file")"
        cp "$upstream/$file" "flow/xf/$file"
    done
    {
        echo "The files below are copied unchanged from"
        echo "https://github.com/USAREUR-AF/goat-transformers (Apache License 2.0)"
        echo "at the commit Cargo.toml pins. tools/bite2pack/refresh.sh --upstream rewrites them."
        echo
        echo "commit: $pinned"
        for file in $copied; do echo "file: $file"; done
    } > flow/xf/UPSTREAM
    ;;
"") ;;
*)
    echo "usage: tools/bite2pack/refresh.sh [--check | --upstream <goat-transformers checkout>]" >&2
    exit 2
    ;;
esac

generate
python3 tools/bite2pack/embed.py
