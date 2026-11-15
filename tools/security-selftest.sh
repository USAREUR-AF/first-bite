#!/usr/bin/env bash
# Prove the transport security levels on this machine: real handshakes with
# throwaway certificates against local servers, and every refusal a capture
# run can stop with. No real service is contacted.
#
#   tools/security-selftest.sh [--fips] [--keep]
#
#   --fips   also build with the cargo feature `fips` and prove `fips: required`
#            (needs CMake and Go; x86-64 or 64-bit Arm Linux with glibc)
#   --keep   leave the temporary directory and the container image in place
#
# It builds `mind` three or four times (debug, in target/), starts an HTTPS
# server from Python's standard library, `openssl s_server`, and a stock
# eclipse/zenoh router in a container, and removes all of them afterwards,
# with the image if this run pulled it.
#
# Needs: cargo, python3, curl and openssl. Two things are skipped with a
# message when their peer is missing: the hybrid handshake without OpenSSL
# 3.5 or later (earlier releases have no X25519MLKEM768 to answer with), and
# the Zenoh sections without docker.
#
# Environment:
#   CONTAINER_ENGINE   docker (default) or podman
#   CARGO_BUILD_JOBS   passed through to cargo
#
# The expected messages quote configuration keys in backticks, inside single
# quotes so that nothing expands.
# shellcheck disable=SC2016
set -euo pipefail

with_fips=no
keep=no
for argument in "$@"; do
  case "$argument" in
    --fips) with_fips=yes ;;
    --keep) keep=yes ;;
    *)
      echo "usage: tools/security-selftest.sh [--fips] [--keep]" >&2
      exit 2
      ;;
  esac
done

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
engine="${CONTAINER_ENGINE:-docker}"
zenoh_image="eclipse/zenoh:1.9.0"
work="$(mktemp -d "${TMPDIR:-/tmp}/first-bite-selftest.XXXXXX")"
container="first-bite-selftest-$$"
pids=()
pulled_image=no
failures=0
passed=0
skipped=0

cleanup() {
  local pid
  for pid in "${pids[@]:-}"; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  done
  if command -v "$engine" >/dev/null 2>&1; then
    "$engine" rm -f "$container" >/dev/null 2>&1 || true
    if [ "$pulled_image" = yes ] && [ "$keep" = no ]; then
      "$engine" rmi "$zenoh_image" >/dev/null 2>&1 || true
    fi
  fi
  if [ "$keep" = yes ]; then
    echo "kept $work"
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT

section() { printf '\n== %s\n' "$*"; }
ok() {
  passed=$((passed + 1))
  printf 'ok    %s\n' "$*"
}
fail() {
  failures=$((failures + 1))
  printf 'FAIL  %s\n' "$*"
}
skip() {
  skipped=$((skipped + 1))
  printf 'skip  %s\n' "$*"
}

for tool in cargo python3 curl openssl; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "security-selftest: $tool is not installed" >&2
    exit 2
  }
done
# Without X25519MLKEM768 in this machine's OpenSSL there is no local peer for
# the hybrid handshake.
hybrid_peer=yes
if ! openssl list -kem-algorithms 2>/dev/null | grep -q 'X25519MLKEM768'; then
  hybrid_peer=no
fi

free_port() {
  python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'
}

# ---------------------------------------------------------------- binaries
# build <name> <cargo arguments>: one build of mind, kept as $work/bin/<name>.
mkdir -p "$work/bin"
build() {
  local name="$1"
  shift
  echo "building mind ($name): cargo build -p mind $*"
  cargo build --locked -p mind "$@" 2>"$work/build-$name.log" || {
    tail -n 40 "$work/build-$name.log" >&2
    echo "security-selftest: the $name build failed" >&2
    exit 1
  }
  cp target/debug/mind "$work/bin/$name"
}
section "builds"
build plain --no-default-features --features azure,aws,zenoh
build full --features zenoh,quic
if [ "$with_fips" = yes ]; then
  build fips --features zenoh,fips
fi
build default
PLAIN="$work/bin/plain"     # azure, aws, zenoh: no pqc, no quic, no fips
DEFAULT="$work/bin/default" # azure, aws, pqc
FULL="$work/bin/full"       # azure, aws, pqc, zenoh, quic
FIPS="$work/bin/fips"       # azure, aws, pqc, zenoh, fips

