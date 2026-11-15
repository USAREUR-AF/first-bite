# Filebeat example

Optional. Ships every line of `/var/log/first-bite/*.log` (the log lines of
`mind`, which include one line per published record at log level `info`) to an
Elasticsearch node. Nothing in first-bite depends on it.

| File | What |
|---|---|
| `install.sh` | installs the Filebeat `.deb` found in this directory, writes `/etc/filebeat/filebeat.yml`, enables and restarts `filebeat.service` |
| `filebeat.yml` | the configuration it installs: one `filestream` input, one `output.elasticsearch` |

```bash
# On a machine with internet access; the version is FILEBEAT_VERSION in install.sh.
curl -LO https://artifacts.elastic.co/downloads/beats/filebeat/filebeat-8.19.25-amd64.deb
curl -LO https://artifacts.elastic.co/downloads/beats/filebeat/filebeat-8.19.25-amd64.deb.sha512
sha512sum -c filebeat-8.19.25-amd64.deb.sha512

# On the target, with the .deb next to install.sh.
sudo ./install.sh --output https://elasticsearch.example.org:9200
```

For `arm64` replace `amd64` in the file names. The configuration that the
package ships is kept as `/etc/filebeat/filebeat.yml.packaged`; a second run
keeps the installed configuration. Credentials go in the Filebeat keystore, as
the comment in `filebeat.yml` shows, not in the file.

[`mocks/elastic`](../../mocks/elastic) starts a local Elasticsearch to point
`--output http://127.0.0.1:9200` at.
