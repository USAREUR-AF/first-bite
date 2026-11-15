# mocks: s3

Stands in for S3-compatible object storage. [`compose.yml`](compose.yml) runs
[S3Mock](https://github.com/adobe/S3Mock) on `127.0.0.1:9090` and creates the
bucket `first-bite` at start. It accepts any access key and secret. Objects
are gone when the container stops.

```bash
docker compose -f mocks/s3/compose.yml up
curl -s 'http://127.0.0.1:9090/first-bite?list-type=2'    # list the objects
```

Subscriber entry that uploads every record to the bucket:

```yaml
subscribers:
  - name: s3
    subscriber_type: "objectstorage"
    subtype: "awsobject"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics: ["*"]
    aws_access_key_id: "anything"       # the mock accepts any value; a real
    aws_secret_access_key: "anything"   # key is written as a ${NAME} reference
    aws_default_region: "us-east-1"
    aws_s3_endpoint: "http://127.0.0.1:9090"
    aws_bucket_name: "first-bite"
```
