#!/usr/bin/env bash
# Refuse material that does not belong in this repository.
#
#   tools/check-public-tree.sh            scan every tracked file
#   tools/check-public-tree.sh <path>...  scan the given files
#
# Three checks:
#   1. credential shapes: private keys, cloud access key ids, tokens;
#   2. capture files outside examples/;
#   3. needles: names that must never appear (a deployment's hosts, places and
#      accounts; the message and field names of a restricted message set). The
#      needle list is NOT in this repository. It is read from the environment
#      variable LEAK_NEEDLES or from the untracked file .leak-needles, one per
#      line: a plain line matches case-insensitively as a substring, a line
#      starting with "re:" is an extended regular expression matched
#      case-sensitively. Without a list, check 3 is skipped and says so.
#
# A needle hit prints the file, the line number and a 12-character id of the
# needle, never the needle or the line.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

files=()
if [ "$#" -gt 0 ]; then
  files=("$@")
else
  while IFS= read -r -d '' f; do files+=("$f"); done < <(git ls-files -z)
fi

fail=0

# Scan only regular files, and never this script (it holds the patterns).
scan=()
for f in "${files[@]}"; do
  [ -f "$f" ] && [ "$f" != "tools/check-public-tree.sh" ] && scan+=("$f")
done
[ "${#scan[@]}" -gt 0 ] || { echo "check-public-tree: nothing to scan"; exit 0; }

# Print "file:line" for every match of the grep arguments over the scan set.
# /dev/null keeps grep printing the file name when one file is scanned.
matches() {
  grep -nI "$@" -- "${scan[@]}" /dev/null 2>/dev/null | cut -d: -f1,2 || true
}

# 1. credential shapes
credential_patterns=(
  '-----BEGIN [A-Z ]*PRIVATE KEY-----'
  '\b(AKIA|ASIA)[0-9A-Z]{16}\b'
  '\bgh[pousr]_[A-Za-z0-9]{36,}\b'
  '\bgithub_pat_[A-Za-z0-9_]{60,}\b'
  '\bxox[baprs]-[A-Za-z0-9-]{10,}\b'
  '\bAGE-SECRET-KEY-1[0-9A-Z]{58}\b'
)
for pattern in "${credential_patterns[@]}"; do
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    echo "credential shape: $hit"
    fail=1
  done < <(matches -E -e "$pattern")
done

# 2. capture files
for f in "${files[@]}"; do
  case "$f" in
    examples/*.pcap) ;;
    *.pcap|*.pcapng|*.cap)
      echo "capture file outside examples/: $f"
      fail=1
      ;;
  esac
done

# 3. needles
needles=""
if [ -n "${LEAK_NEEDLES:-}" ]; then
  needles="$LEAK_NEEDLES"
elif [ -f .leak-needles ]; then
  needles="$(cat .leak-needles)"
fi

if [ -z "$needles" ]; then
  echo "note: no needle list (LEAK_NEEDLES or .leak-needles); name check skipped"
else
  hash_id() {
    if command -v sha256sum >/dev/null 2>&1; then
      printf '%s' "$1" | sha256sum | cut -c1-12
    else
      printf '%s' "$1" | shasum -a 256 | cut -c1-12
    fi
  }
  count=0
  while IFS= read -r needle; do
    case "$needle" in ''|'#'*) continue ;; esac
    count=$((count + 1))
    id=$(hash_id "$needle")
    if [ "${needle#re:}" != "$needle" ]; then
      grep_args=(-E -e "${needle#re:}")
    else
      grep_args=(-iF -e "$needle")
    fi
    while IFS= read -r hit; do
      [ -n "$hit" ] || continue
      echo "needle $id: $hit"
      fail=1
    done < <(matches "${grep_args[@]}")
    # File names leak too.
    while IFS= read -r hit; do
      [ -n "$hit" ] || continue
      echo "needle $id: path of $hit"
      fail=1
    done < <(printf '%s\n' "${scan[@]}" | grep "${grep_args[@]}" || true)
  done <<< "$needles"
  echo "checked ${#scan[@]} files against $count needles"
fi

if [ "$fail" -ne 0 ]; then
  echo "check-public-tree: FAILED" >&2
  exit 1
fi
echo "check-public-tree: ok"
