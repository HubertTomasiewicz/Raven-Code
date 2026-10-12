#!/usr/bin/env python3
"""Generates the Raven Pico Windows icon (multi-size .ico) and doc previews
from the brand SVG (white crow on dark green #026625).

Usage:  python3 scripts/make_icon.py

Inputs:  apps/raven-pico/assets/crow_icon.svg
Outputs: apps/raven-pico/assets/raven.ico
         docs/images/raven-icon-256.png
Requires: cairosvg + Pillow (pip install cairosvg pillow)
"""

from pathlib import Path

import cairosvg
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SVG = ROOT / "apps" / "raven-pico" / "assets" / "crow_icon.svg"
ICO = ROOT / "apps" / "raven-pico" / "assets" / "raven.ico"
PREVIEW = ROOT / "docs" / "images" / "raven-icon-256.png"

SIZES = [16, 24, 32, 48, 64, 128, 256]


def main() -> None:
    # Rasterize the master at 256px (RGBA) from the vector source.
    master_png = ICO.with_suffix(".tmp.png")
    cairosvg.svg2png(url=str(SVG), write_to=str(master_png), output_width=256, output_height=256)
    master = Image.open(master_png).convert("RGBA")

    # Pre-render each size from the vector for crisp small sizes, then pack
    # them into a single .ico (Pillow writes BMP entries + PNG for 256).
    rendered: dict[int, Image.Image] = {256: master}
    for size in SIZES[:-1]:
        tmp = ICO.with_suffix(f".tmp{size}.png")
        cairosvg.svg2png(url=str(SVG), write_to=str(tmp), output_width=size, output_height=size)
        rendered[size] = Image.open(tmp).convert("RGBA")
        tmp.unlink(missing_ok=True)

    master.save(
        ICO,
        format="ICO",
        sizes=[(size, size) for size in SIZES],
        append_images=[rendered[size] for size in SIZES[:-1]],
    )
    master_png.unlink(missing_ok=True)

    PREVIEW.parent.mkdir(parents=True, exist_ok=True)
    master.save(PREVIEW, format="PNG")

    print(f"wrote {ICO}")
    print(f"wrote {PREVIEW}")


if __name__ == "__main__":
    main()
