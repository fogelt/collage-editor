#!/usr/bin/env python3
"""Draw the Collage Editor app icon and export PNG/ICO sizes.

The icon is a small collage board with three photo tiles on a deep navy-to-indigo
gradient and an "add photos" badge. It is rendered at 4x and downsampled with
Lanczos so the edges stay crisp at tray and taskbar sizes.

Usage:
    py tools/make_icon.py
    py tools/make_icon.py --output-dir assets/icons

Requires Pillow:
    py -m pip install pillow
"""

from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

# Render at 4x, then downsample once at the end.
SCALE = 4
SIZE = 1024
CANVAS = SIZE * SCALE

NAVY = (10, 14, 24, 255)
INDIGO = (43, 58, 143, 255)
ACCENT = (96, 130, 255, 255)
CARD = (247, 249, 255, 255)
WHITE = (255, 255, 255, 255)


def mix(a: tuple[int, int, int, int], b: tuple[int, int, int, int], t: float):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def vertical_gradient(width: int, height: int, top, bottom) -> Image.Image:
    strip = Image.new("RGBA", (1, height))
    pixels = strip.load()
    for y in range(height):
        pixels[0, y] = mix(top, bottom, y / max(height - 1, 1))
    return strip.resize((width, height), Image.Resampling.BILINEAR)


def rounded_mask(width: int, height: int, radius: int) -> Image.Image:
    mask = Image.new("L", (width, height), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, width - 1, height - 1), radius=radius, fill=255
    )
    return mask


def draw_photo_tile(
    width: int,
    height: int,
    sky_top,
    sky_bottom,
    sun,
    mountain_back,
    mountain_front,
) -> Image.Image:
    tile = vertical_gradient(width, height, sky_top, sky_bottom)
    draw = ImageDraw.Draw(tile)

    sun_r = min(width, height) * 0.13
    sun_cx = width * 0.72
    sun_cy = height * 0.26
    draw.ellipse(
        (sun_cx - sun_r, sun_cy - sun_r, sun_cx + sun_r, sun_cy + sun_r),
        fill=sun,
    )

    draw.polygon(
        [
            (0, height * 0.78),
            (width * 0.27, height * 0.42),
            (width * 0.47, height * 0.66),
            (width * 0.63, height * 0.38),
            (width, height * 0.76),
            (width, height),
            (0, height),
        ],
        fill=mountain_back,
    )
    draw.polygon(
        [
            (0, height * 0.9),
            (width * 0.36, height * 0.57),
            (width * 0.58, height * 0.78),
            (width * 0.8, height * 0.54),
            (width, height * 0.88),
            (width, height),
            (0, height),
        ],
        fill=mountain_front,
    )

    return tile


