#!/usr/bin/env python3
"""A REST consumer that accepts every POST and prints what it received.

    python3 mocks/echo/echo.py [--port 5001]

Stands in for the endpoint of a `rest` subscriber. The body of each POST, on
any path, is printed on one line and answered with a JSON receipt. Only the
standard library is used; it listens on 127.0.0.1.
"""
import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Echo(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length).decode("utf-8", errors="replace")
        print(f"{self.path} {body}", flush=True)
        receipt = json.dumps({"received_bytes": length, "path": self.path}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(receipt)))
        self.end_headers()
        self.wfile.write(receipt)

    def log_message(self, format, *args):
        # The body line above is the output; the default access log is noise.
        pass


def main():
    parser = argparse.ArgumentParser(description="Print the body of every POST.")
    parser.add_argument("--port", type=int, default=5001)
    port = parser.parse_args().port
    print(f"listening on http://127.0.0.1:{port}", flush=True)
    try:
        ThreadingHTTPServer(("127.0.0.1", port), Echo).serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
