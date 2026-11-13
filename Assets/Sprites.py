#!/usr/bin/env python3
"""Pixel Studio 2D sprite assets: the pastel chibi collection, the 32×52 cast, the 2D VFX pack and the
animated bust portraits. All characters are original designs. Each builder writes engine command
scripts (*.jsonl + *_showcase.jsonl) to Assets/Build/; replay them with Native/Script.rs."""

import colorsys
import json
import math
import os
import random

# ════ Pastel chibi collection (64×80) + shared 2D rasterizer ════════════════════════════════════

# Pixel Studio pastel chibi collection — original characters, 64×80 sprites.
#
# Style targets (studied from modern anime pixel-art chibis): ~2-head proportions, big soft
# hair volumes built from tapered strand clumps, large gradient eyes with highlights, blush,
# pastel hue-shifted palettes, colored (sel-out) outlines instead of black, cast shadows of the
# bangs on the face, and sparkle effects.
#
# Pipeline (exactly what an AI agent does through MCP):
# 1. Python blocks each character in FLAT colors on separate layers
# (HairBack → Body → Face → HairFront) and hand-authors the eye/mouth stamps.
# 2. The engine shades every layer: `auto_shade` (hue-shifted cel shading, cast shadows from
# the layers above), then `outline mode=selout`, then `pixel_cleanup`.
# 3. Details (eyes, blush, hair shine) and `draw_sparkle` effects go on top.
#
# All characters are original designs.
#
# python3 Assets/Sprites.py chibi  ->  Assets/Build/chibi.jsonl + chibi_showcase.jsonl
# 

W, H = 64, 80
lines = []


def cmd(op, **a):
    lines.append(json.dumps({"op": op, "actor": "chibi-builder", **a}, separators=(",", ":")))


# ------------------------------------------------------------------ color helpers


def hx(rgb):
    return "#%02X%02X%02X" % tuple(max(0, min(255, round(v * 255))) for v in rgb)


def rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) / 255 for i in (0, 2, 4))


def adjust(h, dl=0.0, ds=0.0, dh=0.0):
    r, g, b = rgb(h)
    hh, l, s = colorsys.rgb_to_hls(r, g, b)
    return hx(colorsys.hls_to_rgb((hh + dh) % 1.0, max(0, min(1, l + dl)), max(0, min(1, s + ds))))


def mixc(a, b, t):
    ra, rb = rgb(a), rgb(b)
    return hx(tuple(ra[i] + (rb[i] - ra[i]) * t for i in range(3)))


# ------------------------------------------------------------------ rasterizer


