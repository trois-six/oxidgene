# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow==12.3.0"]
# ///
"""Encode the screenshot captures and assemble the README carousel.

`just screenshots` runs the Playwright screenshot script, which saves raw
captures under target/screenshots/raw/, then this script:

- composes archive-register from the pieces the screenshot script leaves
  under target/screenshots/raw/parts/<language>/: the profile whose source
  was clicked, its link outlined in the theme's accent colour, and over it
  the archive portal's tab that link opened, its register already blurred
  by the screenshot script, drawn as a window with a title bar giving the
  portal's address and a drop shadow, an arrow leading from the link to it.
  The portal is live: when the screenshot script could not capture it, the
  committed image is kept;
- writes each capture to assets/screenshots/<name>.webp as a lossless WebP,
  the French ones to assets/screenshots/fr/<name>.webp for the
  documentation site:
  the very pixels of the capture, at about half the size of the same image
  as an optimised PNG (measured on these captures: 1.3 MB against 2.7 MB).
  A capture that differs from the committed image only by the rounding
  Chromium's image scaling sometimes varies by (a handful of pixels, one
  level apart) keeps the committed file, so a run over unchanged pages
  rewrites nothing;
- assembles the carousel frames, read back from those stills, into
  assets/screenshots/readme-carousel.webp, an animated WebP (GitHub renders it in a README, which runs no
  JavaScript), each frame shown for three seconds, resized to 1200 pixels
  wide and lossily compressed. There is no cross-fade: its blended frames
  each change the whole picture, which measured three times the size of
  plain cuts;
- removes the images of captures the screenshot script no longer takes.

Pillow is pinned: its bundled libwebp decides the output bytes, so the same
captures always give the same files.
"""

import json
import sys
from pathlib import Path
from urllib.parse import urlsplit

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
RAW = ROOT / "target" / "screenshots" / "raw"
PARTS = RAW / "parts"
OUT = ROOT / "assets" / "screenshots"
CAROUSEL_NAME = "readme-carousel"

# The carousel, in order: a tour of the application.
CAROUSEL = [
    "pedigree",
    "person",
    "events-timeline",
    "archive-register",
    "media-viewer",
    "fan-chart-dark",
    "couple",
    "statistics-map",
    "kinship",
    "search",
    "dictionary-sources",
    "dictionary-places",
    "tags",
    "media-library",
    "tools-ancestry",
    "history",
    "app-settings-dark",
]
CAROUSEL_WIDTH = 1200
HOLD_MS = 3000
CAROUSEL_QUALITY = 70
# What a re-run may differ by and still count as the same image.
NOISE_LEVEL = 2
NOISE_PIXELS = 200

# The composed image, and the languages the screenshot script captures it in
# (English in the folder's root, the others in a folder of their own).
COMPOSITE = "archive-register"
LANGUAGES = ("en", "fr")
# Its colours and typeface: the light theme's, in which the profile is taken.
THEME = json.loads((ROOT / "assets" / "themes" / "light.json").read_text(encoding="utf-8"))["colors"]
FONT = ROOT / "assets" / "fonts" / "lato-400-latin.woff2"
# The portal's window: its width once scaled down, the height of its title
# bar, its corners, and its distance to the image's right and bottom edges.
WINDOW_WIDTH = 820
TITLE_BAR = 34
CORNER = 10
MARGIN = 44
# Drawn this many times larger, then scaled down: smooth edges.
SUPERSAMPLE = 3


def same_picture(capture: Image.Image, target: Path) -> bool:
    """Whether the committed `target` shows `capture`, up to rendering noise."""
    if not target.exists():
        return False
    committed = Image.open(target).convert("RGB")
    if committed.size != capture.size:
        return False
    difference = ImageChops.difference(capture, committed)
    if max(high for _, high in difference.getextrema()) > NOISE_LEVEL:
        return False
    changed = difference.point(lambda level: 255 if level else 0).convert("1")
    return changed.histogram()[255] <= NOISE_PIXELS


def still(name: str, image: Image.Image) -> Path:
    target = OUT / f"{name}.webp"
    target.parent.mkdir(parents=True, exist_ok=True)
    if not same_picture(image, target):
        image.save(target, format="WEBP", lossless=True, quality=100, method=6)
    return target


