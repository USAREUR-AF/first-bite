#!/usr/bin/env python3
"""Build the documentation site into site/_build. Standard library only.

    tools/build-site.py [--out site/_build]

What it does:
  * copies site/ (the landing page, the stylesheet, and the interactive
    decoder when it is present), leaving out names that start with "_";
  * copies the illustrations from docs/img and the brief from docs/brief;
  * turns every docs/**/*.md except plan.md, decisions.md and the notes in
    docs/img and docs/brief into a page with
    site/_page.html, a sub-directory becoming a group in the navigation, and
    takes the header and footer from site/index.html so there is one copy.

The Markdown reader covers headings, paragraphs, fenced code, tables, nested
ordered and unordered lists, block quotes, rules, figures from docs/img (an
image, or a <picture> block with a light and a dark file), and
inline code, bold, italics and links. A construct it does not know, or a link
to a file that does not exist, stops the build.

The site makes no request to another host: no fonts, scripts or analytics.
"""
import argparse
import html
import os
import re
import shutil
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(ROOT, "docs")
REPO = "https://github.com/USAREUR-AF/first-bite"

# Every docs/**/*.md is published except these working documents. A new
# document needs no change here: it gets its title as its label, and a
# sub-directory becomes a group in the navigation.
UNPUBLISHED = {"plan.md", "decisions.md"}
# docs/img and docs/brief hold the figures and the brief, with notes for people working on them.
UNPUBLISHED_FOLDERS = {"img", "brief"}
# Optional: order of the top-level pages, shorter navigation labels, the line
# on the landing-page card and group names. Pages not named here follow in
# alphabetical order.
ORDER = ["architecture", "dictionary-format", "performance", "tap", "deployment", "goat-integration", "related-projects"]
LABEL = {"tap": "Tap or mirror port", "goat-integration": "Zenoh data fabric", "related-projects": "Related projects",
         "README": "Overview"}
ABOUT = {
    "architecture": "The crates, the path from frame to record, sinks and transforms.",
    "dictionary-format": "Every key of a dictionary and everything bite check reports.",
    "performance": "The measured figures and the command behind each.",
    "tap": "Taps, mirror ports and tunnelled mirrors; capture interfaces; loss counters.",
    "deployment": "Which archive for which machine, the service, the container, offline builds.",
    "goat-integration": "Where records go on a goat data fabric.",
    "related-projects": "first-bite beside Open Billie and octo-pod.",
}
GROUP = {"goat": "goat network", "security": "Security"}


def fail(message):
    raise SystemExit(f"build-site: {message}")


def plain(text):
    return html.unescape(re.sub(r"<[^>]+>", "", text))


def slug(text):
    text = re.sub(r"[^\w\- ]", "", plain(text).lower())
    return text.strip().replace(" ", "-")


def discover():
    """Page keys ("tap", "goat/contract") in navigation order."""
    keys = []
    for folder, _, names in os.walk(DOCS):
        if os.path.relpath(folder, DOCS).split(os.sep)[0] in UNPUBLISHED_FOLDERS:
            continue
        for name in names:
            if name.endswith(".md") and name not in UNPUBLISHED:
                keys.append(os.path.relpath(os.path.join(folder, name), DOCS)[:-3].replace(os.sep, "/"))
    rank = {key: n for n, key in enumerate(ORDER)}
    return sorted(keys, key=lambda k: ("/" in k, k.split("/")[0] if "/" in k else "", rank.get(k, len(ORDER)), k))


def figure_alt(name):
    svg = open(os.path.join(DOCS, "img", f"{name}.svg"), encoding="utf-8").read()
    found = re.search(r"<desc[^>]*>(.*?)</desc>", svg, re.S)
    if not found:
        fail(f"docs/img/{name}.svg has no <desc>")
    return found.group(1)


def figure(name, caption=None, root=""):
    alt = html.escape(html.unescape(figure_alt(name)), quote=True)
    cap = f"<figcaption>{caption}</figcaption>" if caption else ""
    return (
        f'<figure><div class="fig" tabindex="0" role="group" aria-label="Illustration; scrolls sideways on a narrow screen">'
        f'<picture><source media="(prefers-color-scheme: dark)" srcset="{root}img/{name}-dark.svg">'
        f'<img src="{root}img/{name}.svg" alt="{alt}"></picture></div>{cap}</figure>')


