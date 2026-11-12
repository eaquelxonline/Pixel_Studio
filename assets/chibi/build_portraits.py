#!/usr/bin/env python3
"""Pixel Studio anime bust portraits (128×128, 8-frame loops) — original characters.

  * "Shizuku" — rainy evening: cool gradient-mapped mood, cyan rim light, falling rain.
  * "Astra"   — starlit mage: wide-brim hat casting a shadow over the hair, twinkling sky.

Flat shapes are blocked in by this script; the engine does the craft passes:
auto_shade (4-tone hair, cast shadows of bangs and hat), selout outlines, rim light,
gradient_map grading and draw_sparkle. Eyes are built from layered shapes (sclera, gradient
iris, thick upper lash with wing, lower lash, double highlights) at full resolution.
"""

import colorsys
import json
import math
import os

from build_chibi import Layer, adjust, mixc  # shared rasterizer and color helpers

W = H = 128
FR = 8
here = os.path.dirname(os.path.abspath(__file__))
TAU = math.tau


def hnoise(x, y, seed=0):
    n = (x * 374761393 + y * 668265263 + seed * 1013904223) & 0xFFFFFFFF
    n = (n ^ (n >> 13)) * 1274126177 & 0xFFFFFFFF
    return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0


import build_chibi  # noqa: E402

build_chibi.W, build_chibi.H = W, H  # the rasterizer clips to these


def face_shape(lay, cx, top, skin):
    """Anime face: round cranium, soft cheeks, small pointed chin."""
    for y in range(top, top + 60):
        t = (y - top) / 60
        if t < 0.45:
            half = 29 * math.sqrt(max(0.0, 1 - ((0.45 - t) / 0.45) ** 2))
        else:
            k = (t - 0.45) / 0.55
            half = 29 * (1 - k ** 1.7) + 2.5 * k
        for x in range(int(cx - half), int(cx + half) + 1):
            lay.put(x, y, skin)


def eye(det, ex, ey, iris, lash, right=False, closed=False):
    """Layered anime eye centered at (ex, ey), ~22×17 px: sclera, gradient iris with dark rim
    and pupil, thick upper lash with a wing, soft lower lash, double catchlights."""
    wing = 1 if right else -1
    if closed:
        for k in range(-10, 11):
            y = ey + 2 + (k * k) / 26
            det.put(ex + k, y, lash)
            if abs(k) < 8:
                det.put(ex + k, y + 1, lash)
        return
    sclera = "#F7F4FF"
    for y in range(ey - 8, ey + 9):
        for x in range(ex - 10, ex + 11):
            u, v = (x + 0.5 - ex) / 9.8, (y + 0.5 - ey) / 8.0
            if u * u + v * v <= 1:
                det.put(x, y, sclera)
    dark, mid, light, glow = adjust(iris, -0.26, 0.08), iris, adjust(iris, 0.12, 0.06), adjust(iris, 0.26, -0.04)
    pupil = adjust(iris, -0.38, 0.1)
    icx = ex - wing * 1
    for y in range(ey - 9, ey + 9):
        for x in range(icx - 8, icx + 9):
            u, v = (x + 0.5 - icx) / 7.0, (y + 0.5 - ey) / 8.6
            if u * u + v * v > 1:
                continue
            t = (y - (ey - 8)) / 16
            c = dark if t < 0.32 else mid if t < 0.58 else light if t < 0.82 else glow
            if u * u + v * v > 0.72:
                c = dark
            if (u / 0.38) ** 2 + ((v + 0.12) / 0.5) ** 2 <= 1 and t < 0.66:
                c = pupil
            det.put(x, y, c)
    # Upper lash: thick arc (3 px) with an outer wing, thinning at the inner corner.
    for k in range(-11, 12):
        y = ey - 8 + (k * k) / 26
        inner = (k * wing) < -7
        for th in range(2 if inner else 3):
            det.put(ex + k, y + th - 1, lash)
    for k in range(4):
        det.put(ex + wing * (11 + k), ey - 6 + k * 0.7, lash)
        det.put(ex + wing * (11 + k), ey - 5 + k * 0.7, lash)
    # Soft lower lash on the outer half.
    soft = mixc(lash, "#E8B8B8", 0.5)
    for k in range(2, 9):
        det.put(ex + wing * k, ey + 8 - (k - 2) * 0.25, soft)
    # Catchlights.
    hx0 = icx - 4
    for dx in range(3):
        for dy in range(4):
            if (dx, dy) not in ((2, 3), (0, 3)):
                det.put(hx0 + dx, ey - 5 + dy, "#FFFFFF")
    det.put(icx + 3, ey + 4, "#FFFFFF")
    det.put(icx + 4, ey + 4, "#FFFFFF")
    det.put(icx + 3, ey + 5, glow)


