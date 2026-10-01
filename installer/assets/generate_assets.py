#!/usr/bin/env python3
"""Rebuild the paper-and-book StudyAgent installer artwork.

Uses the application icon generated from public/icon.svg. All installer BMPs
are 24-bit RGB: Inno Setup does not reliably composite BMP alpha.
Run after: node node_modules/@tauri-apps/cli/tauri.js icon public/icon.svg
"""
from pathlib import Path
import os
from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SCALE = 4
PAPER = '#F8F6F0'
INK = '#34443F'
MUTED = '#79877F'
BLUE = '#5B8DEF'
SAGE = '#CDD8C7'
LINE = '#DDDCD1'
FONT_DIR = Path(os.environ.get('WINDIR', r'C:\Windows')) / 'Fonts'


def font(name, size):
    return ImageFont.truetype(str(FONT_DIR / name), round(size * SCALE))


def icon(size):
    return Image.open(ROOT / 'src-tauri/icons/icon.png').convert('RGBA').resize(
        (round(size * SCALE), round(size * SCALE)), Image.Resampling.LANCZOS)


def centered(draw, x, y, text, face, color):
    box = draw.textbbox((0, 0), text, font=face)
    draw.text((round(x * SCALE - (box[2] - box[0]) / 2), round(y * SCALE)),
              text, font=face, fill=color)


def make_wizard():
    canvas = Image.new('RGB', (164 * SCALE, 314 * SCALE), PAPER)
    d = ImageDraw.Draw(canvas)

    def rect(bounds, color, radius=0):
        box = tuple(round(v * SCALE) for v in bounds)
        if radius:
            d.rounded_rectangle(box, radius=round(radius * SCALE), fill=color)
        else:
            d.rectangle(box, fill=color)

    def line(points, color=LINE, width=1):
        d.line([(round(x * SCALE), round(y * SCALE)) for x, y in points],
               fill=color, width=round(width * SCALE))

    # Spacious editorial header, with the independent book mark.
    mark = icon(56)
    canvas.paste(mark, (54 * SCALE, 27 * SCALE), mark)
    centered(d, 82, 94, 'StudyAgent', font('seguisb.ttf', 16), INK)
    centered(d, 82, 120, '陪你把每一天学扎实', font('msyh.ttc', 8.5), MUTED)
    line([(66, 145), (98, 145)], SAGE, 2)

    # A quiet desk vignette: notebook, bookmark, pencil and leafy sprig.
    # Flat paper colours, no glow, circuitry, glass or technical ornament.
    rect((25, 218, 146, 265), '#E5E8DC', 12)
    rect((37, 181, 115, 246), '#C4D1BC', 4)
    rect((41, 177, 117, 240), '#FFFFFF', 4)
    rect((41, 177, 47, 240), '#E5EADC', 2)
    for y in (195, 205, 215, 225):
        line([(56, y), (103, y)], '#E3E6DF', 0.7)
    d.polygon([(94*SCALE, 177*SCALE), (102*SCALE, 177*SCALE),
               (102*SCALE, 201*SCALE), (98*SCALE, 197*SCALE),
               (94*SCALE, 201*SCALE)], fill=BLUE)
    line([(124, 242), (133, 199)], '#BBA67B', 4)
    line([(124, 242), (123, 247)], INK, 2)
    line([(28, 237), (20, 198)], '#97AD92', 1.2)
    for x, y, side in ((23, 211, -1), (25, 220, 1), (21, 203, 1)):
        d.ellipse(((x-9 if side < 0 else x)*SCALE, (y-6)*SCALE,
                   (x if side < 0 else x+9)*SCALE, (y+2)*SCALE), fill=SAGE)
    centered(d, 82, 284, '计划  ·  专注  ·  复盘', font('msyh.ttc', 8), MUTED)
    return canvas.resize((164, 314), Image.Resampling.LANCZOS)


def make_wizard_small():
    # Opaque white matches the installer header, avoiding BMP alpha black boxes.
    canvas = Image.new('RGB', (55 * SCALE, 58 * SCALE), '#FFFFFF')
    mark = icon(42)
    canvas.paste(mark, (round(6.5 * SCALE), 8 * SCALE), mark)
    return canvas.resize((55, 58), Image.Resampling.LANCZOS)


def main():
    for filename, artwork in [('wizard.bmp', make_wizard()),
                              ('wizard-small.bmp', make_wizard_small())]:
        artwork.save(HERE / filename, 'BMP')
        print(f'{filename}: {artwork.size}, {artwork.mode}')
    # A readable preview of the actual installation artwork.
    preview = Image.new('RGB', (820, 680), PAPER)
    d = ImageDraw.Draw(preview)
    d.text((28, 20), 'StudyAgent', font=font('seguisb.ttf', 8), fill=INK)
    d.text((28, 68), '应用图标与安装向导', font=font('msyh.ttc', 5), fill=MUTED)
    large = make_wizard().resize((246, 471), Image.Resampling.LANCZOS)
    preview.paste(large, (36, 135))
    mark = icon(55)
    preview.paste(mark, (400, 165), mark)
    d.text((400, 425), '独立书页 Logo', font=font('msyh.ttc', 4), fill=INK)
    preview.paste(make_wizard_small().resize((110, 116), Image.Resampling.LANCZOS),
                  (630, 170))
    d.text((610, 315), '安装页角标', font=font('msyh.ttc', 3.5), fill=MUTED)
    out = ROOT / 'output/branding'
    out.mkdir(parents=True, exist_ok=True)
    preview.save(out / 'studyagent-branding-preview.png')


if __name__ == '__main__':
    main()
