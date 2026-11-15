# Telegraf example

Optional. Reports CPU, memory, disk and network metrics of the host, and the
resource use of the processes of `first-bite.service`, to an InfluxDB 2 server
every ten seconds. Nothing in first-bite depends on it.

| File | What |
|---|---|
| `install.sh` | installs the Telegraf `.deb` found in this directory, writes `/etc/telegraf/telegraf.conf`, enables and restarts `telegraf.service` |
| `telegraf.conf` | the configuration it installs: inputs `cpu`, `mem`, `disk`, `net`, `procstat`; output `influxdb_v2` |

```bash
# On a machine with internet access; the version is TELEGRAF_VERSION in install.sh.
curl -LO https://dl.influxdata.com/telegraf/releases/telegraf_1.40.3-1_amd64.deb

# On the target, with the .deb next to install.sh.
sudo ./install.sh --output https://influxdb.example.org:8086
```

For `arm64` replace `amd64` in the file name. The organisation (`example-org`)
and bucket (`first-bite`) are set in `telegraf.conf`; edit them before
installing. The token is read from the environment: put
`INFLUX_TOKEN=<token>` in `/etc/default/telegraf`, which the packaged unit
loads, and restart `telegraf`. The configuration that the package ships is kept
as `/etc/telegraf/telegraf.conf.packaged`; a second run keeps the installed
configuration.
