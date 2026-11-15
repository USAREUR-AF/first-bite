#!/usr/bin/env python3
"""POST first-bite records from a JSON-lines file to a URL.

    tools/replay-records.py examples/weather-station.records.jsonl http://127.0.0.1:5001/records
    tools/replay-records.py records.jsonl https://collector.example/records --rate 50 --loop

Each line of the file is one record, as the `local` sink writes them and as
`examples/weather-station.records.jsonl` holds them. Each is sent as the `rest`
sink sends it: one POST per record, the record as compact JSON in the body,
`Content-Type: application/json`. A consumer can therefore be built and tested
against recorded data, with no capture and no `mind` running.

Only the standard library is used. The exit status is 0 when every record was
accepted (a 2xx answer), 1 otherwise.
"""
import argparse
import json
import sys
import time
import urllib.error
import urllib.request


def records(path):
    """The records of a JSON-lines file, as compact JSON bytes."""
    out = []
    with open(path, encoding="utf-8") as lines:
        for number, line in enumerate(lines, 1):
            if not line.strip():
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as error:
                sys.exit(f"{path}:{number}: not JSON: {error}")
            if not isinstance(record, dict):
                sys.exit(f"{path}:{number}: a record is a JSON object")
            out.append(json.dumps(record, separators=(",", ":"), ensure_ascii=False).encode("utf-8"))
    if not out:
        sys.exit(f"{path}: no records")
    return out


def post(url, body, timeout):
    """POST one record; the HTTP status, or the reason no answer came."""
    request = urllib.request.Request(
        url, data=body, method="POST", headers={"Content-Type": "application/json"}
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as answer:
            answer.read()
            return answer.status, None
    except urllib.error.HTTPError as error:
        return error.code, None
    except (urllib.error.URLError, OSError) as error:
        return None, str(getattr(error, "reason", error))


def main():
    parser = argparse.ArgumentParser(description="POST first-bite records from a JSON-lines file to a URL.")
    parser.add_argument("file", help="JSON-lines file of records")
    parser.add_argument("url", help="where to POST each record, http:// or https://")
    parser.add_argument("--rate", type=float, default=0, metavar="N",
                        help="records per second; 0 (the default) sends without pausing")
    parser.add_argument("--loop", action="store_true",
                        help="start again at the end of the file, until interrupted")
    parser.add_argument("--timeout", type=float, default=10, metavar="SECONDS",
                        help="how long to wait for each answer (default 10)")
    args = parser.parse_args()
    if not args.url.startswith(("http://", "https://")):
        parser.error("the URL must start with http:// or https://")
    if args.rate < 0:
        parser.error("--rate cannot be negative")

    bodies = records(args.file)
    interval = 1 / args.rate if args.rate else 0
    sent = refused = 0
    due = time.monotonic()
    try:
        while True:
            for body in bodies:
                if interval:
                    time.sleep(max(0.0, due - time.monotonic()))
                    due += interval
                status, reason = post(args.url, body, args.timeout)
                if status is not None and 200 <= status < 300:
                    sent += 1
                else:
                    refused += 1
                    what = f"answered {status}" if status is not None else reason
                    print(f"record {sent + refused}: {args.url} {what}", file=sys.stderr)
            if not args.loop:
                break
    except KeyboardInterrupt:
        pass
    print(f"{sent} records accepted, {refused} not accepted", file=sys.stderr)
    return 0 if refused == 0 and sent > 0 else 1


if __name__ == "__main__":
    sys.exit(main())
