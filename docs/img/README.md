# Illustrations

The figures used by the README, the documentation site and the PDF brief.
Each is a standalone SVG (no external CSS, fonts or scripts) in two files:
`<name>.svg` for a light page and `<name>-dark.svg` for a dark one.

| Figure | Shows |
|---|---|
| `how-it-works` | two native systems on a link, a tap, first-bite decoding the copy, records fanning out to consumers |
| `tap-placement` | breakout tap, aggregating tap and mirror port: loss and directions for each |
| `frame-to-record` | the 23 bytes of the weather-station `Observation` vector, the bit range and dictionary entry of each field, and the decoded values |
| `components` | the six crates and what crosses each boundary |
| `performance` | the measured figures of [`../performance.md`](../performance.md) |
| `one-decode-many-consumers` | point-to-point integrations against one tap and publish/subscribe |
| `beyond-ip` | what is built, designed and not built for IP, CAN and other buses |
| `record-flow` | publishers, subscribers with topic filters, sinks, and a subpub republishing on its own socket |
| `loss-counters` | the stages a frame passes through and the three counters that say where one was lost |
| `platforms` | which archive runs where, from [`../deployment.md`](../deployment.md) |

## Regenerate and preview

`build.py` writes all twenty files. It needs Python 3 and nothing else.

```bash
python3 docs/img/build.py            # write the figures
python3 docs/img/build.py --check    # fail if a committed figure is out of date
make site && python3 -m http.server -d site/_build 8000   # see them in the site
```

The figures are generated so that numbers come from one place: the bit ranges
and values in `frame-to-record` are read from
`dictionaries/weather-station/WX/messages/Observation.json` and decoded in
`build.py` independently of `bite`, and the bars in `performance` are computed
from the constants at the top of that section, which carry the figures of
`docs/performance.md`. Change the source, run the script, commit both.

Every label is measured against `font-metrics.json` before it is placed, and
the script stops if a label does not fit its box or leaves the canvas.
`font-metrics.json` holds, per character, the widest advance across Helvetica
Neue, Helvetica and Arial, in thousandths of an em.

## Visual system

| | Value |
|---|---|
| Canvas | 760 units wide; height as the content needs; a 12-radius card with a hairline border |
| Type | `"Helvetica Neue", Helvetica, Arial, "Liberation Sans", sans-serif`; code and bytes in `Menlo, Consolas, "Liberation Mono", "Courier New", monospace` |
| Sizes | title 18 bold, box heading 14 or 15 bold, labels and detail 13; nothing below 13 |
| Corner radius | boxes 8, small devices 6, zones and the card 12, bars 4 |
| Lines | outlines and neutral connectors 1.25; data flow 2 in the accent; a native link 3 in ink; hairlines 1 |
| Arrowheads | filled triangles 10 long and 10 wide, drawn as paths, tip on the edge of the target |

| Mark | Means |
|---|---|
| accent outline on an accent tint | first-bite, or a part of it |
| plain outline on paper | a system, a step or a consumer that is not first-bite |
| thick ink line | the native link |
| solid accent arrow | data flowing through first-bite |
| dashed outline, dashed arrow | data read at start (the dictionary), or something designed and not built |
| dotted outline on grey | not built |
| green word | built; or the native path, which is left as it was |
| amber word | designed, built but not run, or a cost |

Status is always written as a word as well as drawn. In `frame-to-record`
alone the four tints identify fields, with a number on each, and carry no
status.

| Role | Light | Dark |
|---|---|---|
| ink, quiet | `#16191f`, `#515a66` | `#e6e9ee`, `#a3abb8` |
| paper, panel | `#ffffff`, `#f4f6f8` | `#0f1317`, `#171b21` |
| rule, line | `#dfe3e8`, `#7d8899` | `#2c333d`, `#7a8594` |
| accent, accent tint | `#1f4fbf`, `#e6edfb` | `#8fb0ff`, `#172544` |
| good, good tint | `#24703a`, `#e3f4e7` | `#6cc487`, `#172b1e` |
| warn, warn tint | `#8a5300`, `#fcefd6` | `#e3b04f`, `#31281a` |
| info, info tint | `#5b4bb0`, `#eeebfa` | `#b3a6f5`, `#221d3d` |
| mute, mute tint | `#5a606a`, `#eeeeec` | `#9aa2ad`, `#23262b` |

Text on its background is at least 4.5:1 and outlines at least 3:1 in both
themes. The site (`site/assets/site.css`) and the brief
(`docs/brief/brief.css`) use the same palette.

## Using a figure

GitHub shows an SVG only through an image tag. To follow the reader's theme:

```html
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/img/how-it-works-dark.svg">
  <img src="docs/img/how-it-works.svg" alt="…">
</picture>
```

Each file carries a `<title>` (the finding) and a `<desc>` (the whole message
in words); use the `<desc>` as the `alt` text.
