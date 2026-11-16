#!/usr/bin/env bash
# Refuse material that does not belong in this repository.
#
#   tools/check-public-tree.sh                scan every tracked file
#   tools/check-public-tree.sh <path>...      scan the given files
#   tools/check-public-tree.sh --tree <rev>   scan every file of the commit
#                                             <rev>, read from the object
#                                             store: nothing of that commit
#                                             is checked out or run
#
# --tree is how the leak gate reads a pull request (leak-gate.yml): the gate is
# this file as merged, and the pull request is only ever data to it. This file
# is scanned like any other, so its own patterns are written not to match
# themselves.
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
#      case-sensitively. Without a list, check 4 is skipped and says so. A
#      list that cannot be read, or a needle grep cannot use, fails the check.
#      All plain needles are matched in one grep -F pass, all re: lines in one
#      grep -E pass.
#
# A hit prints the file, the line number and the class of what was found, or
# for a needle a 12-character id of it: never the matched text or the line.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

work="$(mktemp -d "${TMPDIR:-/tmp}/check-public-tree.XXXXXX")"
trap 'rm -rf "$work"' EXIT

# files: every path given, for the checks on names. scan: the files grep
# reads. names: the path each of them is reported under.
files=()
scan=()
names=()
if [ "${1:-}" = "--tree" ]; then
  [ "$#" -eq 2 ] || { echo "usage: check-public-tree.sh --tree <rev>" >&2; exit 2; }
  rev="$(git rev-parse --verify --quiet "$2^{commit}")" ||
    { echo "check-public-tree: $2 is not a commit in this repository" >&2; exit 2; }
  # Each blob is written under a number, never under its path: a path in a
  # pull request is data too. A symbolic link is a blob holding its target and
  # is scanned as that text; a submodule has no blob here and only its path is
  # checked.
  store="$work/blobs"
  mkdir "$store"
  while IFS= read -r -d '' entry; do
    path="${entry#*$'\t'}"
    read -r _ kind object <<<"${entry%%$'\t'*}"
    files+=("$path")
    [ "$kind" = blob ] || continue
    git cat-file blob "$object" >"$store/${#scan[@]}"
    names+=("$path")
    scan+=("$store/${#scan[@]}")
  done < <(git ls-tree -r -z "$rev")
else
  store=""
  if [ "$#" -gt 0 ]; then
    files=("$@")
  else
    while IFS= read -r -d '' f; do files+=("$f"); done < <(git ls-files -z)
  fi
  # Regular files only. A symbolic link is not followed: what it points to
  # may be outside the repository.
  for f in "${files[@]}"; do
    f="${f#./}"
    [ -f "$f" ] && [ ! -L "$f" ] && scan+=("$f") && names+=("$f")
  done
fi

fail=0

[ "${#files[@]}" -gt 0 ] || { echo "check-public-tree: nothing to scan"; exit 0; }

# Rewrite the "file:line" lines on standard input to name the path each
# scanned file is reported under.
reported() {
  local hit number
  if [ -z "$store" ]; then
    cat
    return
  fi
  while IFS= read -r hit; do
    hit="${hit#"$store/"}"
    number="${hit%%:*}"
    printf '%s:%s\n' "${names[$number]}" "${hit#*:}"
  done
}

# Print "file:line" for every match of the grep arguments over the scan set.
# /dev/null keeps grep printing the file name when one file is scanned.
matches() {
  [ "${#scan[@]}" -gt 0 ] || return 0
  grep -nI "$@" -- "${scan[@]}" /dev/null 2>/dev/null | cut -d: -f1,2 | reported || true
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
allowed_lines="$work/allowed"
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
      cut -d: -f1,2 | reported | sort -u
  )
}

hex='[0-9A-Fa-f]'
octet='[0-9]{1,3}'
dotted='[0-9]+(\.[0-9]+)+'
shape ssh-public-key \
  '(ssh-(ed25519|rsa|dss)|ecdsa-sha2-nistp[0-9]+|sk-(ssh-ed25519|ecdsa-sha2-nistp256)@openssh\.com) AAAA[A-Za-z0-9+/]{40,}' \
  '' '' ${scan[@]+"${scan[@]}"}
shape age-recipient '[a-z0-9]*age1[a-z0-9]{58,}' 'age1[a-z0-9]{58}' '' ${scan[@]+"${scan[@]}"}
shape mac-address "(${hex}|:)*(${hex}{2}:){5}${hex}{2}(${hex}|:)*" "(${hex}{2}:){5}${hex}{2}" \
  '(ff:){5}ff|(00:){5}00' ${scan[@]+"${scan[@]}"}
shape private-ipv4 "$dotted" \
  "(10\\.${octet}|172\\.(1[6-9]|2[0-9]|3[01])|192\\.168|169\\.254)\\.${octet}\\.${octet}" \
  '' ${scan[@]+"${scan[@]}"}
shape cgnat-ipv4 "$dotted" \
  "100\\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\\.${octet}\\.${octet}" \
  '' ${scan[@]+"${scan[@]}"}
shape email-address \
  '[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}' '' \
  '.*@([a-z0-9-]+\.)*example\.org|.*@users\.noreply\.github\.com' ${scan[@]+"${scan[@]}"}

