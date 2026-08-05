# Application icons

`tuxstack.png` at the repository root is the source application icon supplied for
TuxStack. The generated `hicolor/<size>x<size>/apps/` files use the same
freedesktop icon name referenced by:

- `packaging/desktop/io.github.tuxstack.TuxStack.desktop`
- `packaging/desktop/io.github.tuxstack.TuxStack.metainfo.xml`

Packaging should install each hicolor PNG below
`/usr/share/icons/hicolor/<size>x<size>/apps/` and then refresh the icon cache.
The GUI also embeds a 512×512 copy in Qt resources so development builds and
`cargo run -p tuxstack` have the same window icon without an installed icon
theme entry.

To regenerate the derived sizes from the source icon with ImageMagick:

```bash
for size in 16 22 24 32 48 64 128 256 512; do
  magick tuxstack.png \
    -resize "${size}x${size}" \
    "packaging/icons/hicolor/${size}x${size}/apps/io.github.tuxstack.TuxStack.png"
done
```
