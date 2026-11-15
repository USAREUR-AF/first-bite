# Logo

Two fangs biting into a byte while it is still on the wire: first-bite reads
every message before it reaches its addressee, and takes nothing from it. The
eight bits read `01100010`, which is `0x62`, the letter `b`.

| File | Use |
|---|---|
| `first-bite.svg` | the mark; square, legible down to about 32 px |
| `first-bite-wordmark.svg` | the mark with the name and the line "the first bite of every byte, before it reaches its addressee" |
| `first-bite.png` | 256 px raster of the mark |

Both SVGs carry their own dark background, so they look the same on a light or
a dark page. Colours: background `#10161f`, fangs `#f4f6f9`, bits `#9db0c6`,
wire `#6f8196`, drop `#f04438`.

To regenerate the PNG:

```bash
chrome --headless=new --default-background-color=00000000 \
    --screenshot=icon/first-bite.png --window-size=256,256 file://$PWD/icon/first-bite.svg
```
