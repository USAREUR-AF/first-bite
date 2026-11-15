#!/usr/bin/env bash
# Self-test of tools/check-public-tree.sh: plants one of each thing it must
# refuse in a throwaway git repository and checks that each is caught, by its
# class, without the planted text appearing in the output; then checks that
# clean input, documentation addresses and annotated lines pass.
#
#   tools/check-public-tree.test.sh
#
# Every planted value is assembled from pieces here, so this file passes the
# check it tests.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/check-public-tree-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT

git -C "$work" init --quiet
mkdir -p "$work/tools"
cp "$here/check-public-tree.sh" "$work/tools/"

failures=0
cases=0

# Run the check in the throwaway repository. LEAK_NEEDLES is the first
# argument; the rest are passed on.
check() {
  local needles=$1
  shift
  (cd "$work" && git add -A && LEAK_NEEDLES="$needles" bash tools/check-public-tree.sh "$@" 2>&1)
}

# caught <name> <expected output line> <planted text> [needles]
# The file `planted` holds the text; the check must fail, print the expected
# line, and print no part of what was planted.
caught() {
  local name=$1 expected=$2 planted=$3 needles=${4:-} output status=0
  cases=$((cases + 1))
  printf 'first line\n%s\n' "$planted" >"$work/planted"
  output="$(check "$needles")" || status=$?
  rm -f "$work/planted"
  if [ "$status" -eq 0 ]; then
    echo "FAIL $name: the check passed"
  elif ! grep -qxF -e "$expected" <<<"$output"; then
    echo "FAIL $name: expected the line '$expected'"
  elif grep -qF -e "$planted" <<<"$output"; then
    echo "FAIL $name: the output repeats what was planted"
  else
    return 0
  fi
  failures=$((failures + 1))
}

# passes <name> <text> [path]
passes() {
  local name=$1 text=$2 path=${3:-clean} output status=0
  cases=$((cases + 1))
  mkdir -p "$work/$(dirname "$path")"
  printf '%s\n' "$text" >"$work/$path"
  output="$(check "")" || status=$?
  rm -f "$work/$path"
  if [ "$status" -ne 0 ]; then
    echo "FAIL $name: the check refused it"
    printf '    %s\n' "${output//$'\n'/$'\n'    }"
    failures=$((failures + 1))
  fi
}

a40="$(printf 'A%.0s' $(seq 40))"
q58="$(printf 'q%.0s' $(seq 58))"
at='@'

# 1. credential shapes
caught "private key" "credential shape: planted:2" "-----BEGIN OPENSSH PRIVATE"" KEY-----"
caught "private key" "credential shape: planted:2" "-----BEGIN RSA PRIVATE"" KEY-----"
caught "access key id" "credential shape: planted:2" "AKIA""ABCDEFGHIJKLMNOP"
caught "token" "credential shape: planted:2" "ghp""_$(printf 'a%.0s' $(seq 36))"

# 2. capture files
cases=$((cases + 1))
printf 'x' >"$work/trace.pcap"
if output="$(check "")"; then
  echo "FAIL capture file: the check passed"
  failures=$((failures + 1))
elif ! grep -qxF "capture file outside examples/: trace.pcap" <<<"$output"; then
  echo "FAIL capture file: not reported"
  failures=$((failures + 1))
fi
rm -f "$work/trace.pcap"

# 3. identifiers by shape
caught "ssh ed25519 key" "ssh-public-key: planted:2" "ssh-ed25519 AAAA""C3NzaC1lZDI1NTE5$a40 someone"
caught "ssh rsa key" "ssh-public-key: planted:2" "ssh-rsa AAAA""B3NzaC1yc2E$a40"
caught "ssh ecdsa key" "ssh-public-key: planted:2" "ecdsa-sha2-nistp256 AAAA""E2VjZHNh$a40"
caught "age recipient" "age-recipient: planted:2" "recipient: age""1$q58"
caught "mac address" "mac-address: planted:2" "link/ether 3c:22:fb"":9a:01:7e brd"
caught "mac address, upper case" "mac-address: planted:2" "3C:22:FB"":9A:01:7E"
caught "private address 10/8" "private-ipv4: planted:2" "host 10"".20.30.40 is up"
caught "private address 172.16/12" "private-ipv4: planted:2" "172"".31.255.1"
caught "private address 192.168/16" "private-ipv4: planted:2" "gateway=192"".168.1.1"
caught "link-local address" "private-ipv4: planted:2" "169"".254.10.10"
caught "mesh address" "cgnat-ipv4: planted:2" "peer 100"".64.0.7"
caught "mesh address, top of range" "cgnat-ipv4: planted:2" "100"".127.255.254"
caught "e-mail address" "email-address: planted:2" "contact first.last${at}army.example.com"
caught "e-mail beside an allowed one" "email-address: planted:2" \
  "a${at}example.org, b${at}mail.example.net"
caught "session link" "assistant-reference: planted:2" "https://claude"".ai/code/session_0123"
caught "vendor link" "assistant-reference: planted:2" "see https://www.anthropic"".com/x"
caught "attribution trailer" "assistant-reference: planted:2" \
  "Co-Authored""-By: Claude <noreply${at}example.org>"
caught "generated-with line" "assistant-reference: planted:2" "Generated"" with [Claude Code]"
caught "annotation without a reason" "private-ipv4: planted:2" "10"".1.2.3 # opsec-allow:"

# 4. needles
caught "needle" "needle $(printf '%s' 'Fort Example' | shasum -a 256 2>/dev/null | cut -c1-12 || true): planted:2" \
  "deployed at fort example today" "Fort Example"

# What must pass.
passes "plain text" "nothing to see: version 1.10.20, build 12:34:56, ratio 3:2"
passes "documentation addresses" \
  "192.0.2.10 198.51.100.7 203.0.113.250 127.0.0.1 127.8.9.10 0.0.0.0 255.255.255.255 8.8.8.8"
passes "public and near-miss addresses" "100.63.255.255 100.128.0.1 172.15.0.1 172.32.0.1 11.0.0.1 1.10.2.3"
passes "allowed mail domains" \
  "a${at}example.org b${at}mail.example.org 12345+name${at}users.noreply.github.com"
passes "broadcast and all-zero MAC" "ff:ff:ff:ff:ff:ff 00:00:00:00:00:00 FF:FF:FF:FF:FF:FF"
caught "mac address after a broadcast one" "mac-address: planted:2" "ff:ff:ff:ff:ff:ff 3c:22:fb"":9a:01:7e"
caught "address at the end of a sentence" "private-ipv4: planted:2" "It answers on 10"".20.30.40."
caught "two addresses on a line" "cgnat-ipv4: planted:2" "203.0.113.5 100"".64.0.7"
passes "a fingerprint is not a MAC" "SHA256 de:ad:be:ef:00:11:22:33:44:55:66:77"
passes "annotated line" "bind 10"".0.0.1 # opsec-allow: address in a parser test, no host has it"
passes "assistant reference in AGENTS.md" "https://claude"".ai/code/session_0123" "AGENTS.md"
passes "assistant reference under .claude/" "Claude""-Session: abc" ".claude/notes.md"
passes "capture file in examples/" "x" "examples/sample.pcap"

if [ "$failures" -ne 0 ]; then
  echo "check-public-tree self-test: $failures of $cases cases FAILED" >&2
  exit 1
fi
echo "check-public-tree self-test: $cases cases ok"
