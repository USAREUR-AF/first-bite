#!/usr/bin/env python3
"""Build the documentation site into site/_build. Standard library only.

    tools/build-site.py [--out site/_build]

What it does:
  * copies site/ (the landing page, the stylesheet, and the interactive
    decoder when it is present), leaving out names that start with "_";
  * copies the illustrations from docs/img and the brief from docs/brief;
  * turns the Markdown pages listed in PAGES into HTML with site/_page.html,
    taking the header and footer from site/index.html so there is one copy.

The Markdown reader covers what the pages in docs/ use: headings, paragraphs,
fenced code, tables, ordered and unordered lists, and inline code, bold,
italics and links. A construct it does not know stops the build.

The site makes no request to another host: no fonts, scripts or analytics.
"""
import argparse
import html
import os
import re
import shutil
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = "https://github.com/USAREUR-AF/first-bite"

# Markdown file in docs/, navigation label, one-line description, figure to show.
# docs/plan.md and docs/decisions.md are working documents and are not published.
PAGES = [
    ("architecture", "Architecture", "The crates, the path from frame to record, sinks and transforms.", None),
    ("dictionary-format", "Dictionary format", "Every key of a dictionary and everything bite check reports.", "frame-to-record"),
    ("performance", "Performance", "The measured figures and the command behind each.", "performance"),
    ("tap", "Running on a tap", "Tap or mirror port, capture interfaces, loss counters.", "tap-placement"),
    ("deployment", "Deployment", "Release tarball, systemd service, container, offline build.", "platforms"),
    ("goat-integration", "Zenoh data fabric", "Where records go on a goat data fabric.", None),
]
# A mermaid block cannot render without a script; the page shows this figure in its place.
MERMAID_FIGURE = {"architecture": "components"}


def fail(message):
    raise SystemExit(f"build-site: {message}")


def slug(text):
    text = re.sub(r"<[^>]+>", "", text).lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.strip().replace(" ", "-")


def figure_alt(name):
    svg = open(os.path.join(ROOT, "docs", "img", f"{name}.svg"), encoding="utf-8").read()
    found = re.search(r"<desc[^>]*>(.*?)</desc>", svg, re.S)
    if not found:
        fail(f"docs/img/{name}.svg has no <desc>")
    return found.group(1)


def figure(name, caption=None):
    alt = html.escape(html.unescape(figure_alt(name)), quote=True)
    cap = f"<figcaption>{caption}</figcaption>" if caption else ""
    return (
        f'<figure><div class="fig" tabindex="0" role="group" aria-label="Illustration; scrolls sideways on a narrow screen">'
        f'<picture><source media="(prefers-color-scheme: dark)" srcset="img/{name}-dark.svg">'
        f'<img src="img/{name}.svg" alt="{alt}"></picture></div>{cap}</figure>')


# -- Markdown ---------------------------------------------------------------
def link_target(url):
    if re.match(r"^(https?:|mailto:|#)", url):
        return url
    path, _, fragment = url.partition("#")
    fragment = "#" + fragment if fragment else ""
    if path.endswith(".md") and "/" not in path:
        stem = path[:-3]
        if stem not in {p[0] for p in PAGES}:
            fail(f"link to {path}, which is not a published page")
        return stem + ".html" + fragment
    if path.startswith("../"):
        return f"{REPO}/tree/main/{path[3:]}{fragment}"
    if path.startswith(("img", "brief")):
        return f"{REPO}/tree/main/docs/{path}{fragment}"
    fail(f"do not know how to rewrite the link {url!r}")


def inline(text):
    spans = []

    def keep(match):
        spans.append("<code>" + html.escape(match.group(1), quote=False) + "</code>")
        return f"\x00{len(spans) - 1}\x00"

    text = re.sub(r"`([^`]+)`", keep, text)
    text = html.escape(text, quote=False)
    text = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)",
                  lambda m: f'<a href="{html.escape(link_target(m.group(2)), quote=True)}">{m.group(1)}</a>', text)
    text = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
    text = re.sub(r"(?<![*\w])\*([^*\s][^*]*)\*(?![*\w])", r"<em>\1</em>", text)
    return re.sub(r"\x00(\d+)\x00", lambda m: spans[int(m.group(1))], text)


