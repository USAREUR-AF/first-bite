#!/usr/bin/env python3
"""Print brief.html to first-bite-brief.pdf with headless Chrome. Standard library only.

    python3 docs/brief/build.py

Chrome or Chromium is found from $CHROME, then the usual install paths and
names. The script fails unless the PDF has exactly ten A4 pages (checked with
pdfinfo when it is installed).
"""
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
PAGES = 10
CANDIDATES = [
    os.environ.get("CHROME", ""),
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "google-chrome", "google-chrome-stable", "chromium", "chromium-browser",
]


def main():
    chrome = next((c for c in CANDIDATES if c and (os.path.exists(c) or shutil.which(c))), None)
    if not chrome:
        sys.exit("brief: no Chrome or Chromium found; set CHROME to its path")
    out = HERE / "first-bite-brief.pdf"
    fresh = HERE / "first-bite-brief.pdf.new"
    with tempfile.TemporaryDirectory() as profile:
        subprocess.run(
            [chrome, "--headless=new", "--disable-gpu", "--no-sandbox", f"--user-data-dir={profile}",
             "--no-pdf-header-footer", f"--print-to-pdf={fresh}", (HERE / "brief.html").as_uri()],
            check=True, timeout=120, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if not fresh.exists():
        sys.exit("brief: Chrome wrote no PDF")
    if shutil.which("pdfinfo"):
        info = subprocess.run(["pdfinfo", str(fresh)], check=True, capture_output=True, text=True).stdout
        pages = int(re.search(r"^Pages:\s+(\d+)", info, re.M).group(1))
        if pages != PAGES or "A4" not in info:
            fresh.unlink()
            sys.exit(f"brief: expected {PAGES} A4 pages, got {pages}; a page overflows")
    fresh.replace(out)
    print(f"wrote {out.relative_to(HERE.parent.parent)}")


if __name__ == "__main__":
    main()
