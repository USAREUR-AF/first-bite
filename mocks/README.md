# mocks

Local stand-ins for the services that first-bite sends records and logs to.
Each listens on `127.0.0.1` only and keeps nothing after it stops.

| Directory | Stands in for | Listens on | Start |
|---|---|---|---|
| [`echo`](echo) | the HTTP endpoint of a `rest` subscriber | 5001 | `make echo` |
| [`azure`](azure) | Azure Blob Storage, for an `azureobject` subscriber | 10000 | `make blob` |
| [`s3`](s3) | S3-compatible storage, for an `awsobject` subscriber | 9090 | `make bucket` |
| [`elastic`](elastic) | the Elasticsearch that [`deployment/filebeat`](../deployment/filebeat) ships logs to | 9200 | `docker compose -f mocks/elastic/compose.yml up` |

A subscriber is an entry in the YAML file named by `SUBSCRIBER_CONFIG_PATH`;
each directory's README gives the entry that points at its mock. To put
records through them without a network capture:

```bash
make mock        # mind reading frames from UDP 127.0.0.1:12345
make mockplay    # sends examples/weather-station.pcap to it
```

Measured with `mind --mock` and `tools/mockplay.py
examples/weather-station.pcap` sent twice (8 records): `echo` printed 8
bodies, Azurite answered 16 blob uploads with `201`, and one replay (4
records) left 8 objects in the S3Mock bucket. The object-storage sinks write
each record under two key layouts.
