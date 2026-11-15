#!/usr/bin/env python3
"""Bundle the example dictionaries into one JSON file for the decoder page.

    tools/bundle-dictionaries.py [--out site/dictionaries.json] [dictionary-dir ...]

Without directories, every directory under dictionaries/ is bundled. The output
is a list, one entry per dictionary, each with the dictionary's directory name
and its files as relative path to text: the form `loadDictionary` takes.
"""

import argparse
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def bundle(directory: pathlib.Path) -> dict:
    files = {
        path.relative_to(directory).as_posix(): path.read_text(encoding="utf-8")
        for path in sorted(directory.rglob("*.json"))
        if path.is_file()
    }
    if not any(name.endswith("/metadata.json") for name in files):
        sys.exit(f"{directory}: no interface (no <INTERFACE>/metadata.json)")
    return {"id": directory.name, "files": files}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=pathlib.Path, default=ROOT / "site" / "dictionaries.json")
    parser.add_argument("directories", nargs="*", type=pathlib.Path)
    args = parser.parse_args()

    directories = args.directories or sorted(
        path for path in (ROOT / "dictionaries").iterdir() if path.is_dir()
    )
    bundles = [bundle(directory) for directory in directories]
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(bundles, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    count = sum(len(entry["files"]) for entry in bundles)
    print(f"{args.out}: {len(bundles)} dictionaries, {count} files")


if __name__ == "__main__":
    main()
