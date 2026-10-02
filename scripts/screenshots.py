# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow==12.3.0"]
# ///
"""Encode the screenshot captures and assemble the README carousel.

`just screenshots` runs the Playwright screenshot script, which saves raw
captures under target/screenshots/raw/, then this script:

- writes each capture to assets/screenshots/<name>.webp as a lossless WebP:
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

import sys
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parent.parent
RAW = ROOT / "target" / "screenshots" / "raw"
OUT = ROOT / "assets" / "screenshots"
CAROUSEL_NAME = "readme-carousel"

# The carousel, in order: a tour of the application.
CAROUSEL = [
    "pedigree",
    "person",
    "fan-chart-dark",
    "couple",
    "statistics-map",
    "kinship",
    "search",
    "media-library",
    "history",
    "app-settings-dark",
]
CAROUSEL_WIDTH = 1200
HOLD_MS = 3000
CAROUSEL_QUALITY = 70
# What a re-run may differ by and still count as the same image.
NOISE_LEVEL = 2
NOISE_PIXELS = 200


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


def still(name: str) -> Path:
    target = OUT / f"{name}.webp"
    image = Image.open(RAW / f"{name}.png").convert("RGB")
    if not same_picture(image, target):
        image.save(target, format="WEBP", lossless=True, quality=100, method=6)
    return target


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
    captures = sorted(path.stem for path in RAW.glob("*.png"))
    missing = [name for name in CAROUSEL if name not in captures]
    if missing:
        print(f"missing captures for the carousel: {', '.join(missing)}", file=sys.stderr)
        return 1
    OUT.mkdir(parents=True, exist_ok=True)
    written = [still(name) for name in captures]
    written.append(carousel())
    kept = {path.name for path in written}
    for stale in sorted(OUT.iterdir()):
        if stale.name not in kept:
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