def draw_icon() -> Image.Image:
    background = vertical_gradient(CANVAS, CANVAS, NAVY, INDIGO)
    mask = rounded_mask(CANVAS, CANVAS, radius=round(CANVAS * 0.225))
    icon = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    icon.paste(background, (0, 0), mask)

    # Soft glow behind the board.
    glow = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    glow_draw = ImageDraw.Draw(glow)
    glow_draw.ellipse(
        (CANVAS * 0.12, CANVAS * 0.1, CANVAS * 0.9, CANVAS * 0.92),
        fill=(96, 130, 255, 110),
    )
    glow = glow.filter(ImageFilter.GaussianBlur(CANVAS * 0.09))
    icon = Image.alpha_composite(icon, glow)

    board_box = (
        round(CANVAS * 0.185),
        round(CANVAS * 0.16),
        round(CANVAS * 0.815),
        round(CANVAS * 0.78),
    )
    board_radius = round(CANVAS * 0.052)

    # Drop shadow under the board.
    shadow = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    shadow_draw = ImageDraw.Draw(shadow)
    shadow_draw.rounded_rectangle(
        (
            board_box[0] + CANVAS * 0.015,
            board_box[1] + CANVAS * 0.025,
            board_box[2] + CANVAS * 0.015,
            board_box[3] + CANVAS * 0.025,
        ),
        radius=board_radius,
        fill=(0, 0, 0, 150),
    )
    shadow = shadow.filter(ImageFilter.GaussianBlur(CANVAS * 0.025))
    icon = Image.alpha_composite(icon, shadow)

    # White collage board.
    board = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    ImageDraw.Draw(board).rounded_rectangle(
        board_box, radius=board_radius, fill=CARD
    )
    icon = Image.alpha_composite(icon, board)

    inset = round(CANVAS * 0.038)
    gap = round(CANVAS * 0.026)
    left = board_box[0] + inset
    top = board_box[1] + inset
    right = board_box[2] - inset
    bottom = board_box[3] - inset

    inner_width = right - left
    inner_height = bottom - top
    tall_width = round(inner_width * 0.54)
    side_width = inner_width - tall_width - gap
    half_height = (inner_height - gap) // 2
    tile_radius = round(CANVAS * 0.026)

    tiles = [
        (
            (left, top, left + tall_width, bottom),
            (
                (255, 170, 92, 255),
                (255, 231, 184, 255),
                (255, 214, 92, 255),
                (112, 62, 80, 255),
                (66, 45, 82, 255),
            ),
        ),
        (
            (left + tall_width + gap, top, right, top + half_height),
            (
                (104, 180, 255, 255),
                (218, 239, 255, 255),
                (255, 214, 82, 255),
                (36, 94, 136, 255),
                (68, 142, 178, 255),
            ),
        ),
        (
            (left + tall_width + gap, top + half_height + gap, right, bottom),
            (
                (132, 218, 196, 255),
                (229, 247, 220, 255),
                (255, 226, 116, 255),
                (36, 110, 82, 255),
                (66, 158, 110, 255),
            ),
        ),
    ]

    for box, palette in tiles:
        width = box[2] - box[0]
        height = box[3] - box[1]
        tile = draw_photo_tile(width, height, *palette)
        tile_mask = rounded_mask(width, height, radius=tile_radius)
        layer = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
        layer.paste(tile, (box[0], box[1]), tile_mask)
        icon = Image.alpha_composite(icon, layer)

    # Add-photos badge in the bottom-right corner.
    badge_r = CANVAS * 0.125
    badge_cx = board_box[2] - CANVAS * 0.01
    badge_cy = board_box[3] - CANVAS * 0.005
    badge = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    badge_draw = ImageDraw.Draw(badge)
    badge_draw.ellipse(
        (
            badge_cx - badge_r,
            badge_cy - badge_r,
            badge_cx + badge_r,
            badge_cy + badge_r,
        ),
        fill=ACCENT,
        outline=WHITE,
        width=round(CANVAS * 0.012),
    )

    plus_width = round(CANVAS * 0.024)
    plus_length = badge_r * 0.95
    badge_draw.line(
        [
            (badge_cx - plus_length / 2, badge_cy),
            (badge_cx + plus_length / 2, badge_cy),
        ],
        fill=WHITE,
        width=plus_width,
    )
    badge_draw.line(
        [
            (badge_cx, badge_cy - plus_length / 2),
            (badge_cx, badge_cy + plus_length / 2),
        ],
        fill=WHITE,
        width=plus_width,
    )

    icon = Image.alpha_composite(icon, badge)
    return icon.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "assets" / "icons",
        help="Where to write the generated icons (default: assets/icons)",
    )
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    icon = draw_icon()

    icon.save(args.output_dir / "icon.png")
    for edge in (512, 256, 128, 64, 32):
        icon.resize((edge, edge), Image.Resampling.LANCZOS).save(
            args.output_dir / f"icon-{edge}.png"
        )

    icon.save(
        args.output_dir / "icon.ico",
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )

    print(f"Wrote icons to {args.output_dir}")


if __name__ == "__main__":
    main()