def build(kind, f):
    L = {k: Layer() for k in ("BG", "HairBack", "Body", "Face", "HairFront", "Hat", "Details", "FX")}
    cx = 64
    if kind == "rain":
        hair, iris, skin = "#2E3F6E", "#5E7FD0", "#F3D9CF"
        cloth, cloth2 = "#E9EEF8", "#3A4A78"
        bg0, bg1 = "#1E2B4F", "#3D5A8C"
    else:
        hair, iris, skin = "#F2C3A0", "#E0A33A", "#FBE2D6"
        cloth, cloth2 = "#5B3F8F", "#F2D27A"
        bg0, bg1 = "#120F2E", "#2C2A6B"
    hair2 = adjust(hair, -0.03)
    lash = adjust(hair, -0.25, 0.05) if kind == "rain" else "#5A2E3A"
    bg = L["BG"]
    for y in range(H):
        for x in range(W):
            t = y / H
            level = int(t * 16)
            bayer = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5][(y % 4) * 4 + x % 4]
            c = bg1 if (t * 16 - level) * 16 > bayer else bg0
            bg.put(x, y, mixc(bg0, bg1, min(1, (level + (1 if c == bg1 else 0)) / 16)))
    if kind == "rain":
        # Blurry window lights (bokeh) behind the glass.
        for i, (bx, by, r) in enumerate([(18, 30, 7), (104, 22, 9), (90, 70, 6), (14, 88, 8), (112, 104, 5), (40, 12, 4)]):
            L["BG"].ellipse(bx, by, r, r, mixc(bg1, "#8FB8E8", 0.35 + 0.1 * (i % 2)))
    # Hair back mass with long side falls.
    hb = L["HairBack"]
    hb.ellipse(cx, 60, 38, 40, hair)
    for side in (-1, 1):
        for k in range(4):
            x0 = cx + side * (22 + k * 4)
            hb.strand((x0, 50), (x0 + side * (8 + k * 2), 90), (x0 + side * (2 + k * 3), 128 - k * 6), 7 - k, 5, hair if k % 2 == 0 else hair2)
    # Body: neck, shoulders, collar.
    body = L["Body"]
    body.poly([(cx - 8, 88), (cx + 8, 88), (cx + 9, 106), (cx - 9, 106)], skin)
    body.poly([(cx - 46, 128), (cx - 38, 108), (cx - 14, 100), (cx + 14, 100), (cx + 38, 108), (cx + 46, 128)], cloth)
    if kind == "rain":
        body.poly([(cx - 18, 100), (cx, 116), (cx + 18, 100), (cx + 24, 104), (cx, 124), (cx - 24, 104)], cloth2)  # sailor collar
        body.poly([(cx - 5, 114), (cx + 5, 114), (cx + 3, 124), (cx - 3, 124)], "#C94A62")
    else:
        body.poly([(cx - 14, 100), (cx, 110), (cx + 14, 100), (cx + 12, 106), (cx, 116), (cx - 12, 106)], cloth2)
        body.ellipse(cx, 112, 4, 4, "#7FD6F2")  # gem brooch
    # Face.
    face_shape(L["Face"], cx, 38, skin)
    # Hair front: crown cap + bang clumps + side locks.
    hf = L["HairFront"]
    hf.ellipse(cx, 60, 38, 40, hair, clip=lambda x, y: y < 46)
    bangs = [(-30, 70), (-21, 62), (-12, 66), (-4, 60), (4, 64), (12, 59), (20, 65), (29, 71)]
    ends = []
    for i, (tx, ty) in enumerate(bangs):
        root = (cx + tx * 0.5, 30)
        ctrl = (cx + tx * 0.95 - 3, 44)
        sway = math.sin(TAU * f / FR + i) * 0.6 if kind == "rain" else 0
        end = (cx + tx + sway, ty)
        hf.strand(root, ctrl, end, 7.5, 4.5, hair)
        ends.append(end)
    for side in (-1, 1):
        hf.strand((cx + side * 27, 44), (cx + side * 34, 80), (cx + side * 27, 112), 6.5, 4.5, hair)
        hf.strand((cx + side * 22, 50), (cx + side * 27, 78), (cx + side * 21, 98), 4, 3, hair2)
    if kind == "rain":
        # Hair tucked behind the right ear: clear the inner right lock, show the ear.
        L["Face"].ellipse(cx + 30, 70, 3.5, 6, skin)
    # Hat (Astra).
    if kind == "star":
        hat, band = "#4A3A86", "#E8C25A"
        hl = L["Hat"]
        hl.ellipse(cx + 2, 34, 58, 11, hat)
        hl.poly([(cx - 26, 34), (cx + 28, 34), (cx + 16, 10), (cx + 30, -2), (cx + 48, 2), (cx + 30, 4), (cx + 4, 10)], hat)
        hl.poly([(cx - 25, 30), (cx + 27, 30), (cx + 28, 35), (cx - 26, 35)], band)
        # Crescent moon pin.
        for y in range(18, 34):
            for x in range(cx - 22, cx - 4):
                d1 = math.hypot(x + 0.5 - (cx - 13), y + 0.5 - 26)
                d2 = math.hypot(x + 0.5 - (cx - 9.5), y + 0.5 - 23.5)
                if d1 <= 6.5 and d2 > 5.5:
                    hl.put(x, y, "#FFE79A")
    # ---- details
    det = L["Details"]
    covered = set(hf.px) | set(L["Hat"].px)
    blink = f == 6
    for right, ex in ((False, cx - 15), (True, cx + 15)):
        eye(det, ex, 70, iris, lash, right, closed=blink)
        # Brows: thin strokes, hidden wherever the bangs cover them.
        for k in range(-6, 7):
            x, y = ex + k + (1 if right else -1), 57 + (k * k) / 40 + (0.12 * k if right else -0.12 * k)
            if (round(x), round(y)) not in covered:
                det.put(x, y, mixc(hair, skin, 0.25))
    det.put(cx + 1, 82, adjust(skin, -0.12, 0.05))
    det.put(cx + 1, 83, adjust(skin, -0.08, 0.05))
    mouth = "#C0566E"
    for k in range(-3, 4):
        det.put(cx + k, 90 + (0 if abs(k) < 3 else -1), mouth)
    for bx in (cx - 22, cx + 14):
        for k in range(4):
            det.put(bx + k * 2, 80, mixc(skin, "#F28AA0", 0.55))
            det.put(bx + k * 2 + 1, 79, mixc(skin, "#F28AA0", 0.4))
    # Separation grooves between bang clumps + hair shine ring.
    groove = adjust(hair, -0.1)
    for a, b in zip(ends, ends[1:]):
        nx, ny = (a[0] + b[0]) / 2, min(a[1], b[1]) + 1
        for k in range(9):
            if (round(nx - k * 0.3), round(ny - k)) in hf.px and (round(nx - k * 0.3), round(ny - k)) not in L["Hat"].px:
                det.put(nx - k * 0.3, ny - k, groove)
    shine, shine2 = adjust(hair, 0.16, -0.05), adjust(hair, 0.09, -0.03)
    for k in range(-28, 29):
        if k % 7 in (5, 6):
            continue
        a = math.pi * (1.5 + k / 70)
        x, y = cx - 2 + math.cos(a) * 30, 46 + math.sin(a) * 9 + 0.03 * k * k * 0.3
        for t, c in ((0, shine), (1, shine2)):
            if (round(x), round(y + t)) in hf.px and (round(x), round(y + t)) not in L["Hat"].px:
                det.put(x, y + t, c)
    # ---- FX
    fx = L["FX"]
    if kind == "rain":
        for i in range(70):
            x0 = hnoise(i, 1, 3) * 140 - 6
            spd = 16 + hnoise(i, 2, 3) * 10
            y0 = (hnoise(i, 3, 3) * 160 + f * spd) % 160 - 20
            ln = 5 + int(hnoise(i, 4, 3) * 6)
            c = "#C9DCF7" if i % 3 else "#E8F1FF"
            for k in range(ln):
                fx.put(x0 - k * 0.25, y0 + k, c)
    return L


