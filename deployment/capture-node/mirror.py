#!/usr/bin/env python3
"""Copy completed capture files to S3-compatible storage.

Run by first-bite-capture-mirror.service. Settings come from the environment
(capture.env): S3_ENDPOINT, S3_BUCKET, S3_PREFIX, S3_REGION. The access key is
read from the file systemd hands the service ($CREDENTIALS_DIRECTORY/s3), or
from the file S3_CREDENTIALS_FILE names when run by hand.

A file is complete when the capture has started a newer one. Each complete
file is PUT once, as <prefix>/<file name>, signed with AWS Signature Version
4; a marker in uploaded/ records it. A file that fails is tried again on the
next run. A mirror run deletes no capture file: the capture's ring does that.

    mirror.py --prune N

deletes all but the N newest capture files. The capture service runs it
before each start, because dumpcap's ring counts only the files of its own
run: without it every restart would leave a ring's worth of files behind.

Only the standard library is used.
"""
import datetime
import hashlib
import hmac
import http.client
import os
import ssl
import sys
import urllib.parse

CAPTURE_DIR = os.environ.get("CAPTURE_DIR", "/var/lib/first-bite-capture")
PATTERN_SUFFIX = ".pcapng"
TIMEOUT_SECONDS = 300
CHUNK = 1024 * 1024


def log(message):
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    print(f"{stamp} {message}", flush=True)


def credentials():
    """The access key id and secret from the credentials file."""
    directory = os.environ.get("CREDENTIALS_DIRECTORY")
    path = os.path.join(directory, "s3") if directory else os.environ.get("S3_CREDENTIALS_FILE", "")
    values = {}
    try:
        with open(path, encoding="utf-8") as lines:
            for line in lines:
                line = line.strip()
                if line and not line.startswith("#") and "=" in line:
                    key, value = line.split("=", 1)
                    values[key.strip()] = value.strip()
    except OSError as error:
        sys.exit(f"cannot read the credentials file: {error.strerror}")
    key_id, secret = values.get("access_key_id"), values.get("secret_access_key")
    if not key_id or not secret:
        sys.exit("the credentials file must set access_key_id and secret_access_key")
    return key_id, secret


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(CHUNK), b""):
            digest.update(block)
    return digest.hexdigest()


def sign(key, text):
    return hmac.new(key, text.encode("utf-8"), hashlib.sha256).digest()