# -- Markdown ---------------------------------------------------------------
class Page:
    def __init__(self, key, pages):
        self.key, self.pages = key, pages
        self.folder = os.path.dirname(key)
        self.root = "../" * key.count("/")

    def target(self, url):
        """Where a link in this page's Markdown points on the site."""
        if re.match(r"^(https?:|mailto:|#)", url):
            return url
        path, _, fragment = url.partition("#")
        fragment = "#" + fragment if fragment else ""
        full = os.path.normpath(os.path.join(DOCS, self.folder, path))
        inside = os.path.relpath(full, ROOT).replace(os.sep, "/")
        if inside.startswith(".."):
            fail(f"docs/{self.key}.md links outside the repository: {url}")
        if inside.startswith("docs/") and inside.endswith(".md") and inside[5:-3] in self.pages:
            return os.path.relpath(inside[5:-3] + ".html", self.folder or ".").replace(os.sep, "/") + fragment
        if not os.path.exists(full):
            fail(f"docs/{self.key}.md links to {url}, which does not exist")
        kind = "tree" if os.path.isdir(full) else "blob"
        return f"{REPO}/{kind}/main/{inside}{fragment}"

    def image(self, alt, src):
        full = os.path.normpath(os.path.join(DOCS, self.folder, src))
        if os.path.dirname(full) != os.path.join(DOCS, "img") or not os.path.exists(full):
            fail(f"docs/{self.key}.md shows {src}; only images in docs/img are published")
        return f'<img src="{self.root}img/{os.path.basename(full)}" alt="{html.escape(alt, quote=True)}">'

    def inline(self, text):
        spans = []

        def keep(match):
            spans.append("<code>" + html.escape(match.group(1), quote=False) + "</code>")
            return f"\x00{len(spans) - 1}\x00"

        text = re.sub(r"`([^`]+)`", keep, text)
        text = html.escape(text, quote=False)
        text = re.sub(r"!\[([^\]]*)\]\(([^)\s]+)\)", lambda m: self.image(m.group(1), m.group(2)), text)
        text = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)",
                      lambda m: f'<a href="{html.escape(self.target(m.group(2)), quote=True)}">{m.group(1)}</a>', text)
        text = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
        text = re.sub(r"(?<![*\w])\*([^*\s][^*]*)\*(?![*\w])", r"<em>\1</em>", text)
        return re.sub(r"\x00(\d+)\x00", lambda m: spans[int(m.group(1))], text)

    def list(self, lines):
        """lines: a list block with its common indent removed."""
        ordered = lines[0][0].isdigit()
        items = []
        for line in lines:
            if re.match(r"^(\d+\.|[-*]) ", line):
                items.append([line.split(" ", 1)[1]])
            else:
                items[-1].append(line)
        out = []
        for item in items:
            text, nested = [item[0]], []
            for extra in item[1:]:
                stripped = extra.strip()
                if nested or re.match(r"^(\d+\.|[-*]) ", stripped):
                    nested.append(extra)
                elif stripped:
                    text.append(stripped)
            sub = ""
            if nested:
                indent = min(len(n) - len(n.lstrip()) for n in nested if n.strip())
                sub = self.list([n[indent:] for n in nested if n.strip()])
            out.append(f"<li>{self.inline(' '.join(text))}{sub}</li>")
        tag = "ol" if ordered else "ul"
        return f"<{tag}>{''.join(out)}</{tag}>"

    def markdown(self, source):
        """Return ([html blocks], title, [(id, text) for each h2])."""
        cells = lambda row: [c.strip() for c in row.strip().strip("|").split("|")]
        starts = r"^(#|```|\||>|(\d+\.|[-*]) )"
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
                if lang == "mermaid":
                    fail(f"docs/{self.key}.md line {i + 1}: a mermaid block needs a script to render; "
                         "use a figure from docs/img in a <picture> block")
                body = html.escape("\n".join(lines[i + 1:j]), quote=False)
                out.append(f'<pre tabindex="0"><code>{body}</code></pre>')
                i = j + 1
            elif line.startswith("#"):
                level = len(line) - len(line.lstrip("#"))
                text = self.inline(line[level:].strip())
                if level == 1:
                    title = plain(text)
                    out.append(f"<h1>{text}</h1>")
                else:
                    ident = slug(text)
                    if level == 2:
                        toc.append((ident, plain(text)))
                    out.append(f'<h{level} id="{ident}">{text}</h{level}>')
                i += 1
            elif re.match(r"^(-{3,}|\*{3,})\s*$", line):
                out.append("<hr>")
                i += 1
            elif line.startswith("|") and i + 1 < len(lines) and re.match(r"^\|[\s:|-]+\|$", lines[i + 1].strip()):
                j = i + 2
                while j < len(lines) and lines[j].startswith("|"):
                    j += 1
                table = ['<div class="table" tabindex="0" role="group" aria-label="Table; scrolls sideways on a narrow screen"><table><thead><tr>']
                table += [f"<th>{self.inline(c)}</th>" for c in cells(line)]
                table.append("</tr></thead><tbody>")
                for row in lines[i + 2:j]:
                    table.append("<tr>" + "".join(f"<td>{self.inline(c)}</td>" for c in cells(row)) + "</tr>")
                table.append("</tbody></table></div>")
                out.append("".join(table))
                i = j
            elif line.startswith(">"):
                j = i
                while j < len(lines) and lines[j].startswith(">"):
                    j += 1
                quote = " ".join(part[1:].strip() for part in lines[i:j])
                out.append(f"<blockquote><p>{self.inline(quote)}</p></blockquote>")
                i = j
            elif re.match(r"^(\d+\.|[-*]) ", line):
                j = i
                while j < len(lines) and (re.match(r"^(\d+\.|[-*]) ", lines[j]) or lines[j].startswith("  ")):
                    j += 1
                out.append(self.list(lines[i:j]))
                i = j
            elif line.strip() == "<picture>":
                # The light and dark pair of a figure in docs/img, as GitHub shows it.
                j = i
                while j < len(lines) and lines[j].strip() != "</picture>":
                    j += 1
                found = re.search(r'<img\b[^>]*\bsrc="([^"]+)"', "\n".join(lines[i:j]))
                if j == len(lines) or not found:
                    fail(f"docs/{self.key}.md line {i + 1}: a <picture> block with no <img src> or no end")
                full = os.path.normpath(os.path.join(DOCS, self.folder, found.group(1)))
                name = os.path.basename(full)[:-4]
                if os.path.dirname(full) != os.path.join(DOCS, "img") or not os.path.exists(full[:-4] + "-dark.svg"):
                    fail(f"docs/{self.key}.md line {i + 1}: {found.group(1)} is not a light and dark figure in docs/img")
                out.append(figure(name, root=self.root))
                i = j + 1
            elif line.startswith(("<", "    ")):
                fail(f"docs/{self.key}.md line {i + 1}: construct not handled: {line[:40]!r}")
            else:
                j = i
                while j < len(lines) and lines[j].strip() and not re.match(starts, lines[j]):
                    j += 1
                out.append("<p>" + self.inline(" ".join(part.strip() for part in lines[i:j])) + "</p>")
                i = j
        if not title:
            fail(f"docs/{self.key}.md has no title")
        return out, title, toc


