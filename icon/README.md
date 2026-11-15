# icon

`first-bite.svg` is the source; `first-bite.png` is a 128 by 128 pixel
rendering of it. Regenerate the PNG after changing the SVG (needs ImageMagick):

```bash
magick -background none -size 128x128 icon/first-bite.svg icon/first-bite.png
```
