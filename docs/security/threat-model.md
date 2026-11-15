# Threat model

What first-bite protects, where untrusted input enters, what the design does
about it, and what it does not do. Status words: **built** (the code exists
and runs), **designed** (decided, not implemented), **not built** (nothing
exists).

## Assets

| Asset | Where it is | Who should reach it |
|---|---|---|
| Decoded records: every captured UDP and TCP packet in full, with its decoded fields | the ZeroMQ PUB socket; `/var/log/first-bite/mind.log` at log level `info`; files written by `--output` and the local-file sink; the configured sinks | the operator and the systems the operator configured |
| Storage credentials: the Azure account key, the S3 access key id and secret | the service environment (`/etc/first-bite/first-bite.env`) or systemd credential files; in process memory | the service process and root |
| The capture privilege: `CAP_NET_RAW` and `CAP_NET_ADMIN` on the tapped interfaces | the `mind` process | nothing that enters through a packet, a dictionary or a socket |
| The dictionary: the message definitions of the tapped system | `/etc/first-bite/dictionary` | the operator; it can be as sensitive as the document it transcribes |

## Trust boundaries and what is done at each

| Boundary | Trust | What the design does | Status |
|---|---|---|---|
| The tapped network to `mind` | hostile: any host on the link chooses the bytes of every frame | Frames are parsed by `etherparse` and the decoder in safe Rust; first-party code has no `unsafe`. A malformed frame is counted and dropped. A field beyond the payload decodes to `null`. Decoding one packet costs at most the fields of one message (4,096). Random and truncated payloads are decoded in `bite/tests/robustness.rs` | built |
| | | With two or more interfaces, frames wait in a queue bounded at 65,536 frames and 256 MiB; a frame that does not fit is dropped and counted | built |
| | | The process holds only the two capture capabilities, as an unprivileged user, under the systemd sandbox in `deployment/first-bite.service` (`NoNewPrivileges`, `ProtectSystem=strict`, restricted address families, no devices) | built |
| | | Packet content reaches the log only as JSON, and the logger writes control characters as escapes, so a packet cannot forge a log line | built |
| Dictionary files to the decoder | supplied by the operator, possibly received from someone else; in the browser, any file a visitor is handed | Limits in `bite/src/limits.rs`: 4 MiB per file, 10,000 files, 4,096 fields per message, 65,536 entries per `values` object, fields within the first 65,535 bytes of a payload, JSON nesting of 128. Anything over a limit is reported and left out. `bite/tests/hostile_dictionary.rs` loads adversarial and damaged dictionaries | built |
| | | A dictionary is data: it names bit ranges and labels. Nothing in it is executed, and no name in it becomes a file path. In an object name a topic cannot leave its prefix | built |
| | | The decoder page builds the document with text nodes only, reads at most 4 MiB per file and 10,000 files, and draws at most 1,024 bytes of a payload. Files are read in the page and sent nowhere | built |
| | | Dictionary signing: a way to check who wrote a dictionary before loading it | not built |
| The ZeroMQ PUB socket to its readers | unauthenticated and unencrypted: whoever can connect reads every record | The built-in publisher binds `127.0.0.1`. A publisher bound to an address other machines can reach logs a warning at startup. The container example publishes the port on the host's loopback only | built |
| | | Authentication or encryption on the socket (ZeroMQ CURVE) | not built |
| A publisher socket to a subscriber | the subscriber trusts the configured address to be a first-bite publisher | Messages over 16 MiB are refused by the socket. A record that is not JSON is not forwarded to the REST sink. The topic is sanitised before it is used in an object name or a Zenoh key | built |
| | | Record signing: a way for a reader to check that a record came from a given capture point and was not altered | not built |
| `mind` to a sink (REST, Azure Blob Storage, S3, Zenoh) | the endpoint is configured by the operator; the network between is not trusted | TLS certificates are verified for `https://` endpoints and there is no setting to turn that off. The REST and Azure clients follow no redirect. An answer is read up to 1 MiB and quoted up to 512 characters. An `http://` endpoint is allowed and sends records in clear: the operator's choice | built |
| | | Credentials are not in configuration files: a key holds a `${NAME}` reference resolved from the service environment, or a `_file` path. A credential is never logged or quoted in an error: its type prints `***`, and the S3 sink removes both keys from a store's answer. `/etc/first-bite` is `0750 root:firstbite`, files `0640` | built |
| | | Zenoh endpoints, mode and TLS identity come from a standard Zenoh configuration file that first-bite passes through unread | built |
| The host | root and the `firstbite` user are trusted; other local users are not | Record files are created `0640`; the log directory and the working directory are `0750`; the environment file is `0640 root:firstbite`. `install.sh` sets these on every run | built |
| The `--mock` UDP socket | development only: every datagram becomes a record | Binds `127.0.0.1` unless another address is given | built |
| Contributors to the repository | a pull request is untrusted until reviewed | The controls in [SECURITY.md](../../SECURITY.md#how-this-repository-is-watched): leak gate, pin check, advisory audit, dependency policy, CodeQL, Scorecard, Dependabot. Commits on `main` are signed | built |

## What the design does not do

| Not done | Consequence | Status |
|---|---|---|
| Encryption or authentication on the ZeroMQ sockets | A socket bound beyond loopback gives every record to any host that can connect, and a subscriber cannot tell a first-bite publisher from another process at the configured address. Keep publishers on loopback, or put the hosts on a network that provides both | not built |
| Dictionary signing | The decoder loads whatever is in the dictionary directory. A changed dictionary changes what records say; it cannot run code or exceed the limits above. Protect the directory with file permissions (`install.sh` makes it writable by root only) | not built |
| Record signing | A consumer of the socket, a file or a bucket cannot verify where a record came from or that it is unaltered | not built |
| Encryption of records at rest | The log, `--output` files and local-file sink are plain JSON, protected by file permissions only | not built |
| Rate limiting of the tapped traffic | A flood on the tapped link is processed as fast as the host allows; frames beyond that are dropped by the kernel or the queue and counted in the log | not built |
| Keeping a credential out of process memory or the service environment | A process running as `firstbite` or root can read the credentials. systemd credential files (`LoadCredential=`) keep them out of the environment file | built as described |
