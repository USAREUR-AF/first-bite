# Joining a goatnet

goat support is optional and off by default: it is the cargo feature `goat`,
and a `mind` built without it behaves exactly as one that never heard of goat.

This page is for an operator who has a goatnet to publish to and a host
running first-bite. It goes from the files the goatnet's administrator hands
over to decoded records arriving on the fabric. What goat requires, and where
goat says so, is in [contract.md](contract.md).

## What you need from the goatnet's administrator

| Item | What it is |
|---|---|
| mesh enrolment for this host | A goat router listens only on the goatnet's mesh. The host must be a mesh peer before anything here can connect; that is done with the goatnet's own tooling. |
| the onboarding archive | `goat-onboarding-<prefix>-<stamp>.tar.gz`: your certificate, its key, the goatnet's root certificate and a `NOTES.md` with the router endpoint. `<prefix>` is a 32-character slot UUID on a slot-pool goatnet, or your org prefix on a per-org goatnet. |
| the router endpoint | `tls/<host>:7447`. It is in `NOTES.md`; ask if the line reads `<router-endpoint, ask operator>`. |
| the identity model | Whether the goatnet is slot-pool or per-org. The archive's file names show it: `<32 hex digits>-cert.pem` is a slot. |

You choose three names yourself: the **format** (the message set, for example
`weather-station`), the **site** (where the source is) and the **instance**
(this publisher among others of the same format). Each is 1 to 63 characters
of `a-z 0-9 _ -` and becomes a chunk of the keys.

## 1. Build with the feature

```bash
cargo build --release -p mind --features goat
```

The feature adds the Zenoh client (`=1.9.0`, the version goat pins) and the
certificate checks. A binary without it answers `mind goat …` with the command
above.

## 2. Write the configuration

Unpack the archive into a directory only the user that runs `mind` can read,
and generate the configuration from it. The packaged service runs as
`firstbite`:

```bash
sudo install -d -m 700 -o firstbite -g firstbite /etc/first-bite/goat
sudo -u firstbite tar -xzf goat-onboarding-*.tar.gz -C /etc/first-bite/goat
sudo -u firstbite chmod 600 /etc/first-bite/goat/*-key.pem
sudo -u firstbite mind goat init --bundle /etc/first-bite/goat \
  --format weather-station --site hilltop-7 --instance tap-1 \
  --out /etc/first-bite/goat/goat.yml
```

Run the other `mind goat` commands below as the same user (`sudo -u
firstbite …`): only it can read the key.

`mind goat init` reads the certificate, key and root file names and the
`Router endpoint:` line of `NOTES.md`; `--endpoint tls/<host>:<port>` overrides
it. The result, for a slot-pool goatnet:

```yaml
goatnet:
  endpoints: ["tls/zenoh.goat.example:7447"]
identity:
  certificate: "/etc/first-bite/goat/e7a9c75b532e3d434bb09fefd3ce1ec7-cert.pem"
  private_key: "/etc/first-bite/goat/e7a9c75b532e3d434bb09fefd3ce1ec7-key.pem"
  ca_certificate: "/etc/first-bite/goat/ca-root.pem"
  slot: "e7a9c75b532e3d434bb09fefd3ce1ec7"
publish:
  format: weather-station
  site: hilltop-7
  instance: tap-1
```

The file holds paths, never key material.
[`flow/config/goat.example.yml`](../../flow/config/goat.example.yml) explains
every setting, including `org:` for a per-org goatnet, `alias:`, `version:` and
`verify_name_on_connect:`.

## 3. Check it, offline

```bash
mind goat check --config /etc/first-bite/goat/goat.yml \
  --dictionary /etc/first-bite/dictionary
```

Nothing is sent. The command checks the configuration, that the key is closed
to other users, that the certificate's common name is the configured slot or
org, that the certificate is inside its validity period by this host's clock,
and that the root verifies it. Then it prints the two write grants the router
must hold for this certificate and every key that will be published:

```text
goat configuration /etc/first-bite/goat/goat.yml
  ok    configuration      slot e7a9c75b532e3d434bb09fefd3ce1ec7
  ok    private key        /etc/first-bite/goat/e7a9c75b532e3d434bb09fefd3ce1ec7-key.pem is readable by its owner only
  ok    certificate name   common name e7a9c75b532e3d434bb09fefd3ce1ec7
  ok    certificate dates  valid until 2027-11-14 09:12 UTC (364 days from now)
  ok    root certificate   /etc/first-bite/goat/ca-root.pem verifies the certificate

router        tls/zenoh.goat.example:7447 (client mode, mutual TLS, host name verified)
session       identifier e7a9c75b532e3d434bb09fefd3ce1ec7 (the slot UUID)

the router must grant the certificate common name e7a9c75b532e3d434bb09fefd3ce1ec7:
  put  e7a9c75b532e3d434bb09fefd3ce1ec7/**
  put  attest/schema/**

would publish:
  announcement  e7a9c75b532e3d434bb09fefd3ce1ec7/services/shims/weather-station/tap-1/announce  (at startup)
  health        e7a9c75b532e3d434bb09fefd3ce1ec7/health/shims/weather-station/tap-1  (every 30 s)
  records of dictionary weather-station 1.0.0, each with its schema:
    WX/Observation
      record  e7a9c75b532e3d434bb09fefd3ce1ec7/telemetry/weather-station/hilltop-7/wx/observation
      schema  attest/schema/e7a9c75b532e3d434bb09fefd3ce1ec7/telemetry/weather-station/hilltop-7/wx/observation
    …

all checks passed; nothing was sent
```

