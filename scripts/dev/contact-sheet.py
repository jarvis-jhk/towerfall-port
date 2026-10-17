"""Tile PNG captures into one labelled contact sheet.

Usage: python -I scripts/dev/contact-sheet.py OUT.png [--cols N] DIR_OR_PNG...
Directories contribute their *.png files in name order; each tile is
labelled with its file name. Needs Pillow (tools/venv).
"""

import sys
from pathlib import Path

from PIL import Image, ImageDraw


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    out = Path(argv[0])
    args = argv[1:]
    cols = 3
    if args[:1] == ["--cols"]:
        cols = int(args[1])
        args = args[2:]
    files = []
    for a in args:
        p = Path(a)
        files.extend(sorted(p.glob("*.png")) if p.is_dir() else [p])
    if not files:
        print("no images")
        return 1
    tiles = [Image.open(f).convert("RGB") for f in files]
    w = max(t.width for t in tiles)
    h = max(t.height for t in tiles)
    label = 12
    rows = (len(tiles) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * (w + 4) + 4, rows * (h + label + 4) + 4), (24, 24, 32))
    d = ImageDraw.Draw(sheet)
    for i, (t, f) in enumerate(zip(tiles, files)):
        x = 4 + (i % cols) * (w + 4)
        y = 4 + (i // cols) * (h + label + 4)
        d.text((x + 2, y), f"{f.parent.name}/{f.stem}", fill=(230, 230, 230))
        sheet.paste(t, (x, y + label))
    out.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out)
    print(f"{out}: {len(tiles)} tiles")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
