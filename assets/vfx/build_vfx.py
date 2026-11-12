#!/usr/bin/env python3
"""Voxel VFX library: 16-frame campfire (seamless loop), arcane portal (loop) and explosion.

Frame-by-frame 3D pixel animation: every frame is its own voxel model (the static base is
built once and cloned with voxel_model_clone), then the sequence is rendered with one shared
camera into sprite sheets, GIFs and editable 2D frames. All through public tools.

    python3 build_vfx.py  ->  vfx.jsonl (models) and vfx_showcase.jsonl (+ renders)
"""

import math
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from voxkit import Grid, mix, write_jsonl  # noqa: E402

FRAMES = 16
C = 32
cmds = []


def cmd(op, **a):
    cmds.append({"op": op, "actor": "vfx-builder", **a})


cmd("project_rename", name="Pixel Studio VFX")
for name, rough, metal, em in (
    ("stone", 0.85, 0.0, 0.0), ("wood", 0.8, 0.0, 0.0), ("char", 0.95, 0.0, 0.0), ("dirt", 0.95, 0.0, 0.0),
    ("ember", 0.9, 0.0, 0.9), ("flame", 0.9, 0.0, 1.05), ("spark", 0.5, 0.0, 1.5), ("smoke", 1.0, 0.0, 0.0),
    ("arcane", 0.2, 0.0, 0.95), ("rune", 0.3, 0.2, 0.8), ("flash", 0.5, 0.0, 2.2), ("debris", 0.9, 0.0, 0.0),
):
    cmd("material_set", name=name, roughness=rough, metallic=metal, emission=em)

TAU = math.tau


def frame_models(prefix):
    return [f"{prefix}_{t:02d}" for t in range(FRAMES)]


# =============================================================== campfire (seamless loop)
base = Grid(seed=1)
rng = random.Random(3)
for x in range(C - 14, C + 15):
    for z in range(C - 14, C + 15):
        r = math.hypot(x + .5 - C, z + .5 - C)
        if r < 13.5 - rng.random() * 1.2:
            base.put(x, 0, z, "#3B3532" if r < 6 else ("#5A3E2A" if (x + z) % 3 else "#4E3624"), "dirt")
# stone ring
for i in range(10):
    a = TAU * i / 10 + 0.15
    sx, sz = C + 9.5 * math.cos(a), C + 9.5 * math.sin(a)
    w = 1.6 + rng.random() * 0.6
    for x in range(int(sx - w), int(sx + w) + 1):
        for z in range(int(sz - w), int(sz + w) + 1):
            h = 3 if abs(x - sx) < w - 0.6 and abs(z - sz) < w - 0.6 else 2
            for y in range(1, h + 1):
                inner = math.hypot(x - C, z - C) < 9
                col = "#8C878C" if y == h else ("#5E524E" if inner else "#6C6870")
                base.put(x, y, z, col, "stone")