def put(endpoint, bucket, key, path, region, key_id, secret):
    """PUT one file, path-style; the HTTP status and the answer's body."""
    url = urllib.parse.urlsplit(endpoint)
    object_path = urllib.parse.quote(f"{url.path.rstrip('/')}/{bucket}/{key}", safe="/-_.~")
    payload_hash = sha256_file(path)
    now = datetime.datetime.now(datetime.timezone.utc)
    amz_date, date = now.strftime("%Y%m%dT%H%M%SZ"), now.strftime("%Y%m%d")
    headers = {
        "host": url.netloc,
        "x-amz-content-sha256": payload_hash,
        "x-amz-date": amz_date,
    }
    signed_headers = ";".join(sorted(headers))
    canonical = "\n".join(
        ["PUT", object_path, ""]
        + [f"{name}:{headers[name]}" for name in sorted(headers)]
        + ["", signed_headers, payload_hash]
    )
    scope = f"{date}/{region}/s3/aws4_request"
    to_sign = "\n".join(
        ["AWS4-HMAC-SHA256", amz_date, scope, hashlib.sha256(canonical.encode("utf-8")).hexdigest()]
    )
    signing_key = sign(sign(sign(sign(("AWS4" + secret).encode("utf-8"), date), region), "s3"), "aws4_request")
    signature = hmac.new(signing_key, to_sign.encode("utf-8"), hashlib.sha256).hexdigest()
    headers["Authorization"] = (
        f"AWS4-HMAC-SHA256 Credential={key_id}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
    headers["Content-Length"] = str(os.path.getsize(path))
    headers["Content-Type"] = "application/vnd.tcpdump.pcap"

    if url.scheme == "https":
        connection = http.client.HTTPSConnection(
            url.netloc, timeout=TIMEOUT_SECONDS, context=ssl.create_default_context()
        )
    else:
        connection = http.client.HTTPConnection(url.netloc, timeout=TIMEOUT_SECONDS)
    try:
        with open(path, "rb") as body:
            connection.request("PUT", object_path, body=body, headers=headers)
            answer = connection.getresponse()
            return answer.status, answer.read(2000).decode("utf-8", errors="replace")
    finally:
        connection.close()


def capture_files():
    """The capture files, oldest first. dumpcap restarts its numbering with
    each run, so the order is by modification time, then by name."""
    found = []
    for name in os.listdir(CAPTURE_DIR):
        if name.endswith(PATTERN_SUFFIX):
            try:
                found.append((os.path.getmtime(os.path.join(CAPTURE_DIR, name)), name))
            except FileNotFoundError:
                pass
    return [name for _, name in sorted(found)]


def prune(keep):
    """Delete all but the `keep` newest capture files."""
    files = capture_files()
    old = files[: max(0, len(files) - keep)]
    for name in old:
        try:
            os.remove(os.path.join(CAPTURE_DIR, name))
        except FileNotFoundError:
            pass
    if old:
        log(f"start of capture: deleted the {len(old)} oldest files, kept {len(files) - len(old)}")
    return 0


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--prune" and sys.argv[2].isdigit():
        return prune(int(sys.argv[2]))
    if len(sys.argv) != 1:
        sys.exit("usage: mirror.py [--prune N]")
    endpoint = os.environ.get("S3_ENDPOINT", "").strip()
    bucket = os.environ.get("S3_BUCKET", "").strip()
    prefix = os.environ.get("S3_PREFIX", "").strip().strip("/")
    region = os.environ.get("S3_REGION", "").strip() or "us-east-1"
    if not endpoint or not bucket:
        sys.exit("S3_ENDPOINT and S3_BUCKET must be set in capture.env")
    if urllib.parse.urlsplit(endpoint).scheme not in ("http", "https"):
        sys.exit("S3_ENDPOINT must start with http:// or https://")
    key_id, secret = credentials()

    markers = os.path.join(CAPTURE_DIR, "uploaded")
    os.makedirs(markers, exist_ok=True)
    # The newest file is still being written.
    files = capture_files()
    complete = files[:-1]
    # A marker whose file the ring has deleted is no longer needed.
    for marker in os.listdir(markers):
        if marker not in files:
            os.remove(os.path.join(markers, marker))

    sent = failed = lost = 0
    for name in complete:
        marker = os.path.join(markers, name)
        if os.path.exists(marker):
            continue
        key = f"{prefix}/{name}" if prefix else name
        path = os.path.join(CAPTURE_DIR, name)
        try:
            size = os.path.getsize(path)
            status, body = put(endpoint, bucket, key, path, region, key_id, secret)
        except FileNotFoundError:
            # The ring deleted it between the listing and the upload.
            log(f"{name}: deleted by the capture before it was mirrored")
            lost += 1
            continue
        except (OSError, http.client.HTTPException) as error:
            log(f"{name}: not mirrored: {error}")
            failed += 1
            # The storage is not answering; the rest wait for the next run.
            break
        if 200 <= status < 300:
            open(marker, "w", encoding="utf-8").close()
            log(f"{name}: mirrored to {bucket}/{key} ({size} bytes)")
            sent += 1
        else:
            log(f"{name}: not mirrored: the storage answered {status}: {' '.join(body.split())[:300]}")
            failed += 1
    waiting = sum(1 for name in complete if not os.path.exists(os.path.join(markers, name)))
    log(
        f"run finished: {sent} mirrored, {failed} failed, {lost} deleted by the capture first, "
        f"{waiting} complete files waiting, {len(files)} on disk"
    )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
