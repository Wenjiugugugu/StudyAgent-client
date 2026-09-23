# -*- coding: utf-8 -*-
"""用系统 Edge 对本地 vite 页面截图，便于 UI 层级调优验证。"""
import sys
from playwright.sync_api import sync_playwright

url = sys.argv[1]
out = sys.argv[2]
sel = sys.argv[3] if len(sys.argv) > 3 else None
width = int(sys.argv[4]) if len(sys.argv) > 4 else 1400
height = int(sys.argv[5]) if len(sys.argv) > 5 else 1000

EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

with sync_playwright() as p:
    b = p.chromium.launch(executable_path=EDGE, headless=True)
    page = b.new_page(viewport={"width": width, "height": height}, device_scale_factor=1)
    page.goto(url, wait_until="networkidle", timeout=60000)
    page.wait_for_timeout(1500)
    if sel:
        loc = page.locator(sel)
        loc.first.wait_for(state="visible", timeout=15000)
        loc.first.screenshot(path=out)
    else:
        page.screenshot(path=out, full_page=True)
    b.close()
print("saved", out)