# ------------------------------------------------------------ certificates
section "throwaway certificates"
tls="$work/tls"
mkdir -p "$tls"
(
  cd "$tls"
  ec() { openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes "$@" 2>/dev/null; }
  ec -x509 -keyout ca.key -out ca.pem -days 2 -subj "/CN=first-bite selftest CA" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign"
  # A second authority that signed nothing the servers trust.
  ec -x509 -keyout other-ca.key -out other-ca.pem -days 2 -subj "/CN=another CA" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign"
  ec -keyout server.key -out server.csr -subj "/CN=localhost"
  printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n' >server.ext
  openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out server.pem \
    -days 2 -extfile server.ext 2>/dev/null
  ec -keyout client.key -out client.csr -subj "/CN=first-bite selftest client"
  printf 'extendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\n' >client.ext
  openssl x509 -req -in client.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out client.pem \
    -days 2 -extfile client.ext 2>/dev/null
  chmod 0644 ./*.key
)
ok "a certificate authority, a server certificate for localhost and a client certificate in $tls"

# ------------------------------------------------------- configurations
# Each case is a directory holding publishers.yml and subscribers.yml, the
# latter with the security block as its top-level `security:` key.
zmq_port="$(free_port)"
new_case() {
  case_dir="$work/case-$1"
  mkdir -p "$case_dir"
  cat >"$case_dir/publishers.yml" <<EOF
publishers:
  - name: records
    publisher_type: PUB
    address: "${2:-tcp://127.0.0.1:$zmq_port}"
    topic: "subsystem/message_type"
EOF
}
# subscribers <security block lines> <subscriber entries>
subscribers() {
  {
    if [ -n "$1" ]; then
      printf 'security:\n%s\n' "$1"
    fi
    printf 'subscribers:\n%s\n' "$2"
  } >"$case_dir/subscribers.yml"
}
feed="    zmq_url: \"127.0.0.1\"
    zmq_port: $zmq_port
    topics: [\"*\"]"

# mind <binary> <arguments>: run with the current case's configuration.
mind() {
  local binary="$1"
  shift
  env -u SECURITY_CONFIG_PATH -u SUBPUB_CONFIG_PATH \
    PUBLISHER_CONFIG_PATH="$case_dir/publishers.yml" \
    SUBSCRIBER_CONFIG_PATH="$case_dir/subscribers.yml" \
    RUST_LOG="${RUST_LOG:-info}" NO_COLOR=1 \
    "$binary" "$@"
}

# refused <label> <binary> <text>: `mind security check` must fail with a
# message that contains <text>, and a capture run must stop with the same
# message before it reads a frame.
refused() {
  local label="$1" binary="$2" text="$3" output line
  if output="$(mind "$binary" security check 2>&1)"; then
    fail "$label: security check passed; expected: $text"
    return
  fi
  line="$(printf '%s\n' "$output" | grep -F -- "$text" | head -n 1 || true)"
  if [ -z "$line" ]; then
    fail "$label: security check did not say: $text"
    printf '%s\n' "$output" | sed 's/^/        /'
    return
  fi
  if output="$(mind "$binary" --pcap-file examples/weather-station.pcap \
    --dictionary dictionaries/weather-station 2>&1)"; then
    fail "$label: the capture run started"
    return
  fi
  if ! printf '%s\n' "$output" | grep -qF -- "$text"; then
    fail "$label: the capture run stopped without saying: $text"
    printf '%s\n' "$output" | tail -n 5 | sed 's/^/        /'
    return
  fi
  if printf '%s\n' "$output" | grep -q 'reading from'; then
    fail "$label: the capture run read frames before it stopped"
    return
  fi
  ok "$label"
  printf '        %s\n' "$line"
}

# accepted <label> <binary>: `mind security check` must pass.
accepted() {
  local output
  if output="$(mind "$2" security check 2>&1)"; then
    ok "$1"
    printf '        %s\n' "$output"
  else
    fail "$1"
    printf '%s\n' "$output" | sed 's/^/        /'
  fi
}

rest_to() { # <name> <url host part> <port>
  printf '  - name: %s\n    subscriber_type: rest\n%s\n    endpoint_url: "%s"\n    endpoint_port: %s\n    endpoint_path: /records' \
    "$1" "$feed" "$2" "$3"
}

# ----------------------------------------------------------- fail closed
section "fail closed: a level the binary was not built for"
new_case nopqc
subscribers "  transport: tls
  key_exchange: hybrid-pqc-required" "$(rest_to ToCollector https://localhost 8443)"
refused "hybrid-pqc-required on a build without pqc" "$PLAIN" \
  'security: `key_exchange: hybrid-pqc-required` needs a binary built with the cargo feature `pqc`; this one was built with: azure, aws, zenoh'
new_case noquic
subscribers "  transport: tls
  quic: required" "$(rest_to ToCollector https://localhost 8443)"
refused "quic: required on a build without quic" "$DEFAULT" \
  'security: `quic: required` needs a binary built with the cargo feature `quic`; this one was built with: azure, aws, pqc'
new_case nofips
subscribers "  transport: tls
  fips: required" "$(rest_to ToCollector https://localhost 8443)"
refused "fips: required on a build without fips" "$FULL" \
  'security: `fips: required` needs a binary built with the cargo feature `fips`; this one was built with: azure, aws, zenoh, quic, pqc'

section "fail closed: a sink that cannot meet the level"
new_case http-under-tls
subscribers "  transport: tls" "$(rest_to ToCollector 203.0.113.10 5001)"
refused "http:// endpoint under tls" "$DEFAULT" \
  'subscriber ToCollector: level `tls` needs an https:// endpoint; `endpoint_url` gives http://203.0.113.10:5001/records'
new_case offhost-under-loopback
subscribers "  transport: loopback-plaintext" "$(rest_to ToCollector https://collector.example 443)"
refused "off-host sink under loopback-plaintext" "$DEFAULT" \
  'subscriber ToCollector: level `loopback-plaintext` refuses a sink that leaves this host; `endpoint_url` gives https://collector.example/records'
new_case mtls-no-identity
subscribers "  transport: mtls" "$(rest_to ToCollector https://localhost 8443)"
refused "mtls with no client certificate configured" "$DEFAULT" \
  'subscriber ToCollector: level `mtls` needs `client_cert` and `client_key`'
new_case mtls-azure-public
subscribers "  transport: mtls
  client_cert: $tls/client.pem
  client_key: $tls/client.key" "  - name: ToBlob
    subscriber_type: objectstorage
    subtype: azureobject
$feed
    cloud_account: exampleaccount
    cloud_access_key: \"eW91ci1hY2Nlc3Mta2V5\"
    cloud_container: records
    cloud_location_type: Public"
refused "mtls with a sink that takes no client certificate" "$DEFAULT" \
  'subscriber ToBlob: level `mtls` needs a client certificate, and the public Azure Blob service authorises requests by Shared Key and takes no client certificate'
subscribers "  transport: mtls
  client_cert: $tls/client.pem
  client_key: $tls/client.key" "  - name: ToBlob
    subscriber_type: objectstorage
    subtype: azureobject
$feed
    cloud_account: exampleaccount
    cloud_access_key: \"eW91ci1hY2Nlc3Mta2V5\"
    cloud_container: records
    cloud_location_type: Public
    security:
      transport: tls
      reason: \"the storage service has no client-certificate authentication\""
accepted "the same sink with a per-sink override that states a reason" "$DEFAULT"
subscribers "  transport: mtls
  client_cert: $tls/client.pem
  client_key: $tls/client.key" "$(rest_to ToCollector https://localhost 8443)
    security:
      transport: tls"
refused "a per-sink override with no reason" "$DEFAULT" \
  'subscriber ToCollector: `security.transport` differs from the security block; say why in `security.reason`'

section "fail closed: certificate or key unreadable"
new_case unreadable-cert
subscribers "  transport: mtls
  client_cert: $work/absent/client.pem
  client_key: $tls/client.key" "$(rest_to ToCollector https://localhost 8443)"
refused "client certificate file missing" "$DEFAULT" \
  "subscriber ToCollector: \`security.client_cert\`: cannot read $work/absent/client.pem"
new_case unreadable-key
subscribers "  transport: mtls
  client_cert: $tls/client.pem
  client_key: $tls/server.ext" "$(rest_to ToCollector https://localhost 8443)"
refused "client key file holds no key" "$DEFAULT" \
  "subscriber ToCollector: \`security.client_key\`: cannot read a private key from $tls/server.ext"
new_case unreadable-ca
subscribers "  transport: tls
  ca_bundle: $work/absent/ca.pem" "$(rest_to ToCollector https://localhost 8443)"
refused "certificate authority bundle missing" "$DEFAULT" \
  "subscriber ToCollector: \`security.ca_bundle\`: cannot read $work/absent/ca.pem"

section "fail closed: emulator endpoints over plain HTTP"
emulators="  - name: ToBlobEmulator
    subscriber_type: objectstorage
    subtype: azureobject
$feed
    cloud_account: devstoreaccount1
    cloud_access_key: \"Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==\"
    cloud_container: first-bite
    cloud_location_type: Emulator
    cloud_address: \"127.0.0.1\"
    cloud_port: 10000"
new_case emulator-under-tls
subscribers "  transport: tls" "$emulators"
refused "an emulator over http:// under tls" "$DEFAULT" \
  'subscriber ToBlobEmulator: level `tls` needs an https:// endpoint; `cloud_address` gives http://127.0.0.1:10000/devstoreaccount1; for a local emulator give this subscriber `security: { transport: loopback-plaintext, reason: ... }`'
new_case emulator-under-loopback
subscribers "  transport: loopback-plaintext" "$emulators"
accepted "the same emulator under loopback-plaintext" "$DEFAULT"
new_case emulator-override
subscribers "  transport: tls" "$emulators
    security:
      transport: loopback-plaintext
      reason: \"local emulator on the development machine\""
accepted "the same emulator under tls with a per-sink override" "$DEFAULT"

section "fail closed: the ZeroMQ socket"
new_case bind-refused "tcp://0.0.0.0:$zmq_port"
subscribers "  transport: tls" ""
refused "a bind address that is not loopback" "$DEFAULT" \
  "publisher records: \`address\` tcp://0.0.0.0:$zmq_port is not loopback, and the ZeroMQ socket is unauthenticated and unencrypted"
new_case bind-allowed "tcp://0.0.0.0:$zmq_port"
subscribers "  transport: tls
  allow_plaintext_bind: true" ""
output="$(mind "$DEFAULT" --pcap-file examples/weather-station.pcap \
  --dictionary dictionaries/weather-station 2>&1)" || true
line="$(printf '%s\n' "$output" | grep -F "publisher records binds tcp://0.0.0.0:$zmq_port, which is not loopback" | head -n 1 || true)"
if [ -n "$line" ] && printf '%s\n' "$output" | grep -qE 'done: [1-9][0-9]* frames'; then
  ok "the same address with allow_plaintext_bind: true starts, with a warning that names it"
  printf '        %s\n' "$(printf '%s\n' "$line" | grep -o 'publisher records binds.*')"
else
  fail "allow_plaintext_bind: true"
  printf '%s\n' "$output" | tail -n 5 | sed 's/^/        /'
fi
new_case feed-offhost
subscribers "  transport: tls" "  - name: Rates
    subscriber_type: stateful
    zmq_url: \"192.0.2.10\"
    zmq_port: $zmq_port
    topics: [\"*\"]"
refused "a subscriber that reads the feed from another host" "$DEFAULT" \
  'subscriber Rates: `zmq_url` 192.0.2.10 is not loopback: the ZeroMQ feed is unauthenticated and unencrypted'

section "fail closed: the Zenoh configuration"
default_key='selftest/{message}'
zenoh_sink() { # <name> <zenoh config file> [key template]
  printf '  - name: %s\n    subscriber_type: zenoh\n%s\n    zenoh_config: "%s"\n    key_template: "%s"' \
    "$1" "$feed" "$2" "${3:-$default_key}"
}
new_case zenoh-tcp
cat >"$case_dir/zenoh.json5" <<'EOF'
{ mode: "client", connect: { endpoints: ["tcp/192.0.2.20:7447"] } }
EOF
subscribers "  transport: tls" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5")"
refused "a tcp/ endpoint under tls" "$FULL" \
  'subscriber ToZenoh: `zenoh_config`: level `tls` refuses the plaintext endpoint tcp/192.0.2.20:7447; use tls/ or quic/'
subscribers "  transport: mtls" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5")"
refused "a tcp/ endpoint under mtls" "$FULL" \
  'subscriber ToZenoh: `zenoh_config`: level `mtls` refuses the plaintext endpoint tcp/192.0.2.20:7447; use tls/ or quic/'
new_case zenoh-default
printf '{}\n' >"$case_dir/zenoh.json5"
subscribers "  transport: tls" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5")"
refused "Zenoh's defaults (a plaintext listener on every interface) under tls" "$FULL" \
  'subscriber ToZenoh: `zenoh_config`: level `tls` refuses the plaintext listener tcp/[::]:0 that mode peer opens by default'
new_case zenoh-no-client-auth
cat >"$case_dir/zenoh.json5" <<EOF
{ mode: "client", connect: { endpoints: ["tls/localhost:7447"] },
  transport: { link: { tls: { root_ca_certificate: "$tls/ca.pem" } } } }
EOF
subscribers "  transport: mtls" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5")"
refused "a tls/ endpoint with no client certificate under mtls" "$FULL" \
  'subscriber ToZenoh: `zenoh_config`: level `mtls` needs `transport.link.tls.connect_certificate`, `connect_private_key` and `enable_mtls: true`'
new_case zenoh-quic-not-built
cat >"$case_dir/zenoh.json5" <<EOF
{ mode: "client", connect: { endpoints: ["quic/localhost:7447"] },
  transport: { link: { tls: { root_ca_certificate: "$tls/ca.pem" } } } }
EOF
subscribers "  transport: tls" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5")"
refused "a quic/ endpoint on a build without quic" "$PLAIN" \
  'subscriber ToZenoh: `zenoh_config`: endpoint quic/localhost:7447 needs a binary built with the cargo feature `quic`; this one was built with: azure, aws, zenoh'
new_case zenoh-per-sink-kx
cat >"$case_dir/zenoh.json5" <<EOF
{ mode: "client", connect: { endpoints: ["tls/localhost:7447"] },
  transport: { link: { tls: { root_ca_certificate: "$tls/ca.pem" } } } }
EOF
subscribers "  transport: tls" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5")
    security:
      key_exchange: hybrid-pqc-required
      reason: \"this router only\""
refused "hybrid-pqc-required for one Zenoh sink, whose client cannot be set apart" "$FULL" \
  'subscriber ToZenoh: `security.min_tls` and `security.key_exchange` cannot differ for a Zenoh sink'
new_case zenoh-unreadable
cat >"$case_dir/zenoh.json5" <<EOF
{ mode: "client", connect: { endpoints: ["tls/localhost:7447"] },
  transport: { link: { tls: { root_ca_certificate: "$tls/ca.pem",
    connect_certificate: "$work/absent/client.pem", connect_private_key: "$tls/client.key",
    enable_mtls: true } } } }
EOF
subscribers "  transport: mtls" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5")"
refused "a Zenoh client certificate that cannot be read" "$FULL" \
  "subscriber ToZenoh: \`zenoh_config\`: connect_certificate: cannot read $work/absent/client.pem"

section "fail closed: the block itself"
new_case two-blocks
subscribers "  transport: tls" "$(rest_to ToCollector https://localhost 8443)"
printf 'security:\n  transport: mtls\n' >"$case_dir/security.yml"
if output="$(SECURITY_CONFIG_PATH="$case_dir/security.yml" \
  PUBLISHER_CONFIG_PATH="$case_dir/publishers.yml" \
  SUBSCRIBER_CONFIG_PATH="$case_dir/subscribers.yml" "$DEFAULT" security check 2>&1)"; then
  fail "the block written in two files"
else
  line="$(printf '%s\n' "$output" | grep -F 'security: the block is written in both' || true)"
  if [ -n "$line" ]; then
    ok "the block written in two files"
    printf '        %s\n' "$line"
  else
    fail "the block written in two files: $output"
  fi
fi
new_case unknown-key
subscribers "  transport: tls
  cipher: aes" ""
refused "an unknown key in the block" "$DEFAULT" 'unknown field `cipher`'

# --------------------------------------------------------- local servers
# An HTTPS server that answers every POST with 200 and counts them. With a
# fourth argument it requires a client certificate signed by that authority.
cat >"$work/https_server.py" <<'EOF'
import http.server
import ssl
import sys

port, cert, key, count_file = int(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
client_ca = sys.argv[5] if len(sys.argv) > 5 else None


class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        with open(count_file, "a") as out:
            out.write(self.path + "\n")
        self.send_response(200)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, *args):
        pass


context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(cert, key)
if client_ca:
    context.verify_mode = ssl.CERT_REQUIRED
    context.load_verify_locations(client_ca)
server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
server.socket = context.wrap_socket(server.socket, server_side=True)
server.serve_forever()
EOF

wait_for_port() {
  for _ in $(seq 1 50); do
    if python3 -c "import socket, sys; s = socket.socket(); s.settimeout(0.2); sys.exit(s.connect_ex(('127.0.0.1', $1)))" 2>/dev/null; then
      return 0
    fi
    sleep 0.2
  done
  echo "security-selftest: nothing listens on port $1" >&2
  return 1
}

start_https() { # <count file> [client CA]: sets $https_port
  https_port="$(free_port)"
  python3 "$work/https_server.py" "$https_port" "$tls/server.pem" "$tls/server.key" "$@" &
  pids+=("$!")
  wait_for_port "$https_port"
}

# start_s_server <log> <groups>: an `openssl s_server` that offers only the
# named groups and prints what each client offered and used. Sets $s_port.
start_s_server() {
  s_port="$(free_port)"
  openssl s_server -accept "127.0.0.1:$s_port" -cert "$tls/server.pem" -key "$tls/server.key" \
    -tls1_3 -rev -groups "$2" >"$1" 2>&1 &
  pids+=("$!")
  wait_for_port "$s_port"
}

# capture <binary> <condition command...>: run a capture that reads frames
# from a UDP socket, send the example capture file to it until the condition
# holds or thirty seconds pass, then stop it. The log is $capture_log.
capture() {
  local binary="$1" mock_port pid
  shift
  mock_port="$(free_port)"
  capture_log="$case_dir/mind.log"
  mind "$binary" --mock --mock-address "127.0.0.1:$mock_port" \
    --dictionary dictionaries/weather-station >"$capture_log" 2>&1 &
  pid=$!
  pids+=("$pid")
  for _ in $(seq 1 50); do
    grep -q 'reading from' "$capture_log" 2>/dev/null && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.2
  done
  for _ in $(seq 1 15); do
    kill -0 "$pid" 2>/dev/null || break
    python3 tools/mockplay.py examples/weather-station.pcap \
      --address "127.0.0.1:$mock_port" >/dev/null 2>&1 || true
    sleep 2
    if "$@"; then
      break
    fi
  done
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  "$@"
}
posted() { [ -s "$1" ]; }
logged() { grep -qE -- "$1" "$capture_log"; }

# probe <label> <binary> <subscriber> <expected text>
probe() {
  local output line
  output="$(mind "$2" security show --probe "$3" 2>&1)" || true
  line="$(printf '%s\n' "$output" | grep -E '^probe ' | head -n 1 || true)"
  if printf '%s\n' "$line" | grep -qF -- "$4"; then
    ok "$1"
    printf '        %s\n' "$line"
  else
    fail "$1: expected: $4"
    printf '%s\n' "$output" | tail -n 4 | sed 's/^/        /'
  fi
}

# --------------------------------------------------------------- REST
section "REST sink over TLS and mutual TLS"
new_case rest-tls
start_https "$case_dir/posts"
subscribers "  transport: tls
  ca_bundle: $tls/ca.pem" "$(rest_to ToCollector https://localhost "$https_port")"
if capture "$DEFAULT" posted "$case_dir/posts"; then
  ok "REST over TLS: $(wc -l <"$case_dir/posts" | tr -d ' ') records posted to https://localhost:$https_port/records"
else
  fail "REST over TLS: no record arrived"
  tail -n 5 "$capture_log" | sed 's/^/        /'
fi
probe "REST over TLS: handshake" "$DEFAULT" ToCollector 'negotiated TLSv1_3'
subscribers "  transport: tls
  ca_bundle: $tls/other-ca.pem" "$(rest_to ToCollector https://localhost "$https_port")"
probe "REST over TLS: a server signed by an authority outside the bundle is refused" \
  "$DEFAULT" ToCollector 'handshake failed'

new_case rest-mtls
start_https "$case_dir/posts" "$tls/ca.pem"
subscribers "  transport: mtls
  ca_bundle: $tls/ca.pem
  client_cert: $tls/client.pem
  client_key: $tls/client.key" "$(rest_to ToCollector https://localhost "$https_port")"
if capture "$DEFAULT" posted "$case_dir/posts"; then
  ok "REST over mutual TLS: $(wc -l <"$case_dir/posts" | tr -d ' ') records accepted with the client certificate"
else
  fail "REST over mutual TLS: no record arrived"
  tail -n 5 "$capture_log" | sed 's/^/        /'
fi
probe "REST over mutual TLS: handshake" "$DEFAULT" ToCollector 'negotiated TLSv1_3'
rm -f "$case_dir/posts"
subscribers "  transport: tls
  ca_bundle: $tls/ca.pem" "$(rest_to ToCollector https://localhost "$https_port")"
if capture "$DEFAULT" logged 'subscriber ToCollector: record on .* not delivered|retry 1 of 3' &&
  ! posted "$case_dir/posts"; then
  ok "REST without the client certificate: the server refused every record"
  printf '        %s\n' "$(grep -oE '(subscriber ToCollector: record on|https://localhost).*' "$capture_log" | head -n 1 | cut -c1-200)"
else
  fail "REST without the client certificate: expected a refusal and no record"
  tail -n 5 "$capture_log" | sed 's/^/        /'
fi
probe "REST without the client certificate: handshake" "$DEFAULT" ToCollector 'the peer refused the connection'

# ---------------------------------------------------- key exchange groups
section "hybrid post-quantum key exchange"
if [ "$hybrid_peer" = no ]; then
  skip "the hybrid handshake: $(openssl version | cut -d' ' -f1-2) has no X25519MLKEM768 (3.5 or later has)"
else
new_case kx-hybrid
start_s_server "$case_dir/s_server.log" X25519MLKEM768
subscribers "  transport: tls
  ca_bundle: $tls/ca.pem
  key_exchange: hybrid-pqc-preferred" "$(rest_to ToPeer https://localhost "$s_port")"
probe "a peer that accepts only X25519MLKEM768, hybrid-pqc-preferred" "$DEFAULT" ToPeer \
  'key exchange group X25519MLKEM768'
sleep 0.5
if grep -q '^Peer Temp Key: X25519MLKEM768' "$case_dir/s_server.log"; then
  ok "the peer's own report of that handshake (openssl s_server)"
  grep -E '^(Protocol version|Ciphersuite|Supported groups|Peer Temp Key)' "$case_dir/s_server.log" |
    head -n 4 | sed 's/^/        /'
else
  fail "openssl s_server did not report X25519MLKEM768"
  tail -n 6 "$case_dir/s_server.log" | sed 's/^/        /'
fi
subscribers "  transport: tls
  ca_bundle: $tls/ca.pem
  key_exchange: classical" "$(rest_to ToPeer https://localhost "$s_port")"
probe "the same peer, key_exchange: classical: no group in common" "$DEFAULT" ToPeer 'handshake failed'
fi

new_case kx-classical-peer
start_s_server "$case_dir/s_server.log" X25519:P-256
subscribers "  transport: tls
  ca_bundle: $tls/ca.pem
  key_exchange: hybrid-pqc-required" "$(rest_to ToPeer https://localhost "$s_port")"
probe "hybrid-pqc-required refuses a peer that does not offer the hybrid group" "$DEFAULT" ToPeer \
  'handshake failed'
if capture "$DEFAULT" logged 'subscriber ToPeer: record on .* not delivered|retry 1 of 3'; then
  ok "the sink itself delivers nothing to that peer"
  printf '        %s\n' "$(grep -oE '(subscriber ToPeer: record on|https://localhost).*' "$capture_log" | head -n 1 | cut -c1-220)"
else
  fail "hybrid-pqc-required: the sink did not report a refused handshake"
  tail -n 5 "$capture_log" | sed 's/^/        /'
fi
subscribers "  transport: tls
  ca_bundle: $tls/ca.pem
  key_exchange: hybrid-pqc-preferred" "$(rest_to ToPeer https://localhost "$s_port")"
probe "hybrid-pqc-preferred falls back to a classical group with the same peer" "$DEFAULT" ToPeer \
  'key exchange group X25519,'

# ------------------------------------------------------------------ FIPS
section "FIPS module"
if [ "$with_fips" = yes ]; then
  new_case fips
  start_https "$case_dir/posts"
  subscribers "  transport: tls
  fips: required
  ca_bundle: $tls/ca.pem" "$(rest_to ToCollector https://localhost "$https_port")"
  output="$(mind "$FIPS" security show 2>&1)"
  if printf '%s\n' "$output" | grep -q '^FIPS module active yes' &&
    printf '%s\n' "$output" | grep -q '^  FIPS mode        yes'; then
    ok "fips: required on a fips build: the provider reports the FIPS module active"
    printf '%s\n' "$output" | grep -E '^(crypto provider|FIPS module active|key exchange|built with)' | sed 's/^/        /'
  else
    fail "fips: required on a fips build"
    printf '%s\n' "$output" | sed 's/^/        /'
  fi
  if capture "$FIPS" posted "$case_dir/posts"; then
    ok "fips: required: $(wc -l <"$case_dir/posts" | tr -d ' ') records posted over TLS by the fips build"
  else
    fail "fips: required: no record arrived"
    tail -n 5 "$capture_log" | sed 's/^/        /'
  fi
  probe "fips: required: handshake" "$FIPS" ToCollector 'FIPS mode yes'
  subscribers "  transport: tls
  fips: required" "  - name: ToS3
    subscriber_type: objectstorage
    subtype: awsobject
$feed
    aws_access_key_id: exampleaccesskey
    aws_secret_access_key: examplesecretkey
    aws_default_region: us-east-1
    aws_s3_endpoint: \"https://s3.example\"
    aws_bucket_name: records"
  refused "fips: required with a sink whose signing runs outside the module" "$FIPS" \
    'subscriber ToS3: `fips: required`: request signing (SigV4, computed by the AWS SDK with the `hmac` and `sha2` crates) runs outside the FIPS module'
else
  skip "fips: required on a fips build (run with --fips)"
fi

# ----------------------------------------------------------------- Zenoh
section "Zenoh sink over mutual TLS and QUIC (stock $zenoh_image router)"
zenoh_client() { # <file> <endpoint> <with client certificate: yes|no>
  local identity=""
  if [ "$3" = yes ]; then
    identity="connect_certificate: \"$tls/client.pem\", connect_private_key: \"$tls/client.key\", enable_mtls: true,"
  fi
  cat >"$1" <<EOF
{
  mode: "client",
  connect: { endpoints: ["$2"] },
  scouting: { multicast: { enabled: false } },
  transport: { link: { tls: { root_ca_certificate: "$tls/ca.pem", $identity } } },
}
EOF
}
stored() { # <key expression>: the router's storage holds a sample under it
  curl -fsS "http://127.0.0.1:$rest_port/$1" 2>/dev/null | grep -q '"key"'
}
if ! command -v "$engine" >/dev/null 2>&1 || ! "$engine" info >/dev/null 2>&1; then
  skip "Zenoh sections: $engine is not available"
else
  if ! "$engine" image inspect "$zenoh_image" >/dev/null 2>&1; then
    "$engine" pull -q "$zenoh_image" >/dev/null
    pulled_image=yes
  fi
  router_port="$(free_port)"
  rest_port="$(free_port)"
  mkdir -p "$work/router"
  cp "$tls/ca.pem" "$tls/server.pem" "$tls/server.key" "$work/router/"
  # TLS and QUIC listeners that require a client certificate; the storage
  # and the REST plugin let this script read back what was put.
  cat >"$work/router/router.json5" <<'EOF'
{
  mode: "router",
  listen: { endpoints: ["tls/0.0.0.0:7447", "quic/0.0.0.0:7447"] },
  scouting: { multicast: { enabled: false } },
  transport: { link: { tls: {
    root_ca_certificate: "/selftest/ca.pem",
    listen_certificate: "/selftest/server.pem",
    listen_private_key: "/selftest/server.key",
    enable_mtls: true,
  } } },
  plugins_loading: { enabled: true },
  plugins: {
    rest: { http_port: 8000 },
    storage_manager: { storages: { selftest: { key_expr: "selftest/**", volume: "memory" } } },
  },
}
EOF
  chmod -R a+rX "$work/router"
  "$engine" run -d --name "$container" \
    -p "127.0.0.1:$router_port:7447/tcp" -p "127.0.0.1:$router_port:7447/udp" \
    -p "127.0.0.1:$rest_port:8000/tcp" \
    --volume "$work/router:/selftest:ro" \
    "$zenoh_image" -c /selftest/router.json5 >/dev/null
  up=no
  for _ in $(seq 1 60); do
    if curl -fsS "http://127.0.0.1:$rest_port/selftest/**" >/dev/null 2>&1; then
      up=yes
      break
    fi
    sleep 0.5
  done
  if [ "$up" = no ]; then
    fail "the Zenoh router did not start"
    "$engine" logs "$container" 2>&1 | tail -n 15 | sed 's/^/        /'
  else
    new_case zenoh-mtls
    zenoh_client "$case_dir/zenoh.json5" "tls/localhost:$router_port" yes
    subscribers "  transport: mtls
  client_cert: $tls/client.pem
  client_key: $tls/client.key" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5" 'selftest/tls/{message}')"
    if capture "$FULL" stored 'selftest/tls/**'; then
      ok "Zenoh over tls/ with mutual TLS: records stored by the router"
      printf '        %s\n' "$(curl -fsS "http://127.0.0.1:$rest_port/selftest/tls/**" | grep -oE '"key":"[^"]*"' | sort -u | tr '\n' ' ')"
    else
      fail "Zenoh over tls/ with mutual TLS: nothing reached the router"
      tail -n 5 "$capture_log" | sed 's/^/        /'
    fi
    probe "Zenoh over tls/ with mutual TLS: handshake" "$FULL" ToZenoh 'negotiated TLSv1_3'

    new_case zenoh-quic
    zenoh_client "$case_dir/zenoh.json5" "quic/localhost:$router_port" yes
    subscribers "  transport: mtls
  quic: required
  client_cert: $tls/client.pem
  client_key: $tls/client.key" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5" 'selftest/quic/{message}')"
    if capture "$FULL" stored 'selftest/quic/**'; then
      ok "Zenoh over quic/ with mutual TLS: records stored by the router"
      printf '        %s\n' "$(curl -fsS "http://127.0.0.1:$rest_port/selftest/quic/**" | grep -oE '"key":"[^"]*"' | sort -u | tr '\n' ' ')"
    else
      fail "Zenoh over quic/: nothing reached the router"
      tail -n 5 "$capture_log" | sed 's/^/        /'
    fi

    new_case zenoh-no-identity
    zenoh_client "$case_dir/zenoh.json5" "tls/localhost:$router_port" no
    subscribers "  transport: tls" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5" 'selftest/anonymous/{message}')"
    if capture "$FULL" logged 'subscriber ToZenoh: (cannot prepare the sink|record on .* not delivered)' &&
      ! stored 'selftest/anonymous/**'; then
      ok "Zenoh without the client certificate: the router refused the session"
      printf '        %s\n' "$(grep -oE 'subscriber ToZenoh: .*' "$capture_log" | head -n 1 | cut -c1-200)"
    else
      fail "Zenoh without the client certificate: expected a refusal"
      tail -n 5 "$capture_log" | sed 's/^/        /'
    fi

    new_case zenoh-pqc-required
    zenoh_client "$case_dir/zenoh.json5" "tls/localhost:$router_port" yes
    subscribers "  transport: mtls
  key_exchange: hybrid-pqc-required
  client_cert: $tls/client.pem
  client_key: $tls/client.key" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5" 'selftest/pqc/{message}')"
    probe "hybrid-pqc-required refuses the stock router, which offers classical groups only" \
      "$FULL" ToZenoh 'handshake failed'
    if capture "$FULL" logged 'subscriber ToZenoh: (cannot prepare the sink|record on .* not delivered)' &&
      ! stored 'selftest/pqc/**'; then
      ok "the Zenoh session itself is not opened under hybrid-pqc-required"
      printf '        %s\n' "$(grep -oE 'subscriber ToZenoh: .*' "$capture_log" | head -n 1 | cut -c1-200)"
    else
      fail "hybrid-pqc-required: the Zenoh session opened"
      tail -n 5 "$capture_log" | sed 's/^/        /'
    fi
  fi
fi

# -------------------------------------------------------- posture reports
section "mind security show: no security block, the shipped local configuration"
case_dir="$work/case-show-1"
mkdir -p "$case_dir"
cp flow/config/publishers.local.yml "$case_dir/publishers.yml"
cp flow/config/subscribers.local.yml "$case_dir/subscribers.yml"
mind "$DEFAULT" security show

# The reports below name the temporary files of this run; show them at the
# paths an installed service uses.
as_installed() {
  sed -e "s#$tls#/etc/first-bite/tls#g" -e "s#$case_dir#/etc/first-bite/config#g"
}

section "mind security show: level tls, REST, S3 and a local file"
new_case show-2 tcp://127.0.0.1:12000
subscribers "  transport: tls
  min_tls: 1.2" "$(rest_to ToCollector https://collector.example 8443)
  - name: ToS3
    subscriber_type: objectstorage
    subtype: awsobject
$feed
    aws_access_key_id: exampleaccesskey
    aws_secret_access_key: examplesecretkey
    aws_default_region: us-east-1
    aws_s3_endpoint: \"https://s3.us-east-1.amazonaws.com\"
    aws_bucket_name: records
  - name: ToFile
    subscriber_type: local
$feed
    local_logfile: records.log"
mind "$DEFAULT" security show | as_installed

section "mind security show: level mtls, hybrid required, Zenoh over QUIC, one override"
new_case show-3 tcp://127.0.0.1:12000
zenoh_client "$case_dir/zenoh.json5" "quic/router.example:7447" yes
subscribers "  transport: mtls
  key_exchange: hybrid-pqc-required
  client_cert: $tls/client.pem
  client_key: $tls/client.key" "$(zenoh_sink ToZenoh "$case_dir/zenoh.json5")
$(rest_to ToCollector https://collector.example 8443)
    security:
      ca_bundle: $tls/ca.pem
  - name: ToBlob
    subscriber_type: objectstorage
    subtype: azureobject
$feed
    cloud_account: exampleaccount
    cloud_access_key: \"eW91ci1hY2Nlc3Mta2V5\"
    cloud_container: records
    cloud_location_type: Public
    security:
      transport: tls
      reason: \"the storage service has no client-certificate authentication\""
mind "$FULL" security show | as_installed

printf '\n%d passed, %d failed, %d skipped\n' "$passed" "$failures" "$skipped"
[ "$failures" -eq 0 ]
