#!/usr/bin/env python3
"""Refuse a container image or a GitHub Action that is not pinned.

    tools/check-pins.py              check the tracked files
    tools/check-pins.py --self-test  check that the checker catches each case

A tag can be moved to different content after review; a digest or a commit
cannot. What is checked, in every tracked file outside private/:

  workflow    .github/workflows/*.yml: every `uses:` names a commit by its
              full 40-character SHA (a local `./` action is the repository's
              own); every `image:` and `docker://` reference carries a digest.
  dockerfile  every `FROM` carries `@sha256:<digest>`, names an earlier build
              stage, or is `scratch`.
  compose     every `image:` in a compose file carries a digest, unless the
              service has `build:` (the image is built from this tree).
  makefile    every `<NAME>_IMAGE` variable carries a digest.
  dependabot  a directory holding a pinned Dockerfile or compose file is
              listed in .github/dependabot.yml under its ecosystem, so that a
              pin is followed by an update and does not go stale.

Not checked, on purpose: the `*_RUNS_IN` lists in the Makefile. They are the
distribution images the release binaries are run in after the build; the point
of that run is the current image of each release, and nothing built there is
published.

A finding prints `<file>:<line>: <what is wrong>`. Exit 1 on any finding.
"""
import os
import re
import subprocess
import sys
import tempfile

SHA = re.compile(r"^[0-9a-f]{40}$")
DIGEST = re.compile(r"@sha256:[0-9a-f]{64}$")
USES = re.compile(r"^\s*(?:-\s*)?uses:\s*(\S+)")
IMAGE = re.compile(r"^\s*(?:-\s*)?image:\s*(\S+)")
BUILD = re.compile(r"^\s*build:")
SERVICE = re.compile(r"^  (\S[^:]*):\s*(?:#.*)?$")
FROM = re.compile(r"^\s*FROM\s+(?:--\S+\s+)*(\S+)(?:\s+AS\s+(\S+))?", re.IGNORECASE)
MAKE_IMAGE = re.compile(r"^([A-Z0-9_]*_IMAGE)\s*[?:+]?=\s*(\S+)")


def unquote(value):
    return value.strip("\"'")


def tracked(root):
    out = subprocess.run(
        ["git", "ls-files", "-z"], cwd=root, check=True, capture_output=True
    ).stdout.decode()
    return [p for p in out.split("\0") if p and not p.startswith("private/")]


def lines(root, path):
    try:
        with open(os.path.join(root, path), encoding="utf-8") as handle:
            return handle.read().splitlines()
    except (OSError, UnicodeDecodeError):
        return []


def check_workflow(root, path, findings):
    for number, line in enumerate(lines(root, path), 1):
        match = USES.match(line)
        if match:
            ref = unquote(match.group(1))
            if ref.startswith("./"):
                continue
            if ref.startswith("docker://"):
                if not DIGEST.search(ref):
                    findings.append(f"{path}:{number}: image {ref} has no @sha256 digest")
                continue
            _, _, version = ref.partition("@")
            if not SHA.match(version):
                findings.append(
                    f"{path}:{number}: action {ref} is not pinned to a 40-character commit SHA"
                )
        match = IMAGE.match(line)
        if match:
            ref = unquote(match.group(1))
            if not DIGEST.search(ref) and "${{" not in ref:
                findings.append(f"{path}:{number}: image {ref} has no @sha256 digest")


def check_dockerfile(root, path, findings):
    stages = set()
    for number, line in enumerate(lines(root, path), 1):
        match = FROM.match(line)
        if not match:
            continue
        ref, stage = match.group(1), match.group(2)
        if ref != "scratch" and ref not in stages and not DIGEST.search(ref):
            findings.append(f"{path}:{number}: base image {ref} has no @sha256 digest")
        if stage:
            stages.add(stage)


def check_compose(root, path, findings):
    # Services are the keys indented two spaces under `services:`; an image
    # is judged with the service it belongs to.
    service_lines = []
    pending = []

    def flush():
        if not any(BUILD.match(text) for _, text in service_lines):
            findings.extend(pending)

    for number, line in enumerate(lines(root, path), 1):
        if SERVICE.match(line):
            flush()
            service_lines, pending = [], []
        service_lines.append((number, line))
        match = IMAGE.match(line)
        if match:
            ref = unquote(match.group(1))
            if not DIGEST.search(ref):
                pending.append(f"{path}:{number}: image {ref} has no @sha256 digest")
    flush()


def check_makefile(root, path, findings):
    for number, line in enumerate(lines(root, path), 1):
        match = MAKE_IMAGE.match(line)
        if match and not DIGEST.search(match.group(2)):
            findings.append(
                f"{path}:{number}: {match.group(1)} = {match.group(2)} has no @sha256 digest"
            )


def dependabot_directories(root):
    """Ecosystem name to the set of directories it watches."""
    watched = {}
    ecosystem = None
    for line in lines(root, ".github/dependabot.yml"):
        match = re.match(r"^\s*-\s*package-ecosystem:\s*(\S+)", line)
        if match:
            ecosystem = unquote(match.group(1))
            watched.setdefault(ecosystem, set())
            continue
        if ecosystem is None:
            continue
        match = re.match(r"^\s*directory:\s*(\S+)", line)
        if match:
            watched[ecosystem].add(unquote(match.group(1)).strip("/"))
        match = re.match(r"^\s*-\s*[\"']?(/[^\"'\s]*)[\"']?\s*$", line)
        if match:
            watched[ecosystem].add(match.group(1).strip("/"))
    return watched


