#!/usr/bin/env python3
"""Build Windows .cur fallbacks from the app's hand cursor PNGs.

The PNGs are Kenney's CC0 cursor pack (`hand_thin_open` and
`hand_thin_closed` from `PNG/Outline/Double`). The canvas draws the PNGs
directly; these .cur files are only a native fallback.

Usage:
    py tools/make_cursors.py

Requires Pillow:
    py -m pip install pillow
"""

from __future__ import annotations

import struct
from pathlib import Path

from PIL import Image

SIZE = 32


def save_cur(image: Image.Image, path: Path, hotspot: tuple[int, int]) -> None:
    image = image.convert("RGBA")
    width, height = image.size
    pixels = list(image.getdata())

    xor = bytearray()
    for y in range(height - 1, -1, -1):
        for x in range(width):
            red, green, blue, alpha = pixels[y * width + x]
            xor.extend((blue, green, red, alpha))

    row_bytes = ((width + 31) // 32) * 4
    and_mask = bytearray()
    for y in range(height - 1, -1, -1):
        row = bytearray(row_bytes)
        for x in range(width):
            if pixels[y * width + x][3] == 0:
                row[x // 8] |= 1 << (7 - (x % 8))
        and_mask.extend(row)

    bitmap_header = struct.pack(
        "<IiiHHIIiiII",
        40,
        width,
        height * 2,
        1,
        32,
        0,
        len(xor) + len(and_mask),
        0,
        0,
        0,
        0,
    )
    image_data = bitmap_header + bytes(xor) + bytes(and_mask)

    header = struct.pack("<HHH", 0, 2, 1)
    entry = struct.pack(
        "<BBBBHHII",
        width % 256,
        height % 256,
        0,
        0,
        hotspot[0],
        hotspot[1],
        len(image_data),
        6 + 16,
    )
    path.write_bytes(header + entry + image_data)


def main() -> None:
    assets = Path(__file__).resolve().parent.parent / "assets" / "cursors"

    cursors = [
        ("grab.png", "grab.cur", (15, 2)),
        ("grabbing.png", "grabbing.cur", (15, 6)),
    ]

    for source_name, target_name, hotspot in cursors:
        image = Image.open(assets / source_name).convert("RGBA")
        image = image.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
        save_cur(image, assets / target_name, hotspot)

    print(f"Wrote cursor fallbacks to {assets}")


if __name__ == "__main__":
    main()
