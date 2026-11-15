# capture node

A kit for a site that allows a box that records traffic and does not allow a
live decoder or an uplink. The box writes what a tap or mirror port delivers
to capture files, in a ring of bounded size. It decodes nothing and publishes
nothing. The files are decoded later, on another machine, by
`mind --pcap-file`. Where the site has storage the box may reach, a timer
copies completed files to an S3-compatible bucket.

| Piece | Status |
|---|---|
| capture to a ring of files, bounded by size and by age | built; measured in a container |
| mirror of completed files to S3-compatible storage, signed requests | built; measured against S3Mock and MinIO |
| install and uninstall script, logrotate rule | built; measured in a container |
| replay of the files through `mind` | built; measured |
| the apt-offline patch procedure below | written from the tool's documentation; not run |
| the kit on a physical tap, at rate | not measured |

## How this differs from what already exists

| | What it does | Live decode | Network path needed |
|---|---|---|---|
| this kit | writes frames to files; optionally copies the files to a bucket | no | none, or one bucket |
| [`deployment/`](../README.md) in this repository | runs `mind` as a service: capture, decode, publish | yes | whatever the sinks need |
| [hello-galaxy-legacy-feed](https://github.com/USAREUR-AF/hello-galaxy-legacy-feed) | carries, mirrors or decodes a sensor feed live onto a goatnet, in three rungs | from its third rung | a goatnet |
| [octo-pod](https://github.com/USAREUR-AF/octo-pod) | records received feeds locally inside a pod and can release them to a fabric | no | none until release |

Use this kit when files on a disk are all the site permits. Use one of the
others when the data may leave the box as it arrives.

## Files

| File | What |
|---|---|
| [`install.sh`](install.sh) | installs the user, the units, the mirror script, the settings and the logrotate rule; `--uninstall` removes the units and keeps the data |
| [`first-bite-capture.service`](first-bite-capture.service) | runs `dumpcap` as user `fbcapture` with capture capabilities only |
| [`first-bite-capture-mirror.service`](first-bite-capture-mirror.service), [`.timer`](first-bite-capture-mirror.timer) | every five minutes, copies completed files to the bucket |
| [`mirror.py`](mirror.py) | the copy: Python 3, standard library only |
| [`capture.env.example`](capture.env.example) | becomes `/etc/first-bite-capture/capture.env`: interface, filter, ring size, bucket |
| [`s3-credentials.example`](s3-credentials.example) | becomes `/etc/first-bite-capture/s3-credentials`, owned by root, mode 0600 |
| [`first-bite-capture.logrotate`](first-bite-capture.logrotate) | weekly rotation of the mirror's log, eight kept |
| [`selftest.sh`](selftest.sh) | runs all of it in a container that boots systemd (needs docker) |

## Install

Ubuntu 22.04 or later. Two packages: `wireshark-common`, which holds `dumpcap`,
and `python3`.

```bash
sudo apt-get install wireshark-common python3   # answer No to "non-superusers capture"
sudo ./install.sh --interface eth0
```

With storage to mirror to:

```bash
sudo ./install.sh --interface eth0 \
    --endpoint https://storage.example.org --bucket captures --prefix site-a/node-1
sudoedit /etc/first-bite-capture/s3-credentials     # the access key
```

The access key lives in that one file. systemd reads it as root and hands it
to the mirror service alone (`LoadCredential=`); it is in no unit, no
environment file and no command line. Give the key write access to the one
bucket and nothing more.

The capture interface needs no address: on a tap or mirror port, bring it up
without one ([docs/tap.md](../../docs/tap.md)).

| Path | What |
|---|---|
| `/var/lib/first-bite-capture/capture_<n>_<time>.pcapng` | the capture files; `uploaded/` holds one marker per mirrored file |
| `/etc/first-bite-capture/capture.env` | settings |
| `/var/log/first-bite-capture/mirror.log` | one line per mirrored file and per run |
| `journalctl -u first-bite-capture` | what `dumpcap` reports, including frames it dropped |

## Why dumpcap

`dumpcap` rotates on size and on time together and keeps a fixed number of
files, deleting the oldest, in one process: `-b filesize:… -b duration:…
-b files:…`. `tcpdump` has a size rotation (`-C` with `-W`) and a time
rotation (`-G`), and combining them leaves pruning to a second program.
`dumpcap` writes pcapng, which keeps the interface name and the count of
dropped frames in the file, and it reports drops when it stops. Its cost is
the size of `wireshark-common`.

## Disk sizing

A new file starts every `CAPTURE_FILE_KB` kilobytes or `CAPTURE_FILE_SECONDS`
seconds, whichever comes first, and `CAPTURE_FILES` files are kept. The oldest
is deleted to make room, mirrored or not.

| Bound | Value | With the defaults (100 MB, 900 s, 200 files) |
|---|---|---|
| Disk the capture can use | 2 × `CAPTURE_FILES` × `CAPTURE_FILE_KB` | 40 GB |
| Longest time the ring holds | `CAPTURE_FILES` × `CAPTURE_FILE_SECONDS` | 50 hours |

The factor 2: `dumpcap` counts only the files of its own run, so the files of
the run before a restart are kept as well, cut back to the newest
`CAPTURE_FILES` at each start.

A file is started on time even when no frame arrived, so on a quiet link the
ring holds `CAPTURE_FILES` × `CAPTURE_FILE_SECONDS` of time and no more. On a
busy link size decides: the ring holds `CAPTURE_FILES` × `CAPTURE_FILE_KB`
divided by the link's byte rate.

| Mean traffic captured | Bytes per day | Time held by 20 GB |
|---|---|---|
| 100 kbit/s | 1.1 GB | 50 hours (the age bound) |
| 1 Mbit/s | 10.8 GB | 44 hours |
| 10 Mbit/s | 108 GB | 4.4 hours |
| 100 Mbit/s | 1.08 TB | 27 minutes |

Size the ring for the longest time the box must hold data with no mirror and
nobody collecting, and set `CAPTURE_FILTER` (for example `udp port 40100`) so
that only the traffic of interest is written. Put `/var/lib/first-bite-capture`
on a filesystem of its own, so a full ring cannot fill the system disk.

## Decode the files later

On a machine with first-bite and the dictionary, each file is replayed as if
it were arriving; every record carries the time the frame was captured.

```bash
for file in capture_*.pcapng; do
    mind --pcap-file "$file" --dictionary /path/to/dictionary
done
```

Records go to the sinks configured on that machine
([docs/deployment.md](../../docs/deployment.md)). Files copied by hand from
`/var/lib/first-bite-capture` and files fetched from the bucket are the same
files.

## Mirror

Every five minutes the mirror copies each completed file it has not copied
yet. A file is complete when the capture has started a newer one. Objects are
named `<S3_PREFIX>/<file name>`; give each node its own prefix. Requests are
path-style and signed with AWS Signature Version 4, over HTTPS when the
endpoint says `https://`, with the machine's certificate store.

When the storage does not answer, the run stops and fails, nothing is marked,
and the next run starts where this one stopped. A file the ring deleted before
it was copied is reported in the log; if that happens, the ring is too small
for the time the storage was away.

```text
2026-11-15T16:21:06Z capture_00002_20261115162052.pcapng: mirrored to first-bite/selftest/capture_00002_20261115162052.pcapng (20004 bytes)
2026-11-15T16:21:06Z run finished: 5 mirrored, 0 failed, 1 deleted by the capture first, 1 complete files waiting, 7 on disk
```

Without `--endpoint` the timer is off and files leave the box only when
somebody copies them.

## Patch an air-gapped node with apt-offline

A node with no network path still needs security updates. `apt-offline` moves
the work to a machine that has one: the node writes down what it needs, the
connected machine downloads it, the node installs it. One removable medium
goes each way. This procedure follows the tool's documentation; it has not
been run on this kit.

Once, while the node is being prepared and still has access to a package
mirror: `sudo apt-get install apt-offline`.

1. **On the node**, write the request:

   ```bash
   sudo apt-offline set /media/usb/node.sig --update --upgrade
   ```

   Add `--install-packages <name>` to ask for a new package as well.

2. **On a connected machine** with `apt-offline` installed (any Debian or
   Ubuntu release), download what the request names:

   ```bash
   apt-offline get /media/usb/node.sig --bundle /media/usb/node.zip
   ```

3. **On the node**, load the bundle and upgrade from it:

   ```bash
   sudo apt-offline install /media/usb/node.zip
   sudo apt-get upgrade
   ```

   `apt` checks each package against the release signatures in the bundle,
   as it does online.

4. Restart what was upgraded: `sudo systemctl restart first-bite-capture`
   after a `wireshark-common` or `libpcap` update; reboot after a kernel
   update. The ring keeps its files across both.

Handle the medium under the site's rules for removable media: it is the one
thing that crosses the boundary.

## Check the kit

```bash
deployment/capture-node/selftest.sh
```

builds a container with systemd, installs the kit, captures on the loopback
interface with a ring of six small files, mirrors to the S3 stand-in of
[`mocks/s3`](../../mocks/s3), stops the storage to see a run fail, restarts
the capture to see earlier files cut back, rotates the log and uninstalls.
