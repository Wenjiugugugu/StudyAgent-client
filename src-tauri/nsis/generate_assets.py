#!/usr/bin/env python3
"""Generate modern, on-brand NSIS installer assets for StudyAgent.

Outputs (next to this script):
  sidebar.bmp  -> 164x314  (MUI2 welcome/finish page bitmap)
  header.bmp   -> 150x57   (MUI2 header bitmap)

Design language:
  - Brand-blue gradient sidebar with a soft top-center glow for depth
  - Real app icon (../icons/icon.png) as the logo mark, lifted by a
    frosted radial backdrop so the blue hexagon reads on blue
  - Clean white header: real icon mark + "StudyAgent" wordmark
  - Rendered at 4x supersampling then Lanczos-downscaled; faint noise
    added to defeat gradient banding in the final BMP.

Run:  python generate_assets.py
"""
from __future__ import annotations

import math
import os
import random
from PIL import Image, ImageDraw, ImageFilter, ImageFont

# --- brand palette ----------------------------------------------------------
BRAND_LIGHT = (107, 160, 255)   # #6ba0ff
BRAND = (91, 141, 239)          # #5b8def
BRAND_DEEP = (47, 95, 191)      # #2f5fbf
BRAND_DARKER = (33, 70, 150)    # deeper bottom for richer gradient
INK = (31, 41, 55)              # #1f2937
WHITE = (255, 255, 255)

# NSIS MUI2 standard bitmap sizes
SIDEBAR_W, SIDEBAR_H = 164, 314
HEADER_W, HEADER_H = 150, 57
SCALE = 4  # supersampling factor

HERE = os.path.dirname(os.path.abspath(__file__))
ICON_PATH = os.path.join(HERE, "..", "icons", "icon.png")
FONT_DIR = os.environ.get("WINDIR", r"C:\Windows") + r"\Fonts"
FONT_REGULAR = FONT_DIR + r"\segoeui.ttf"
FONT_SEMIBOLD = FONT_DIR + r"\seguisb.ttf"   # Segoe UI Semibold
FONT_YAHEI = FONT_DIR + r"\msyh.ttc"         # Microsoft YaHei (CJK)


def _font(path: str, size: int) -> ImageFont.FreeTypeFont:
    try:
        return ImageFont.truetype(path, size)
    except OSError:
        return ImageFont.truetype(FONT_REGULAR, size)


def _flat_hexagon_points(cx, cy, r):
    h = r * math.sqrt(3) / 2
    return [
        (cx - r / 2, cy - h), (cx + r / 2, cy - h),
        (cx + r, cy), (cx + r / 2, cy + h),
        (cx - r / 2, cy + h), (cx - r, cy),
    ]


def _v_gradient(w, h, top, bottom):
    grad = Image.new("RGB", (w, h), top)
    px = grad.load()
    for y in range(h):
        t = y / max(h - 1, 1)
        r = int(top[0] + (bottom[0] - top[0]) * t)
        g = int(top[1] + (bottom[1] - top[1]) * t)
        b = int(top[2] + (bottom[2] - top[2]) * t)
        for x in range(w):
            px[x, y] = (r, g, b)
    return grad


def _radial_glow(w, h, cx, cy, radius, color, max_alpha=120):
    """Soft radial glow as an RGBA layer."""
    layer = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    steps = 60
    for i in range(steps, 0, -1):
        rr = int(radius * i / steps)
        a = int(max_alpha * (1 - i / steps) ** 2)
        d.ellipse((cx - rr, cy - rr, cx + rr, cy + rr),
                  fill=(color[0], color[1], color[2], a))
    return layer.filter(ImageFilter.GaussianBlur(radius * 0.12))


def _add_noise(img, amount=2):
    """Faint RGB noise to break up gradient banding."""
    px = img.load()
    w, h = img.size
    rng = random.Random(1)
    for y in range(h):
        for x in range(w):
            r, g, b = px[x, y][:3]
            n = rng.randint(-amount, amount)
            px[x, y] = (max(0, min(255, r + n)),
                        max(0, min(255, g + n)),
                        max(0, min(255, b + n)))
    return img


