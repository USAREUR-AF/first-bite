#!/usr/bin/env bash
# Prove the optional goat support against a local stand-in for a goatnet
# router. No goatnet, certificate authority or goat service is contacted.
#
#   tools/goat-selftest.sh
#
# What it builds, in a temporary directory that is removed at the end:
#   * a throwaway certificate authority and leaf certificates, with openssl;
#   * a stock eclipse/zenoh:1.9.0 router in a container, configured the way
#     goat renders a slot-pool router (goatcore
#     ops/ansible/roles/goat-zenoh/templates/config.json5.j2): a TLS listener
#     that requires a client certificate, and access control that is
#     default-deny with one write rule per certificate common name.
#
# What it shows:
#   1. `mind` with the `goat` feature joins with the configuration it
#      generates, and its records arrive on the expected keys at a reader
#      that holds another certificate of the same authority;
#   2. the announcement and the schema declarations can be read back;
#   3. a publisher whose certificate has no write rule connects, and its
#      samples are dropped without an error; `mind goat probe` reports it;
#   4. `mind goat check` refuses each wrong input, naming the fix.
#
# Skipped (exit 0, with a message) when docker or openssl is missing.
#
# Environment:
#   CONTAINER_ENGINE   docker (default) or podman
#   GOAT_SELFTEST_MIND path of a `mind` built with --features goat; built
#                      with cargo when unset
set -euo pipefail

engine="${CONTAINER_ENGINE:-docker}"
image="eclipse/zenoh:1.9.0"
root="$(cd "$(dirname "$0")/.." && pwd)"

for tool in "$engine" openssl python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "goat-selftest: skipped: $tool is not installed"
    exit 0
  fi
done
if ! "$engine" info >/dev/null 2>&1; then
  echo "goat-selftest: skipped: the $engine daemon is not reachable"
  exit 0
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/goat-selftest.XXXXXX")"
router="fb-goat-selftest-$$"
had_image=yes
"$engine" image inspect "$image" >/dev/null 2>&1 || had_image=no
pids=()

