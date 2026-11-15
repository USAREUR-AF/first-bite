# Logo

Two fangs biting into a byte while it is still on the wire: first-bite reads
every message before it reaches its addressee, and takes nothing from it. The
eight bits read `01100010`, which is `0x62`, the letter `b`.

| File | Use |
|---|---|
| `first-bite.svg` | the mark; square, legible down to about 32 px |
| `first-bite-wordmark.svg` | the mark with the name and the line "the first bite of every byte, before it reaches its addressee" |
| `first-bite-light.svg`, `first-bite-dark.svg` | the mark with no background, for a light page and for a dark page |
| `first-bite-wordmark-light.svg`, `first-bite-wordmark-dark.svg` | the wordmark with no background, for a light page and for a dark page |
| `first-bite.png` | 256 px raster of the mark |

`first-bite.svg` and `first-bite-wordmark.svg` carry their own dark badge with a
lighter edge. Where the page background is known, use the `-light` and `-dark`
pair; in Markdown that GitHub renders, switch between them with `<picture>`:

```html
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="./icon/first-bite-wordmark-dark.svg">
  <img alt="first-bite" src="./icon/first-bite-wordmark-light.svg" width="520">
</picture>
```

Badge colours: background `#10161f`, fangs `#f4f6f9`, bits `#9db0c6`,
wire `#6f8196`, drop `#f04438`.

To regenerate the PNG:

```bash
chrome --headless=new --default-background-color=00000000 \
    --screenshot=icon/first-bite.png --window-size=256,256 file://$PWD/icon/first-bite.svg
```
