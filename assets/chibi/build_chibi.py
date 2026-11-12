#!/usr/bin/env python3
"""Pixel Studio pastel chibi collection — original characters, 64×80 sprites.

Style targets (studied from modern anime pixel-art chibis): ~2-head proportions, big soft
hair volumes built from tapered strand clumps, large gradient eyes with highlights, blush,
pastel hue-shifted palettes, colored (sel-out) outlines instead of black, cast shadows of the
bangs on the face, and sparkle effects.

Pipeline (exactly what an AI agent does through MCP):
  1. Python blocks each character in FLAT colors on separate layers
     (HairBack → Body → Face → HairFront) and hand-authors the eye/mouth stamps.
  2. The engine shades every layer: `auto_shade` (hue-shifted cel shading, cast shadows from
     the layers above), then `outline mode=selout`, then `pixel_cleanup`.
  3. Details (eyes, blush, hair shine) and `draw_sparkle` effects go on top.

All characters are original designs.

    python3 build_chibi.py  ->  chibi.jsonl + chibi_showcase.jsonl
"""

import colorsys
import json
import math
import os

W, H = 64, 80
here = os.path.dirname(os.path.abspath(__file__))
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
    def __init__(self):
        self.px = {}

    def put(self, x, y, c):
        x, y = int(round(x)), int(round(y))
        if 0 <= x < W and 0 <= y < H:
            self.px[(x, y)] = c

    def ellipse(self, cx, cy, rx, ry, c, clip=None):
        for y in range(int(cy - ry - 1), int(cy + ry + 2)):
            for x in range(int(cx - rx - 1), int(cx + rx + 2)):
                if ((x + 0.5 - cx) / rx) ** 2 + ((y + 0.5 - cy) / ry) ** 2 <= 1 and (clip is None or clip(x, y)):
                    self.put(x, y, c)

    def poly(self, pts, c):
        ys = [p[1] for p in pts]
        for y in range(int(min(ys)) - 1, int(max(ys)) + 2):
            for x in range(0, W):
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

def main():
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


if __name__ == "__main__":
    main()
