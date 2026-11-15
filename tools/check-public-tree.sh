#!/usr/bin/env bash
# Refuse material that does not belong in this repository.
#
#   tools/check-public-tree.sh            scan every tracked file
#   tools/check-public-tree.sh <path>...  scan the given files
#
# Four checks:
#   1. credential shapes: private keys, cloud access key ids, tokens;
#   2. capture files outside examples/;
#   3. identifiers of people, machines and tools, by shape:
#        ssh-public-key     an OpenSSH public key
#        age-recipient      an age public key
#        mac-address        exactly six colon-separated octets, other than
#                           broadcast and all-zero
#        private-ipv4       10/8, 172.16/12, 192.168/16, 169.254/16
#        cgnat-ipv4         100.64/10, the range mesh VPNs assign from
#        email-address      outside example.org and users.noreply.github.com
#        assistant-reference  an AI assistant's session link or attribution
#                           trailer, outside AGENTS.md, CLAUDE.md and .claude/
#      Documentation addresses (RFC 5737: 192.0.2.0/24, 198.51.100.0/24,
#      203.0.113.0/24), 127.0.0.0/8 and 0.0.0.0 are not matched. A line that
#      must hold one of these shapes and identifies nothing real carries
#      `opsec-allow: <reason>` and is skipped by this check alone;
#   4. needles: names that must never appear (a deployment's hosts, places and
#      accounts; the message and field names of a restricted message set). The
#      needle list is NOT in this repository. It is read from the environment
#      variable LEAK_NEEDLES or from the untracked file .leak-needles, one per
#      line: a plain line matches case-insensitively as a substring, a line
#      starting with "re:" is an extended regular expression matched
#      case-sensitively. Without a list, check 4 is skipped and says so.
#
# A hit prints the file, the line number and the class of what was found, or
# for a needle a 12-character id of it: never the matched text or the line.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

files=()
if [ "$#" -gt 0 ]; then
  files=("$@")
else
  while IFS= read -r -d '' f; do files+=("$f"); done < <(git ls-files -z)
fi

fail=0

# Scan only regular files, and never this script (it holds the patterns). A
# symbolic link is not followed: the tree being scanned may be a pull request,
# and what a link points to is outside it.
scan=()
for f in "${files[@]}"; do
  f="${f#./}"
  [ -f "$f" ] && [ ! -L "$f" ] && [ "$f" != "tools/check-public-tree.sh" ] && scan+=("$f")
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

# 3. identifiers by shape
allowed_lines="$(mktemp "${TMPDIR:-/tmp}/check-public-tree.XXXXXX")"
trap 'rm -f "$allowed_lines"' EXIT
# "file:line" of every line that carries the annotation with a reason.
matches -E -e 'opsec-allow: *[^ ]' >"$allowed_lines"

# Print "<class>: file:line" for every match of <pattern> that also matches
# <exact> in full, is not an allowed value, and is not on an annotated line.
# <pattern> takes a whole run of the characters the shape is made of, and
# <exact> then says which runs are the shape, so that a longer run (eight
# octets of a fingerprint, five numbers of a version) is not mistaken for one
# and two shapes side by side are both seen. Empty <exact>: every match.
# Empty <allowed>: no exception.
#   shape <class> <pattern> <exact> <allowed> <file>...
shape() {
  local class=$1 pattern=$2 exact=$3 allowed=$4 hit
  shift 4
  [ "$#" -gt 0 ] || return 0
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    grep -qxF -e "$hit" "$allowed_lines" && continue
    echo "$class: $hit"
    fail=1
  done < <(
    # Each line is "file:line:match".
    grep -nIoE -e "$pattern" -- "$@" /dev/null 2>/dev/null |
      { if [ -n "$exact" ]; then grep -E -e "^[^:]*:[0-9]+:($exact)\$" || true; else cat; fi; } |
      { if [ -n "$allowed" ]; then grep -viE -e "^[^:]*:[0-9]+:($allowed)\$" || true; else cat; fi; } |
      cut -d: -f1,2 | sort -u
  )
}

hex='[0-9A-Fa-f]'
octet='[0-9]{1,3}'
dotted='[0-9]+(\.[0-9]+)+'
shape ssh-public-key \
  '(ssh-(ed25519|rsa|dss)|ecdsa-sha2-nistp[0-9]+|sk-(ssh-ed25519|ecdsa-sha2-nistp256)@openssh\.com) AAAA[A-Za-z0-9+/]{40,}' \
  '' '' "${scan[@]}"
shape age-recipient '[a-z0-9]*age1[a-z0-9]{58,}' 'age1[a-z0-9]{58}' '' "${scan[@]}"
shape mac-address "(${hex}|:)*(${hex}{2}:){5}${hex}{2}(${hex}|:)*" "(${hex}{2}:){5}${hex}{2}" \
  '(ff:){5}ff|(00:){5}00' "${scan[@]}"
shape private-ipv4 "$dotted" \
  "(10\\.${octet}|172\\.(1[6-9]|2[0-9]|3[01])|192\\.168|169\\.254)\\.${octet}\\.${octet}" \
  '' "${scan[@]}"
shape cgnat-ipv4 "$dotted" \
  "100\\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\\.${octet}\\.${octet}" \
  '' "${scan[@]}"
shape email-address \
  '[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}' '' \
  '.*@([a-z0-9-]+\.)*example\.org|.*@users\.noreply\.github\.com' "${scan[@]}"

# The files that instruct coding assistants name them; nothing else does.
elsewhere=()
for f in "${scan[@]}"; do
  case "$f" in
    AGENTS.md | CLAUDE.md | */AGENTS.md | */CLAUDE.md | .claude/* | */.claude/*) ;;
    *) elsewhere+=("$f") ;;
  esac
done
if [ "${#elsewhere[@]}" -gt 0 ]; then
  shape assistant-reference \
    'claude\.ai/|anthropic\.com|chatgpt\.com/|chat\.openai\.com/|Claude-Session:|Co-Authored-By:[^<]*(Claude|Copilot|ChatGPT|Codex|Gemini|Cursor|Devin)|Generated (with|by) \[?(Claude|Copilot|ChatGPT|Codex|Gemini|Cursor)' \
    '' '' "${elsewhere[@]}"
fi

# 4. needles
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
