#!/usr/bin/env python3
"""Pixel Studio 2D pixel-art VFX pack (64×64 frames).

Classic pixel-art effect rules: strict 4–6 color band palettes, clean clusters, no
anti-aliasing, bright core → saturated body → dark edge, and seamless loops where the effect
loops (time is mapped onto a circle so frame N wraps to frame 0).

    fire (16, loop) · slash (8) · hit spark (6) · magic burst (12) · smoke puff (8)
    lightning (8) · heal (12, loop) · explosion (12)

Frames are written with `draw_ascii` and tagged per effect.
"""

import json
import math
import os
import random

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

here = os.path.dirname(os.path.abspath(__file__))
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