# The files that instruct coding assistants name them; nothing else does.
elsewhere=()
for index in "${!scan[@]}"; do
  case "${names[$index]}" in
    AGENTS.md | CLAUDE.md | */AGENTS.md | */CLAUDE.md | .claude/* | */.claude/*) ;;
    *) elsewhere+=("${scan[$index]}") ;;
  esac
done
# [-] and [:] keep the two trailer patterns from matching this line.
shape assistant-reference \
  'claude\.ai/|anthropic\.com|chatgpt\.com/|chat\.openai\.com/|Claude[-]Session:|Co-Authored[-]By:[^<]*(Claude|Copilot|ChatGPT|Codex|Gemini|Cursor|Devin)|Generated (with|by) \[?(Claude|Copilot|ChatGPT|Codex|Gemini|Cursor)' \
  '' '' ${elsewhere[@]+"${elsewhere[@]}"}

# 4. needles
needles=""
if [ -n "${LEAK_NEEDLES:-}" ]; then
  needles="$LEAK_NEEDLES"
elif [ -e .leak-needles ]; then
  # A list that is there and cannot be read stops the check: it never passes
  # as if there were no list.
  [ -f .leak-needles ] && [ -r .leak-needles ] ||
    { echo "check-public-tree: .leak-needles cannot be read" >&2; exit 2; }
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
  # The list is read in one pass per kind, not once per needle: grep -F -f
  # matches every plain needle together. Plain needles go to one file, the
  # re: lines (without the prefix) to another.
  plain="$work/needles.plain"
  regex="$work/needles.re"
  printf '%s\n' "$needles" | awk -v plain="$plain" -v regex="$regex" '
    /^$/ || /^#/ { next }
    /^re:/ { print substr($0, 4) > regex; next }
    { print > plain }
  '
  touch "$plain" "$regex"
  count=$(($(wc -l <"$plain") + $(wc -l <"$regex")))

  # grep exits 0 on a hit, 1 on none; anything else is an error, and an error
  # fails the check instead of passing it.
  #   needle_grep <out> <grep arguments>...
  needle_grep() {
    local out=$1 status=0
    shift
    LC_ALL=C grep "$@" >"$out" 2>"$work/grep.err" || status=$?
    if [ "$status" -gt 1 ]; then
      echo "check-public-tree: the needle check could not run (grep exit $status)" >&2
      fail=1
      return 1
    fi
    return 0
  }

  # Print the 12-character id of the plain needle that equals <text> without
  # regard to case.
  plain_id() {
    local found
    found="$(LC_ALL=C grep -ixF -m1 -e "$1" "$plain" || true)"
    hash_id "${found:-$1}"
  }

  if [ -s "$plain" ]; then
    # Contents: "file:line:matched text"; the text maps a hit to its needle's
    # id and is never printed.
    if [ "${#scan[@]}" -gt 0 ] && needle_grep "$work/plain.hits" -nIoiF -f "$plain" -- "${scan[@]}" /dev/null; then
      while IFS= read -r hit; do
        [ -n "$hit" ] || continue
        file="${hit%%:*}"
        rest="${hit#*:}"
        line="${rest%%:*}"
        text="${rest#*:}"
        printf 'needle %s: %s\n' "$(plain_id "$text")" "$(printf '%s:%s\n' "$file" "$line" | reported)"
        fail=1
      done <"$work/plain.hits"
    fi
    # File names leak too. Contents: "number:matched text", number being the
    # position of the path in the list.
    printf '%s\n' "${files[@]}" >"$work/paths"
    if needle_grep "$work/path.hits" -noiF -f "$plain" -- "$work/paths"; then
      while IFS= read -r hit; do
        [ -n "$hit" ] || continue
        number="${hit%%:*}"
        printf 'needle %s: path of %s\n' "$(plain_id "${hit#*:}")" "${files[$((number - 1))]}"
        fail=1
      done <"$work/path.hits"
    fi
  fi

  if [ -s "$regex" ]; then
    # One pass over every re: line; only when it finds something is each line
    # run alone, to name the one that matched.
    any=0
    if [ "${#scan[@]}" -gt 0 ] && needle_grep "$work/re.any" -lIE -f "$regex" -- "${scan[@]}" /dev/null; then
      [ -s "$work/re.any" ] && any=1
    fi
    printf '%s\n' "${files[@]}" >"$work/paths"
    if needle_grep "$work/re.paths" -E -f "$regex" -- "$work/paths"; then
      [ -s "$work/re.paths" ] && any=1
    fi
    if [ "$any" -eq 1 ]; then
      while IFS= read -r pattern; do
        id="$(hash_id "re:$pattern")"
        if [ "${#scan[@]}" -gt 0 ] && needle_grep "$work/re.one" -nIE -e "$pattern" -- "${scan[@]}" /dev/null; then
          while IFS= read -r hit; do
            [ -n "$hit" ] || continue
            echo "needle $id: $(printf '%s\n' "$hit" | cut -d: -f1,2 | reported)"
            fail=1
          done <"$work/re.one"
        fi
        if needle_grep "$work/re.one" -nE -e "$pattern" -- "$work/paths"; then
          while IFS= read -r hit; do
            [ -n "$hit" ] || continue
            echo "needle $id: path of ${files[$((${hit%%:*} - 1))]}"
            fail=1
          done <"$work/re.one"
        fi
      done <"$regex"
    fi
  fi
  echo "checked ${#files[@]} files against $count needles"
fi

if [ "$fail" -ne 0 ]; then
  echo "check-public-tree: FAILED" >&2
  exit 1
fi
echo "check-public-tree: ok"