def colour(name: str, alpha: int = 255) -> tuple[int, int, int, int]:
    value = THEME[name].lstrip("#")
    return (int(value[0:2], 16), int(value[2:4], 16), int(value[4:6], 16), alpha)


def rounded_mask(size: tuple[int, int], radius: int) -> Image.Image:
    """An antialiased mask of a rectangle of `size` with rounded corners."""
    width, height = size
    mask = Image.new("L", (width * SUPERSAMPLE, height * SUPERSAMPLE), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, width * SUPERSAMPLE - 1, height * SUPERSAMPLE - 1), radius * SUPERSAMPLE, fill=255
    )
    return mask.resize(size, Image.LANCZOS)


def address(url: str, font: ImageFont.FreeTypeFont, room: float) -> str:
    """The portal's address as a browser's address bar shows it: its host
    and path, cut with an ellipsis to fit in `room` pixels."""
    parts = urlsplit(url)
    text = f"{parts.hostname}{parts.path}"
    if font.getlength(text) <= room:
        return text
    while text and font.getlength(f"{text}…") > room:
        text = text[:-1]
    return f"{text}…"


def window(portal: Image.Image, url: str) -> Image.Image:
    """The portal's tab as a window: scaled down, under a title bar giving
    its address, with rounded corners and a hairline border."""
    height = round(portal.height * WINDOW_WIDTH / portal.width)
    size = (WINDOW_WIDTH, TITLE_BAR + height)
    frame = Image.new("RGBA", size, colour("bg-panel"))
    frame.paste(portal.convert("RGB").resize((WINDOW_WIDTH, height), Image.LANCZOS), (0, TITLE_BAR))
    draw = ImageDraw.Draw(frame)
    draw.line((0, TITLE_BAR - 1, WINDOW_WIDTH, TITLE_BAR - 1), fill=colour("border"))
    for index in range(3):
        centre = 18 + index * 18
        draw.ellipse((centre - 6, TITLE_BAR / 2 - 6, centre + 6, TITLE_BAR / 2 + 6), fill=colour("border"))
    field = (76, 6, WINDOW_WIDTH - 16, TITLE_BAR - 7)
    draw.rounded_rectangle(field, 6, fill=colour("bg-card"), outline=colour("border"))
    font = ImageFont.truetype(str(FONT), 13)
    room = field[2] - field[0] - 24
    draw.text((field[0] + 12, (field[1] + field[3]) / 2), address(url, font, room), font=font, fill=colour("text-secondary"), anchor="lm")
    frame.putalpha(rounded_mask(size, CORNER))
    border = Image.new("RGBA", (size[0] * SUPERSAMPLE, size[1] * SUPERSAMPLE), (0, 0, 0, 0))
    ImageDraw.Draw(border).rounded_rectangle(
        (0, 0, border.width - 1, border.height - 1), CORNER * SUPERSAMPLE, outline=colour("connector", 160), width=SUPERSAMPLE
    )
    return Image.alpha_composite(frame, border.resize(size, Image.LANCZOS))


def bezier(start: tuple[float, float], control: tuple[float, float], end: tuple[float, float], steps: int = 48) -> list[tuple[float, float]]:
    points = []
    for step in range(steps + 1):
        t = step / steps
        points.append(tuple((1 - t) ** 2 * a + 2 * (1 - t) * t * b + t**2 * c for a, b, c in zip(start, control, end)))
    return points


