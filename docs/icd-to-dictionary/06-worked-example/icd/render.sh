#!/usr/bin/env bash
# Render the document to PDF, as a real interface document usually arrives.
#
#   docs/icd-to-dictionary/06-worked-example/icd/render.sh
#
# Needs pandoc and Chrome or Chromium (set CHROME to its path); uses qpdf if it
# is installed. The PDF is committed; run this after changing the Markdown.
set -euo pipefail
cd "$(dirname "$0")"
chrome="${CHROME:-}"
if [ -z "$chrome" ]; then
  for candidate in "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" chromium chromium-browser google-chrome; do
    if command -v "$candidate" >/dev/null 2>&1 || [ -x "$candidate" ]; then
      chrome="$candidate"
      break
    fi
  done
fi
[ -n "$chrome" ] || { echo "render: set CHROME to a Chrome or Chromium binary" >&2; exit 2; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
name=GZC-ICD-001-B
pandoc "$name.md" --from gfm --to html5 --standalone --metadata "title=$name" \
  --css render.css --embed-resources --output "$work/$name.html"
"$chrome" --headless --disable-gpu --no-pdf-header-footer \
  --print-to-pdf="$work/$name.pdf" "file://$work/$name.html" 2>/dev/null
# qpdf, when present, makes the file about a third of the size.
if command -v qpdf >/dev/null 2>&1; then
  qpdf --object-streams=generate --recompress-flate --compression-level=9 "$work/$name.pdf" "$name.pdf"
else
  cp "$work/$name.pdf" "$name.pdf"
fi
echo "wrote $name.pdf"