def is_dockerfile(path):
    name = os.path.basename(path)
    return name == "Dockerfile" or name.startswith("Dockerfile.") or name.endswith(".Dockerfile")


def is_compose(path):
    name = os.path.basename(path)
    return name in ("compose.yml", "compose.yaml") or re.match(
        r"^docker-compose.*\.ya?ml$", name
    )


def check(root):
    findings = []
    watched = dependabot_directories(root)
    for path in tracked(root):
        directory = os.path.dirname(path)
        before = len(findings)
        if path.startswith(".github/workflows/") and path.endswith((".yml", ".yaml")):
            check_workflow(root, path, findings)
        elif is_dockerfile(path):
            check_dockerfile(root, path, findings)
            ecosystem = "docker"
        elif is_compose(path):
            check_compose(root, path, findings)
            ecosystem = "docker-compose"
        elif os.path.basename(path) == "Makefile":
            check_makefile(root, path, findings)
        if (is_dockerfile(path) or is_compose(path)) and len(findings) == before:
            has_reference = any(
                DIGEST.search(unquote(word))
                for line in lines(root, path)
                for word in line.split()
            )
            if has_reference and directory not in watched.get(ecosystem, set()):
                findings.append(
                    f"{path}:1: pinned images here are not updated: add directory "
                    f"\"/{directory}\" to the {ecosystem} entry of .github/dependabot.yml"
                )
    return findings


def self_test():
    digest = "@sha256:" + "a" * 64
    sha = "b" * 40
    clean = {
        ".github/workflows/ci.yml": f"jobs:\n  a:\n    steps:\n      - uses: actions/checkout@{sha} # v1\n      - uses: ./.github/actions/local\n",
        ".github/dependabot.yml": 'updates:\n  - package-ecosystem: docker\n    directory: /\n  - package-ecosystem: docker-compose\n    directories:\n      - "/mocks/x"\n',
        "Dockerfile": f"FROM rust:1{digest} AS builder\nFROM builder AS test\nFROM scratch\nFROM --platform=linux/amd64 debian:12{digest}\n",
        "mocks/x/compose.yml": f"services:\n  a:\n    image: example/a:1{digest}\n  b:\n    build:\n      context: .\n    image: local:dev\n",
        "Makefile": f"PORTABLE_IMAGE ?= ubuntu:20.04{digest}\nPORTABLE_RUNS_IN ?= ubuntu:20.04\n",
        "private/Dockerfile": "FROM python:3\n",
    }
    broken = [
        ("a tag on an action", ".github/workflows/ci.yml", "      - uses: actions/checkout@v4\n", "not pinned to a 40-character commit SHA"),
        ("a short SHA on an action", ".github/workflows/ci.yml", "      - uses: actions/checkout@abc1234\n", "not pinned"),
        ("a branch on an action", ".github/workflows/ci.yml", "      - uses: owner/action/path@main\n", "not pinned"),
        ("a docker action by tag", ".github/workflows/ci.yml", "      - uses: docker://alpine:3\n", "no @sha256 digest"),
        ("a job container by tag", ".github/workflows/ci.yml", "    container:\n      image: ubuntu:24.04\n", "no @sha256 digest"),
        ("a base image by tag", "Dockerfile", "FROM debian:trixie-slim\n", "base image debian:trixie-slim"),
        ("a base image by latest", "Dockerfile", "FROM ubuntu\n", "base image ubuntu"),
        ("a compose image by tag", "mocks/x/compose.yml", "  c:\n    image: example/c:2\n", "image example/c:2"),
        ("a build image by tag", "Makefile", "STATIC_IMAGE ?= alpine:3.24\n", "STATIC_IMAGE"),
        ("a pinned Dockerfile nobody updates", "sub/Dockerfile", f"FROM debian:12{digest}\n", "dependabot"),
        ("a pinned compose file nobody updates", "other/compose.yml", f"services:\n  a:\n    image: example/a:1{digest}\n", "dependabot"),
    ]

    def run(files):
        with tempfile.TemporaryDirectory() as root:
            subprocess.run(["git", "init", "--quiet", root], check=True)
            for path, text in files.items():
                full = os.path.join(root, path)
                os.makedirs(os.path.dirname(full), exist_ok=True)
                with open(full, "w", encoding="utf-8") as handle:
                    handle.write(text)
            subprocess.run(["git", "add", "-A"], cwd=root, check=True)
            return check(root)

    failures = []
    found = run(clean)
    if found:
        failures.append(f"the clean fixture was refused: {found}")
    for name, path, addition, expected in broken:
        files = dict(clean)
        files[path] = files.get(path, "") + addition
        found = run(files)
        if len(found) != 1 or expected not in found[0] or not found[0].startswith(path + ":"):
            failures.append(f"{name}: expected one finding in {path} containing {expected!r}, got {found}")
    for failure in failures:
        print(f"FAIL {failure}")
    if failures:
        print(f"check-pins self-test: {len(failures)} FAILED", file=sys.stderr)
        return 1
    print(f"check-pins self-test: {len(broken) + 1} cases ok")
    return 0


def main():
    if sys.argv[1:] == ["--self-test"]:
        return self_test()
    if sys.argv[1:]:
        print(__doc__)
        return 2
    root = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True, text=True
    ).stdout.strip()
    findings = check(root)
    for finding in findings:
        print(finding)
    if findings:
        print(f"check-pins: {len(findings)} unpinned reference(s)", file=sys.stderr)
        return 1
    print("check-pins: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
