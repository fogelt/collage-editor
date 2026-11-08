#!/usr/bin/env python3
"""Generate the small English and Swedish flag images used by the language switch."""

from pathlib import Path

from PIL import Image, ImageDraw

SCALE = 8
WIDTH = 48
HEIGHT = 32
CANVAS = (WIDTH * SCALE, HEIGHT * SCALE)

UK_BLUE = (1, 33, 105, 255)
UK_WHITE = (255, 255, 255, 255)
UK_RED = (200, 16, 46, 255)
SE_BLUE = (0, 106, 167, 255)
SE_YELLOW = (254, 204, 0, 255)


def draw_uk() -> Image.Image:
    w, h = CANVAS
    image = Image.new("RGBA", CANVAS, UK_BLUE)
    draw = ImageDraw.Draw(image)

    # St Patrick / Union Jack diagonals.
    draw.line((0, 0, w, h), fill=UK_WHITE, width=round(h * 0.22))
    draw.line((w, 0, 0, h), fill=UK_WHITE, width=round(h * 0.22))
    draw.line((0, 0, w, h), fill=UK_RED, width=round(h * 0.09))
    draw.line((w, 0, 0, h), fill=UK_RED, width=round(h * 0.09))

    # White and red crosses.
    white_v = round(w * 0.20)
    white_h = round(h * 0.30)
    red_v = round(w * 0.12)
    red_h = round(h * 0.18)
    draw.rectangle((w // 2 - white_v, 0, w // 2 + white_v, h), fill=UK_WHITE)
    draw.rectangle((0, h // 2 - white_h, w, h // 2 + white_h), fill=UK_WHITE)
    draw.rectangle((w // 2 - red_v, 0, w // 2 + red_v, h), fill=UK_RED)
    draw.rectangle((0, h // 2 - red_h, w, h // 2 + red_h), fill=UK_RED)

    return image.resize((WIDTH, HEIGHT), Image.Resampling.LANCZOS)


def draw_sweden() -> Image.Image:
    w, h = CANVAS
    image = Image.new("RGBA", CANVAS, SE_BLUE)
    draw = ImageDraw.Draw(image)

    vertical_x = round(w * 0.30)
    vertical_w = round(w * 0.12)
    horizontal_y = round(h * 0.50)
    horizontal_h = round(h * 0.18)

    draw.rectangle(
        (vertical_x - vertical_w // 2, 0, vertical_x + vertical_w // 2, h),
        fill=SE_YELLOW,
    )
    draw.rectangle(
        (0, horizontal_y - horizontal_h // 2, w, horizontal_y + horizontal_h // 2),
        fill=SE_YELLOW,
    )

    return image.resize((WIDTH, HEIGHT), Image.Resampling.LANCZOS)


def main() -> None:
    output = Path(__file__).resolve().parent.parent / "assets" / "flags"
    output.mkdir(parents=True, exist_ok=True)
    draw_uk().save(output / "en.png")
    draw_sweden().save(output / "sv.png")
    print(f"Wrote flags to {output}")


if __name__ == "__main__":
    main()