def _draw_text_centered(draw, cx, y, text, font, fill):
    bbox = draw.textbbox((0, 0), text, font=font)
    tw = bbox[2] - bbox[0]
    draw.text((cx - tw // 2, y), text, font=font, fill=fill)


def _load_icon(size):
    """Load the app icon resized to (size, size) preserving RGBA."""
    ic = Image.open(ICON_PATH).convert("RGBA")
    return ic.resize((size, size), Image.LANCZOS)


def make_sidebar():
    w, h = SIDEBAR_W * SCALE, SIDEBAR_H * SCALE
    base = _v_gradient(w, h, BRAND_LIGHT, BRAND_DARKER).convert("RGBA")

    # top-center soft glow for depth
    glow = _radial_glow(w, h, w // 2, int(h * 0.16),
                        int(70 * SCALE), (210, 225, 255), max_alpha=70)
    base = Image.alpha_composite(base, glow)
    draw = ImageDraw.Draw(base, "RGBA")

    # --- faint decorative hexagons in the lower area ---------------------
    rng = random.Random(7)
    for _ in range(6):
        cx = rng.randint(10, w - 10)
        cy = rng.randint(int(h * 0.55), h - 8)
        rr = rng.randint(int(14 * SCALE), int(34 * SCALE))
        pts = _flat_hexagon_points(cx, cy, rr)
        draw.polygon(pts, outline=(255, 255, 255, 18), width=max(2, SCALE))

    # --- white plate behind the icon (contrast + depth) ------------------
    icx, icy = w // 2, int(h * 0.20)
    plate_r = int(30 * SCALE)
    # soft drop shadow under the plate
    shadow = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    off = int(3 * SCALE)
    sd.ellipse((icx - plate_r + off, icy - plate_r + off + int(2 * SCALE),
                icx + plate_r + off, icy + plate_r + off + int(2 * SCALE)),
               fill=(0, 10, 40, 95))
    shadow = shadow.filter(ImageFilter.GaussianBlur(int(3 * SCALE)))
    base = Image.alpha_composite(base, shadow)
    draw = ImageDraw.Draw(base, "RGBA")
    # solid white plate so the blue hexagon icon pops on blue
    draw.ellipse((icx - plate_r, icy - plate_r, icx + plate_r, icy + plate_r),
                 fill=(255, 255, 255, 248))
    # --- real app icon ----------------------------------------------------
    icon_px = int(38 * SCALE)
    icon = _load_icon(icon_px)
    base.alpha_composite(icon, (icx - icon_px // 2, icy - icon_px // 2))
    draw = ImageDraw.Draw(base, "RGBA")

    # --- wordmark + tagline ----------------------------------------------
    f_name = _font(FONT_SEMIBOLD, int(15 * SCALE))
    f_tag = _font(FONT_YAHEI, int(9 * SCALE))
    ty = icy + plate_r + int(12 * SCALE)
    _draw_text_centered(draw, w // 2, ty, "StudyAgent", f_name, WHITE)
    _draw_text_centered(draw, w // 2, ty + int(20 * SCALE),
                        "AI 学习工作台", f_tag, (255, 255, 255, 210))

    # --- version footer ---------------------------------------------------
    f_ver = _font(FONT_REGULAR, int(7 * SCALE))
    _draw_text_centered(draw, w // 2, h - int(15 * SCALE),
                        "v0.3.1", f_ver, (255, 255, 255, 130))

    out = base.convert("RGB")
    out = _add_noise(out, amount=2)
    return out.resize((SIDEBAR_W, SIDEBAR_H), Image.LANCZOS)


def make_header():
    w, h = HEADER_W * SCALE, HEADER_H * SCALE
    base = Image.new("RGBA", (w, h), WHITE + (255,))
    draw = ImageDraw.Draw(base, "RGBA")

    # real icon mark on the left
    icx, icy = int(16 * SCALE), h // 2
    icon_px = int(20 * SCALE)
    icon = _load_icon(icon_px)
    base.alpha_composite(icon, (icx - icon_px // 2, icy - icon_px // 2))
    draw = ImageDraw.Draw(base, "RGBA")

    # wordmark
    f = _font(FONT_SEMIBOLD, int(12 * SCALE))
    draw.text((icx + icon_px // 2 + int(6 * SCALE),
               icy - int(7 * SCALE)),
              "StudyAgent", font=f, fill=INK)

    # bottom accent rule
    draw.rectangle((0, h - max(2, SCALE), w, h), fill=BRAND)

    out = base.convert("RGB")
    return out.resize((HEADER_W, HEADER_H), Image.LANCZOS)


def main():
    out_dir = HERE
    sb = make_sidebar()
    hd = make_header()
    sb_path = os.path.join(out_dir, "sidebar.bmp")
    hd_path = os.path.join(out_dir, "header.bmp")
    sb.save(sb_path, "BMP")
    hd.save(hd_path, "BMP")
    print(f"wrote {sb_path} ({os.path.getsize(sb_path)} bytes, {sb.size})")
    print(f"wrote {hd_path} ({os.path.getsize(hd_path)} bytes, {hd.size})")


if __name__ == "__main__":
    main()