class Layer:
    def __init__(self, w=W, h=H):
        self.w, self.h = w, h
        self.px = {}

    def put(self, x, y, c):
        x, y = int(round(x)), int(round(y))
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[(x, y)] = c

    def ellipse(self, cx, cy, rx, ry, c, clip=None):
        for y in range(int(cy - ry - 1), int(cy + ry + 2)):
            for x in range(int(cx - rx - 1), int(cx + rx + 2)):
                if ((x + 0.5 - cx) / rx) ** 2 + ((y + 0.5 - cy) / ry) ** 2 <= 1 and (clip is None or clip(x, y)):
                    self.put(x, y, c)

    def poly(self, pts, c):
        ys = [p[1] for p in pts]
        for y in range(int(min(ys)) - 1, int(max(ys)) + 2):
            for x in range(0, self.w):
                if inside(pts, x + 0.5, y + 0.5):
                    self.put(x, y, c)

    def capsule(self, a, b, r0, r1, c):
        steps = int(max(abs(b[0] - a[0]), abs(b[1] - a[1])) * 3) + 2
        for i in range(steps + 1):
            t = i / steps
            x, y = a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t
            r = r0 + (r1 - r0) * t
            self.disc(x, y, r, c)

    def disc(self, x, y, r, c):
        for yy in range(int(y - r - 1), int(y + r + 2)):
            for xx in range(int(x - r - 1), int(x + r + 2)):
                if (xx + 0.5 - x) ** 2 + (yy + 0.5 - y) ** 2 <= r * r:
                    self.put(xx, yy, c)

    def strand(self, p0, p1, p2, w0, w1, c, tip=0.0):
        """Tapered quadratic-Bezier hair clump."""
        n = 60
        for i in range(n + 1):
            t = i / n
            x = (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t * t * p2[0]
            y = (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t * t * p2[1]
            w = w0 + (w1 - w0) * t
            if t > 0.75:
                w *= max(tip, 1 - (t - 0.75) / 0.25 * (1 - tip))
            self.disc(x, y, max(w, 0.45), c)

    def stamp(self, x0, y0, rows, legend, flip=False):
        for dy, row in enumerate(rows):
            for dx, ch in enumerate(row):
                if ch in legend:
                    xx = x0 + (len(row) - 1 - dx if flip else dx)
                    self.put(xx, y0 + dy, legend[ch])

    def erase_where(self, pred):
        self.px = {k: v for k, v in self.px.items() if not pred(*k)}


def inside(pts, x, y):
    c = False
    j = len(pts) - 1
    for i in range(len(pts)):
        xi, yi = pts[i]
        xj, yj = pts[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi + 1e-9) + xi:
            c = not c
        j = i
    return c


# ------------------------------------------------------------------ face details (hand-authored)

EYE_OPEN = [
    ".LLLLL.",
    "LLLLLLL",
    ".DWWDD.",
    ".DWWDD.",
    ".DDDMD.",
    ".DMMMD.",
    ".MMBMM.",
    ".MBBBM.",
    "..BBwB.",
    "...LL..",
]
EYE_HAPPY = [
    "..LLL..",
    ".L...L.",
    "L.....L",
]
EYE_CLOSED = [
    "L.....L",
    ".LLLLL.",
]
MOUTH = {
    "smile": ["L...L", ".LLL."],
    "cat": ["L.L.L", ".L.L."],
    "open": [".LLL.", "LRRRL", ".LPL."],
    "o": [".L.", "LRL", ".L."],
}


def eye_colors(iris):
    return {
        "L": adjust(iris, -0.42, 0.05, 0.03),
        "D": adjust(iris, -0.22, 0.05),
        "M": iris,
        "B": adjust(iris, 0.16, 0.05, -0.02),
        "W": "#FFFFFF",
        "w": "#FFFFFF",
    }


# ------------------------------------------------------------------ character construction


def build(ch, frame):
    """Returns {layer: Layer} for one animation frame of one character."""
    anim, f = frame
    hair, skin, iris = ch["hair"], ch.get("skin", "#FDE6D8"), ch["iris"]
    hair2 = adjust(hair, -0.035)  # alternate clump color → strand separation after shading
    bob = [0, 1, 1, 0][f] if anim == "idle" else 0
    hop = [0, -3, -5, -2][f] if anim == "cheer" else 0
    sway = [0, 1, 0, -1][f]
    dy = bob + hop
    L = {k: Layer() for k in ("Back", "HairBack", "Body", "Face", "HairFront", "Arms", "Details", "FX")}
    hb, body, face, hf, det = L["HairBack"], L["Body"], L["Face"], L["HairFront"], L["Details"]
    arms = L["Arms"]
    cx, cy = 32, 31 + dy  # head center

    # ---- hair back volume
    style = ch["style"]
    hb.ellipse(cx, cy - 2, 18, 17, hair)
    if style in ("long", "twin_low", "drills", "twintails_high"):
        length = 66 if style == "long" else 58
        for i, side in enumerate((-1, 1)):
            for k in range(3):
                x0 = cx + side * (10 + k * 3)
                c = hair if k % 2 == 0 else hair2
                hb.strand((x0, cy - 4), (x0 + side * (4 + k), cy + 14), (x0 + side * (1 + k) + sway * (k + 1) * 0.5, length + dy - k * 3), 4.2 - k * 0.6, 3.0, c)
    if style == "twin_low":
        for side in (-1, 1):
            hb.strand((cx + side * 15, cy + 8), (cx + side * 23, cy + 22), (cx + side * 19 + sway, 70 + dy), 5, 4, hair2)
    if style == "twintails_high":
        for side in (-1, 1):
            base = (cx + side * 15, cy - 12)
            hb.strand(base, (cx + side * 29, cy - 4), (cx + side * 25 + sway, cy + 30), 5.5, 4.5, hair)
            hb.strand(base, (cx + side * 26, cy + 4), (cx + side * 22 + sway, cy + 34), 4, 2.5, hair2)
    if style == "drills":
        for side in (-1, 1):
            for k in range(4):
                yy = cy + 6 + k * 7
                hb.ellipse(cx + side * (19 + (k % 2)) + sway * 0.5, yy + dy * 0, 5 - k * 0.5, 4, hair if k % 2 else hair2)
    if style == "bob":
        for side in (-1, 1):
            hb.strand((cx + side * 12, cy - 6), (cx + side * 21, cy + 6), (cx + side * 17, cy + 15), 6, 4, hair)
    if style == "short_boy":
        hb.ellipse(cx, cy + 2, 17, 13, hair2)

    # ---- body (back items first: hood)
    outfit = ch["outfit"]
    if ch.get("hood"):
        L["Back"].ellipse(cx, cy + 13, 15, 8, ch["hood"])
        if ch.get("hood_ears") == "bunny":
            for side in (-1, 1):
                L["Back"].strand((cx + side * 8, cy - 10), (cx + side * 15, cy - 22), (cx + side * 13, cy - 30), 3.4, 2.5, ch["hood"], tip=0.6)
        if ch.get("hood_ears") == "cat":
            for side in (-1, 1):
                L["Back"].poly([(cx + side * 7, cy - 12), (cx + side * 17, cy - 24), (cx + side * 17, cy - 8)], ch["hood"])

    top, top2, bottom = outfit["top"], outfit.get("inner", "#FFFFFF"), outfit.get("bottom", outfit["top"])
    legs, shoes = ch.get("legs", skin), ch.get("shoes", "#B46A5C")
    leg_y = 62 + dy
    for side in (-1, 1):
        lx = cx + side * 4
        body.capsule((lx, leg_y), (lx + side * 0.5, 72 + dy), 2.6, 2.3, legs)
        if ch.get("socks"):
            body.capsule((lx + side * 0.5, 69 + dy), (lx + side * 0.5, 72 + dy), 2.5, 2.4, ch["socks"])
        body.ellipse(lx + side * 1, 74.5 + dy, 3.6, 2.2, shoes)
    # torso
    body.poly([(cx - 8, 46 + dy), (cx + 8, 46 + dy), (cx + 10, 60 + dy), (cx - 10, 60 + dy)], top2 if outfit.get("open") else top)
    if outfit.get("open"):
        body.poly([(cx - 9, 46 + dy), (cx - 3, 46 + dy), (cx - 3, 61 + dy), (cx - 12, 61 + dy)], top)
        body.poly([(cx + 3, 46 + dy), (cx + 9, 46 + dy), (cx + 12, 61 + dy), (cx + 3, 61 + dy)], top)
    kind = outfit.get("kind", "skirt")
    if kind == "skirt":
        body.poly([(cx - 9, 57 + dy), (cx + 9, 57 + dy), (cx + 14, 66 + dy), (cx - 14, 66 + dy)], bottom)
    elif kind == "dress":
        body.poly([(cx - 9, 50 + dy), (cx + 9, 50 + dy), (cx + 15, 67 + dy), (cx - 15, 67 + dy)], bottom)
        if outfit.get("frill"):
            for x in range(cx - 15, cx + 16, 2):
                body.ellipse(x + 0.5, 67 + dy, 1.4, 1.3, outfit["frill"])
    elif kind == "shorts":
        body.poly([(cx - 9, 57 + dy), (cx + 9, 57 + dy), (cx + 10, 64 + dy), (cx - 10, 64 + dy)], bottom)
    elif kind == "coat":
        body.poly([(cx - 10, 46 + dy), (cx + 10, 46 + dy), (cx + 13, 68 + dy), (cx - 13, 68 + dy)], top)
        for y in range(48 + dy, 67 + dy):
            body.put(cx, y, outfit["trim"])
    if outfit.get("collar"):
        col = outfit["collar"]
        body.poly([(cx - 9, 45 + dy), (cx + 9, 45 + dy), (cx + 6, 51 + dy), (cx, 54 + dy), (cx - 6, 51 + dy)], col)
    if outfit.get("ribbon"):
        r = outfit["ribbon"]
        body.poly([(cx, 51 + dy), (cx - 4, 49 + dy), (cx - 4, 54 + dy)], r)
        body.poly([(cx, 51 + dy), (cx + 4, 49 + dy), (cx + 4, 54 + dy)], r)
        body.poly([(cx - 1, 51 + dy), (cx - 2, 57 + dy), (cx, 56 + dy)], r)
        body.poly([(cx + 1, 51 + dy), (cx + 2, 57 + dy), (cx, 56 + dy)], r)
    # arms
    pose = ch["pose"] if anim != "idle" else "down"
    sleeve = outfit.get("sleeve", top)
    big = 3.6 if outfit.get("oversized") else 2.8
    for side in (-1, 1):
        sh = (cx + side * 8, 48 + dy)
        if pose == "cheer" or (pose == "wave" and side == 1):
            lift = [0, 2, 3, 1][f]
            if pose == "wave":
                hand = (cx + 17 + [0, 2, 0, -2][f], 24 + dy)
            else:
                hand = (cx + side * 18, 25 + dy - lift)
            elbow = (cx + side * 15, 40 + dy)
        elif pose == "front":
            elbow, hand = (cx + side * 11, 55 + dy), (cx + side * 2, 58 + dy)
        else:
            elbow, hand = (cx + side * 11, 54 + dy), (cx + side * 12, 60 + dy + bob * 0)
        arms.capsule(sh, elbow, big, big * 0.95, sleeve)
        arms.capsule(elbow, hand, big * 0.95, big * (1.05 if outfit.get("oversized") else 0.85), sleeve)
        arms.disc(hand[0], hand[1] + (0 if pose != "cheer" else -1), 2.2, skin)

    # ---- face
    face.ellipse(cx, cy + 3, 15.5, 13.5, skin)
    face.ellipse(cx, cy + 12, 3, 3.5, skin)  # neck hint behind the chin line

    # ---- hair front: bangs from the crown, side locks, ahoge
    tips = ch.get("bangs", [(-12, -1), (-7, 3), (-2, 0), (3, 4), (8, 1), (13, -2)])
    bang_ends = []
    for i, (tx, ty) in enumerate(tips):
        root = (cx + tx * 0.55 + 2, cy - 15 + abs(tx) * 0.12)
        end = (cx + tx + sway * 0.3, cy + ty - 2)
        ctrl = (cx + tx * 0.95 - 1.5, cy - 9)
        hf.strand(root, ctrl, end, 4.6, 3.0, hair)
        bang_ends.append(end)
    hf.ellipse(cx, cy - 9, 16.5, 9, hair, clip=lambda x, y: y < cy - 5)
    for side in (-1, 1):
        lock_len = ch.get("lock", 14)
        hf.strand((cx + side * 13, cy - 8), (cx + side * 17, cy + 4), (cx + side * 14 + sway * 0.5, cy + lock_len), 3.8, 2.6, hair2 if side < 0 else hair)
    if ch.get("ahoge", True):
        hf.strand((cx + 1, cy - 16), (cx + 6, cy - 26), (cx - 1 + sway, cy - 24), 1.3, 1.0, hair, tip=0.7)
    if style == "short_boy":
        for k, tx in enumerate((-14, -9, 10, 14)):
            hf.strand((cx + tx * 0.5, cy - 14), (cx + tx, cy - 6), (cx + tx * 1.05, cy + 4), 3, 2, hair2 if k % 2 else hair)

    # accessories (shaded with the hair layer)
    acc = ch.get("acc")
    if acc == "headphones":
        hf.capsule((cx - 17, cy - 2), (cx - 17, cy + 5), 3.2, 3.2, ch["acc_color"])
        hf.capsule((cx + 17, cy - 2), (cx + 17, cy + 5), 3.2, 3.2, ch["acc_color"])
        for t in range(0, 21):
            a = math.pi * (1 + t / 20)
            hf.disc(cx + math.cos(a) * 17, cy - 4 + math.sin(a) * 15, 1.4, adjust(ch["acc_color"], -0.1))
    if acc == "flower":
        fx, fy = cx + 11, cy - 9
        for k in range(5):
            a = k / 5 * math.tau
            hf.disc(fx + math.cos(a) * 2.2, fy + math.sin(a) * 2.2, 1.6, ch["acc_color"])
        hf.disc(fx, fy, 1.0, "#FFE27A")
    if acc == "snowflake":
        fx, fy = cx - 11, cy - 9
        for k in range(3):
            a = k / 3 * math.pi
            hf.capsule((fx - math.cos(a) * 3, fy - math.sin(a) * 3), (fx + math.cos(a) * 3, fy + math.sin(a) * 3), 0.6, 0.6, ch["acc_color"])
    if acc == "bow":
        bx, by = cx + 10, cy - 13
        hf.poly([(bx, by), (bx - 6, by - 4), (bx - 6, by + 4)], ch["acc_color"])
        hf.poly([(bx, by), (bx + 6, by - 4), (bx + 6, by + 4)], ch["acc_color"])
        hf.disc(bx, by, 1.4, adjust(ch["acc_color"], -0.08))
    if acc == "star":
        bx, by = cx - 10, cy - 10
        for k in range(5):
            a = -math.pi / 2 + k / 5 * math.tau
            hf.capsule((bx, by), (bx + math.cos(a) * 3.2, by + math.sin(a) * 3.2), 1.0, 0.5, ch["acc_color"])

    # ---- details: eyes, mouth, blush, hair shine (hand-painted, never auto-shaded)
    ey = cy
    blink = anim == "idle" and f == 3
    happy = anim == "cheer" and ch.get("happy_cheer", True)
    ec = eye_colors(iris)
    for side, ex in ((-1, cx - 11), (1, cx + 4)):
        if blink:
            det.stamp(ex, ey + 6, EYE_CLOSED, ec)
        elif happy:
            det.stamp(ex, ey + 4, EYE_HAPPY, ec)
        else:
            det.stamp(ex, ey, EYE_OPEN, ec)
            det.put(ex - 1 if side < 0 else ex + 7, ey + 1, ec["L"])  # outer lash wing
    blush = mixc(skin, ch.get("blush", "#F59AAE"), 0.75)
    blush_l = mixc(skin, ch.get("blush", "#F59AAE"), 0.45)
    for bx in (cx - 13, cx + 9):
        det.stamp(bx, ey + 10, [".abba", "abba."], {"a": blush_l, "b": blush})
    mouth = ch.get("mouth", "smile") if anim == "idle" else ch.get("mouth2", "open")
    m = MOUTH[mouth]
    mc = {"L": "#B4506A", "R": "#A83A55", "P": "#F28FA0"}
    det.stamp(cx - len(m[0]) // 2, ey + 11, m, mc)
    # Strand separation lines between the bang clumps (from each notch upward).
    groove = adjust(hair, -0.12, 0.0, 0.01)
    for a, b in zip(bang_ends, bang_ends[1:]):
        nx, ny = (a[0] + b[0]) / 2, min(a[1], b[1]) + 1
        for k in range(4):
            x, y = nx - k * 0.25, ny - k
            if (round(x), round(y)) in hf.px:
                det.put(x, y, groove)
    shine = adjust(hair, 0.12, -0.05)
    for k in range(-10, 11):
        if k % 5 == 4:
            continue
        a = math.pi * (1.5 + k / 24)
        x, y = cx - 1 + math.cos(a) * 12, cy - 8 + math.sin(a) * 5 + 0.15 * abs(k)
        for t in (0, 1):
            if (round(x), round(y + t)) in hf.px:
                det.put(x, y + t, shine if t == 0 else adjust(hair, 0.06, -0.03))

    # ---- FX
    if anim == "cheer":
        fx = L["FX"]
        for i, (sx, sy, s) in enumerate([(7, 14, 3), (56, 10, 4), (4, 44, 2), (59, 40, 3), (50, 4, 2)]):
            size = [s, s - 1, s + 1, s][(f + i) % 4]
            fx.px[("sparkle", i)] = (sx, sy + [0, -1, -2, -1][(f + i) % 4], max(1, size))
    return L


# ------------------------------------------------------------------ cast (original designs)

CAST = [
    dict(name="Mochi", hair="#F7B6CF", iris="#E2557E", style="twin_low", pose="cheer", hood="#FFF1E2", hood_ears="bunny",
         outfit=dict(top="#FFF1E2", inner="#FFFFFF", bottom="#F7B6CF", oversized=True, open=True, kind="skirt"),
         socks="#FFFFFF", shoes="#D9788E", acc="bow", acc_color="#FF8FB1", mouth="cat"),
    dict(name="Yuzu", hair="#F8E08E", iris="#4FAE7B", style="bob", pose="wave", lock=10,
         outfit=dict(top="#9FE0C4", inner="#FFFFFF", bottom="#6F7FA8", oversized=True, open=True, kind="shorts"),
         legs="#FDE6D8", socks="#FFFFFF", shoes="#F6A86B", acc="headphones", acc_color="#C9A7F2"),
    dict(name="Fuyu", hair="#EEF4FF", iris="#5FA6E8", style="long", pose="front", lock=20,
         outfit=dict(top="#CFE6FF", bottom="#E8F3FF", kind="dress", frill="#FFFFFF", collar="#FFFFFF", ribbon="#7FB5F0"),
         legs="#FFFFFF", shoes="#7A8BB5", acc="snowflake", acc_color="#8EC5FF", mouth="smile"),
    dict(name="Kuro", hair="#7E68B8", iris="#F2C14E", style="bob", pose="cheer", hood="#3E3A55", hood_ears="cat", lock=12,
         outfit=dict(top="#3E3A55", inner="#C8B6F2", bottom="#2E2B40", oversized=True, open=True, kind="shorts"),
         legs="#FDE6D8", socks="#2E2B40", shoes="#C8B6F2", mouth="cat", mouth2="open"),
    dict(name="Sora", hair="#8FD3F5", iris="#3C78D8", style="twintails_high", pose="wave",
         outfit=dict(top="#FFFFFF", bottom="#3D4F8F", collar="#3D4F8F", ribbon="#E8536B", kind="skirt"),
         socks="#FFFFFF", shoes="#5A4038", acc="star", acc_color="#FFD86B"),
    dict(name="Ren", hair="#C9D1DE", iris="#E8A33D", style="short_boy", pose="front", ahoge=True,
         outfit=dict(top="#34406B", trim="#F29B4B", bottom="#2A3050", kind="coat", sleeve="#34406B"),
         legs="#2A3050", shoes="#1F2238", mouth="smile", mouth2="smile", happy_cheer=False, blush="#F2B8A8"),
    dict(name="Lila", hair="#C7A6EE", iris="#D9488F", style="drills", pose="front",
         outfit=dict(top="#3A2F4A", bottom="#3A2F4A", kind="dress", frill="#FFFFFF", collar="#FFFFFF", ribbon="#D9488F"),
         legs="#2A2236", shoes="#2A2236", acc="bow", acc_color="#2A2236", mouth="smile"),
    dict(name="Hana", hair="#F7B48A", iris="#2FA89A", style="long", pose="cheer", lock=16,
         outfit=dict(top="#9BD37A", inner="#FFF6D8", bottom="#FFF6D8", oversized=True, open=True, kind="dress"),
         legs="#FDE6D8", shoes="#F6C64B", acc="flower", acc_color="#FFFFFF", mouth="smile"),
]

def build_chibi(out):
    here = out
    LAYERS = ["Back", "HairBack", "Body", "Face", "HairFront", "Arms", "Details", "FX"]
    cmd("project_new", width=W, height=H, name="Pastel Chibi Collection")
    cmd("layer_rename", layer=0, name="Back")
    for name in LAYERS[1:]:
        cmd("layer_create", name=name)

    frame = 0
    for ci, ch in enumerate(CAST):
        for anim in ("idle", ch["pose"] if ch["pose"] in ("cheer", "wave") else "idle2"):
            for f in range(4):
                if frame > 0:
                    cmd("frame_add")
                cmd("frame_set_duration", frame=frame, duration=170 if anim.startswith("idle") else 120)
                layers = build(ch, ("idle" if anim == "idle2" else anim, f if anim != "idle2" else (f + 2) % 4))
                for name in LAYERS[:-2]:
                    px = [[x, y, c] for (x, y), c in sorted(layers[name].px.items())]
                    if px:
                        cmd("set_pixels", layer=name, frame=frame, pixels=px)
                # Engine finishing passes: shading with cast shadows, sel-out outlines, cleanup.
                cmd("auto_shade", layer="HairBack", frame=frame, tones=4, bevel=3, contrast=0.45, cast_shadow=0)
                cmd("auto_shade", layer="Back", frame=frame, tones=3, contrast=0.4, cast_shadow=3)
                cmd("auto_shade", layer="Body", frame=frame, tones=3, bevel=2.5, contrast=0.4, cast_shadow=3)
                cmd("auto_shade", layer="Face", frame=frame, tones=2, bevel=2, form=0.25, contrast=0.3, cast_shadow=3)
                cmd("auto_shade", layer="HairFront", frame=frame, tones=4, bevel=2.5, contrast=0.45, cast_shadow=0, min_region=3)
                cmd("auto_shade", layer="Arms", frame=frame, tones=3, bevel=2, contrast=0.4, cast_shadow=0)
                for name in ("Back", "HairBack", "Body", "Face", "HairFront", "Arms"):
                    cmd("outline", layer=name, frame=frame, mode="selout", darkness=0.48)
                cmd("pixel_cleanup", layer="HairFront", frame=frame)
                det = [[x, y, c] for (x, y), c in sorted(layers["Details"].px.items())]
                cmd("set_pixels", layer="Details", frame=frame, pixels=det)
                sp = [v for k, v in layers["FX"].px.items()]
                if sp:
                    cmd("draw_sparkle", layer="FX", frame=frame, points=[[x, y, s] for x, y, s in sp], color="#FFD9B8", core="#FFFFFF")
                frame += 1
        base = ci * 8
        cmd("animation_tag_create", name=f"{ch['name']}_idle", **{"from": base, "to": base + 3}, direction="pingpong")
        second = ch["pose"] if ch["pose"] in ("cheer", "wave") else "idle2"
        cmd("animation_tag_create", name=f"{ch['name']}_{second}", **{"from": base + 4, "to": base + 7})

    with open(os.path.join(here, "chibi.jsonl"), "w") as fh:
        fh.write("\n".join(lines) + "\n")

    show = list(lines)
    for i in range(frame):
        show.append(json.dumps({"op": "export_png", "frame": i, "scale": 1, "_save": f"frames/f{i:03d}.png"}))
    for ch in CAST:
        second = ch["pose"] if ch["pose"] in ("cheer", "wave") else "idle2"
        for tag in ("idle", second):
            show.append(json.dumps({"op": "export_gif", "tag": f"{ch['name']}_{tag}", "scale": 4, "_save": f"gif/{ch['name'].lower()}_{tag}.gif"}))
    show.append(json.dumps({"op": "export_atlas", "padding": 1, "image": "chibi_atlas.png", "_save": "chibi_atlas.png", "_json": True}))
    show.append(json.dumps({"op": "project_save", "_save": "pastel_chibi.pxs"}))
    with open(os.path.join(here, "chibi_showcase.jsonl"), "w") as fh:
        fh.write("\n".join(show) + "\n")
    print(f"{len(CAST)} characters, {frame} frames, {len(lines)} commands")


# ════ build_cast ════════════════════════════════════════════════════════════════════════════════

def build_cast(out):
    """Pixel Studio 2D cast — original anime-style pixel characters (32×52 sprites).

    Paper-doll pipeline used by sprite studios:
      1. Hand-authored region maps: head/face base, hair styles (front + back layers), outfits,
         eye stamps and accessories.
      2. Automatic pixel-art shading per region with 5-tone hue-shifted ramps (light from the
         top-left), priority-based inner lines and a selective (colored) outline.
      3. Hand detail passes: eye shine, hair strand highlights, cloth folds, blush.
      4. Animation by construction: idle (breathing, blink, hair sway) and a 4-frame walk.

    Output goes into Pixel Studio through `draw_ascii` (the same tool AI agents use), one frame
    per pose, tagged per character and animation. Characters are original designs.

        python3 build_cast.py  ->  cast.jsonl, cast_showcase.jsonl, preview PNGs (if Pillow exists)
    """
    W, H = 32, 52

    # ------------------------------------------------------------------ color ramps


    def hex2rgb(h):
        h = h.lstrip("#")
        return tuple(int(h[i:i + 2], 16) / 255 for i in (0, 2, 4))


    def rgb2hex(c):
        return "#%02X%02X%02X" % tuple(max(0, min(255, round(v * 255))) for v in c)


    def shift_hue(h, target, amount):
        d = ((target - h + 0.5) % 1.0) - 0.5
        return (h + max(-amount, min(amount, d))) % 1.0


    def ramp(base, dark=0.30, light=0.16):
        """5 tones: [outline, shadow, base, light, highlight], hue-shifted (shadows → purple, lights → yellow)."""
        r, g, b = hex2rgb(base)
        h, l, s = colorsys.rgb_to_hls(r, g, b)
        tones = []
        for k, dl in ((-2, -dark * 1.55), (-1, -dark * 0.62), (0, 0.0), (1, light * 0.62), (2, light)):
            hh = shift_hue(h, 0.76, 0.045 * -k) if k < 0 else shift_hue(h, 0.14, 0.03 * k) if k > 0 else h
            ss = min(1.0, s * (1.12 if k < 0 else 1.0 - 0.12 * k))
            ll = max(0.06, min(0.96, l + dl))
            tones.append(rgb2hex(colorsys.hls_to_rgb(hh, ll, ss)))
        return tones


    # ------------------------------------------------------------------ canvas of regions


    class Doll:
        """Region map: (x, y) -> (region, forced tone or None)."""

        def __init__(self):
            self.px = {}

        def span(self, y, x0, x1, region, tone=None):
            for x in range(x0, x1 + 1):
                if 0 <= x < W and 0 <= y < H:
                    self.px[(x, y)] = (region, tone)

        def spans(self, region, rows, tone=None):
            for y, x0, x1 in rows:
                self.span(y, x0, x1, region, tone)

        def mirror_spans(self, region, rows, tone=None):
            """Rows given for the left half (x ≤ 15); mirrored onto the right half."""
            for y, x0, x1 in rows:
                self.span(y, x0, x1, region, tone)
                self.span(y, 31 - x1, 31 - x0, region, tone)

        def stamp(self, x, y, rows, legend, mirror=False):
            for dy, row in enumerate(rows):
                for dx, ch in enumerate(row):
                    if ch == "." or ch == " ":
                        continue
                    reg, tone = legend[ch]
                    xx = x + (len(row) - 1 - dx if mirror else dx)
                    if 0 <= xx < W and 0 <= y + dy < H:
                        self.px[(xx, y + dy)] = (reg, tone)

        def erase(self, cells):
            for c in cells:
                self.px.pop(c, None)

        def shifted(self, dy, pred):
            """Copy with the cells matching pred(x, y, region) moved vertically by dy."""
            out = Doll()
            moving = {k: v for k, v in self.px.items() if pred(k[0], k[1], v[0])}
            for k, v in self.px.items():
                if k not in moving:
                    out.px[k] = v
            for (x, y), v in moving.items():
                if 0 <= y + dy < H:
                    out.px[(x, y + dy)] = v
            return out


    # Draw order / "in front" priority for inner lines (higher draws the line).
    PRIORITY = {"skin": 1, "leg": 2, "shoe": 4, "bottom": 3, "top": 3, "coat": 5, "white": 4, "accent": 6, "hair": 7, "hat": 8, "metal": 6, "weapon": 9, "hairback": 0}
    DETAIL = {"eyeL", "eyeD", "shine", "lash", "mouth", "blush", "sclera", "brow"}


    def shade(doll, pal):
        """Turns a region map into final colors using pixel-art lighting rules."""
        px = doll.px
        out = {}

        def reg(x, y):
            v = px.get((x, y))
            return v[0] if v else None

        for (x, y), (r, tone) in px.items():
            if r in DETAIL:
                out[(x, y)] = pal["eye"][{"eyeL": 3, "eyeD": 1, "lash": 0}.get(r, 2)] if r in ("eyeL", "eyeD", "lash") else {
                    "shine": "#FFFFFF", "sclera": "#F6F1EC", "mouth": pal["mouth"], "blush": pal["blush"], "brow": pal["hair"][0]}[r]
                continue
            ramp_ = pal[{"hairback": "hair"}.get(r, r)]
            if tone is not None:
                out[(x, y)] = ramp_[tone]
                continue
            n4 = [reg(x + 1, y), reg(x - 1, y), reg(x, y + 1), reg(x, y - 1)]
            # Selective outline on the silhouette.
            if any(n is None for n in n4):
                out[(x, y)] = ramp_[0]
                continue
            # Inner line where this region sits in front of another one.
            if any(n != r and n not in DETAIL and PRIORITY.get(r, 0) > PRIORITY.get(n, 0) for n in n4):
                out[(x, y)] = ramp_[0] if r in ("hair", "hat", "coat", "metal", "weapon", "accent") else ramp_[1]
                continue
            lit = reg(x - 1, y) != r or reg(x, y - 1) != r
            dark = reg(x + 1, y) != r or reg(x, y + 1) != r or reg(x + 2, y) != r or reg(x + 1, y + 1) != r
            if r == "skin":
                out[(x, y)] = ramp_[1] if dark and not lit else ramp_[3] if lit and not dark else ramp_[2]
            else:
                out[(x, y)] = ramp_[1] if dark else ramp_[3] if lit else ramp_[2]
        return out


    # ------------------------------------------------------------------ base body (front)


    def base_body(d, skirt_flare=True):
        # head
        d.spans("skin", [(4, 12, 19), (5, 10, 21), (6, 9, 22)] + [(y, 8, 23) for y in range(7, 13)] + [(13, 8, 23), (14, 9, 22), (15, 10, 21), (16, 12, 19), (17, 14, 17)])
        d.spans("skin", [(10, 7, 7), (11, 7, 7), (10, 24, 24), (11, 24, 24)])  # ears
        d.spans("skin", [(18, 14, 17), (19, 14, 17)])  # neck
        d.span(18, 14, 17, "skin", 1)  # under-chin shadow
        # torso (filled later by outfit), arms
        d.spans("skin", [(y, 10 + (1 if 25 <= y <= 29 else 0), 21 - (1 if 25 <= y <= 29 else 0)) for y in range(20, 31)])
        d.mirror_spans("skin", [(y, 7, 9) for y in range(21, 33)] + [(33, 7, 8), (34, 7, 8)])
        # hips + legs
        d.spans("skin", [(y, 10, 21) for y in range(31, 35)])
        d.mirror_spans("leg", [(y, 11, 14) for y in range(35, 46)] + [(46, 11, 14)])
        d.mirror_spans("shoe", [(47, 10, 14), (48, 10, 15), (49, 10, 15)])
        d.mirror_spans("shoe", [(50, 10, 15)], tone=0)


    def face(d, eyes="round", mouth="smile"):
        eye = {
            "round": ["llll", "lsdl", "lddl", "lEEl", ".ll."],
            "sharp": ["llll", "lsdd", "lddl", ".lEl", "...."],
            "soft": [".lll", "lsdl", "lddl", "lEEl", ".ll."],
        }[eyes]
        legend = {"l": ("lash", None), "s": ("shine", None), "E": ("eyeL", None), "d": ("eyeD", None), "w": ("sclera", None)}
        d.stamp(10, 9, eye, legend)
        d.stamp(18, 9, eye, legend, mirror=True)
        d.span(8, 10, 12, "brow")
        d.span(8, 19, 21, "brow")
        d.px[(16, 13)] = ("skin", 1)  # nose
        if mouth == "smile":
            d.span(15, 15, 16, "mouth")
        elif mouth == "open":
            d.span(15, 15, 16, "mouth")
            d.span(16, 15, 16, "mouth")
        elif mouth == "flat":
            d.span(15, 15, 15, "mouth")
        d.span(14, 9, 10, "blush")
        d.span(14, 21, 22, "blush")
        d.px[(15, 13)] = ("skin", 3)


    # ------------------------------------------------------------------ hair styles


    def hair_back(d, style):
        """Hair behind the head/body."""
        if style in ("long", "witch"):
            d.mirror_spans("hairback", [(y, 6, 9) for y in range(8, 34)] + [(34, 7, 9), (35, 8, 9)])
        if style == "twintails":
            d.mirror_spans("hairback", [(y, 2, 6) for y in range(9, 30)] + [(30, 3, 6), (31, 4, 6), (32, 4, 5)])
            d.mirror_spans("accent", [(8, 5, 7), (9, 5, 7)])
        if style == "ponytail":
            d.spans("hairback", [(y, 22, 27) for y in range(5, 26)] + [(26, 23, 27), (27, 24, 27), (28, 25, 26)])
            d.spans("accent", [(5, 22, 24), (6, 22, 24)])
        if style == "bob":
            d.mirror_spans("hairback", [(y, 6, 9) for y in range(8, 20)])


    def hair_front(d, style):
        """Hair cap + bangs over the head (hand-shaped clumps)."""
        cap = [(1, 12, 19), (2, 10, 21), (3, 8, 23), (4, 7, 24), (5, 7, 24), (6, 6, 25), (7, 6, 25)]
        d.spans("hair", cap)
        if style in ("long", "bob", "twintails", "ponytail", "witch"):
            # side-swept bangs with three pointed clumps
            d.spans("hair", [(8, 6, 11), (8, 13, 18), (8, 20, 25), (9, 6, 10), (9, 14, 17), (9, 21, 25), (10, 6, 9), (10, 15, 16), (10, 22, 25)])
            d.mirror_spans("hair", [(y, 6, 7) for y in range(11, 19)])  # side locks framing the face
            d.mirror_spans("hair", [(19, 6, 6), (20, 6, 6)])
        if style == "spiky":
            d.spans("hair", [(0, 11, 12), (0, 18, 19), (1, 9, 22), (2, 7, 24), (3, 6, 25)])
            d.spans("hair", [(8, 6, 10), (8, 12, 15), (8, 17, 21), (8, 23, 25), (9, 6, 8), (9, 13, 14), (9, 18, 20), (9, 24, 25), (10, 6, 7), (10, 19, 19), (10, 25, 25)])
            d.mirror_spans("hair", [(11, 6, 7), (12, 6, 7), (13, 7, 7)])
            d.spans("hair", [(2, 4, 6), (3, 4, 5), (1, 25, 27), (2, 26, 27)])  # spikes
        if style == "short":
            d.spans("hair", [(8, 6, 12), (8, 14, 19), (8, 21, 25), (9, 6, 9), (9, 15, 18), (9, 22, 25), (10, 6, 8), (10, 23, 25)])
            d.mirror_spans("hair", [(11, 6, 7), (12, 7, 7)])
        # anime "angel ring" sheen across the crown
        for (x, y, t) in [(9, 4, 3), (10, 4, 4), (11, 4, 4), (12, 3, 4), (13, 3, 4), (14, 3, 3), (17, 3, 3), (18, 3, 4), (19, 4, 4), (20, 4, 4), (21, 5, 3)]:
            if d.px.get((x, y), ("",))[0] == "hair":
                d.px[(x, y)] = ("hair", t)
        # darker strands between bang clumps
        for (x, y) in ((12, 6), (12, 7), (19, 6), (19, 7), (16, 5), (16, 6)):
            if d.px.get((x, y), ("",))[0] == "hair":
                d.px[(x, y)] = ("hair", 1)
        # strand highlights (top-left lit)
        for (x, y) in ((11, 3), (12, 3), (13, 2), (10, 4), (16, 3), (17, 2), (20, 4), (21, 5)):
            if d.px.get((x, y), ("",))[0] == "hair":
                d.px[(x, y)] = ("hair", 4)
        for (x, y) in ((9, 5), (14, 4), (19, 4)):
            if d.px.get((x, y), ("",))[0] == "hair":
                d.px[(x, y)] = ("hair", 3)


    # ------------------------------------------------------------------ outfits


    def outfit(d, kind):
        sleeves = lambda reg, end=27, tone=None: d.mirror_spans(reg, [(y, 7, 9) for y in range(21, end + 1)], tone)  # noqa: E731
        def torso(reg, y0=20, y1=30):
            # shoulders → narrower waist → hips: less boxy silhouette
            d.spans(reg, [(y, 10 + (1 if 25 <= y <= 29 else 0), 21 - (1 if 25 <= y <= 29 else 0)) for y in range(y0, y1 + 1)])
        if kind == "sailor":
            torso("white")
            sleeves("white", 25)
            d.mirror_spans("coat", [(20, 10, 15), (21, 10, 14), (22, 10, 13), (23, 11, 13), (24, 12, 14)])  # navy collar
            d.spans("accent", [(23, 14, 17), (24, 15, 16), (25, 14, 17), (26, 14, 14), (26, 17, 17)])  # ribbon
            d.spans("bottom", [(31, 10, 21), (32, 9, 22), (33, 9, 22), (34, 8, 23), (35, 8, 23), (36, 8, 23)])
            for x in (10, 13, 16, 19, 22):
                d.px[(x, 35)] = ("bottom", 1)
                d.px[(x, 36)] = ("bottom", 1)  # pleats
            d.mirror_spans("white", [(y, 11, 14) for y in range(40, 46)])  # knee socks
        elif kind == "adventurer":
            torso("top")
            torso("coat", 20, 31)
            d.spans("top", [(y, 14, 17) for y in range(20, 29)])  # open jacket shows shirt
            sleeves("coat", 29)
            d.spans("accent", [(30, 10, 21)])  # belt
            d.px[(15, 30)] = ("metal", 3)
            d.px[(16, 30)] = ("metal", 3)
            d.mirror_spans("bottom", [(y, 10, 15) for y in range(31, 35)] + [(y, 11, 14) for y in range(35, 44)])  # trousers
            d.mirror_spans("shoe", [(44, 10, 14), (45, 10, 14), (46, 10, 14)])  # boots
        elif kind == "robe":
            torso("coat")
            sleeves("coat", 30)
            d.mirror_spans("coat", [(31, 6, 9), (32, 6, 9)])  # wide sleeve ends
            d.spans("coat", [(y, 9 - (y - 31) // 3, 22 + (y - 31) // 3) for y in range(31, 46)])  # long robe
            d.spans("accent", [(29, 10, 21)])
            d.spans("accent", [(y, 15, 16) for y in range(20, 29)])  # trim
            d.mirror_spans("white", [(20, 13, 15), (21, 14, 15)])
        elif kind == "armor":
            torso("metal")
            sleeves("top", 29)
            d.mirror_spans("metal", [(20, 6, 10), (21, 6, 10), (22, 6, 9), (23, 7, 9)])  # pauldrons
            d.spans("accent", [(y, 15, 16) for y in range(21, 30)])  # tabard stripe
            d.spans("accent", [(30, 10, 21)])
            d.spans("bottom", [(y, 10, 21) for y in range(31, 37)])
            d.mirror_spans("metal", [(y, 11, 14) for y in range(38, 46)])  # greaves
            d.mirror_spans("metal", [(37, 11, 14)], tone=0)
        elif kind == "maid":
            torso("coat")
            sleeves("coat", 26)
            d.mirror_spans("white", [(26, 7, 9), (27, 7, 9)])  # cuffs
            d.spans("white", [(y, 12, 19) for y in range(24, 31)])  # apron bib
            d.spans("coat", [(31, 9, 22), (32, 8, 23), (33, 8, 23), (34, 7, 24), (35, 7, 24), (36, 7, 24), (37, 7, 24)])
            d.spans("white", [(y, 11, 20) for y in range(31, 37)])  # apron skirt
            d.spans("white", [(37, 7, 24)])  # frill
            d.spans("accent", [(20, 14, 17), (21, 15, 16)])  # bow
            d.mirror_spans("white", [(y, 11, 14) for y in range(38, 46)])  # stockings
        elif kind == "idol":
            torso("white")
            sleeves("white", 23)
            d.spans("accent", [(y, 10, 21) for y in range(26, 29)])  # sash
            d.spans("top", [(31, 9, 22), (32, 8, 23), (33, 7, 24), (34, 6, 25)])  # ruffled skirt
            d.spans("white", [(35, 6, 25)])
            d.mirror_spans("white", [(y, 11, 14) for y in range(39, 46)])
            d.spans("accent", [(20, 13, 18), (21, 15, 16)])
        elif kind == "fighter":
            d.spans("top", [(y, 10, 21) for y in range(20, 26)])  # crop top
            d.spans("accent", [(y, 10, 21) for y in range(30, 32)])
            d.spans("bottom", [(y, 10, 21) for y in range(32, 36)] + [(36, 10, 14), (36, 17, 21)])  # shorts
            d.mirror_spans("shoe", [(y, 10, 14) for y in range(41, 47)])  # tall boots
            d.mirror_spans("accent", [(26, 7, 9), (27, 7, 9)])  # wraps
        elif kind == "coat":
            torso("coat", 20, 31)
            sleeves("coat", 30)
            d.spans("coat", [(y, 9, 22) for y in range(32, 42)])  # long coat tails
            d.spans("coat", [(y, 15, 16) for y in range(32, 42)], tone=1)
            d.spans("white", [(20, 14, 17), (21, 15, 16)])
            d.spans("accent", [(y, 15, 16) for y in range(22, 31)])
            d.mirror_spans("bottom", [(y, 11, 14) for y in range(42, 46)])
        elif kind == "ninja":
            torso("coat")
            sleeves("coat", 28)
            d.mirror_spans("coat", [(20, 10, 12), (21, 11, 13), (22, 12, 14), (23, 13, 15)], tone=1)  # gi fold
            d.spans("accent", [(19, 12, 19), (20, 11, 20)])  # scarf
            d.spans("accent", [(21, 20, 22), (22, 21, 23), (23, 22, 24), (24, 22, 24)])  # scarf tail
            d.spans("white", [(29, 10, 21)])  # belt
            d.mirror_spans("bottom", [(y, 10, 15) for y in range(30, 36)] + [(y, 11, 14) for y in range(36, 44)])
            d.mirror_spans("white", [(44, 11, 14), (45, 11, 14)])  # leg wraps
        elif kind == "blazer":
            torso("white")
            torso("coat")
            d.spans("white", [(y, 14, 17) for y in range(20, 27)])
            d.spans("accent", [(21, 15, 16), (22, 15, 16), (23, 15, 16), (24, 14, 17), (25, 15, 16)])  # tie
            sleeves("coat", 29)
            d.spans("bottom", [(31, 10, 21), (32, 9, 22), (33, 9, 22), (34, 8, 23), (35, 8, 23)])
            for x in range(9, 23, 3):
                d.px[(x, 33)] = ("bottom", 1)
                d.px[(x, 34)] = ("bottom", 1)
            d.mirror_spans("leg", [(y, 11, 14) for y in range(36, 46)])


    def hat(d, kind):
        if kind == "witch":
            d.spans("hat", [(-0, 18, 19), (1, 17, 20), (2, 16, 20)])
            d.spans("hat", [(y, 13 - (y - 3) // 2, 21) for y in range(3, 7)])
            d.spans("hat", [(7, 3, 28), (8, 4, 27)])  # brim
            d.spans("accent", [(6, 11, 21)])
            d.spans("hat", [(0, 20, 21), (1, 21, 22)])  # tip bends
        if kind == "cap":
            d.spans("hat", [(1, 11, 20), (2, 9, 22), (3, 8, 23), (4, 7, 24), (5, 7, 24)])
            d.spans("hat", [(6, 7, 27), (7, 14, 28)])  # visor
            d.px[(15, 3)] = ("accent", 3)
            d.px[(16, 3)] = ("accent", 3)
        if kind == "headband":
            d.spans("white", [(3, 9, 22)])
            d.mirror_spans("white", [(2, 9, 10), (1, 9, 9)])


    def weapon(d, kind):
        if kind == "sword":
            d.spans("metal", [(y, 26, 26) for y in range(18, 31)])
            d.spans("metal", [(y, 27, 27) for y in range(19, 31)], tone=3)
            d.spans("accent", [(31, 24, 29)])
            d.spans("leg", [(32, 26, 27), (33, 26, 27)], tone=0)
        if kind == "staff":
            d.spans("weapon", [(y, 4, 4) for y in range(10, 49)])
            d.spans("accent", [(7, 3, 5), (8, 2, 6), (9, 3, 5)])
            d.px[(4, 8)] = ("accent", 4)


    # ------------------------------------------------------------------ cast (original characters)

    CAST = [
        dict(name="Aoi", hair="long", hair_c="#2B3F8C", skin="#F6CDB0", eye="#3C7BE0", outfit="sailor", top="#F2F2F6", coat="#26336E", bottom="#2B3570",
             accent="#D9334A", leg="#F6CDB0", shoe="#5A3A2A", white="#F4F4F8", eyes="round"),
        dict(name="Ren", hair="spiky", hair_c="#6B3A1E", skin="#F0C29E", eye="#2F8F5A", outfit="adventurer", top="#E9DFC7", coat="#3E7D4A", bottom="#3A5E7A",
             accent="#7A4B28", leg="#3A5E7A", shoe="#4A3020", white="#EDEDED", eyes="sharp", hat="cap", hat_c="#C8323A", weapon="sword"),
        dict(name="Mira", hair="witch", hair_c="#7E4FC7", skin="#F7D4BC", eye="#D9902F", outfit="robe", top="#3A2C5E", coat="#3A2C5E", bottom="#3A2C5E",
             accent="#F09A2A", leg="#3A2C5E", shoe="#2C2238", white="#F3E9D6", eyes="soft", hat="witch", hat_c="#2F2650", weapon="staff", weapon_c="#7A4B2A"),
        dict(name="Kai", hair="short", hair_c="#E8C24A", skin="#F3C6A5", eye="#3F6FD8", outfit="armor", top="#3B4A7A", coat="#3B4A7A", bottom="#3B4A7A",
             accent="#2F5FC4", leg="#3B4A7A", shoe="#5E6B85", white="#EEEEEE", eyes="sharp", metal="#AEBBD0", weapon="sword"),
        dict(name="Hana", hair="bob", hair_c="#F29BB8", skin="#F8D5BF", eye="#C94A6A", outfit="maid", top="#2A2A3A", coat="#2A2A3A", bottom="#2A2A3A",
             accent="#D9334A", leg="#F4F4F8", shoe="#2A2234", white="#F7F7FA", eyes="round", hat="headband"),
        dict(name="Sora", hair="twintails", hair_c="#3FC9D9", skin="#F8D5BF", eye="#E2508A", outfit="idol", top="#F57FB0", coat="#F57FB0", bottom="#F57FB0",
             accent="#FFD24A", leg="#F8D5BF", shoe="#E2508A", white="#FFFFFF", eyes="round"),
        dict(name="Rika", hair="ponytail", hair_c="#D93A2E", skin="#EFC09C", eye="#37A85A", outfit="fighter", top="#D7363F", coat="#D7363F", bottom="#262838",
             accent="#F0C04A", leg="#EFC09C", shoe="#3A2A26", white="#F2F2F2", eyes="sharp"),
        dict(name="Yuki", hair="spiky", hair_c="#E6ECF5", skin="#F6D3BE", eye="#3FA2E8", outfit="coat", top="#2F6FC4", coat="#2B4F9C", bottom="#2A2E44",
             accent="#7FD3F5", leg="#2A2E44", shoe="#2A2234", white="#F7F7FA", eyes="soft"),
        dict(name="Kuro", hair="short", hair_c="#262433", skin="#E9BB97", eye="#C43A3A", outfit="ninja", top="#2C2F3F", coat="#2C2F3F", bottom="#2C2F3F",
             accent="#C8323A", leg="#2C2F3F", shoe="#1F1F2A", white="#D8D2C2", eyes="sharp", mouth="flat"),
        dict(name="Luna", hair="long", hair_c="#F2D06B", skin="#F7D3B9", eye="#8A4FD8", outfit="blazer", top="#F4F4F8", coat="#9C2F3A", bottom="#2F6A6A",
             accent="#2F4FA0", leg="#2C2C3A", shoe="#3A2A26", white="#F4F4F8", eyes="round"),
    ]


    def skin_ramp(base):
        """Skin needs warm, low-saturation shadows (no red hands): outline is a deep brown."""
        r, g, b = hex2rgb(base)
        h, l, s = colorsys.rgb_to_hls(r, g, b)
        tone = lambda dh, dl, ds: rgb2hex(colorsys.hls_to_rgb((h + dh) % 1, max(0.05, min(0.97, l + dl)), max(0, min(1, s * ds))))  # noqa: E731
        return [tone(0.0, -0.50, 0.55), tone(-0.012, -0.13, 0.95), base, tone(0.01, 0.05, 0.9), tone(0.02, 0.09, 0.8)]


    def palette_for(c):
        return {
            "skin": skin_ramp(c["skin"]),
            "hair": ramp(c["hair_c"]),
            "eye": ramp(c["eye"], 0.34, 0.2),
            "top": ramp(c["top"]),
            "coat": ramp(c["coat"]),
            "bottom": ramp(c["bottom"]),
            "accent": ramp(c["accent"]),
            "leg": skin_ramp(c["leg"]) if c["leg"] == c["skin"] else ramp(c["leg"], 0.22, 0.1),
            "shoe": ramp(c["shoe"]),
            "white": ramp(c["white"], 0.22, 0.05),
            "metal": ramp(c.get("metal", "#B5C0D2"), 0.38, 0.2),
            "hat": ramp(c.get("hat_c", c["hair_c"])),
            "weapon": ramp(c.get("weapon_c", "#8A6A4A")),
            "mouth": "#B5545C",
            "blush": "#F7B9AE",
        }


    def build(c, breathe=0, blink=False, sway=0, step=None):
        d = Doll()
        hair_back(d, c["hair"])
        base_body(d)
        outfit(d, c["outfit"])
        face(d, c.get("eyes", "round"), c.get("mouth", "smile"))
        hair_front(d, c["hair"])
        if c.get("hat"):
            hat(d, c["hat"])
        if c.get("weapon"):
            weapon(d, c["weapon"])
        if blink:
            for (x, y), (r, _) in list(d.px.items()):
                if r in ("eyeL", "eyeD", "shine", "sclera", "lash") and 10 <= y <= 13:
                    d.px[(x, y)] = ("skin", None)
            d.span(12, 10, 13, "lash")
            d.span(12, 18, 21, "lash")
        # Animation by construction.
        if step is not None:
            lift = {0: (2, 0), 1: (0, 0), 2: (0, 2), 3: (0, 0)}[step]
            bob = 1 if step in (1, 3) else 0
            d = d.shifted(-bob, lambda x, y, r: y < 35)  # upper body rises on passing frames
            for side, amount in (("L", lift[0]), ("R", lift[1])):
                if amount:
                    d = d.shifted(-amount, lambda x, y, r, s=side: y >= 38 and ((x < 16) == (s == "L")) and r in ("leg", "shoe", "white", "metal", "bottom"))
        if breathe:
            d = d.shifted(breathe, lambda x, y, r: y < 31 and r != "hairback")
        if sway:
            d = d.shifted(sway, lambda x, y, r: r == "hairback" and y > 20)
        return d


    def to_rows(colors):
        """Final colors -> ASCII rows + legend for draw_ascii."""
        keys = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#$%&*+=?@^~<>/|!"
        legend, inv = {}, {}
        rows = []
        for y in range(H):
            row = []
            for x in range(W):
                col = colors.get((x, y))
                if col is None:
                    row.append(".")
                    continue
                if col not in inv:
                    inv[col] = keys[len(inv)]
                    legend[inv[col]] = col
                row.append(inv[col])
            rows.append("".join(row))
        return rows, legend


    # ------------------------------------------------------------------ script

    cmds = []


    def cmd(op, **a):
        cmds.append({"op": op, "actor": "cast-builder", **a})


    cmd("project_new", width=W, height=H, name="Pixel Studio Cast")
    cmd("layer_rename", layer=0, name="Character")
    frame = 0
    poses = []
    for c in CAST:
        pal = palette_for(c)
        seq = [("idle", dict(breathe=0)), ("idle", dict(breathe=0, sway=1)), ("idle", dict(breathe=1, sway=1)), ("idle", dict(breathe=1, blink=True))]
        seq += [("walk", dict(step=i)) for i in range(4)]
        start = {}
        for anim, kw in seq:
            colors = shade(build(c, **kw), pal)
            rows, legend = to_rows(colors)
            if frame > 0:
                cmd("frame_add", index=frame, duration=180 if anim == "idle" else 140)
            else:
                cmd("frame_set_duration", frame=0, duration=180)
            cmd("draw_ascii", frame=frame, layer="Character", rows=rows, legend=legend)
            start.setdefault(anim, frame)
            poses.append((c["name"], anim, colors))
            frame += 1
        cmd("animation_tag_create", name=f"{c['name']}_idle", **{"from": start["idle"], "to": start["idle"] + 3}, direction="pingpong")
        cmd("animation_tag_create", name=f"{c['name']}_walk", **{"from": start["walk"], "to": start["walk"] + 3})

    here = out
    with open(os.path.join(here, "cast.jsonl"), "w") as f:
        for c in cmds:
            f.write(json.dumps(c, separators=(",", ":")) + "\n")

    show = list(cmds)
    for c in CAST:
        for anim in ("idle", "walk"):
            show.append({"op": "export_gif", "_save": f"gifs/{c['name'].lower()}_{anim}.gif", "tag": f"{c['name']}_{anim}", "scale": 6})
    show.append({"op": "export_spritesheet", "_save": "cast_sheet_1x.png", "columns": 8, "_json": True})
    show.append({"op": "export_spritesheet", "_save": "cast_sheet_4x.png", "columns": 8, "scale": 4})
    show.append({"op": "export_normal_map", "_save": "aoi_normal_8x.png", "frame": 0, "scale": 8})
    show.append({"op": "project_save", "_save": "PixelStudio_Cast.pxs"})
    with open(os.path.join(here, "cast_showcase.jsonl"), "w") as f:
        for c in show:
            f.write(json.dumps(c, separators=(",", ":")) + "\n")

    # Quick lineup preview without the engine (artist feedback loop).
    try:
        from PIL import Image

        names = [c["name"] for c in CAST]
        lineup = Image.new("RGBA", (W * len(CAST) + 8 * (len(CAST) + 1), H + 16), (56, 132, 214, 255))
        for i, c in enumerate(CAST):
            colors = next(col for (n, a, col) in poses if n == c["name"] and a == "idle")
            for (x, y), hexc in colors.items():
                lineup.putpixel((8 + i * (W + 8) + x, 8 + y), tuple(int(hexc[k:k + 2], 16) for k in (1, 3, 5)) + (255,))
        lineup.resize((lineup.width * 4, lineup.height * 4), Image.NEAREST).save(os.path.join(here, "preview_lineup.png"))
    except ImportError:
        pass
    print(f"{len(CAST)} characters, {frame} frames, {len(cmds)} commands")


# ════ build_vfx2d ═══════════════════════════════════════════════════════════════════════════════

def build_vfx2d(out):
    """Pixel Studio 2D pixel-art VFX pack (64×64 frames).

    Classic pixel-art effect rules: strict 4–6 color band palettes, clean clusters, no
    anti-aliasing, bright core → saturated body → dark edge, and seamless loops where the effect
    loops (time is mapped onto a circle so frame N wraps to frame 0).

        fire (16, loop) · slash (8) · hit spark (6) · magic burst (12) · smoke puff (8)
        lightning (8) · heal (12, loop) · explosion (12)

    Frames are written with `draw_ascii` and tagged per effect.
    """
    S = 64
    TAU = math.tau


    def hnoise(x, y, z, seed=0):
        n = (int(x) * 374761393 + int(y) * 668265263 + int(z) * 2147483647 + seed * 1013904223) & 0xFFFFFFFF
        n = (n ^ (n >> 13)) * 1274126177 & 0xFFFFFFFF
        return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0


    def vnoise(x, y, z, seed=0):
        xi, yi, zi = math.floor(x), math.floor(y), math.floor(z)
        fx, fy, fz = x - xi, y - yi, z - zi
        f = lambda t: t * t * (3 - 2 * t)  # noqa: E731
        sx, sy, sz = f(fx), f(fy), f(fz)
        acc = 0.0
        for dz in (0, 1):
            for dy in (0, 1):
                for dx in (0, 1):
                    w = (sx if dx else 1 - sx) * (sy if dy else 1 - sy) * (sz if dz else 1 - sz)
                    acc += w * hnoise(xi + dx, yi + dy, zi + dz, seed)
        return acc


    def loop_noise(x, y, t, period, seed=0, r=1.2):
        """Noise that loops over `period` frames (time on a circle)."""
        a = TAU * t / period
        return vnoise(x + r * math.cos(a), y + r * math.sin(a), 0.0, seed)


    def band(v, thresholds, colors):
        for th, c in zip(thresholds, colors):
            if v >= th:
                return c
        return None


    # ------------------------------------------------------------------ effects

    FIRE = ["#FFF9D6", "#FFE066", "#FFA431", "#F2581F", "#B72727", "#5E1A2A"]


    def fire(t, n=16):
        img = {}
        for y in range(S):
            for x in range(S):
                dx = (x - 32) / 16.0
                h = (58 - y) / 44.0  # 0 at base → 1 at top
                if h < 0 or h > 1.15:
                    continue
                width = max(0.0, 1 - h) ** 0.8 * (1.0 - 0.15 * math.sin(h * 6 + TAU * t / n))
                sway = 0.25 * math.sin(TAU * t / n + h * 3.5) * h
                body = 1 - abs(dx - sway) / max(width, 0.01)
                turb = loop_noise(x * 0.18, y * 0.16 + t * 0.9, t, n, 3) * 0.55 + loop_noise(x * 0.45, y * 0.4 + t * 1.6, t, n, 9) * 0.3
                v = body * 0.95 + turb * 0.55 - h * 0.55
                c = band(v, [0.95, 0.78, 0.6, 0.42, 0.28, 0.18], FIRE)
                if c:
                    img[(x, y)] = c
        # embers rising (looping)
        for i in range(10):
            p = (t / n + i * 0.137) % 1
            ex = 32 + math.sin(TAU * (p + i * 0.31)) * (4 + i % 5)
            ey = 46 - p * 44
            img[(round(ex), round(ey))] = FIRE[1] if p < 0.5 else FIRE[3]
        # logs
        for x in range(18, 47):
            for y in range(57, 61):
                if abs(x - 32) + (y - 57) * 3 < 16:
                    img[(x, y)] = "#6E4128" if (x + y) % 5 else "#4A2A1C"
        for x in range(20, 45):
            img[(x, 61)] = "#3A1F16"
        return img


    SLASH = ["#FFFFFF", "#D9FBFF", "#7FE3FF", "#3A8FE0", "#26407A"]


    def slash(t, n=8):
        img = {}
        p = t / (n - 1)
        start, span = math.radians(200), math.radians(220)
        head = start + span * min(1.0, 0.25 + p * 1.5)
        tail = start + span * min(0.9, max(0.0, (p - 0.2) * 1.3))
        fade = max(0.0, (p - 0.55) / 0.45)
        for y in range(S):
            for x in range(S):
                dx, dy = x - 32 + 0.5, y - 34 + 0.5
                r = math.hypot(dx, dy)
                a = math.atan2(-dy, dx) % TAU
                if a < math.pi / 2:
                    a += TAU
                if not (tail <= a <= head):
                    continue
                u = (a - tail) / max(head - tail, 1e-3)  # 0 at tail, 1 at head
                thick = 1.5 + 7 * u * (1 - 0.7 * fade)
                d = abs(r - 22)
                if d > thick:
                    continue
                k = d / thick
                c = SLASH[0] if k < 0.25 and u > 0.4 else SLASH[1] if k < 0.5 else SLASH[2] if k < 0.75 else SLASH[3]
                if fade > 0.5 and k > 0.4:
                    c = SLASH[4]
                img[(x, y)] = c
        return img


    SPARK = ["#FFFFFF", "#FFF2A6", "#FFC53D", "#F2742B", "#9C2F2F"]


    def hit(t, n=6):
        img = {}
        rng = random.Random(4)
        rays = [(rng.uniform(0, TAU), rng.uniform(14, 26)) for _ in range(8)]
        grow = [0.45, 0.9, 1.0, 0.85, 0.6, 0.3][t]
        for ang, ln in rays:
            L = ln * grow
            for s in range(int(L)):
                w = max(0, 2.6 * (1 - s / max(L, 1)) * (1.2 - t * 0.18))
                for o in range(-int(w), int(w) + 1):
                    x = 32 + math.cos(ang) * s - math.sin(ang) * o * 0.6
                    y = 32 + math.sin(ang) * s + math.cos(ang) * o * 0.6
                    k = s / max(L, 1)
                    img[(round(x), round(y))] = SPARK[0] if k < 0.3 and t < 3 else SPARK[1] if k < 0.55 else SPARK[2] if k < 0.8 else SPARK[3]
        core = [6, 9, 7, 4, 2, 0][t]
        for y in range(32 - core, 33 + core):
            for x in range(32 - core, 33 + core):
                if abs(x - 32) + abs(y - 32) <= core:
                    img[(x, y)] = SPARK[0] if abs(x - 32) + abs(y - 32) < core * 0.6 else SPARK[1]
        if t >= 3:
            for i in range(10):
                a = TAU * i / 10 + 0.3
                r = 12 + t * 4
                img[(round(32 + math.cos(a) * r), round(32 + math.sin(a) * r))] = SPARK[4 if t > 4 else 3]
        return img


    ARC = ["#FFFFFF", "#E7C9FF", "#B57BFF", "#7A3FE0", "#3D2690", "#7FF3FF"]


    def magic(t, n=12):
        img = {}
        p = t / (n - 1)
        R = 6 + 22 * (1 - (1 - p) ** 2)
        thick = 4 * (1 - p) + 1
        for y in range(S):
            for x in range(S):
                r = math.hypot(x - 32 + .5, y - 32 + .5)
                d = abs(r - R)
                if d <= thick:
                    k = d / thick
                    img[(x, y)] = ARC[1] if k < 0.35 else ARC[2] if k < 0.7 else ARC[3]
                elif r < R * 0.55 and p < 0.5:
                    img[(x, y)] = ARC[0] if r < R * 0.25 else ARC[1]
        # rotating glyph spokes
        for i in range(6):
            a = TAU * i / 6 + p * 2.2
            for s in range(int(R * 0.6), int(R * 0.85)):
                img[(round(32 + math.cos(a) * s), round(32 + math.sin(a) * s))] = ARC[5]
        # sparkles flung outward
        rng = random.Random(8)
        for i in range(18):
            a = rng.uniform(0, TAU)
            r = R * (0.8 + rng.uniform(0, 0.6)) + p * 6
            x, y = round(32 + math.cos(a) * r), round(32 + math.sin(a) * r)
            if (t + i) % 3:
                img[(x, y)] = ARC[0]
                if p < 0.7:
                    for o in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                        img[(x + o[0], y + o[1])] = ARC[2]
        return img


    SMOKE = ["#E9E4E6", "#C5BDC2", "#9A9098", "#6E6470", "#4A4250"]


    def smoke(t, n=8):
        img = {}
        p = t / (n - 1)
        rng = random.Random(12)
        puffs = [(rng.uniform(-10, 10), rng.uniform(-6, 6), rng.uniform(5, 9)) for _ in range(7)]
        for (ox, oy, r0) in puffs:
            cx, cy = 32 + ox * (1 + p), 40 + oy - p * 14
            r = r0 * (0.6 + p * 0.9)
            for y in range(int(cy - r - 1), int(cy + r + 2)):
                for x in range(int(cx - r - 1), int(cx + r + 2)):
                    d = math.hypot(x - cx, y - cy) / r
                    if d > 1:
                        continue
                    shade_ = d * 0.6 + (y - cy + r) / (2 * r) * 0.5 + p * 0.5
                    c = band(1 - shade_, [0.75, 0.55, 0.35, 0.15, -0.2], SMOKE)
                    # dissolve with an ordered dither as it fades
                    if p > 0.55 and ((x + y * 2) % 4) < (p - 0.55) * 9:
                        continue
                    if c:
                        img[(x, y)] = c
        return img


    BOLT = ["#FFFFFF", "#E3F6FF", "#8FD8FF", "#3F8CFF", "#2A3FA0"]


    def lightning(t, n=8):
        img = {}
        rng = random.Random(30 + t // 2)
        if t in (6, 7):
            rng = random.Random(77)
        pts = [(32, 0)]
        x = 32.0
        for y in range(4, 62, 4):
            x += rng.uniform(-5, 5)
            x = max(10, min(54, x))
            pts.append((x, y))
        branches = []
        for _ in range(3):
            i = rng.randrange(2, len(pts) - 3)
            bx, by = pts[i]
            b = [(bx, by)]
            for k in range(4):
                bx += rng.choice((-1, 1)) * rng.uniform(2, 5)
                by += 4
                b.append((bx, by))
            branches.append(b)
        intensity = [1, 0.6, 1, 0.8, 0.5, 0.9, 0.3, 0.15][t]

        def stroke(path, width):
            for (x0, y0), (x1, y1) in zip(path, path[1:]):
                steps = int(max(abs(x1 - x0), abs(y1 - y0))) + 1
                for s in range(steps + 1):
                    u = s / steps
                    x, y = x0 + (x1 - x0) * u, y0 + (y1 - y0) * u
                    for o in range(-width, width + 1):
                        c = BOLT[0] if o == 0 else BOLT[2] if abs(o) == 1 else BOLT[3]
                        key = (round(x) + o, round(y))
                        if key not in img or c == BOLT[0]:
                            img[key] = c

        stroke(pts, 2 if intensity > 0.7 else 1)
        for b in branches:
            stroke(b, 1 if intensity > 0.5 else 0)
        if intensity < 0.4:
            for k in list(img):
                if img[k] != BOLT[0] and (k[0] + k[1]) % 2:
                    img[k] = BOLT[4]
        # impact glow at the ground
        for y in range(56, 64):
            for x in range(20, 45):
                d = math.hypot((x - 32) / 1.8, y - 62)
                if d < 7 * intensity:
                    img.setdefault((x, y), BOLT[2] if d < 3 else BOLT[3])
        return img


    HEAL = ["#FFFFFF", "#D9FFC9", "#8BF07A", "#3FBF5A", "#1F7A4A"]


    def heal(t, n=12):
        img = {}
        for i in range(9):
            p = (t / n + i / 9) % 1
            x = 32 + math.sin(TAU * (p * 0.8 + i * 0.37)) * (6 + (i % 4) * 4)
            y = 58 - p * 50
            size = 2 if p < 0.75 else 1
            col = HEAL[0] if p < 0.4 else HEAL[2] if p < 0.75 else HEAL[3]
            for o in range(-size, size + 1):
                img[(round(x) + o, round(y))] = col
                img[(round(x), round(y) + o)] = col
        # soft rising column of light (dithered)
        for y in range(14, 58):
            for x in range(26, 39):
                k = abs(x - 32) / 6.5 + (58 - y) / 70
                if k < 1 and ((x + y + t) % 3 == 0 if k > 0.55 else (x + y) % 2 == 0):
                    img.setdefault((x, y), HEAL[3] if k > 0.55 else HEAL[2])
        # swirling ground ring
        for k in range(80):
            a = TAU * k / 80 + TAU * t / n
            x = 32 + math.cos(a) * 18
            y = 56 + math.sin(a) * 5
            if math.sin(3 * a - TAU * t / n) > 0:
                img[(round(x), round(y))] = HEAL[2] if math.sin(a) > 0 else HEAL[4]
        return img


    BOOM = ["#FFFFFF", "#FFF0A6", "#FFB53D", "#F25F22", "#A52A2A", "#5A4A4E", "#3A3036"]


    def explosion(t, n=12):
        img = {}
        p = t / (n - 1)
        if t < 2:
            r = 6 + t * 8
            for y in range(S):
                for x in range(S):
                    d = math.hypot(x - 32, y - 34)
                    if d < r:
                        img[(x, y)] = BOOM[0] if d < r * 0.6 else BOOM[1]
            return img
        R = 10 + 18 * min(1, p * 1.6)
        for y in range(S):
            for x in range(S):
                d = math.hypot(x - 32, (y - 34) * 1.1)
                nz = loop_noise(x * 0.22, y * 0.22, t, n, 5, 0.3)
                edge = R * (0.82 + 0.3 * nz)
                if d > edge:
                    continue
                heat = (1 - d / edge) * (1 - p) * 1.4 + nz * 0.3 - p * 0.35
                if p > 0.45 and d > edge * 0.55 - (p - 0.45) * 20:
                    c = BOOM[5] if nz > 0.45 else BOOM[6]
                    if p > 0.8 and (x + y) % 2:
                        continue
                else:
                    c = band(heat, [0.95, 0.7, 0.45, 0.22, -1], BOOM[:5])
                if c:
                    img[(x, y)] = c
        rng = random.Random(3)
        for i in range(12):
            a = rng.uniform(0, TAU)
            r = R + 4 + p * 14
            img[(round(32 + math.cos(a) * r), round(34 + math.sin(a) * r * 0.9))] = BOOM[2] if t < 8 else BOOM[4]
        return img


    EFFECTS = [
        ("fire", fire, 16, 70, "forward"),
        ("slash", slash, 8, 45, "forward"),
        ("hit", hit, 6, 50, "forward"),
        ("magic", magic, 12, 60, "forward"),
        ("smoke", smoke, 8, 80, "forward"),
        ("lightning", lightning, 8, 50, "forward"),
        ("heal", heal, 12, 70, "forward"),
        ("explosion", explosion, 12, 60, "forward"),
    ]


    def rows_legend(img):
        keys = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        inv, legend, rows = {}, {}, []
        for y in range(S):
            r = []
            for x in range(S):
                c = img.get((x, y))
                if c is None or not (0 <= x < S):
                    r.append(".")
                    continue
                if c not in inv:
                    inv[c] = keys[len(inv)]
                    legend[inv[c]] = c
                r.append(inv[c])
            rows.append("".join(r))
        return rows, legend


    cmds = [{"op": "project_new", "width": S, "height": S, "name": "Pixel Studio VFX 2D"}, {"op": "layer_rename", "layer": 0, "name": "FX"}]
    frame = 0
    previews = []
    for name, fn, n, ms, _ in EFFECTS:
        start = frame
        for t in range(n):
            img = {k: v for k, v in fn(t, n).items() if 0 <= k[0] < S and 0 <= k[1] < S}
            rows, legend = rows_legend(img)
            if frame > 0:
                cmds.append({"op": "frame_add", "index": frame, "duration": ms})
            else:
                cmds.append({"op": "frame_set_duration", "frame": 0, "duration": ms})
            cmds.append({"op": "draw_ascii", "actor": "vfx2d-builder", "frame": frame, "layer": "FX", "rows": rows, "legend": legend})
            previews.append((name, t, img))
            frame += 1
        cmds.append({"op": "animation_tag_create", "name": name, "from": start, "to": frame - 1})

    here = out
    with open(os.path.join(here, "vfx2d.jsonl"), "w") as f:
        for c in cmds:
            f.write(json.dumps(c, separators=(",", ":")) + "\n")
    show = list(cmds)
    for name, *_ in EFFECTS:
        show.append({"op": "export_gif", "_save": f"vfx2d/{name}.gif", "tag": name, "scale": 4})
        show.append({"op": "export_spritesheet", "_save": f"vfx2d/{name}_sheet.png", "tag": name, "_json": True})
    show.append({"op": "project_save", "_save": "vfx2d/PixelStudio_VFX2D.pxs"})
    with open(os.path.join(here, "vfx2d_showcase.jsonl"), "w") as f:
        for c in show:
            f.write(json.dumps(c, separators=(",", ":")) + "\n")

    try:
        from PIL import Image

        cols = 8
        sheet = Image.new("RGBA", (cols * (S + 4) + 4, len(EFFECTS) * (S + 4) + 4), (24, 22, 34, 255))
        for row, (name, fn, n, _, _) in enumerate(EFFECTS):
            frames_ = [img for (nm, t, img) in previews if nm == name]
            pick = [frames_[round(i * (n - 1) / (cols - 1))] for i in range(cols)]
            for col, img in enumerate(pick):
                for (x, y), c in img.items():
                    sheet.putpixel((4 + col * (S + 4) + x, 4 + row * (S + 4) + y), tuple(int(c[k:k + 2], 16) for k in (1, 3, 5)) + (255,))
        sheet.resize((sheet.width * 2, sheet.height * 2), Image.NEAREST).save(os.path.join(here, "preview_vfx2d.png"))
    except ImportError:
        pass
    print(f"{len(EFFECTS)} effects, {frame} frames")


# ════ build_portraits ═══════════════════════════════════════════════════════════════════════════

def build_portraits(out):
    """Pixel Studio anime bust portraits (128×128, 8-frame loops) — original characters.

      * "Shizuku" — rainy evening: cool gradient-mapped mood, cyan rim light, falling rain.
      * "Astra"   — starlit mage: wide-brim hat casting a shadow over the hair, twinkling sky.

    Flat shapes are blocked in by this script; the engine does the craft passes:
    auto_shade (4-tone hair, cast shadows of bangs and hat), selout outlines, rim light,
    gradient_map grading and draw_sparkle. Eyes are built from layered shapes (sclera, gradient
    iris, thick upper lash with wing, lower lash, double highlights) at full resolution.
    """
    W = H = 128
    FR = 8
    here = out
    TAU = math.tau


    def hnoise(x, y, seed=0):
        n = (x * 374761393 + y * 668265263 + seed * 1013904223) & 0xFFFFFFFF
        n = (n ^ (n >> 13)) * 1274126177 & 0xFFFFFFFF
        return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0





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
        L = {k: Layer(W, H) for k in ("BG", "HairBack", "Body", "Face", "HairFront", "Hat", "Details", "FX")}
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


# ════ Bram, the Bronze Warden — skeletal 2D warrior with 9 animations ═════════════════════════


def build_warrior(out):
    """Original warrior "Bram, the Bronze Warden": 128×110 frames (~60 px tall), side view facing right.

    A 2D skeleton (spine, two arms, two legs, sword, shield) is posed per frame from keyframes
    (smoothstep in-betweens), feet are planted on the ground every frame, the scarf and the helmet
    plume follow the body with Verlet physics (follow-through), and the far limbs use one step
    darker colors for depth. The engine then shades every layer (auto_shade with cast shadows),
    draws sel-out outlines, and adds the effects (smears, dust, shockwave, hit sparks, hit-flash).

    Animations (all ≥ 8 frames, varied timing): idle, walk, run, attack_slash, attack_heavy,
    block, jump, hurt, death. Hitboxes are exported as slices.
    """
    W, H = 128, 110
    GROUND = 90  # skeleton space; S() maps it to the canvas at 1.3× scale
    K, OX, GY = 1.3, 52, 104
    lines = []

    def S(pt):
        return (OX + (pt[0] - 40) * K, GY + (pt[1] - GROUND) * K)

    def cmd(op, **a):
        lines.append(json.dumps({"op": op, "actor": "warrior-builder", **a}, separators=(",", ":")))

    C = dict(steel="#8E9CB4", trim="#C8923A", tabard="#2F7A72", cloth="#3E4A6A", skin="#E2A57A", beard="#7A4A2E", leather="#6E4A32",
             boot="#4A3A34", scarf="#C83C3C", blade="#D6E0EC", grip="#5A3A28", face="#2F7A72", boss="#E0B85A")
    BACK = {k: adjust(v, -0.09, -0.03) for k, v in C.items()}  # far side: one step darker, less saturated
    LAYERS = ["Shadow", "Scarf", "ShieldBack", "BackArm", "BackLeg", "Body", "FrontLeg", "Head", "Weapon", "FrontArm", "ShieldFront",
              "Details", "FX", "Impact"]

    def d(a):
        r = math.radians(a)
        return (math.sin(r), math.cos(r))  # 0 = down, 90 = forward (+x), 180 = up

    def at(p, v, s=1.0):
        return (p[0] + v[0] * s, p[1] + v[1] * s)

    def capsule(a, b, ra, rb, n=5):
        ang = math.atan2(b[1] - a[1], b[0] - a[0])
        pts = [(b[0] + math.cos(ang - math.pi / 2 + math.pi * i / n) * rb, b[1] + math.sin(ang - math.pi / 2 + math.pi * i / n) * rb) for i in range(n + 1)]
        pts += [(a[0] + math.cos(ang + math.pi / 2 + math.pi * i / n) * ra, a[1] + math.sin(ang + math.pi / 2 + math.pi * i / n) * ra) for i in range(n + 1)]
        return pts

    def oval(c, rx, ry, rot=0.0, n=18):
        cr, sr = math.cos(math.radians(rot)), math.sin(math.radians(rot))
        return [(c[0] + math.cos(t) * rx * cr - math.sin(t) * ry * sr, c[1] + math.cos(t) * rx * sr + math.sin(t) * ry * cr)
                for t in (i / n * math.tau for i in range(n))]

    # ---------------------------------------------------------------- poses
    BASE = dict(px=40.0, lean=5.0, tilt=0.0, chest=0.0, f_sh=20.0, f_el=35.0, sword=80.0, b_sh=-10.0, b_el=30.0,
                f_hip=12.0, f_knee=10.0, b_hip=-14.0, b_knee=8.0, lift=0.0, rot=0.0, shield_front=0.0)

    def keyed(n, keys):
        """Smoothstep in-betweens between keyframes {frame: overrides}."""
        idx = sorted(keys)
        poses = []
        for f in range(n):
            lo = max(k for k in idx if k <= f)
            hi = min((k for k in idx if k >= f), default=lo)
            a, b = {**BASE, **keys[lo]}, {**BASE, **keys[hi]}
            t = 0 if hi == lo else (f - lo) / (hi - lo)
            t = t * t * (3 - 2 * t)
            poses.append({k: a[k] + (b[k] - a[k]) * t for k in BASE})
        return poses

    READY = {}

    def cyc(n, fn):
        return [{**BASE, **fn(math.tau * k / n)} for k in range(n)]

    ANIMS = []  # (name, poses, durations, wind, loop, events)
    ANIMS.append(("idle", cyc(8, lambda p: dict(chest=-(1 - math.cos(p)) / 2 * 1.2, f_el=35 + 5 * (1 - math.cos(p)) / 2,
                                                b_el=30 + 5 * (1 - math.cos(p)) / 2, tilt=-2 * (1 - math.cos(p)) / 2)),
                  [180, 150, 140, 160, 180, 150, 140, 160], 0.25, True, {}))
    ANIMS.append(("walk", cyc(8, lambda p: dict(lean=7, f_hip=4 + 24 * math.sin(p), b_hip=4 - 24 * math.sin(p),
                                                f_knee=6 + 34 * max(0, math.cos(p)) ** 1.3, b_knee=6 + 34 * max(0, -math.cos(p)) ** 1.3,
                                                f_sh=16 - 16 * math.sin(p), b_sh=-6 + 16 * math.sin(p), tilt=1.5 * math.sin(2 * p))),
                  [100] * 8, 0.7, True, {}))
    ANIMS.append(("run", cyc(8, lambda p: dict(lean=17, f_hip=8 + 38 * math.sin(p), b_hip=8 - 38 * math.sin(p),
                                               f_knee=10 + 72 * max(0, math.cos(p)) ** 1.2, b_knee=10 + 72 * max(0, -math.cos(p)) ** 1.2,
                                               f_sh=24 - 32 * math.sin(p), f_el=70, sword=60, b_sh=4 + 32 * math.sin(p), b_el=70,
                                               lift=2.5 * max(0, -math.cos(2 * p)), chest=-0.6 * abs(math.sin(p)))),
                  [70] * 8, 1.6, True, {"dust": [2, 6]}))
    ANIMS.append(("attack_slash", keyed(10, {
        0: READY,
        1: dict(lean=-3, f_sh=-45, f_el=60, sword=165, b_sh=20, b_el=40, f_hip=18, b_hip=-18, chest=-1),
        2: dict(lean=-8, f_sh=-70, f_el=80, sword=195, b_sh=30, b_el=40, f_hip=20, b_hip=-20, chest=-1),
        3: dict(lean=10, px=43, f_sh=70, f_el=20, sword=10, b_sh=-20, f_hip=26, f_knee=18, b_hip=-24),
        4: dict(lean=16, px=45, f_sh=95, f_el=5, sword=0, b_sh=-30, b_el=40, f_hip=32, f_knee=26, b_hip=-26, b_knee=6),
        5: dict(lean=16, px=45, f_sh=93, f_el=7, sword=0, b_sh=-30, b_el=40, f_hip=32, f_knee=26, b_hip=-26, b_knee=6),
        6: dict(lean=11, px=44, f_sh=60, f_el=30, sword=40, f_hip=24, f_knee=18, b_hip=-20),
        7: dict(lean=7, px=42, f_sh=35, f_el=35, sword=70),
        8: dict(px=41),
        9: READY}), [100, 90, 80, 40, 130, 110, 90, 90, 100, 120], 0.5, False,
        {"smear": {3: (205, 100)}, "hitbox": [4, 5]}))
    ANIMS.append(("attack_heavy", keyed(12, {
        0: READY,
        1: dict(chest=1.5, f_knee=30, b_knee=30, lean=2, f_sh=120, f_el=40, sword=30, b_sh=-25, b_el=40),
        2: dict(lean=-10, f_sh=170, f_el=20, sword=25, b_sh=-40, f_hip=15, b_hip=-20, lift=3),
        3: dict(lean=-6, f_sh=175, f_el=15, sword=25, b_sh=-40, f_hip=25, f_knee=30, b_hip=-10, b_knee=30, lift=7),
        4: dict(lean=14, px=43, f_sh=100, f_el=10, sword=-10, b_sh=-30, f_hip=30, f_knee=20, b_hip=-20, lift=3),
        5: dict(lean=28, px=46, f_sh=58, f_el=5, sword=-3, b_sh=-35, b_el=50, f_hip=42, f_knee=55, b_hip=-30, b_knee=30, chest=1),
        6: dict(lean=28, px=46, f_sh=58, f_el=5, sword=-3, b_sh=-35, b_el=50, f_hip=42, f_knee=55, b_hip=-30, b_knee=30, chest=1),
        7: dict(lean=26, px=46, f_sh=60, f_el=8, sword=-3, b_sh=-30, b_el=45, f_hip=40, f_knee=50, b_hip=-28, b_knee=28, chest=1),
        8: dict(lean=18, px=45, f_sh=65, f_el=20, sword=20, f_hip=30, f_knee=30, b_hip=-22, b_knee=15),
        9: dict(lean=10, px=43, f_sh=40, f_el=30, sword=60),
        10: dict(px=41),
        11: READY}), [100, 110, 90, 70, 40, 140, 100, 90, 90, 90, 100, 120], 0.5, False,
        {"smear": {4: (225, 70)}, "shockwave": 5, "hitbox": [5, 6], "dust": [5]}))
    ANIMS.append(("block", keyed(8, {
        0: READY,
        1: dict(shield_front=1, b_sh=80, b_el=45, lean=6, chest=1, f_knee=20, b_knee=18, f_sh=10, f_el=50, sword=95),
        2: dict(shield_front=1, b_sh=85, b_el=45, lean=7, chest=1.5, f_knee=24, b_knee=20, f_sh=8, f_el=50, sword=95),
        3: dict(shield_front=1, b_sh=85, b_el=45, lean=7, chest=1.5, f_knee=24, b_knee=20, f_sh=8, f_el=50, sword=95),
        4: dict(shield_front=1, b_sh=85, b_el=50, lean=2, px=38, chest=2, f_knee=28, b_knee=24, f_sh=5, f_el=50, sword=95),
        5: dict(shield_front=1, b_sh=85, b_el=48, lean=4, px=39, chest=1.5, f_knee=24, b_knee=20, f_sh=8, f_el=50, sword=95),
        6: dict(shield_front=1, b_sh=45, b_el=40, lean=5, px=39.5),
        7: READY}), [100, 80, 140, 140, 60, 90, 100, 120], 0.3, False, {"spark": 4}))
    ANIMS.append(("jump", keyed(10, {
        0: dict(chest=2, f_knee=45, b_knee=40, f_hip=30, b_hip=5, lean=12, f_sh=-30, f_el=40, b_sh=-30),
        1: dict(f_knee=4, b_knee=6, f_hip=8, b_hip=-25, lift=8, f_sh=120, f_el=20, sword=60, b_sh=60, lean=6),
        2: dict(f_knee=30, b_knee=35, f_hip=30, b_hip=-10, lift=20, f_sh=100, f_el=30, sword=70, b_sh=40, lean=4),
        3: dict(f_knee=60, b_knee=65, f_hip=50, b_hip=10, lift=27, f_sh=80, f_el=40, sword=80, b_sh=30),
        4: dict(f_knee=62, b_knee=66, f_hip=52, b_hip=12, lift=29, f_sh=75, f_el=40, sword=80, b_sh=25),
        5: dict(f_knee=55, b_knee=60, f_hip=45, b_hip=8, lift=28, f_sh=80, f_el=40, sword=80, b_sh=30),
        6: dict(f_knee=20, b_knee=25, f_hip=15, b_hip=-15, lift=22, f_sh=100, f_el=30, sword=70, b_sh=60, lean=2),
        7: dict(f_knee=8, b_knee=10, f_hip=10, b_hip=-12, lift=10, f_sh=90, f_el=30, sword=70, b_sh=50, lean=4),
        8: dict(chest=2.5, f_knee=50, b_knee=45, f_hip=35, b_hip=10, lean=14, f_sh=40, f_el=45, b_sh=-10, b_el=50),
        9: READY}), [120, 60, 70, 80, 120, 100, 80, 70, 110, 120], 0.4, False, {"dust": [8], "launch_dust": [1]}))
    HURT = dict(lean=-12, px=38, tilt=-10, f_sh=-20, f_el=50, sword=110, b_sh=-30, b_el=40, f_knee=15, b_knee=12)
    ANIMS.append(("hurt", keyed(8, {
        0: HURT, 1: dict(HURT, px=37, lean=-16),
        2: dict(lean=-18, px=36, tilt=-14, f_sh=-35, f_el=55, sword=120, b_sh=-40, b_el=40, f_knee=18, b_knee=14),
        3: dict(lean=-17, px=36, tilt=-13, f_sh=-33, f_el=55, sword=120, b_sh=-38, b_el=40, f_knee=18, b_knee=14),
        4: dict(lean=-10, px=37, tilt=-6, f_sh=-10), 5: dict(lean=-2, px=38), 6: dict(px=39), 7: READY}),
        [60, 60, 90, 100, 90, 90, 100, 120], 0.5, False, {"flash": [0, 1]}))
    ANIMS.append(("death", keyed(12, {
        0: HURT,
        1: dict(lean=-20, px=37, tilt=-16, f_sh=-30, f_el=50, sword=120, b_sh=-35, f_knee=20, b_knee=18),
        2: dict(chest=2, f_knee=60, b_knee=70, f_hip=50, b_hip=20, lean=5, tilt=10, f_sh=5, f_el=20, b_sh=0, b_el=20),
        3: dict(chest=2, f_hip=85, f_knee=120, b_hip=70, b_knee=125, lean=10, tilt=18, f_sh=0, f_el=15, b_sh=-5, b_el=15),
        4: dict(chest=2.5, f_hip=86, f_knee=122, b_hip=72, b_knee=126, lean=12, tilt=22, f_sh=0, f_el=15, b_sh=-5, b_el=15),
        5: dict(f_hip=86, f_knee=122, b_hip=72, b_knee=126, lean=14, tilt=22, f_sh=10, f_el=15, b_sh=0, b_el=15, rot=20),
        6: dict(f_hip=80, f_knee=110, b_hip=66, b_knee=110, lean=10, tilt=15, f_sh=40, f_el=20, b_sh=30, b_el=20, rot=50),
        7: dict(f_hip=60, f_knee=60, b_hip=50, b_knee=55, lean=4, tilt=5, f_sh=80, f_el=10, b_sh=70, b_el=10, rot=82),
        8: dict(f_hip=55, f_knee=50, b_hip=45, b_knee=50, lean=4, tilt=5, f_sh=85, f_el=10, b_sh=75, b_el=10, rot=78, lift=1.5),
        9: dict(f_hip=50, f_knee=45, b_hip=40, b_knee=45, lean=3, tilt=4, f_sh=88, f_el=8, b_sh=78, b_el=8, rot=84),
        10: dict(f_hip=50, f_knee=45, b_hip=40, b_knee=45, lean=3, tilt=4, f_sh=88, f_el=8, b_sh=78, b_el=8, rot=85),
        11: dict(f_hip=50, f_knee=45, b_hip=40, b_knee=45, lean=3, tilt=4, f_sh=88, f_el=8, b_sh=78, b_el=8, rot=85)}),
        [60, 100, 100, 120, 160, 90, 80, 70, 90, 120, 200, 600], 0.3, False, {"flash": [0], "drop_sword": 2, "dust": [7]}))

    # ---------------------------------------------------------------- skeleton → polygons
    def skeleton(p):
        P = (p["px"], 50.0)
        lean = math.radians(p["lean"])
        u = (math.sin(lean), -math.cos(lean))  # up the spine
        fw = (math.cos(lean), math.sin(lean))  # forward, perpendicular to the spine
        N = at(at(P, u, 15), (0, p["chest"]))
        Hc = at(N, u, 5.8)
        F, B = at(at(N, u, -2), fw, 1.2), at(at(N, u, -2), fw, -1.6)
        j = dict(P=P, N=N, Hc=Hc, F=F, B=B, u=u, fw=fw)
        j["fe"] = at(F, d(p["f_sh"]), 8.5)
        j["fh"] = at(j["fe"], d(p["f_sh"] + p["f_el"]), 7.5)
        j["be"] = at(B, d(p["b_sh"]), 8.5)
        j["bh"] = at(j["be"], d(p["b_sh"] + p["b_el"]), 7.5)
        for s, hipx in (("f", 1.2), ("b", -1.0)):
            hip = at(P, fw, hipx)
            knee = at(hip, d(p[s + "_hip"]), 11)
            ankle = at(knee, d(p[s + "_hip"] - p[s + "_knee"]), 11)
            j[s + "hip"], j[s + "knee"], j[s + "ankle"] = hip, knee, ankle
        j["blade_dir"] = p["f_sh"] + p["f_el"] + p["sword"]
        return j

    def parts(p, j, sword_free=None):
        """[(layer, color, points)] back to front within each layer."""
        out = []
        u, fw = j["u"], j["fw"]
        for s, lay, pal in (("b", "BackLeg", BACK), ("f", "FrontLeg", C)):
            hip, knee, ankle = j[s + "hip"], j[s + "knee"], j[s + "ankle"]
            out.append((lay, pal["cloth"], capsule(hip, knee, 3.3, 2.7)))
            out.append((lay, pal["steel"], capsule(knee, ankle, 2.8, 2.2)))
            out.append((lay, pal["trim"], oval(knee, 2.3, 2.0)))
            tilt = (p[s + "_hip"] - p[s + "_knee"]) * 0.25
            toe = at(ankle, d(90 + tilt), 6.0)
            heel = at(ankle, d(270 + tilt), 2.0)
            sole = d(tilt)
            out.append((lay, pal["boot"], [at(heel, sole, -1.5), at(ankle, sole, -2.5), at(toe, sole, -1.0), at(toe, sole, 2.2), at(heel, sole, 2.2)]))
        # back arm + shield
        out.append(("BackArm", BACK["cloth"], capsule(j["B"], j["be"], 2.6, 2.2)))
        out.append(("BackArm", BACK["steel"], capsule(j["be"], j["bh"], 2.4, 2.3)))
        out.append(("BackArm", BACK["leather"], oval(j["bh"], 2.2, 2.2)))
        sl = "ShieldFront" if p["shield_front"] > 0.5 else "ShieldBack"
        sc = at(j["bh"], fw, 1.5)
        srot = (p["b_sh"] + p["b_el"]) * 0.15
        pal = C if sl == "ShieldFront" else BACK
        out.append((sl, pal["trim"], oval(sc, 5.2 if sl == "ShieldFront" else 4.4, 10.5, srot)))
        out.append((sl, pal["face"], oval(sc, 3.9 if sl == "ShieldFront" else 3.2, 9.0, srot)))
        out.append((sl, pal["boss"], oval(at(sc, (1, 0), 0.6), 1.6, 2.0, srot)))
        # body: breastplate, belt, tabard
        P, N = j["P"], j["N"]
        tab_swing = (p["f_hip"] + p["b_hip"]) * 0.5
        tb = at(P, d(tab_swing), 10)
        out.append(("Body", C["tabard"], [at(at(P, u, 4), fw, 4.6), at(tb, fw, 4.2), at(at(tb, d(tab_swing), 1.5), fw, 0), at(tb, fw, -4.4), at(at(P, u, 4), fw, -4.4)]))
        out.append(("Body", C["steel"], [at(P, fw, -4.3), at(at(P, u, 7), fw, -5.0), at(at(N, u, -1), fw, -5.2), at(N, fw, -3.0), at(N, fw, 3.4),
                                         at(at(N, u, -2.5), fw, 6.2), at(at(P, u, 8), fw, 5.4), at(P, fw, 4.5)]))
        out.append(("Body", C["leather"], [at(at(P, u, 1), fw, -4.6), at(at(P, u, 3.4), fw, -4.6), at(at(P, u, 3.4), fw, 4.8), at(at(P, u, 1), fw, 4.8)]))
        out.append(("Body", C["steel"], oval(at(N, u, 0.5), 3.4, 2.4, p["lean"])))
        # head: helmet, face, beard, crest
        Hc = j["Hc"]
        ht = p["lean"] + p["tilt"]
        hu, hf = d(180 + ht), d(90 + ht)
        out.append(("Head", C["steel"], oval(Hc, 6.3, 6.6, ht)))
        face = [at(at(Hc, hf, 1.0), hu, 0.5), at(at(Hc, hf, 6.4), hu, 0.8), at(at(Hc, hf, 6.8), hu, -3.0),
                at(at(Hc, hf, 4.6), hu, -6.2), at(at(Hc, hf, 1.2), hu, -5.4)]
        out.append(("Head", C["skin"], face))
        out.append(("Head", C["beard"], [at(at(Hc, hf, 1.0), hu, -3.2), at(at(Hc, hf, 6.6), hu, -2.6), at(at(Hc, hf, 5.0), hu, -7.2),
                                        at(at(Hc, hf, 2.4), hu, -7.6), at(at(Hc, hf, 0.6), hu, -5.0)]))
        out.append(("Head", C["steel"], capsule(at(at(Hc, hf, 5.6), hu, 2.6), at(at(Hc, hf, 6.0), hu, -1.6), 0.8, 0.7, 3)))
        out.append(("Head", C["trim"], capsule(at(at(Hc, hf, 4.5), hu, 5.6), at(at(Hc, hf, -4.5), hu, 5.0), 1.3, 1.1, 4)))
        # sword (in hand, or fallen)
        if sword_free:
            hand, bd = sword_free
        else:
            hand, bd = j["fh"], j["blade_dir"]
        bdir, bperp = d(bd), d(bd + 90)
        guard = at(hand, bdir, 2.5)
        tip = at(guard, bdir, 23)
        out.append(("Weapon", C["grip"], capsule(at(hand, bdir, -3.5), guard, 1.1, 1.1, 3)))
        out.append(("Weapon", C["trim"], oval(at(hand, bdir, -4.2), 1.5, 1.5)))
        out.append(("Weapon", C["blade"], [at(guard, bperp, 1.6), at(at(tip, bdir, -4), bperp, 1.3), tip, at(at(tip, bdir, -4), bperp, -1.3), at(guard, bperp, -1.6)]))
        out.append(("Weapon", C["trim"], capsule(at(guard, bperp, 3.4), at(guard, bperp, -3.4), 1.0, 1.0, 3)))
        # front arm with pauldron
        if not sword_free or True:
            out.append(("FrontArm", C["cloth"], capsule(j["F"], j["fe"], 2.7, 2.3)))
            out.append(("FrontArm", C["steel"], capsule(j["fe"], j["fh"], 2.5, 2.4)))
            out.append(("FrontArm", C["leather"], oval(j["fh"], 2.3, 2.3)))
            out.append(("FrontArm", C["steel"], oval(at(j["F"], d(p["f_sh"]), 1.2), 4.4, 3.6, p["f_sh"] * 0.4)))
            out.append(("FrontArm", C["trim"], capsule(at(at(j["F"], d(p["f_sh"]), 3.6), d(p["f_sh"] + 90), 3.6),
                                                      at(at(j["F"], d(p["f_sh"]), 3.6), d(p["f_sh"] + 90), -3.6), 0.8, 0.8, 3)))
        return out, (tip, guard)

    def transform(pt, rot, pivot, dx, dy):
        if rot:
            r = math.radians(rot)
            x, y = pt[0] - pivot[0], pt[1] - pivot[1]
            pt = (pivot[0] + x * math.cos(r) - y * math.sin(r), pivot[1] + x * math.sin(r) + y * math.cos(r))
        return (pt[0] + dx, pt[1] + dy)

    # ---------------------------------------------------------------- secondary motion (Verlet chains)
    class Chain:
        def __init__(self, n, seg, grav):
            self.n, self.seg, self.grav = n, seg, grav
            self.pts = None

        def step(self, anchor, wind, sub=3):
            if self.pts is None:
                self.pts = [(anchor[0] - i * self.seg, anchor[1] + i * 0.5) for i in range(self.n)]
                self.prev = list(self.pts)
            for _ in range(sub):
                new = [anchor]
                for i in range(1, self.n):
                    x, y = self.pts[i]
                    px, py = self.prev[i]
                    vx, vy = (x - px) * 0.86, (y - py) * 0.86
                    new.append((x + vx - wind * 0.35, y + vy + self.grav))
                for _ in range(4):
                    new[0] = anchor
                    for i in range(1, self.n):
                        ax, ay = new[i - 1]
                        bx, by = new[i]
                        dd = math.hypot(bx - ax, by - ay) or 1e-3
                        k = self.seg / dd
                        new[i] = (ax + (bx - ax) * k, ay + (by - ay) * k)
                self.prev, self.pts = self.pts, new
            return list(self.pts)

    # ---------------------------------------------------------------- document
    total = sum(len(a[1]) for a in ANIMS)
    cmd("project_new", width=W, height=H, name="Bram - the Bronze Warden")
    cmd("layer_rename", layer=0, name=LAYERS[0])
    for name in LAYERS[1:]:
        cmd("layer_create", name=name)
    cmd("frame_add", count=total - 1)

    frame = 0
    durations, tags, fx_jobs, flash_frames, slices = [], [], [], [], []
    for name, poses, durs, wind, loop, ev in ANIMS:
        start = frame
        scarf, plume = Chain(6, 3.2, 0.32), Chain(4, 2.6, 0.12)
        geo = []
        for k, p in enumerate(poses):
            j = skeleton(p)
            # Plant: the lowest foot/knee touches the ground.
            pts_low = [j["fankle"][1] + 2.4, j["bankle"][1] + 2.4, j["fknee"][1] + 2.4, j["bknee"][1] + 2.4]
            dy = GROUND - max(pts_low)
            pivot = (j["fknee"][0], j["fknee"][1] + dy) if p["rot"] else (0, 0)
            sword_free = None
            if "drop_sword" in ev and k >= ev["drop_sword"]:
                q = min(1.0, (k - ev["drop_sword"]) / 3)
                sword_free = ((p["px"] + 22 + 4 * q, 60 + (GROUND - 2.5 - 60) * q * q), 150 - 60 * q)
            plist, (tip, guard) = parts(p, j, None)
            moved = [(lay, col, [transform(pt, p["rot"], pivot, 0, dy) for pt in poly]) for lay, col, poly in plist
                     if not (sword_free and lay == "Weapon")]
            if p["rot"]:
                low = max(pt[1] for _, _, poly in moved for pt in poly)
                moved = [(lay, col, [(x, y + GROUND - low) for x, y in poly]) for lay, col, poly in moved]
            moved = [(lay, col, [(x, y - p["lift"]) for x, y in poly]) for lay, col, poly in moved]
            if sword_free:
                fp, _ = parts(p, j, sword_free)
                moved += [(lay, col, poly) for lay, col, poly in fp if lay == "Weapon"]
            anchor_off = (0, dy - p["lift"])
            nb = transform(at(j["N"], j["fw"], -3.0), p["rot"], pivot, *anchor_off)
            pl = transform(at(at(j["Hc"], j["u"], 5.5), j["fw"], -2.5), p["rot"], pivot, *anchor_off)
            geo.append((p, j, moved, nb, pl, transform(tip, p["rot"], pivot, *anchor_off)))
        # Physics: loops pre-roll two cycles so the recorded cycle closes seamlessly.
        rolls = 3 if loop else 1
        for r in range(rolls):
            for k, (p, j, moved, nb, pl, tip) in enumerate(geo):
                w = wind * (1 + 0.3 * math.sin(math.tau * k / len(geo))) if name == "idle" else wind
                sc, pc = scarf.step(nb, w), plume.step(pl, w * 0.8)
                if r == rolls - 1:
                    geo[k] = (p, j, moved, nb, pl, tip, sc, pc)
        for k, (p, j, moved, nb, pl, tip, sc, pc) in enumerate(geo):
            f = start + k
            for i in range(len(sc) - 1):
                moved.insert(0, ("Scarf", C["scarf"], capsule(sc[i], sc[i + 1], 2.2 - i * 0.18, 2.0 - i * 0.18, 3)))
            for i in range(len(pc) - 1):
                moved.insert(0, ("Scarf", adjust(C["scarf"], 0.06), capsule(pc[i], pc[i + 1], 1.6 - i * 0.25, 1.3 - i * 0.25, 3)))
            feet_x = (min(pt[0] for lay, _, poly in moved if lay in ("FrontLeg", "BackLeg") for pt in poly) +
                      max(pt[0] for lay, _, poly in moved if lay in ("FrontLeg", "BackLeg") for pt in poly)) / 2
            sw = 15 * K * (1 - min(p["lift"], 28) / 40)
            fx_s = S((feet_x, GROUND))[0]
            cmd("draw_ellipse", layer="Shadow", frame=f, x=round(fx_s - sw), y=GY - 2, width=round(sw * 2), height=5, color="#5C6088", filled=True)
            for lay, col, poly in moved:
                cmd("draw_polygon", layer=lay, frame=f, color=col, points=[[round(x, 1), round(y, 1)] for x, y in map(S, poly)])
            # Details: eye, buckle, visor shine (hand-placed after shading, see below).
            rot, piv = p["rot"], None
            eye = (j["Hc"][0], j["Hc"][1])
            ht = p["lean"] + p["tilt"]
            ex, ey = at(at(eye, d(90 + ht), 3.6), d(180 + ht), -0.6)
            if not rot:
                epx = S((ex, ey + (GROUND - max(j[s][1] + 2.4 for s in ("fankle", "bankle", "fknee", "bknee"))) - p["lift"]))
                fx_jobs.append(("details", f, [[round(epx[0]), round(epx[1]), "#1E1A2A"]]))
            if k in ev.get("smear", {}):
                fx_jobs.append(("smear", f, (j, p, ev["smear"][k], GROUND - max(j[s][1] + 2.4 for s in ("fankle", "bankle", "fknee", "bknee")) - p["lift"])))
            if k in ev.get("dust", []) or k in ev.get("launch_dust", []):
                fx_jobs.append(("dust", f, fx_s))
            if ev.get("shockwave") == k:
                fx_jobs.append(("shock", f, S(tip)))
            if ev.get("spark") == k:
                sx = max(pt[0] for lay, _, poly in moved if lay == "ShieldFront" for pt in poly)
                fx_jobs.append(("spark", f, S((sx, 60))))
            if k in ev.get("flash", []):
                flash_frames.append(f)
            if k in ev.get("hitbox", []):
                slices.append((f"hitbox_{name}", f, S(tip)))
        for k, ms in enumerate(durs):
            durations.append((start + k, ms))
        tags.append((name, start, start + len(poses) - 1, loop))
        frame += len(poses)

    # ---------------------------------------------------------------- engine finishing passes
    cmd("layer_blend", layer="Shadow", mode="multiply")
    cmd("layer_opacity", layer="Shadow", opacity=170)
    shade = dict(Scarf=dict(tones=3, bevel=1.5, contrast=0.5), ShieldBack=dict(tones=3, bevel=2, contrast=0.5), BackArm=dict(tones=3, bevel=1.5),
                 BackLeg=dict(tones=3, bevel=1.5), Body=dict(tones=4, bevel=2.5, contrast=0.6), FrontLeg=dict(tones=3, bevel=1.5),
                 Head=dict(tones=3, bevel=2, contrast=0.5), Weapon=dict(tones=3, bevel=1, contrast=0.7), FrontArm=dict(tones=4, bevel=1.5, contrast=0.6),
                 ShieldFront=dict(tones=3, bevel=2, contrast=0.55))
    for lay, a in shade.items():
        cmd("auto_shade", layer=lay, scope="layer", cast_shadow=2, min_region=3, **a)
    for f in range(total):
        for lay in shade:
            cmd("outline", layer=lay, frame=f, mode="selout", darkness=0.62)
    for kind, f, data in fx_jobs:
        if kind == "details":
            cmd("set_pixels", layer="Details", frame=f, pixels=data + [[data[0][0], data[0][1] - 1, "#1E1A2A"]])
        elif kind == "smear":
            j, p, (a0, a1), dy = data
            sh = S((j["F"][0], j["F"][1] + dy))
            for r_in, r_out, col in ((20 * K, 37 * K, "#7FA8D8"), (25 * K, 36 * K, "#CFE6FF"), (31 * K, 35.5 * K, "#FFFFFF")):
                steps = 16
                outer = [at(sh, d(a0 + (a1 - a0) * i / steps), r_out) for i in range(steps + 1)]
                inner = [at(sh, d(a0 + (a1 - a0) * i / steps), r_in + (r_out - r_in) * (1 - i / steps) * 0.85) for i in range(steps, -1, -1)]
                cmd("draw_polygon", layer="FX", frame=f, color=col, points=[[round(x, 1), round(y, 1)] for x, y in outer + inner])
        elif kind == "dust":
            for side in (-1, 1):
                for i in range(3):
                    x = data + side * (8 + i * 5)
                    cmd("draw_ellipse", layer="FX", frame=f, x=round(x - 3 - i), y=GY - 4 - i, width=5 + i * 3, height=4 + i, color="#D8CBB8", filled=True)
            cmd("dither_fill", layer="FX", frame=f, color="#D8CBB8", color2="#A8957E", level=6, only_opaque=True)
        elif kind == "shock":
            cmd("fx_generate", kind="shockwave", layer="Impact", frame=f, frames=4, size=64, x=round(data[0]), y=GY - 8, timing="uniform")
        elif kind == "spark":
            cmd("fx_generate", kind="hit", layer="Impact", frame=f, frames=3, size=36, x=round(data[0]) + 3, y=round(data[1]), timing="uniform")
    for f in flash_frames:  # hit-flash: the whole body turns white for a frame
        for lay in shade:
            cmd("gradient_map", layer=lay, frame=f, colors=["#FFFFFF", "#FFFFFF"], strength=1.0)
    for name, f, tip in slices:
        cmd("slice_set", name=name, frame=f, x=round(tip[0]) - 14, y=round(tip[1]) - 10, width=24, height=20, color="#FF4040")
    cmd("slice_set", name="hurtbox", x=38, y=40, width=30, height=64, color="#40A0FF")
    for f, ms in durations:
        cmd("frame_set_duration", frame=f, duration=ms)
    for name, a, b, loop in tags:
        cmd("animation_tag_create", name=name, **{"from": a, "to": b}, direction="forward", repeat=0 if loop else 1)

    with open(os.path.join(out, "warrior.jsonl"), "w") as fh:
        fh.write("\n".join(lines) + "\n")
    show = list(lines)
    for name, a, b, loop in tags:
        show.append(json.dumps({"op": "export_gif", "tag": name, "scale": 3, "_save": f"warrior/{name}.gif"}))
        show.append(json.dumps({"op": "export_spritesheet", "tag": name, "scale": 1, "padding": 0, "_save": f"warrior/{name}_sheet.png"}))
    for f in range(total):
        show.append(json.dumps({"op": "export_png", "frame": f, "scale": 1, "_save": f"warrior/frames/f{f:03d}.png"}))
    show.append(json.dumps({"op": "export_atlas", "padding": 1, "extrude": 1, "image": "warrior_atlas.png", "_save": "warrior/warrior_atlas.png", "_json": True}))
    show.append(json.dumps({"op": "export_godot_spriteframes", "texture_path": "res://warrior.png", "_save": "warrior/warrior.png", "_json": True}))
    show.append(json.dumps({"op": "project_save", "_save": "warrior/warrior.pxs"}))
    with open(os.path.join(out, "warrior_showcase.jsonl"), "w") as fh:
        fh.write("\n".join(show) + "\n")
    print(f"{len(ANIMS)} animations, {total} frames, {len(lines)} commands")


def main(argv):
    """Usage: python3 Assets/Sprites.py [chibi|cast|vfx2d|portraits|warrior|all]   → command scripts in Assets/Build/."""
    import os
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "Build")
    os.makedirs(out, exist_ok=True)
    targets = {"chibi": build_chibi, "cast": build_cast, "vfx2d": build_vfx2d, "portraits": build_portraits, "warrior": build_warrior}
    wanted = argv[1:] or ["all"]
    for name, fn in targets.items():
        if "all" in wanted or name in wanted:
            print(f"[{name}]")
            fn(out)


if __name__ == "__main__":
    import sys

    main(sys.argv)