def archive_register(folder: Path) -> Image.Image | None:
    """The composite of `folder`'s pieces, or None when the screenshot
    script captured no portal there."""
    if not (folder / "archive-register.json").exists():
        return None
    meta = json.loads((folder / "archive-register.json").read_text(encoding="utf-8"))
    app = Image.open(folder / "archive-register-app.png").convert("RGBA")
    portal = window(Image.open(folder / "archive-register-portal.png"), meta["url"])
    link = meta["link"]
    left, top = link["x"], link["y"]
    right, bottom = left + link["width"], top + link["height"]
    x0, y0 = app.width - portal.width - MARGIN, app.height - portal.height - MARGIN
    if bottom + 24 > y0:
        print(f"{COMPOSITE}: the window covers the link; move the link higher", file=sys.stderr)

    # The link that was clicked, outlined in the accent colour.
    scale = SUPERSAMPLE
    marks = Image.new("RGBA", (app.width * scale, app.height * scale), (0, 0, 0, 0))
    draw = ImageDraw.Draw(marks)
    draw.rounded_rectangle(
        ((left - 4) * scale, (top - 4) * scale, (right + 5) * scale, (bottom + 4) * scale),
        6 * scale, fill=colour("orange", 28), outline=colour("orange"), width=3 * scale,
    )
    composite = Image.alpha_composite(app, marks.resize(app.size, Image.LANCZOS))

    # The window over it, with its shadow.
    shadow = Image.new("RGBA", app.size, (0, 0, 0, 0))
    shade = Image.new("RGBA", portal.size, colour("shadow", 0))
    shade.putalpha(rounded_mask(portal.size, CORNER).point(lambda level: level * 0.32))
    shadow.paste(shade, (x0, y0 + 14))
    composite = Image.alpha_composite(composite, shadow.filter(ImageFilter.GaussianBlur(22)))
    composite.alpha_composite(portal, (x0, y0))

    # The arrow from the link to the window: out of the link's right end,
    # then down onto the window's title bar.
    start = (right + 14, (top + bottom) / 2)
    tip = (min(max(right + 160, x0 + 120), x0 + portal.width - 120), y0 - 6)
    head = 18
    end = (tip[0], tip[1] - head + 4)
    path = [(x * scale, y * scale) for x, y in bezier(start, (tip[0], start[1]), end)]
    arrow = Image.new("RGBA", marks.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(arrow)
    draw.line(path, fill=colour("orange"), width=4 * scale, joint="curve")
    radius = 2 * scale
    draw.ellipse((path[0][0] - radius * 2, path[0][1] - radius * 2, path[0][0] + radius * 2, path[0][1] + radius * 2), fill=colour("orange"))
    draw.polygon(
        [((tip[0] - 10) * scale, (tip[1] - head) * scale), ((tip[0] + 10) * scale, (tip[1] - head) * scale), (tip[0] * scale, tip[1] * scale)],
        fill=colour("orange"),
    )
    composite = Image.alpha_composite(composite, arrow.resize(app.size, Image.LANCZOS))
    return composite.convert("RGB")


def carousel() -> Path:
    frames = []
    for name in CAROUSEL:
        image = Image.open(OUT / f"{name}.webp").convert("RGB")
        height = round(image.height * CAROUSEL_WIDTH / image.width)
        frames.append(image.resize((CAROUSEL_WIDTH, height), Image.LANCZOS))
    target = OUT / f"{CAROUSEL_NAME}.webp"
    frames[0].save(
        target,
        format="WEBP",
        save_all=True,
        append_images=frames[1:],
        duration=HOLD_MS,
        loop=0,
        quality=CAROUSEL_QUALITY,
        method=6,
        minimize_size=True,
    )
    return target


def main() -> int:
    # `name`, or `fr/name` for a French capture; the composite's pieces
    # aside.
    images = {
        path.relative_to(RAW).with_suffix("").as_posix(): path
        for path in sorted(RAW.rglob("*.png"))
        if PARTS not in path.parents
    }
    kept = []
    for language in LANGUAGES:
        name = COMPOSITE if language == "en" else f"{language}/{COMPOSITE}"
        composite = archive_register(PARTS / language)
        if composite is not None:
            images[name] = composite
        elif (OUT / f"{name}.webp").exists():
            print(f"{name}: no capture of the archive portal; the committed image is kept", file=sys.stderr)
            kept.append(OUT / f"{name}.webp")
    missing = [name for name in CAROUSEL if name not in images and OUT / f"{name}.webp" not in kept]
    if missing:
        print(f"missing captures for the carousel: {', '.join(missing)}", file=sys.stderr)
        return 1
    OUT.mkdir(parents=True, exist_ok=True)
    written = [
        still(name, image if isinstance(image, Image.Image) else Image.open(image).convert("RGB"))
        for name, image in sorted(images.items())
    ]
    written.extend(kept)
    written.append(carousel())
    current = set(written)
    for stale in sorted(OUT.rglob("*.webp")):
        if stale not in current:
            stale.unlink()
            print(f"removed {stale.relative_to(ROOT)}")
    total = 0
    for path in written:
        size = path.stat().st_size
        total += size
        print(f"{path.relative_to(ROOT)}  {size / 1024:.0f} KiB")
    print(f"total  {total / 1024:.0f} KiB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