`mind goat config --config <file>` prints the Zenoh client configuration that
will be used, as JSON5 any Zenoh 1.9 tool accepts.

## 4. Confirm the router delivers what you write

```bash
mind goat probe --config /etc/first-bite/goat/goat.yml
```

A goat router drops a write its access rules do not grant and reports nothing:
the put succeeds and the sample goes nowhere. The probe opens two sessions
with your certificate, puts one sample on the health key from the first and
waits five seconds for it on the second. It prints `delivered: …` and exits 0,
or `not delivered within 5 s: …` with the common name and the grant to ask the
administrator for, and exits 1.

## 5. Start publishing

Add a subscriber of type `goat` to the file `SUBSCRIBER_CONFIG_PATH` names,
and name the goat configuration in the environment (for the packaged service,
both lines go in `/etc/first-bite/first-bite.env`):

```yaml
subscribers:
  - name: fabric
    subscriber_type: "goat"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics: ["*"]
```

```bash
GOAT_CONFIG_PATH=/etc/first-bite/goat/goat.yml
SUBSCRIBER_CONFIG_PATH=/etc/first-bite/config/subscribers.yml
```

Start `mind` as usual (`sudo systemctl restart first-bite`, or
`mind <interface> --dictionary <dir>`). The configuration and certificates are
checked again at startup; a mistake stops the process before capture with the
same message `mind goat check` gives. Then the log shows:

```text
goat: joined tls/zenoh.goat.example:7447 as e7a9c75b532e3d434bb09fefd3ce1ec7; Zenoh session e7a9c75b532e3d434bb09fefd3ce1ec7
goat: announced on …/services/shims/weather-station/tap-1/announce and declared 3 schema(s) under attest/schema/e7a9c75b532e3d434bb09fefd3ce1ec7/**
goat: the router delivers this publisher's samples (read back on …/health/shims/weather-station/tap-1 after 12 ms)
```

Only packets that decode as a message of the dictionary are published on the
fabric. `topics` narrows that further, as for any subscriber. Other sinks in
the same file are unaffected and receive every record as before.

## 6. Confirm records are arriving

From any host that holds a certificate of the same goatnet (this one will do):

```bash
mind goat watch --config /etc/first-bite/goat/goat.yml --seconds 60
mind goat watch 'e7a9c75b532e3d434bb09fefd3ce1ec7/telemetry/**' --config … --payload --count 1
mind goat watch 'attest/schema/e7a9c75b532e3d434bb09fefd3ce1ec7/**' --config … --stored
```

Without a key expression `watch` shows everything this identity publishes.
`--stored` asks the router's storage instead of waiting for new samples, which
is how to read the announcement and the schemas after startup. `watch` refuses
`**` and other expressions with no literal chunk: a goat router grants the
whole keyspace to named infrastructure certificates only.

The heartbeat on `<prefix>/health/shims/<format>/<instance>` carries
`records_published`, `records_not_decoded` and `records_failed`; a
`records_published` that grows is the publisher's own count of what it sent.

## Troubleshooting by symptom

| Symptom | Cause | What to do |
|---|---|---|
| `this mind was built without goat support` | the binary lacks the feature | `cargo build --release -p mind --features goat` |
| `sink goat is not compiled in` at startup | same, with a `goat` subscriber configured | same |
| `GOAT_CONFIG_PATH is not set` | the subscriber is configured, the variable is not | set it in the environment or the settings file |
| `identity.slot … is not a slot UUID` | upper case, dashes, or a different value | write the 32 lower-case digits from the certificate's file name, in quotes |
| `has common name "X" but the configuration names "Y"` | the certificate belongs to another slot or org | point `identity.certificate` at the file issued for `Y`, or set `identity.slot` / `identity.org` to match the certificate |
| `expired at …` | certificates last one year | ask the administrator for a new archive |
| `is not valid until …` | this host's clock is behind | fix time sync (`timedatectl set-ntp true`); the fabric does not serve time |
| `is not verified by the root` | certificate and root are from different goatnets or archives | use the root from the same archive |
| `can be read by other users` | key file mode | `chmod 600 <key>` |
| a `Zenoh:` error when the session opens, before any `goat: joined` line | the host is not on the mesh, the endpoint is wrong, or the port is closed | `nc -vz <host> 7447` from this host; check mesh enrolment |
| connection fails with a certificate name error | the endpoint's host name is not in the router's certificate | use the DNS name the administrator gave; as a last resort set `verify_name_on_connect: false` |
| `goat: the router accepted the session but did not deliver a sample` | the router has no write rule for this common name, or was not restarted after the rule was added | send the administrator the two `put` lines of `mind goat check`; re-run `mind goat probe` |
| joined, probe delivers, but `watch` shows no records | no packet decodes: wrong dictionary, wrong interface or filter, or `topics` excludes them | look at `records_not_decoded` in the heartbeat and at mind's `done:`/capture log lines; run `mind goat check --dictionary <dir>` to see the expected keys |
| a second publisher with the same certificate keeps disconnecting the first | both pin the session identifier to the slot UUID | give each its own certificate, or set `identity.pin_session_id: false` on one |
| a consumer says a topic has no schema | the publisher started without a dictionary | start `mind` with `--dictionary` or `BITE_DICT_PATH`; schemas are declared at startup |

## Proving it without a goatnet

`tools/goat-selftest.sh` builds a throwaway certificate authority and a stock
`eclipse/zenoh:1.9.0` router configured as goat configures a slot-pool router,
then runs steps 3 to 6 against it, including a certificate with no write rule.
It needs docker and openssl and contacts nothing else.