# teepee logs with charred, glowing tips
for i in range(5):
    a = TAU * i / 5 + 0.4
    p0 = (C + 6.5 * math.cos(a), 1.0, C + 6.5 * math.sin(a))
    p1 = (C + 0.9 * math.cos(a), 9.0, C + 0.9 * math.sin(a))
    for s in range(0, 41):
        t = s / 40
        x, y, z = (p0[k] + (p1[k] - p0[k]) * t for k in range(3))
        for dx in (-1, 0, 1):
            for dz in (-1, 0, 1):
                if dx * dx + dz * dz <= 1:
                    if t > 0.72:
                        col, mat = ("#FF6A1A", "ember") if (s + dx + dz) % 3 == 0 else ("#231915", "char")
                    else:
                        col, mat = ("#6E4728" if (s // 3 + dx) % 2 else "#5B3A22"), "wood"
                    base.put(round(x + dx * .7), round(y), round(z + dz * .7), col, mat)
# cross logs on the ground
for (x0, z0, x1, z1) in ((C - 8, C - 3, C + 7, C + 4), (C - 6, C + 6, C + 7, C - 5)):
    for s in range(0, 31):
        t = s / 30
        x, z = x0 + (x1 - x0) * t, z0 + (z1 - z0) * t
        for dy in (1, 2):
            for o in (-1, 0, 1):
                col = "#2A1C15" if 0.35 < t < 0.65 else ("#6B4527" if (s + o) % 4 else "#55361F")
                base.put(round(x + o * 0.5), dy, round(z + o * 0.5), col, "char" if 0.35 < t < 0.65 else "wood")

cmd("voxel_model_create", name="Campfire_base", width=64, height=64, depth=64, origin=[C, 0, C])
cmds.extend(base.commands("Campfire_base", actor="vfx-builder"))

for t in range(FRAMES):
    th = TAU * t / FRAMES
    g = Grid(seed=t)
    frng = random.Random(1000 + t)
    # ember bed twinkle
    for x in range(C - 4, C + 5):
        for z in range(C - 4, C + 5):
            r = math.hypot(x + .5 - C, z + .5 - C)
            if r < 4.2:
                hot = 0.5 + 0.5 * math.sin(th * 2 + x * 1.7 + z * 2.3)
                g.put(x, 1, z, mix("#7A1E0E", "#FFB347", hot), "ember")
    # flame body
    for x in range(C - 7, C + 8):
        for z in range(C - 7, C + 8):
            r = math.hypot(x + .5 - C, z + .5 - C)
            if r > 5.6:
                continue
            n = 0.5 + 0.25 * math.sin(th * 2 + x * 0.9 + z * 0.5) + 0.25 * math.sin(th + z * 1.1 - x * 0.6 + 1.3)
            h = (5.6 - r) * 2.7 * (0.7 + 0.6 * n)
            for y in range(2, 2 + int(h) + 1):
                v = (y - 2) / max(h, 1)
                sway = math.sin(th + y * 0.38) * 1.4 * v
                xx = round(x + sway)
                zz = round(z + math.cos(th * 2 + y * 0.3) * 0.8 * v)
                rr = math.hypot(xx + .5 - C, zz + .5 - C)
                if rr > 5.6 * (1 - v * 0.8) + 0.6:
                    continue
                edge = r / 5.6
                if v < 0.3 and edge < 0.45:
                    col = "#FFF7D0"
                elif v < 0.5:
                    col = mix("#FFE066", "#FFB12E", edge)
                elif v < 0.78:
                    col = mix("#FF9A1F", "#FF6A1A", edge)
                else:
                    col = "#E8461C"
                g.put(xx, y, zz, col, "flame")
    # detached licks
    for i in range(3):
        p = (t / FRAMES + i / 3) % 1
        size = 1.6 * (1 - p)
        if size < 0.6:
            continue
        cx = C + math.sin(TAU * (p + i * 0.33)) * 2
        cz = C + math.cos(TAU * (p + i * 0.33)) * 2
        g.ellipsoid(cx, 13 + p * 12, cz, size, size * 1.6, size, mix("#FF8C1A", "#D8331A", p), "flame")
    # sparks
    for i in range(16):
        p = (t / FRAMES + i * 0.1373) % 1
        y = 7 + p * 26
        sx = C + math.sin(TAU * (p * 1.3 + i * 0.29)) * (2 + i % 5)
        sz = C + math.cos(TAU * (p * 1.1 + i * 0.41)) * (2 + i % 4)
        g.put(round(sx), round(y), round(sz), "#FFE58A" if p < 0.5 else "#FF7A2A", "spark")
    # smoke (fades as it rises)
    for i in range(6):
        p = (t / FRAMES + i / 6) % 1
        alpha = int(85 * math.sin(math.pi * p) ** 1.2)
        if alpha < 14:
            continue
        g.ellipsoid(C + math.sin(TAU * (p + i * .2)) * 2.5, 20 + p * 16, C - 2 + p * 3, 1.4 + p * 2.6, 1.2 + p * 2, 1.4 + p * 2.4,
                    "#A39C9A%02X" % alpha, "smoke")
    name = f"Campfire_{t:02d}"
    cmd("voxel_model_clone", model="Campfire_base", name=name)
    cmds.extend(g.commands(name, actor="vfx-builder"))

# =============================================================== arcane portal (loop)
for t in range(FRAMES):
    th = TAU * t / FRAMES
    g = Grid(seed=50 + t)
    # stone dais
    for x in range(C - 15, C + 16):
        for z in range(C - 15, C + 16):
            r = math.hypot(x + .5 - C, z + .5 - C)
            if r < 15:
                g.put(x, 0, z, "#4B4659" if (x + z) % 2 else "#433E50", "stone")
                if 13.4 < r:
                    g.put(x, 1, z, "#5C566C", "stone")
    # rotating rune ring (segmented) and counter-rotating inner ring
    for k in range(240):
        a = TAU * k / 240
        seg_outer = math.sin(5 * a - th) > -0.2
        seg_inner = math.sin(3 * a + 2 * th) > 0.1
        if seg_outer:
            g.put(round(C + 11.5 * math.cos(a)), 1, round(C + 11.5 * math.sin(a)), "#2FD3F5", "arcane")
        if seg_inner:
            g.put(round(C + 7.5 * math.cos(a)), 1, round(C + 7.5 * math.sin(a)), "#9446F0", "arcane")
    # glyph spokes
    for i in range(6):
        a = TAU * i / 6 + th / 2
        for s in range(8, 11):
            g.put(round(C + s * math.cos(a)), 1, round(C + s * math.sin(a)), "#8FF4FF", "rune")
    # floating core crystal, bobbing
    bob = math.sin(th) * 1.5
    for y in range(-6, 7):
        w = (6 - abs(y)) / 2.4
        g.ellipsoid(C, 16 + bob + y, C, max(0.6, w), 0.6, max(0.6, w), mix("#7FF0FF", "#1FA8E0", abs(y) / 6), "arcane")
    # orbiting orbs
    for i in range(3):
        a = th + TAU * i / 3
        g.ellipsoid(C + 9 * math.cos(a), 12 + 3 * math.sin(th * 2 + i), C + 9 * math.sin(a), 1.4, 1.4, 1.4, "#A869FF", "arcane")
    # rising motes
    for i in range(20):
        p = (t / FRAMES + i * 0.083) % 1
        a = TAU * (i * 0.17 + p * 0.5)
        r = 4 + (i % 7)
        g.put(round(C + r * math.cos(a)), round(2 + p * 30), round(C + r * math.sin(a)), "#5FE6FF" if i % 2 else "#B98AFF", "spark")
    name = f"Portal_{t:02d}"
    cmd("voxel_model_create", name=name, width=64, height=64, depth=64, origin=[C, 0, C])
    cmds.extend(g.commands(name, actor="vfx-builder"))

# =============================================================== explosion (one-shot)
deb_rng = random.Random(9)
debris = [(deb_rng.uniform(0, TAU), deb_rng.uniform(0.2, 1.2), deb_rng.uniform(0.7, 1.3)) for _ in range(18)]
for t in range(FRAMES):
    g = Grid(seed=90 + t)
    p = t / (FRAMES - 1)
    cy = 12 + p * 10
    # scorched ground
    for x in range(C - 12, C + 13):
        for z in range(C - 12, C + 13):
            r = math.hypot(x + .5 - C, z + .5 - C)
            if r < 12:
                g.put(x, 0, z, "#2B2321" if r < 3 + 9 * min(1, p * 3) else "#5A4636", "dirt")
    if t <= 3:  # flash
        rad = 3 + t * 2.6
        g.ellipsoid(C, 8 + t, C, rad, rad, rad, "#FFFBE6" if t < 2 else "#FFE58A", "flash")
    elif t <= 10:  # lumpy fireball cooling into smoke
        q = (t - 4) / 6
        lumps = [(0, 0, 0, 9.5)] + [(math.cos(a) * 6.5, math.sin(a * 1.7) * 4 + 2, math.sin(a) * 6.5, 5.5) for a in (0.3, 1.4, 2.5, 3.6, 4.7, 5.6)]
        grow = 0.85 + q * 0.6
        for (lx, ly, lz, lr) in lumps:
            R = lr * grow
            for x in range(int(C + lx - R - 1), int(C + lx + R + 2)):
                for y in range(1, int(cy + ly + R + 2)):
                    for z in range(int(C + lz - R - 1), int(C + lz + R + 2)):
                        d = math.sqrt((x + .5 - C - lx) ** 2 + (y + .5 - cy - ly) ** 2 + (z + .5 - C - lz) ** 2) / R
                        n = 0.5 + 0.5 * math.sin(x * 0.9 + y * 0.7 + t) * math.cos(z * 0.8 - y * 0.5)
                        if d > 0.88 + 0.18 * n:
                            continue
                        heat = (1 - d) * (1 - q) + 0.25 * n
                        if q > 0.45 and d > 0.62:
                            col, mat = mix("#A0958F", "#6B6260", n) + "%02X" % int(200 - 110 * q), "smoke"
                        elif heat > 0.62:
                            col, mat = "#FFF0A8", "flame"
                        elif heat > 0.4:
                            col, mat = "#FFB02E", "flame"
                        elif heat > 0.22:
                            col, mat = "#F26A1B", "flame"
                        else:
                            col, mat = mix("#B8301A", "#5A2A22", q), "flame"
                        g.put(x, y, z, col, mat)
    else:  # rising smoke column, fading
        q = (t - 11) / 4
        alpha = int(150 * (1 - q * 0.8))
        for i in range(9):
            a = TAU * i / 9 + q
            r = 6 + q * 6
            g.ellipsoid(C + r * math.cos(a) * 0.6, 16 + q * 14 + (i % 3) * 3, C + r * math.sin(a) * 0.6, 4 + q * 2, 3.5 + q * 2, 4 + q * 2,
                        mix("#9A928F", "#726A68", i / 9) + "%02X" % alpha, "smoke")
        for i in range(6):
            g.put(C + i - 3, 1, C + (i * 7) % 5 - 2, "#FF7A2A", "ember")
    # flying debris with gravity
    if t >= 1:
        for (a, up, sp) in debris:
            tt = t / 4
            x = C + math.cos(a) * sp * 6 * tt
            z = C + math.sin(a) * sp * 6 * tt
            y = 8 + up * 9 * tt - 1.2 * tt * tt
            if y > 0.5:
                g.box(round(x), round(y), round(z), round(x) + 1, round(y), round(z) + 1, "#3A2F2B" if (t + int(a * 10)) % 2 else "#FF8A2A",
                      "debris" if t % 2 else "ember")
    name = f"Explosion_{t:02d}"
    cmd("voxel_model_create", name=name, width=64, height=64, depth=64, origin=[C, 0, C])
    cmds.extend(g.commands(name, actor="vfx-builder"))

here = os.path.dirname(os.path.abspath(__file__))
write_jsonl(os.path.join(here, "vfx.jsonl"), cmds)

# =============================================================== renders
show = list(cmds)
look = {"pitch": 32, "yaw": 35, "smoothness": 0.0, "supersample": 2, "width": 256, "frame_ms": 66, "bloom": 0.7, "bloom_radius": 4, "shadows": 0.6}
fire_light = [{"position": [C, 7, C], "color": "#FF8A30", "intensity": 5, "radius": 26}]
portal_light = [{"position": [C, 14, C], "color": "#4FD8FF", "intensity": 3.5, "radius": 26}]
boom_light = [{"position": [C, 12, C], "color": "#FFA040", "intensity": 6, "radius": 36}]


def out(op, save, **a):
    show.append({"op": op, "_save": save, **a})


for fx, lights, extra in (("Campfire", fire_light, {"ambient": 0.6}), ("Portal", portal_light, {"ambient": 0.5}), ("Explosion", boom_light, {})):
    seq = {**look, **extra, "models": frame_models(fx), "name": fx.lower(), "lights": lights}
    out("voxel_render_sheet", f"vfx/{fx.lower()}_16f_256.png", _json=True, **seq)
    out("voxel_render_sheet", f"vfx/{fx.lower()}_16f_256_dark_bg.png", **{**seq, "background_gradient": ["#2A2430", "#0B0A10"]})
    out("voxel_render", f"vfx/{fx.lower()}_hero_512.png", model=f"{fx}_05", width=512, pitch=30, yaw=35, smoothness=0.0, supersample=2,
        bloom=0.7, bloom_radius=7, shadows=0.6, lights=lights, background_gradient=["#2A2430", "#0B0A10"], **extra)
    show.append({"op": "voxel_render_to_frames", **{**seq, "layer": fx}})
    out("export_gif", f"vfx/{fx.lower()}_16f.gif", tag=f"{fx.lower()}_S", scale=2)
out("project_save", "vfx/PixelStudio_VFX.pxs")
write_jsonl(os.path.join(here, "vfx_showcase.jsonl"), show)
print(f"{len(cmds)} commands")