cleanup() {
  for pid in "${pids[@]:-}"; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  done
  "$engine" rm -f "$router" >/dev/null 2>&1 || true
  # Remove the image only when this run pulled it.
  [ "$had_image" = yes ] || "$engine" rmi "$image" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

fail() {
  echo "goat-selftest: FAILED: $*" >&2
  exit 1
}

# expect <label> <fixed text> <output>: the output must contain the text.
expect() {
  if printf '%s\n' "$3" | grep -qF -- "$2"; then
    printf '  ok  %s\n' "$1"
  else
    printf '%s\n' "$3" >&2
    fail "$1: expected to see: $2"
  fi
}

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
mind="${GOAT_SELFTEST_MIND:-}"
if [ -z "$mind" ]; then
  echo "building mind with the goat feature"
  (cd "$root" && cargo build --locked -p mind --features goat >"$work/build.log" 2>&1) ||
    { tail -n 30 "$work/build.log" >&2; fail "cargo build"; }
  mind="$root/target/debug/mind"
fi
[ -x "$mind" ] || fail "$mind is not an executable"

# ---------------------------------------------------------------------------
# Certificates: one authority, a router, three slots; one foreign authority.
# The slot UUIDs are sha256("selftest:<k>") cut to 32 hex digits, which is how
# goat derives a slot pool from a salt.
# ---------------------------------------------------------------------------
tls="$work/tls"
mkdir -p "$tls"
chmod 755 "$work" "$tls"
slot() { printf 'selftest:%s' "$1" | openssl dgst -sha256 -r | cut -c1-32; }
publisher="$(slot 0)"   # has a write rule
reader="$(slot 1)"      # has a write rule of its own; reads like every peer
stranger="$(slot 9)"    # same authority, no rule: outside the pool

authority() { # <name>
  openssl ecparam -name prime256v1 -genkey -noout -out "$tls/$1.key" 2>/dev/null
  openssl req -x509 -new -key "$tls/$1.key" -sha256 -days 2 -out "$tls/$1-root.pem" \
    -subj "/CN=goat selftest $1" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
}
leaf() { # <file stem> <common name> <authority> <extension file> [extra x509 arguments]
  local stem="$1" cn="$2" ca="$3" ext="$4"
  shift 4
  openssl ecparam -name prime256v1 -genkey -noout -out "$tls/$stem-key.pem" 2>/dev/null
  openssl req -new -key "$tls/$stem-key.pem" -subj "/CN=$cn" -out "$tls/$stem.csr" 2>/dev/null
  openssl x509 -req -in "$tls/$stem.csr" -CA "$tls/$ca-root.pem" -CAkey "$tls/$ca.key" \
    -CAcreateserial -sha256 -days 2 -extfile "$ext" -out "$tls/$stem-cert.pem" "$@" 2>/dev/null
  chmod 600 "$tls/$stem-key.pem"
}
printf 'basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n' >"$tls/client.ext"
printf 'basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n' >"$tls/server.ext"

authority ca
authority foreign
leaf router goat-zenoh-router ca "$tls/server.ext"
leaf "$publisher" "$publisher" ca "$tls/client.ext"
leaf "$reader" "$reader" ca "$tls/client.ext"
leaf "$stranger" "$stranger" ca "$tls/client.ext"
leaf foreign "$publisher" foreign "$tls/client.ext"
# The router reads its key as the image's user.
chmod 644 "$tls/router-key.pem"

# ---------------------------------------------------------------------------
# Router: the shape of goat's config.json5.j2 for `per-uuid-slot-pool`, with a
# pool of two slots and the built-in memory volume in place of rocksdb.
# ---------------------------------------------------------------------------
slot_rules=""
slot_subjects=""
slot_policies=""
slot_storages=""
for anchor in "$publisher" "$reader"; do
  slot_rules+="
      { id: \"slot-w-$anchor\", messages: [\"put\", \"delete\", \"liveliness_token\"], flows: [\"ingress\"],
        permission: \"allow\", key_exprs: [\"$anchor/**\", \"volatile/$anchor/**\", \"$anchor/**/@adv/**\"] },"
  slot_subjects+="
      { id: \"slot-$anchor\", cert_common_names: [\"$anchor\"] },"
  slot_policies+="
      { rules: [\"slot-w-$anchor\", \"trial-peer-w-attest-schema\"], subjects: [\"slot-$anchor\"] },"
  slot_storages+="
        \"slot_$anchor\": { key_expr: \"$anchor/**\", volume: \"memory\" },"
done
cat >"$tls/router.json5" <<EOF
{
  mode: "router",
  listen: { endpoints: ["tls/0.0.0.0:7447"] },
  scouting: { multicast: { enabled: false } },
  transport: {
    link: {
      tls: {
        listen_certificate: "/tls/router-cert.pem",
        listen_private_key: "/tls/router-key.pem",
        root_ca_certificate: "/tls/ca-root.pem",
        enable_mtls: true,
      },
    },
  },
  plugins_loading: { enabled: true, search_dirs: ["/"] },
  plugins: {
    storage_manager: {
      volumes: { memory: {} },
      storages: {
        attest_schema: { key_expr: "attest/schema/**", volume: "memory" },$slot_storages
      },
    },
  },
  access_control: {
    enabled: true,
    default_permission: "deny",
    rules: [
      { id: "trial-peer-r-all",
        messages: ["declare_subscriber", "query", "declare_queryable", "reply", "declare_liveliness_subscriber", "liveliness_query"],
        flows: ["egress", "ingress"], permission: "allow", key_exprs: ["**", "**/@adv/**"] },
      { id: "trial-peer-deliver-all", messages: ["put", "delete", "liveliness_token"], flows: ["egress"],
        permission: "allow", key_exprs: ["**", "**/@adv/**"] },
      { id: "trial-peer-w-attest-schema", messages: ["put", "delete"], flows: ["ingress"],
        permission: "allow", key_exprs: ["attest/schema/**"] },$slot_rules
    ],
    subjects: [
      { id: "trial-peer-read", link_protocols: ["tls"] },$slot_subjects
    ],
    policies: [
      { rules: ["trial-peer-r-all", "trial-peer-deliver-all"], subjects: ["trial-peer-read"] },$slot_policies
    ],
  },
}
EOF

port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
echo "starting $image as $router on 127.0.0.1:$port"
"$engine" run -d --name "$router" -p "127.0.0.1:$port:7447" -v "$tls:/tls:ro" \
  "$image" -c /tls/router.json5 >/dev/null
for _ in $(seq 1 60); do
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
    break
  fi
  sleep 0.5
done
sleep 2
"$engine" ps --filter "name=$router" --format '{{.Status}}' | grep -q '^Up' ||
  { "$engine" logs "$router" 2>&1 | tail -n 30 >&2; fail "the router did not stay up"; }

# ---------------------------------------------------------------------------
# first-bite configurations
# ---------------------------------------------------------------------------
goat_config() { # <file> <slot> <certificate stem> <root>
  cat >"$1" <<EOF
goatnet:
  name: selftest
  endpoints: ["tls/localhost:$port"]
identity:
  certificate: tls/$3-cert.pem
  private_key: tls/$3-key.pem
  ca_certificate: tls/$4-root.pem
  slot: "$2"
publish:
  format: weather-station
  site: hilltop-7
  instance: selftest-1
  health_interval_seconds: 2
EOF
}
goat_config "$work/publisher.yml" "$publisher" "$publisher" ca
goat_config "$work/reader.yml" "$reader" "$reader" ca
goat_config "$work/stranger.yml" "$stranger" "$stranger" ca
dictionary="$root/dictionaries/weather-station"
prefix="$publisher/telemetry/weather-station/hilltop-7"

echo
echo "4. offline checks (nothing is sent)"
output="$("$mind" goat check --config "$work/publisher.yml" --dictionary "$dictionary" 2>&1)" ||
  { printf '%s\n' "$output" >&2; fail "check refused a correct configuration"; }
expect "a correct configuration passes" "all checks passed; nothing was sent" "$output"
expect "check lists the record keys" "record  $prefix/wx/observation" "$output"

# refuse <label> <expected text> <configuration>: check must fail with the text.
refuse() {
  local output
  if output="$("$mind" goat check --config "$3" 2>&1)"; then
    printf '%s\n' "$output" >&2
    fail "$1: check passed"
  fi
  expect "$1" "$2" "$output"
}
goat_config "$work/wrong-name.yml" "$reader" "$publisher" ca
refuse "certificate for another slot" "but the configuration names \"$reader\" (identity.slot)" "$work/wrong-name.yml"
goat_config "$work/wrong-root.yml" "$publisher" "$publisher" foreign
refuse "root that did not issue the certificate" "it was issued by a different authority" "$work/wrong-root.yml"
dashed="$(printf '%s' "$publisher" | tr 'a-f' 'A-F' | sed 's/^\(.\{8\}\)/\1-/')"
sed "s/slot: \"$publisher\"/slot: \"$dashed\"/" \
  "$work/publisher.yml" >"$work/wrong-uuid.yml"
refuse "malformed UUID" "is not a slot UUID (32 lower-case hexadecimal digits): write it as $publisher" "$work/wrong-uuid.yml"
sed 's/format: weather-station/format: Weather Station/' "$work/publisher.yml" >"$work/wrong-chunk.yml"
refuse "format that cannot be a key chunk" "write it as weather_station" "$work/wrong-chunk.yml"
sed "s|tls/localhost:$port|tcp/localhost:$port|" "$work/publisher.yml" >"$work/wrong-endpoint.yml"
refuse "endpoint that is not TLS" "write it as tls/<host>:<port>" "$work/wrong-endpoint.yml"
sed 's/slot: /mesh_peer_name: thor-a\n  slot: /' "$work/publisher.yml" >"$work/wrong-peer.yml"
refuse "mesh peer name outside the grammar" "it does not start with peer-" "$work/wrong-peer.yml"
chmod 644 "$tls/$publisher-key.pem"
refuse "private key other users can read" "run: chmod 600" "$work/publisher.yml"
chmod 600 "$tls/$publisher-key.pem"
# Certificates outside their validity period, signed by the same authority.
past="$(date -u -d '-10 days' +%Y%m%d%H%M%SZ 2>/dev/null || date -u -v-10d +%Y%m%d%H%M%SZ)"
yesterday="$(date -u -d '-1 day' +%Y%m%d%H%M%SZ 2>/dev/null || date -u -v-1d +%Y%m%d%H%M%SZ)"
tomorrow="$(date -u -d '+1 day' +%Y%m%d%H%M%SZ 2>/dev/null || date -u -v+1d +%Y%m%d%H%M%SZ)"
later="$(date -u -d '+2 days' +%Y%m%d%H%M%SZ 2>/dev/null || date -u -v+2d +%Y%m%d%H%M%SZ)"
if openssl x509 -help 2>&1 | grep -q -- '-not_before'; then
  leaf expired "$publisher" ca "$tls/client.ext" -not_before "$past" -not_after "$yesterday"
  leaf early "$publisher" ca "$tls/client.ext" -not_before "$tomorrow" -not_after "$later"
  goat_config "$work/expired.yml" "$publisher" expired ca
  refuse "expired certificate" "expired at" "$work/expired.yml"
  goat_config "$work/early.yml" "$publisher" early ca
  refuse "certificate not yet valid" "is not valid until" "$work/early.yml"
else
  echo "  --  expiry checks need openssl 3.4 or later for -not_before; covered by the unit tests"
fi

echo
echo "1. records arrive on the expected keys at a reader with another certificate"
free_port() { # <socket type>
  python3 -c "import socket; s=socket.socket(socket.AF_INET, socket.$1); s.bind(('127.0.0.1',0)); print(s.getsockname()[1])"
}
feed_port="$(free_port SOCK_STREAM)"
cat >"$work/publishers.yml" <<EOF
publishers:
  - name: "records"
    publisher_type: PUB
    address: "tcp://127.0.0.1:$feed_port"
    topic: "subsystem/message_type"
EOF
cat >"$work/subscribers.yml" <<EOF
subscribers:
  - name: fabric
    subscriber_type: "goat"
    zmq_url: "127.0.0.1"
    zmq_port: $feed_port
    topics: ["*"]
EOF
"$mind" goat watch "$publisher/**" --config "$work/reader.yml" --seconds 25 >"$work/watch.log" 2>&1 &
pids+=("$!")
watch_pid=$!
sleep 2
mock_port="$(free_port SOCK_DGRAM)"
(cd "$work" && exec env GOAT_CONFIG_PATH="$work/publisher.yml" \
  SUBSCRIBER_CONFIG_PATH="$work/subscribers.yml" PUBLISHER_CONFIG_PATH="$work/publishers.yml" \
  "$mind" --mock --mock-address "127.0.0.1:$mock_port" --dictionary "$dictionary" \
  >"$work/mind.log" 2>&1) &
pids+=("$!")
mind_pid=$!
sleep 6
for _ in 1 2 3; do
  python3 "$root/tools/mockplay.py" "$root/examples/weather-station.pcap" \
    --address "127.0.0.1:$mock_port" >/dev/null
  sleep 2
done
wait "$watch_pid" || true
watched="$(cat "$work/watch.log")"
log="$(cat "$work/mind.log")"
expect "the sink joined with the generated configuration" "goat: joined tls/localhost:$port as $publisher" "$log"
expect "the sink read its own sample back" "goat: the router delivers this publisher's samples" "$log"
expect "an Observation record arrived" "$prefix/wx/observation  application/json" "$watched"
expect "a Heartbeat record arrived" "$prefix/wx/heartbeat  application/json" "$watched"
expect "the heartbeat arrived" "$publisher/health/shims/weather-station/selftest-1  application/json" "$watched"
if printf '%s\n' "$watched" | grep -qF "$prefix/wx/42"; then
  fail "a message type the dictionary does not define was given a key"
fi
echo "  ok  a message type the dictionary does not define was not published"
printf '%s\n' "$watched" | grep -F "$publisher/" | grep -v 'sample(s) on' |
  sed 's/  [0-9]* bytes$//' | sort | uniq -c | sed 's/^/      /'

echo
echo "2. the announcement and the schema declarations can be read back"
announce="$("$mind" goat watch "$publisher/services/**" --stored --seconds 5 --payload --config "$work/reader.yml" 2>&1)" ||
  { printf '%s\n' "$announce" >&2; fail "no announcement in the router's storage"; }
expect "the announcement is where the contract puts it" "$publisher/services/shims/weather-station/selftest-1/announce  application/json" "$announce"
expect "it names what it translates to" "\"translates_to\":[\"$prefix/**\"]" "$announce"
schemas="$("$mind" goat watch "attest/schema/$publisher/**" --stored --seconds 5 --payload --config "$work/reader.yml" 2>&1)" ||
  { printf '%s\n' "$schemas" >&2; fail "no schema declaration in the router's storage"; }
expect "a schema is declared for Observation" "attest/schema/$prefix/wx/observation  application/json" "$schemas"
expect "the declaration is identified by its key" "\"\$id\":\"attest/schema/$prefix/wx/observation\"" "$schemas"
printf '%s\n' "$schemas" | grep -F "attest/schema/" | grep -v '^{' | sed 's/^/      /'
kill "$mind_pid" 2>/dev/null || true
wait "$mind_pid" 2>/dev/null || true

echo
echo "3. a certificate with no write rule: the session opens, the samples are dropped"
output="$("$mind" goat check --config "$work/stranger.yml" 2>&1)" ||
  { printf '%s\n' "$output" >&2; fail "check refused the stranger's configuration"; }
expect "offline, the stranger's certificate is in order" "all checks passed" "$output"
expect "check names the grant to ask for" "put  $stranger/**" "$output"
if output="$("$mind" goat probe --config "$work/stranger.yml" 2>&1)"; then
  printf '%s\n' "$output" >&2
  fail "the probe reported delivery for a certificate with no write rule"
fi
expect "probe reports that nothing was delivered" "not delivered within 5 s: the router accepted the session but did not deliver" "$output"
expect "probe names the common name and the grant" "common name $stranger is granted put on $stranger/**" "$output"
printf '%s\n' "$output" | sed 's/^/      /'
output="$("$mind" goat probe --config "$work/publisher.yml" 2>&1)" ||
  { printf '%s\n' "$output" >&2; fail "the probe failed for the granted certificate"; }
expect "probe confirms delivery for the granted certificate" "delivered: a sample put on $publisher/health/shims/weather-station/selftest-1" "$output"
(cd "$work" && exec env GOAT_CONFIG_PATH="$work/stranger.yml" \
  SUBSCRIBER_CONFIG_PATH="$work/subscribers.yml" PUBLISHER_CONFIG_PATH="$work/publishers.yml" \
  "$mind" --mock --mock-address "127.0.0.1:$mock_port" --dictionary "$dictionary" \
  >"$work/stranger.log" 2>&1) &
pids+=("$!")
stranger_pid=$!
sleep 12
kill "$stranger_pid" 2>/dev/null || true
wait "$stranger_pid" 2>/dev/null || true
log="$(cat "$work/stranger.log")"
expect "the running service logs the same finding at start" "goat: the router accepted the session but did not deliver" "$log"
grep -F "goat: the router accepted" "$work/stranger.log" | sed 's/^.*goat: /      goat: /' | head -n 1
if output="$("$mind" goat probe --config "$work/wrong-root.yml" 2>&1)"; then
  printf '%s\n' "$output" >&2
  fail "the probe connected with a configuration that fails its checks"
fi
expect "probe runs the offline checks before it connects" "is not verified by the root" "$output"

echo
echo "goat-selftest: passed"
