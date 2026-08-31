#!/usr/bin/env python3
"""Generate on-brand Inno Setup wizard bitmaps for StudyAgent.

Outputs (next to this script):
  wizard.bmp        -> 164x314  (Inno Setup WizardImageFile, 左侧大图)
  wizard-small.bmp  ->  55x58   (Inno Setup WizardSmallImageFile, 右上角小图)

Design language:
  - Brand-blue gradient with a soft top-center glow for depth
  - Real app icon (../../src-tauri/icons/icon.png) as the logo mark, lifted by a
    frosted radial backdrop so the blue hexagon reads on blue
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

# Inno Setup wizard bitmap sizes
WIZARD_W, WIZARD_H = 164, 314
SMALL_W, SMALL_H = 55, 58
SCALE = 4  # supersampling factor

HERE = os.path.dirname(os.path.abspath(__file__))
ICON_PATH = os.path.join(HERE, "..", "..", "src-tauri", "icons", "icon.png")
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


def make_wizard():
    w, h = WIZARD_W * SCALE, WIZARD_H * SCALE
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

    out = base.convert("RGB")
    out = _add_noise(out, amount=2)
    return out.resize((WIZARD_W, WIZARD_H), Image.LANCZOS)


def make_wizard_small():
    """右上角小图：透明背景 + 居中应用图标（55x58）。

    32-bit BMP with alpha，Inno Setup 6 按 alpha 通道透明渲染。
    经过 alpha 阈值化以消除 LANCZOS 缩放在图标边缘产生的
    alpha 1-7 "幽灵"像素（这些像素会被 Inno 渲染为暗色斑点）。
    """
    w, h = SMALL_W * SCALE, SMALL_H * SCALE
    base = Image.new("RGBA", (w, h), (0, 0, 0, 0))

    # 应用图标居中
    icon_px = int(40 * SCALE)
    icon = _load_icon(icon_px)
    base.alpha_composite(icon, ((w - icon_px) // 2, (h - icon_px) // 2))

    out = base.resize((SMALL_W, SMALL_H), Image.LANCZOS)

    # alpha 阈值化：< 8 视为完全透明（消除 LANCZOS 边缘振铃暗斑），
    # >= 8 保留原有抗锯齿以维持图标边缘平滑。
    r, g, b, a = out.split()
    a = a.point(lambda v: 0 if v < 8 else v)
    return Image.merge("RGBA", (r, g, b, a))


def main():
    out_dir = HERE
    big = make_wizard()
    small = make_wizard_small()
    big_path = os.path.join(out_dir, "wizard.bmp")
    small_path = os.path.join(out_dir, "wizard-small.bmp")
    big.save(big_path, "BMP")
    small.save(small_path, "BMP")
    print(f"wrote {big_path} ({os.path.getsize(big_path)} bytes, {big.size})")
    print(f"wrote {small_path} ({os.path.getsize(small_path)} bytes, {small.size})")


if __name__ == "__main__":
    main()
