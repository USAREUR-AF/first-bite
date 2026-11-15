# Security policy

first-bite reads raw network traffic and runs with capture privileges. Security reports are prioritised over feature work.

## Reporting a vulnerability

**Do not open a public issue or pull request to report a vulnerability.**

**[Report a vulnerability privately](https://github.com/USAREUR-AF/first-bite/security/advisories/new)**

You can also open the reporting form from this repository's *Security* tab by selecting *Report a vulnerability*.

Reports are private until a maintainer publishes the advisory. Include: a description of the issue and its impact; reproduction steps or a proof of concept (a capture file or a payload in hex is ideal); the affected versions or commits; whether you have shared it elsewhere.

## What to expect

| Step | Target |
|---|---|
| Acknowledgement | 3 working days |
| Triage and severity | 7 working days |
| Fix or mitigation for a confirmed high or critical issue | 30 days, with a coordinated disclosure date agreed with you |
| Public advisory | when the fix is released, crediting you unless you prefer otherwise |

## Scope

In scope: the code in this repository and the release artifacts built from it, including the decoder's handling of malformed packets and malformed dictionaries. Out of scope: upstream projects (libpcap, ZeroMQ, Zenoh, the cloud SDKs) — report those to their maintainers, and tell us too if first-bite's use of them is affected; any deployment you do not own.

A dictionary describes a message set. Do not attach a dictionary or a capture that you are not permitted to share.

## Supported versions

Pre-1.0, the latest tagged release is supported. Fixes land on `main` and are released by tag.
