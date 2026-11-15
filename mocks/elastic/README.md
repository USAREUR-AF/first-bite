# mocks: elastic

Stands in for the Elasticsearch that the Filebeat example in
[`deployment/filebeat`](../../deployment/filebeat) ships the first-bite log
to. [`compose.yml`](compose.yml) runs one Elasticsearch 8 node on
`127.0.0.1:9200` with security off.

```bash
docker compose -f mocks/elastic/compose.yml up
curl -s http://127.0.0.1:9200/_cat/indices        # what has arrived
```

Point Filebeat at it with
`sudo deployment/filebeat/install.sh --output http://127.0.0.1:9200`. No
first-bite subscriber writes to Elasticsearch.
