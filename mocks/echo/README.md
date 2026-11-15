# mocks: echo

Stands in for the HTTP endpoint of a `rest` subscriber. `echo.py` accepts a
POST on any path, prints the path and body on one line, and answers with a
JSON receipt. Python standard library only.

```bash
python3 mocks/echo/echo.py            # listens on http://127.0.0.1:5001
```

Subscriber entry that posts every record to it:

```yaml
subscribers:
  - name: echo
    subscriber_type: "rest"
    subtype: "echo"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics: ["*"]
    endpoint_url: "127.0.0.1"
    endpoint_port: 5001
    endpoint_path: "/records"
```