def cells(row):
    return [c.strip() for c in row.strip().strip("|").split("|")]


def markdown(source, stem):
    """Return (html, title, [(id, text) for each h2])."""
    lines = source.split("\n")
    out, toc, title = [], [], None
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.strip():
            i += 1
        elif line.startswith("```"):
            lang = line[3:].strip()
            j = i + 1
            while not lines[j].startswith("```"):
                j += 1
            body = "\n".join(lines[i + 1:j])
            if lang == "mermaid":
                if stem not in MERMAID_FIGURE:
                    fail(f"docs/{stem}.md has a mermaid block and no figure to show in its place")
                out.append(figure(MERMAID_FIGURE[stem]))
            else:
                out.append(f'<pre tabindex="0"><code>{html.escape(body, quote=False)}</code></pre>')
            i = j + 1
        elif line.startswith("#"):
            level = len(line) - len(line.lstrip("#"))
            text = inline(line[level:].strip())
            if level == 1:
                title = re.sub(r"<[^>]+>", "", text)
                out.append(f"<h1>{text}</h1>")
            else:
                ident = slug(text)
                if level == 2:
                    toc.append((ident, re.sub(r"<[^>]+>", "", text)))
                out.append(f'<h{level} id="{ident}">{text}</h{level}>')
            i += 1
        elif line.startswith("|") and i + 1 < len(lines) and re.match(r"^\|[\s:|-]+\|$", lines[i + 1].strip()):
            head = cells(line)
            j = i + 2
            rows = []
            while j < len(lines) and lines[j].startswith("|"):
                rows.append(cells(lines[j]))
                j += 1
            table = ["<div class=\"table\" tabindex=\"0\" role=\"group\" aria-label=\"Table; scrolls sideways on a narrow screen\"><table><thead><tr>"]
            table += [f"<th>{inline(c)}</th>" for c in head]
            table.append("</tr></thead><tbody>")
            for row in rows:
                table.append("<tr>" + "".join(f"<td>{inline(c)}</td>" for c in row) + "</tr>")
            table.append("</tbody></table></div>")
            out.append("".join(table))
            i = j
        elif re.match(r"^(\d+\.|[-*]) ", line):
            ordered = line[0].isdigit()
            items = []
            while i < len(lines) and (re.match(r"^(\d+\.|[-*]) ", lines[i]) or (lines[i].startswith("  ") and items)):
                if re.match(r"^(\d+\.|[-*]) ", lines[i]):
                    items.append(lines[i].split(" ", 1)[1])
                else:
                    items[-1] += " " + lines[i].strip()
                i += 1
            tag = "ol" if ordered else "ul"
            out.append(f"<{tag}>" + "".join(f"<li>{inline(item)}</li>" for item in items) + f"</{tag}>")
        elif line.startswith(("<", ">", "    ")):
            fail(f"docs/{stem}.md line {i + 1}: construct not handled: {line[:40]!r}")
        else:
            j = i
            while j < len(lines) and lines[j].strip() and not re.match(r"^(#|```|\||(\d+\.|[-*]) )", lines[j]):
                j += 1
            out.append("<p>" + inline(" ".join(part.strip() for part in lines[i:j])) + "</p>")
            i = j
    if not title:
        fail(f"docs/{stem}.md has no title")
    return out, title, toc


# -- pages ------------------------------------------------------------------
def block(text, name):
    found = re.search(rf"<!-- {name} -->(.*?)<!-- /{name} -->", text, re.S)
    if not found:
        fail(f"site/index.html has no <!-- {name} --> block")
    return found.group(1).strip()