# -- pages ------------------------------------------------------------------
def block(text, name):
    found = re.search(rf"<!-- {name} -->(.*?)<!-- /{name} -->", text, re.S)
    if not found:
        fail(f"site/index.html has no <!-- {name} --> block")
    return found.group(1).strip()


def rebase(fragment, root):
    """Point the relative links of the shared header and footer at the site root."""
    return re.sub(r'\b(href|src|srcset)="(?!https?:|mailto:|#)', lambda m: f'{m.group(1)}="{root}', fragment)


def first_sentence(blocks):
    for part in blocks:
        if part.startswith("<p>"):
            text = plain(part)
            cut = re.search(r"\.(\s|$)", text[60:])
            text = text[:60 + cut.start() + 1] if cut else text
            return text if len(text) <= 150 else text[:147].rsplit(" ", 1)[0] + "..."
    return ""


def build(out_dir):
    site = os.path.join(ROOT, "site")
    if os.path.exists(out_dir):
        shutil.rmtree(out_dir)
    shutil.copytree(site, out_dir, ignore=lambda _, names: [n for n in names if n.startswith("_")])

    img = os.path.join(out_dir, "img")
    os.makedirs(img, exist_ok=True)
    for name in sorted(os.listdir(os.path.join(DOCS, "img"))):
        if name.endswith(".svg"):
            shutil.copy(os.path.join(DOCS, "img", name), img)
    for source, name in (("first-bite.svg", "icon.svg"), ("first-bite-light.svg", "mark-light.svg"),
                         ("first-bite-dark.svg", "mark-dark.svg"),
                         ("first-bite-wordmark-light.svg", "wordmark-light.svg"),
                         ("first-bite-wordmark-dark.svg", "wordmark-dark.svg")):
        shutil.copy(os.path.join(ROOT, "icon", source), os.path.join(img, name))
    brief = os.path.join(DOCS, "brief", "first-bite-brief.pdf")
    if os.path.exists(brief):
        shutil.copy(brief, out_dir)

    index_path = os.path.join(out_dir, "index.html")
    index = open(index_path, encoding="utf-8").read()
    header, footer = block(index, "header"), block(index, "footer")
    template = open(os.path.join(site, "_page.html"), encoding="utf-8").read()

    keys = discover()
    known = set(keys)
    parsed = {}
    for key in keys:
        page = Page(key, known)
        source = open(os.path.join(DOCS, key + ".md"), encoding="utf-8").read()
        body, title, toc = page.markdown(source)
        parsed[key] = (page, body, title, toc)
    label = {key: LABEL.get(key, parsed[key][2]) for key in keys}
    about = {key: ABOUT.get(key) or first_sentence(parsed[key][1]) for key in keys}
    group_name = lambda key: GROUP.get(key.split("/")[0], key.split("/")[0].replace("-", " ").capitalize())

    # The landing page lists the documentation pages and shows each figure.
    cards = "".join(
        f'<a class="card" href="{key}.html"><strong>{html.escape(label[key])}</strong><span>{html.escape(about[key])}</span></a>'
        for key in keys)
    index = index.replace("<!-- docs-cards -->", cards)
    index = re.sub(r"<!-- figure: ([a-z-]+)(?: \| (.*?))? -->", lambda m: figure(m.group(1), m.group(2)), index)
    with open(index_path, "w", encoding="utf-8") as fh:
        fh.write(index)

    for k, key in enumerate(keys):
        page, body, title, toc = parsed[key]
        here = page.folder or "."
        rel = lambda other: os.path.relpath(other + ".html", here).replace(os.sep, "/")
        nav, group = [], None
        for other in keys:
            if "/" in other and group_name(other) != group:
                group = group_name(other)
                nav.append(f'<li class="group">{html.escape(group)}</li>')
            current = ' aria-current="page"' if other == key else ""
            nav.append(f'<li><a href="{rel(other)}"{current}>{html.escape(label[other])}</a></li>')
        contents = "".join(f'<li><a href="#{i}">{html.escape(t)}</a></li>' for i, t in toc)
        prev_next = ""
        if k > 0:
            prev_next += f'<a class="prev" href="{rel(keys[k - 1])}"><span>Previous</span>{html.escape(label[keys[k - 1]])}</a>'
        if k + 1 < len(keys):
            prev_next += f'<a class="next" href="{rel(keys[k + 1])}"><span>Next</span>{html.escape(label[keys[k + 1]])}</a>'
        text = template
        for name, value in (("title", html.escape(title)), ("description", html.escape(about[key], quote=True)),
                            ("header", rebase(header, page.root)), ("footer", rebase(footer, page.root)),
                            ("root", page.root), ("nav", "".join(nav)), ("contents", contents),
                            ("content", "\n".join(body)), ("prev_next", prev_next),
                            ("source", f"{REPO}/blob/main/docs/{key}.md")):
            text = text.replace("{{" + name + "}}", value)
        if "{{" in re.sub(r"<pre.*?</pre>|<code>.*?</code>", "", text, flags=re.S):
            fail(f"unfilled placeholder in the page for {key}")
        path = os.path.join(out_dir, key + ".html")
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text)

    # Nothing may load from another host.
    for folder, _, names in os.walk(out_dir):
        for name in names:
            if name.endswith(".html"):
                text = open(os.path.join(folder, name), encoding="utf-8").read()
                remote = re.findall(r'<(?:script|link|img|source|iframe)\b[^>]*(?:src|href|srcset)="(https?:[^"]+)"', text)
                if remote:
                    fail(f"{os.path.relpath(os.path.join(folder, name), out_dir)} loads from another host: {remote[0]}")
    print(f"site built in {os.path.relpath(out_dir, ROOT)}: index.html and {len(keys)} pages")
    if not os.path.exists(os.path.join(site, "decoder", "index.html")):
        print("decoder: site/decoder is absent, so the decoder/ links lead nowhere")
    elif not os.path.isdir(os.path.join(site, "pkg")):
        print("decoder: site/pkg is absent; the interactive decoder page is copied but needs `make wasm` to work")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--out", default=os.path.join(ROOT, "site", "_build"))
    build(os.path.abspath(parser.parse_args().out))
    sys.exit(0)