def emit(kind, name, lines):
    def cmd(op, **a):
        lines.append(json.dumps({"op": op, "actor": "portrait-builder", **a}, separators=(",", ":")))

    order = ["BG", "HairBack", "Body", "Face", "HairFront", "Hat", "Details", "FX"]
    cmd("project_new", width=W, height=H, name=name)
    cmd("layer_rename", layer=0, name="BG")
    for n in order[1:]:
        cmd("layer_create", name=n)
    cmd("frame_add", count=FR - 1)
    rim = "#8FE6FF" if kind == "rain" else "#FFB6E8"
    for f in range(FR):
        cmd("frame_set_duration", frame=f, duration=90 if kind == "rain" else 140)
        L = build(kind, f)
        for n in order:
            if n in ("Details", "FX"):
                continue
            px = [[x, y, c] for (x, y), c in sorted(L[n].px.items())]
            if px:
                cmd("set_pixels", layer=n, frame=f, pixels=px)
        cmd("auto_shade", layer="HairBack", frame=f, tones=4, bevel=6, contrast=0.55 if kind == "rain" else 0.7, cast_shadow=0, rim=rim, rim_strength=0.5)
        cmd("auto_shade", layer="Body", frame=f, tones=3, bevel=4, contrast=0.45, cast_shadow=6, rim=rim, rim_strength=0.45)
        cmd("auto_shade", layer="Face", frame=f, tones=3, bevel=5, form=0.35, contrast=0.32, cast_shadow=6, rim=rim, rim_strength=0.35)
        hc = 0.55 if kind == "rain" else 0.7
        cmd("auto_shade", layer="HairFront", frame=f, tones=4, bevel=5, contrast=hc, cast_shadow=4, min_region=4)
        if kind == "star":
            cmd("auto_shade", layer="Hat", frame=f, tones=4, bevel=4, contrast=0.6, cast_shadow=0, min_region=3)
        for n in ("HairBack", "Body", "Face", "HairFront", "Hat"):
            cmd("outline", layer=n, frame=f, mode="selout", darkness=0.55)
        cmd("pixel_cleanup", layer="HairFront", frame=f)
        det = [[x, y, c] for (x, y), c in sorted(L["Details"].px.items())]
        cmd("set_pixels", layer="Details", frame=f, pixels=det)
        fx = [[x, y, c] for (x, y), c in sorted(L["FX"].px.items())]
        if fx:
            cmd("set_pixels", layer="FX", frame=f, pixels=fx)
        if kind == "star":
            pts = []
            for i in range(26):
                x, y = int(hnoise(i, 7, 9) * 124) + 2, int(hnoise(i, 8, 9) * 60) + 2
                if 30 < x < 100 and y > 20:
                    continue
                pts.append([x, y, [1, 2, 3, 2][(f // 2 + i) % 4] if i % 3 else 1])
            cmd("draw_sparkle", layer="FX", frame=f, points=pts, color="#B9C8FF", core="#FFFFFF")
            cmd("draw_sparkle", layer="FX", frame=f, points=[[100, 92, 3 + (f % 4 == 1)], [22, 104, 2 + (f % 4 == 3)]], color="#FFE6A6")
    if kind == "rain":
        # Cool rainy-evening grade over the character, keeping a bit of skin warmth.
        for n in ("HairBack", "Body", "Face", "HairFront"):
            cmd("gradient_map", layer=n, scope="layer", colors=["#141C3A", "#3B4F86", "#8EA6D8", "#E6EEFF"], strength=0.35)


here_lines = {"rain": [], "star": []}
emit("rain", "Shizuku - Rainy Evening", here_lines["rain"])
emit("star", "Astra - Starlit Mage", here_lines["star"])
for kind, ls in here_lines.items():
    with open(os.path.join(here, f"portrait_{kind}.jsonl"), "w") as fh:
        fh.write("\n".join(ls) + "\n")
    show = list(ls)
    show.append(json.dumps({"op": "export_png", "frame": 0, "scale": 4, "_save": f"portrait_{kind}_x4.png"}))
    show.append(json.dumps({"op": "export_gif", "scale": 3, "_save": f"portrait_{kind}_x3.gif"}))
    show.append(json.dumps({"op": "project_save", "_save": f"portrait_{kind}.pxs"}))
    with open(os.path.join(here, f"portrait_{kind}_showcase.jsonl"), "w") as fh:
        fh.write("\n".join(show) + "\n")
    print(kind, len(ls), "commands")