def build(out_dir):
    site = os.path.join(ROOT, "site")
    if os.path.exists(out_dir):
        shutil.rmtree(out_dir)
    shutil.copytree(site, out_dir, ignore=lambda _, names: [n for n in names if n.startswith("_")])

    img = os.path.join(out_dir, "img")
    os.makedirs(img, exist_ok=True)
    for name in sorted(os.listdir(os.path.join(ROOT, "docs", "img"))):
        if name.endswith(".svg"):
            shutil.copy(os.path.join(ROOT, "docs", "img", name), img)
    for source, name in (("first-bite.svg", "icon.svg"), ("first-bite-light.svg", "mark-light.svg"),
                         ("first-bite-dark.svg", "mark-dark.svg"),
                         ("first-bite-wordmark-light.svg", "wordmark-light.svg"),
                         ("first-bite-wordmark-dark.svg", "wordmark-dark.svg")):
        shutil.copy(os.path.join(ROOT, "icon", source), os.path.join(img, name))
    brief = os.path.join(ROOT, "docs", "brief", "first-bite-brief.pdf")
    if os.path.exists(brief):
        shutil.copy(brief, out_dir)

    index_path = os.path.join(out_dir, "index.html")
    index = open(index_path, encoding="utf-8").read()
    header, footer = block(index, "header"), block(index, "footer")
    template = open(os.path.join(site, "_page.html"), encoding="utf-8").read()

    # The landing page lists the documentation pages and shows each figure.
    cards = "".join(
        f'<a class="card" href="{stem}.html"><strong>{html.escape(label)}</strong><span>{html.escape(about)}</span></a>'
        for stem, label, about, _ in PAGES)
    index = index.replace("<!-- docs-cards -->", cards)
    index = re.sub(r"<!-- figure: ([a-z-]+)(?: \| (.*?))? -->", lambda m: figure(m.group(1), m.group(2)), index)
    with open(index_path, "w", encoding="utf-8") as fh:
        fh.write(index)

    for k, (stem, label, about, fig) in enumerate(PAGES):
        source = open(os.path.join(ROOT, "docs", f"{stem}.md"), encoding="utf-8").read()
        body, title, toc = markdown(source, stem)
        if fig:
            # After the title and the opening paragraph.
            at = next(n for n, part in enumerate(body) if part.startswith("<p>")) + 1
            body.insert(at, figure(fig))
        nav = "".join(
            f'<li><a href="{s}.html"{" aria-current=" + chr(34) + "page" + chr(34) if s == stem else ""}>{html.escape(l)}</a></li>'
            for s, l, _, _ in PAGES)
        contents = "".join(f'<li><a href="#{i}">{html.escape(t)}</a></li>' for i, t in toc)
        prev_next = ""
        if k > 0:
            prev_next += f'<a class="prev" href="{PAGES[k - 1][0]}.html"><span>Previous</span>{html.escape(PAGES[k - 1][1])}</a>'
        if k + 1 < len(PAGES):
            prev_next += f'<a class="next" href="{PAGES[k + 1][0]}.html"><span>Next</span>{html.escape(PAGES[k + 1][1])}</a>'
        page = template
        for key, value in (("title", html.escape(title)), ("description", html.escape(about, quote=True)),
                           ("header", header), ("footer", footer), ("nav", nav), ("contents", contents),
                           ("content", "\n".join(body)), ("prev_next", prev_next),
                           ("source", f"{REPO}/blob/main/docs/{stem}.md")):
            page = page.replace("{{" + key + "}}", value)
        if "{{" in page:
            fail(f"unfilled placeholder in the page for {stem}")
        with open(os.path.join(out_dir, f"{stem}.html"), "w", encoding="utf-8") as fh:
            fh.write(page)

    # Nothing may load from another host.
    for name in sorted(os.listdir(out_dir)):
        if name.endswith(".html"):
            text = open(os.path.join(out_dir, name), encoding="utf-8").read()
            remote = re.findall(r'<(?:script|link|img|source|iframe)\b[^>]*(?:src|href|srcset)="(https?:[^"]+)"', text)
            if remote:
                fail(f"{name} loads from another host: {remote[0]}")
    print(f"site built in {os.path.relpath(out_dir, ROOT)}: index.html and {len(PAGES)} pages")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--out", default=os.path.join(ROOT, "site", "_build"))
    build(os.path.abspath(parser.parse_args().out))
    sys.exit(0)
