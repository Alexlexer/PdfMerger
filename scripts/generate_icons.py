#!/usr/bin/env python3
"""Generate PdfMerger application icons from a deterministic vector-style design."""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
ICON_DIR = ROOT / "assets" / "icons"
SOURCE_SVG = ROOT / "assets" / "icon.svg"
SIZES = (32, 64, 128, 256, 512, 1024)

SVG = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <rect x="40" y="40" width="944" height="944" rx="220" fill="#5e6ad2"/>
  <rect x="184" y="192" width="320" height="472" rx="42" fill="#dfe3ff"/>
  <path d="M406 192v108h98" fill="#b9c1ff"/>
  <rect x="520" y="192" width="320" height="472" rx="42" fill="#ffffff"/>
  <path d="M742 192v108h98" fill="#dfe3ff"/>
  <path d="M344 704l168 124 168-124" fill="none" stroke="#ffbd59" stroke-width="64" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M512 810v92" fill="none" stroke="#ffbd59" stroke-width="64" stroke-linecap="round"/>
</svg>
"""


def draw_icon(size: int) -> Image.Image:
    scale = 4
    canvas = size * scale
    image = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    def box(values):
        return tuple(round(value * canvas / 1024) for value in values)

    draw.rounded_rectangle(box((40, 40, 984, 984)), radius=round(220 * canvas / 1024), fill="#5e6ad2")
    draw.rounded_rectangle(box((184, 192, 504, 664)), radius=round(42 * canvas / 1024), fill="#dfe3ff")
    draw.polygon([box((406, 192, 406, 300))[0:2], box((504, 300, 504, 300))[0:2], box((504, 192, 504, 192))[0:2]], fill="#b9c1ff")
    draw.rounded_rectangle(box((520, 192, 840, 664)), radius=round(42 * canvas / 1024), fill="#ffffff")
    draw.polygon([box((742, 192, 742, 300))[0:2], box((840, 300, 840, 300))[0:2], box((840, 192, 840, 192))[0:2]], fill="#dfe3ff")

    stroke = max(1, round(64 * canvas / 1024))
    points = [box((344, 704, 344, 704))[0:2], box((512, 828, 512, 828))[0:2], box((680, 704, 680, 704))[0:2]]
    draw.line(points, fill="#ffbd59", width=stroke, joint="curve")
    radius = stroke // 2
    for x, y in points:
        draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill="#ffbd59")
    x, y1 = box((512, 810, 512, 810))[0:2]
    _, y2 = box((512, 902, 512, 902))[0:2]
    draw.line((x, y1, x, y2), fill="#ffbd59", width=stroke)
    draw.ellipse((x - radius, y2 - radius, x + radius, y2 + radius), fill="#ffbd59")

    return image.resize((size, size), Image.Resampling.LANCZOS)


def main() -> None:
    ICON_DIR.mkdir(parents=True, exist_ok=True)
    SOURCE_SVG.write_text(SVG, encoding="utf-8", newline="\n")
    images = {size: draw_icon(size) for size in SIZES}
    for size, image in images.items():
        image.save(ICON_DIR / f"icon-{size}.png", optimize=True)
    images[256].save(
        ICON_DIR / "icon.ico",
        format="ICO",
        sizes=[(size, size) for size in (16, 24, 32, 48, 64, 128, 256)],
    )
    images[1024].save(ICON_DIR / "icon.icns", format="ICNS")


if __name__ == "__main__":
    main()
