# deployment

What a host needs to run first-bite as a service.
[docs/deployment.md](../docs/deployment.md) is the guide; a release tarball
carries a copy as `INSTALL.md`.

| File | What |
|---|---|
| [`install.sh`](install.sh) | installs the binaries, the service user, configuration, the unit and the logrotate rule; `--uninstall` removes them |
| [`first-bite.service`](first-bite.service) | systemd unit: runs `mind` as user `firstbite` with capture capabilities only |
| [`first-bite.env.example`](first-bite.env.example) | becomes `/etc/first-bite/first-bite.env`: interface, dictionary, configuration paths, record metadata |
| [`first-bite.logrotate`](first-bite.logrotate) | daily rotation of `/var/log/first-bite/*.log`, seven kept |
| [`capture-node/`](capture-node) | a separate kit for a site that allows no live decoder: capture to a bounded ring of files, mirror them to a bucket, decode later with `mind --pcap-file` |
| [`filebeat/`](filebeat) | optional example: ship the log file to Elasticsearch |
| [`telegraf/`](telegraf) | optional example: report host and service metrics to InfluxDB |

`install.sh` reads the `mind` and `bite` binaries and a `config/` directory
from the directory it is in. A release tarball has them there; in a checkout,
copy `target/release/mind` and `target/release/bite` beside it first.
