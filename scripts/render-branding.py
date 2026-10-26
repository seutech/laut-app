#!/usr/bin/env python3
"""Render the checked-in waveform identity to the raster assets used by app bundles."""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parent.parent
BRANDING = ROOT / "Resources" / "Branding"
TEAL = "#008F88"
INK = "#152A32"
WARM = "#FFFCF7"


def waveform(draw: ImageDraw.ImageDraw, box: tuple[int, int, int, int], tile: bool) -> None:
    left, top, right, bottom = box
    size = right - left
    if tile:
        draw.rounded_rectangle(box, radius=int(size * 0.269), fill=TEAL)
    heights = [0.28, 0.43, 0.59, 0.38, 0.23]
    center = (top + bottom) / 2
    bar_width = int(size * 0.053)
    gap = int(size * 0.060)
    total = 5 * bar_width + 4 * gap
    x = left + (size - total) // 2
    color = "white" if tile else "black"
    for height in heights:
        pixels = int(size * height)
        draw.rounded_rectangle(
            (x, int(center - pixels / 2), x + bar_width, int(center + pixels / 2)),
            radius=bar_width // 2,
            fill=color,
        )
        x += bar_width + gap


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    candidates = [
        Path("/System/Library/Fonts/SFNS.ttf"),
        Path("/System/Library/Fonts/SFNSDisplay.ttf"),
        Path("C:/Windows/Fonts/segoeuib.ttf" if bold else "C:/Windows/Fonts/segoeui.ttf"),
        Path("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf" if bold else "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    ]
    for path in candidates:
        if path.is_file():
            return ImageFont.truetype(path, size=size)
    return ImageFont.load_default(size=size)


def main() -> None:
    canvas = 1254
    icon = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    draw = ImageDraw.Draw(icon)
    waveform(draw, (115, 115, 1139, 1139), tile=True)
    icon.save(BRANDING / "app-icon.png")

    mark = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    waveform(ImageDraw.Draw(mark), (115, 115, 1139, 1139), tile=False)
    mark.save(BRANDING / "mark.png")

    wordmark = Image.new("RGB", (1600, 600), WARM)
    draw = ImageDraw.Draw(wordmark)
    waveform(draw, (140, 80, 580, 520), tile=True)
    draw.text((680, 145), "Laut", font=font(250, bold=True), fill=INK)
    wordmark.save(BRANDING / "wordmark.png", optimize=True)


if __name__ == "__main__":
    main()
