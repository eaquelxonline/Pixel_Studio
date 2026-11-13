#!/usr/bin/env python3
"""Pixel Studio comparative asset-production benchmark.

Builds the same original asset designs through three production paths, using only the
public engine commands (exactly the tools an AI agent calls over MCP):

  A  raw     — flat primitives only (what a naive agent does: shapes, no finishing)
  B  craft   — primitives + finishing tools (pattern_fill, auto_shade, outline selout,
               draw_sparkle, dither_fill, layer_blend …)
  C  voxel   — a 3D voxel model rendered and baked to pixels (voxel_render_to_frames)
  G  generator — one fx_generate call (animated effects)

Assets: cottage (house), ranger (character), slime (monster), treasure chest, potion,
oak tree, explosion (8-frame effect), magic burst (8-frame effect).

Each (asset, path) becomes one command script. `python3 Assets/Bench.py` writes the
scripts, replays them through Native/Script.rs, measures every result and writes
Assets/Build/bench/report.md + a side-by-side comparison sheet.

Metrics: tool calls, payload bytes, engine wall time, distinct colors, shade depth
(average tones per material hue), outline coverage (dark silhouette edge), and pass/fail
against quality bars for game-ready pixel art.
"""

import colorsys
import json
import math
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUT = os.path.join(HERE, "Build", "bench")
RUNNER = os.path.join(ROOT, "Native", "target", "release", "examples", "run_script")


class Script:
    """A command script plus bookkeeping (calls exclude project setup and exports)."""

    def __init__(self, name, w, h, frames=1):
        self.name, self.w, self.h, self.frames = name, w, h, frames
        self.lines, self.calls = [], 0
        self.raw("project_new", width=w, height=h, name=name)
        if frames > 1:
            self.raw("frame_add", count=frames - 1)

    def raw(self, op, **a):
        self.lines.append(json.dumps({"op": op, **a}, separators=(",", ":")))

    def __call__(self, op, **a):
        self.calls += 1
        self.raw(op, **a)

    def layers(self, *names):
        self.layer_names = names
        self.raw("layer_rename", layer=0, name=names[0])
        for n in names[1:]:
            self.raw("layer_create", name=n)

    def payload(self):
        return sum(len(l) for l in self.lines)


def ring(cx, cy, rx, ry, n, phase=0.0):
    return [[round(cx + math.cos(phase + i / n * math.tau) * rx, 1), round(cy + math.sin(phase + i / n * math.tau) * ry, 1)] for i in range(n)]


# ═════════════════════════════════════════════════════════════════════════ cottage (64×64)

COTTAGE = dict(plaster="#E9DCC2", beam="#6E4A33", roof="#B9573D", roof_seam="#7A3329", stone="#9C9AA6", stone_seam="#5E5C6E",
               door="#8B5A3A", door_seam="#5A3624", window="#FFD27A", chimney="#A4604A", flower="#E86F8A", leaf="#5E9E4A")


def cottage_blockout(s, c, layers):
    back, walls, roof, det = layers
    s("draw_rectangle", layer=back, x=42, y=6, width=7, height=14, color=c["chimney"], filled=True)
    s("draw_rectangle", layer=walls, x=12, y=30, width=40, height=26, color=c["plaster"], filled=True)
    s("draw_rectangle", layer=walls, x=12, y=50, width=40, height=6, color=c["stone"], filled=True)
    s("draw_polygon", layer=roof, points=[[5, 32], [18, 12], [46, 12], [59, 32]], color=c["roof"])
    s("draw_rectangle", layer=walls, x=28, y=41, width=9, height=15, color=c["door"], filled=True)
    for wx in (16, 41):
        s("draw_rectangle", layer=walls, x=wx, y=36, width=7, height=7, color=c["window"], filled=True)


def cottage_raw():
    s = Script("cottage_A", 64, 64)
    s.layers("Back", "Walls", "Roof", "Details")
    cottage_blockout(s, COTTAGE, ("Back", "Walls", "Roof", "Details"))
    return s


def cottage_craft():
    c = COTTAGE
    s = Script("cottage_B", 64, 64)
    s.layers("Shadow", "Back", "Walls", "Roof", "Details", "Glow")
    s("draw_ellipse", layer="Shadow", x=6, y=52, width=54, height=9, color="#6A6E9A", filled=True)
    s("layer_blend", layer="Shadow", mode="multiply")
    cottage_blockout(s, c, ("Back", "Walls", "Roof", "Details"))
    # Surfaces: shingles, stone base, plank door, brick chimney, timber frame.
    s("pattern_fill", layer="Roof", pattern="shingle", size=4, color=c["roof_seam"], accent="#C96A4E", seed=3)
    s("pattern_fill", layer="Walls", pattern="stone", size=3, x=12, y=50, width=40, height=6, color=c["stone_seam"], seed=5)
    s("pattern_fill", layer="Walls", pattern="plank", size=3, x=28, y=41, width=9, height=15, color=c["door_seam"], seed=2)
    s("pattern_fill", layer="Back", pattern="brick", size=2, color="#6E3A2E")
    for x0, y0, x1, y1 in [(12, 30, 51, 30), (12, 30, 12, 49), (51, 30, 51, 49), (24, 30, 24, 49), (39, 30, 39, 49), (12, 44, 51, 44), (13, 31, 23, 43), (50, 31, 40, 43)]:
        s("draw_line", layer="Walls", x0=x0, y0=y0, x1=x1, y1=y1, color=c["beam"])
    for wx in (16, 41):
        s("draw_line", layer="Walls", x0=wx + 3, y0=36, x1=wx + 3, y1=42, color=c["beam"])
        s("draw_line", layer="Walls", x0=wx, y0=39, x1=wx + 6, y1=39, color=c["beam"])
    # Finishing: shading with cast shadows (roof over the walls), sel-out outlines.
    s("auto_shade", layer="Back", tones=3, cast_shadow=0, min_region=4)
    s("auto_shade", layer="Walls", tones=3, cast_shadow=4, bevel=2, contrast=0.45, preserve=[c["window"], c["beam"]], min_region=4)
    s("auto_shade", layer="Roof", tones=4, bevel=3, contrast=0.55, cast_shadow=0, min_region=3)
    for layer in ("Back", "Walls", "Roof"):
        s("outline", layer=layer, mode="selout", darkness=0.55)
    # Details: flower boxes, door knob, smoke, window glow.
    for wx in (15, 40):
        s("draw_rectangle", layer="Details", x=wx, y=43, width=9, height=2, color="#7A4A30", filled=True)
        s("set_pixels", layer="Details", pixels=[[wx + i, 42, c["flower"] if i % 2 else c["leaf"]] for i in range(9)])
    s("set_pixel", layer="Details", x=34, y=49, color="#F2C95A")
    s("draw_ellipse", layer="Glow", x=13, y=33, width=13, height=13, color="#3A2408", filled=True)
    s("draw_ellipse", layer="Glow", x=38, y=33, width=13, height=13, color="#3A2408", filled=True)
    s("layer_blend", layer="Glow", mode="add")
    s("outline", layer="Details", mode="selout", darkness=0.5)
    # Palette lock: one shared 32-color palette for every layer keeps the house game-ready.
    s("quantize", scope="all", colors=32)
    return s


def cottage_voxel():
    s = Script("cottage_C", 64, 64)
    s("voxel_model_create", name="Cottage", width=48, height=44, depth=40)
    s("texture_generate", name="stone", kind="stone", size=32, cells=6)
    s("texture_generate", name="wood", kind="wood", size=32, cells=5)
    s("voxel_shape", shape="box", min=[6, 0, 6], max=[42, 4, 34], color="#9C9AA6", material="stone", jitter=0.2)
    s("voxel_shape", shape="box", min=[6, 4, 6], max=[42, 20, 34], color="#E9DCC2", jitter=0.1)
    for k in range(12):  # stepped gable roof
        s("voxel_shape", shape="box", min=[4, 20 + k, 4 + k], max=[44, 21 + k, 36 - k], color="#B9573D", jitter=0.25, seed=k)
    s("voxel_shape", shape="box", min=[30, 24, 12], max=[35, 38, 17], color="#A4604A", material="stone")
    s("voxel_shape", shape="box", min=[20, 4, 34], max=[27, 15, 36], color="#8B5A3A", material="wood")
    for wx in (10, 33):
        s("voxel_shape", shape="box", min=[wx, 9, 34], max=[wx + 5, 14, 35], color="#FFD27A")
    s("voxel_render_to_frames", directions=1, width=64, height=64, yaw=25, pitch=30, outline=True, pixel_palette=24, layer="Cottage")
    return s


# ═════════════════════════════════════════════════════════════════════════ ranger (32×32)

RANGER = dict(skin="#F6D3B8", hood="#4E8B4F", tunic="#6C9A4A", belt="#6B4A32", legs="#5A4E6E", boots="#6B4A32", cape="#3F6E46", hair="#C98A4B")


def ranger_blockout(s, c, back, body, head, front):
    s("draw_polygon", layer=back, points=[[10, 13], [22, 13], [24, 27], [8, 27]], color=c["cape"])
    s("draw_rectangle", layer=body, x=12, y=25, width=3, height=4, color=c["legs"], filled=True)
    s("draw_rectangle", layer=body, x=17, y=25, width=3, height=4, color=c["legs"], filled=True)
    s("draw_rectangle", layer=body, x=11, y=28, width=4, height=2, color=c["boots"], filled=True)
    s("draw_rectangle", layer=body, x=17, y=28, width=4, height=2, color=c["boots"], filled=True)
    s("draw_polygon", layer=body, points=[[11, 15], [21, 15], [22, 26], [10, 26]], color=c["tunic"])
    s("draw_rectangle", layer=body, x=10, y=21, width=12, height=1, color=c["belt"], filled=True)
    s("draw_rectangle", layer=body, x=8, y=16, width=3, height=7, color=c["tunic"], filled=True)
    s("draw_rectangle", layer=body, x=21, y=16, width=3, height=7, color=c["tunic"], filled=True)
    s("draw_rectangle", layer=body, x=8, y=23, width=3, height=2, color=c["skin"], filled=True)
    s("draw_rectangle", layer=body, x=21, y=23, width=3, height=2, color=c["skin"], filled=True)
    s("draw_ellipse", layer=head, x=10, y=4, width=12, height=12, color=c["skin"], filled=True)
    s("draw_polygon", layer=front, points=[[9, 9], [12, 3], [16, 1], [20, 3], [23, 9], [21, 7], [16, 5], [11, 7]], color=c["hood"])
    s("draw_rectangle", layer=front, x=12, y=6, width=8, height=2, color=c["hair"], filled=True)


def ranger_raw():
    s = Script("ranger_A", 32, 32)
    s.layers("Back", "Body", "Head", "Front", "Details")
    ranger_blockout(s, RANGER, "Back", "Body", "Head", "Front")
    s("set_pixels", layer="Details", pixels=[[13, 10, "#2A2238"], [18, 10, "#2A2238"]])
    return s


def ranger_craft():
    s = Script("ranger_B", 32, 32)
    s.layers("Back", "Body", "Head", "Front", "Details")
    ranger_blockout(s, RANGER, "Back", "Body", "Head", "Front")
    s("auto_shade", layer="Back", tones=3, cast_shadow=2, min_region=4)
    s("auto_shade", layer="Body", tones=3, bevel=1.5, cast_shadow=2, min_region=3)
    s("auto_shade", layer="Head", tones=2, bevel=2, form=0.3, contrast=0.3, cast_shadow=2)
    s("auto_shade", layer="Front", tones=3, bevel=1.5, min_region=3, cast_shadow=0)
    for layer in ("Back", "Body", "Head", "Front"):
        s("outline", layer=layer, mode="selout", darkness=0.55)
    s("draw_ascii", layer="Details", x=12, y=9, rows=["L....L", "E....E", "......", ".b..b."],
      legend={"L": "#3A2A3A", "E": "#3B6FB0", "b": "#F2A0A0"})
    s("pixel_cleanup", layer="Front")
    return s


def ranger_voxel():
    s = Script("ranger_C", 32, 32)
    s("voxel_model_create", name="Ranger", width=20, height=30, depth=14)
    c = RANGER
    s("voxel_shape", shape="capsule", a=[8, 1, 7], b=[8, 9, 7], radius=1.6, color=c["legs"], mirror_x=True)
    s("voxel_shape", shape="box", min=[6, 9, 4], max=[14, 18, 10], color=c["tunic"])
    s("voxel_shape", shape="box", min=[5, 13, 3], max=[15, 14, 11], color=c["belt"])
    s("voxel_shape", shape="capsule", a=[4.5, 17, 7], b=[4, 11, 7], radius=1.4, color=c["tunic"], mirror_x=True)
    s("voxel_shape", shape="ellipsoid", center=[10, 22.5, 7], radius=5, color=c["skin"])
    s("voxel_shape", shape="ellipsoid", center=[10, 24.5, 6.4], radii=[5.6, 4.6, 5.6], color=c["hood"])
    s("voxel_shape", shape="box", min=[8, 21, 11], max=[9, 23, 12], color="#2A2238")
    s("voxel_shape", shape="box", min=[11, 21, 11], max=[12, 23, 12], color="#2A2238")
    s("voxel_render_to_frames", directions=1, width=32, height=32, yaw=20, pitch=20, scale=0.95, outline=True, pixel_palette=16, layer="Ranger")
    return s


# ═════════════════════════════════════════════════════════════════════════ slime (32×32)


def slime(path):
    s = Script(f"slime_{path}", 32, 32)
    s.layers("Shadow", "Body", "Details")
    s("draw_ellipse", layer="Shadow", x=5, y=25, width=22, height=5, color="#6A6E9A" if path == "B" else "#55557A", filled=True)
    s("draw_polygon", layer="Body", points=[[5, 27], [6, 18], [10, 11], [16, 8], [22, 11], [26, 18], [27, 27]], color="#7FD07A")
    s("draw_ascii", layer="Details", x=11, y=15, rows=["W...W", "D...D", "..m.."], legend={"W": "#FFFFFF", "D": "#1E3B2A", "m": "#1E3B2A"})
    if path == "B":
        s("layer_blend", layer="Shadow", mode="multiply")
        s("auto_shade", layer="Body", tones=4, bevel=4, contrast=0.6, rim="#D8FFF0", rim_strength=0.4, cast_shadow=0)
        s("outline", layer="Body", mode="selout", darkness=0.55)
        s("draw_sparkle", layer="Details", points=[[11, 12, 1], [13, 11, 1]], style="dot", color="#FFFFFF")
    return s


# ═════════════════════════════════════════════════════════════════════════ chest (32×32)


def chest(path):
    s = Script(f"chest_{path}", 32, 32)
    s.layers("Shadow", "Box", "Metal")
    s("draw_ellipse", layer="Shadow", x=3, y=25, width=26, height=5, color="#6A6E9A", filled=True)
    s("draw_rectangle", layer="Box", x=5, y=15, width=22, height=12, color="#9A6238", filled=True)
    s("draw_polygon", layer="Box", points=[[5, 15], [6, 9], [10, 7], [22, 7], [26, 9], [27, 15]], color="#A86E42")
    for x in (8, 22):
        s("draw_rectangle", layer="Metal", x=x, y=7, width=2, height=20, color="#E8B84A", filled=True)
    s("draw_rectangle", layer="Metal", x=14, y=13, width=4, height=5, color="#E8B84A", filled=True)
    s("set_pixels", layer="Metal", pixels=[[15, 15, "#3A2A20"], [16, 15, "#3A2A20"], [15, 16, "#3A2A20"]])
    if path == "B":
        s("layer_blend", layer="Shadow", mode="multiply")
        s("pattern_fill", layer="Box", pattern="plank", size=3, color="#6A3E22", seed=4)
        s("draw_line", layer="Box", x0=5, y0=15, x1=26, y1=15, color="#5A3420")
        s("auto_shade", layer="Box", tones=3, cast_shadow=2, min_region=4)
        s("auto_shade", layer="Metal", tones=4, bevel=1.5, contrast=0.7, cast_shadow=0, preserve=["#3A2A20"], min_region=3)
        s("outline", layer="Box", mode="selout", darkness=0.6)
        s("outline", layer="Metal", mode="selout", darkness=0.55)
        s("draw_sparkle", layer="Metal", points=[[24, 9, 2]], color="#FFF3C4")
    return s


# ═════════════════════════════════════════════════════════════════════════ potion (24×24)


def potion(path):
    s = Script(f"potion_{path}", 24, 24)
    s.layers("Glass", "Liquid", "Cork", "FX")
    s("draw_ellipse", layer="Glass", x=4, y=8, width=16, height=15, color="#CFE8F2", filled=True)
    s("draw_rectangle", layer="Glass", x=9, y=4, width=6, height=6, color="#CFE8F2", filled=True)
    s("draw_ellipse", layer="Liquid", x=5, y=12, width=14, height=10, color="#E0508C", filled=True)
    s("draw_rectangle", layer="Cork", x=9, y=1, width=6, height=4, color="#B58050", filled=True)
    if path == "B":
        s("auto_shade", layer="Glass", tones=3, contrast=0.4, cast_shadow=0)
        s("auto_shade", layer="Liquid", tones=4, bevel=3, contrast=0.65, cast_shadow=0)
        s("auto_shade", layer="Cork", tones=3, cast_shadow=0, min_region=3)
        s("outline", layer="Glass", mode="selout", darkness=0.6)
        s("outline", layer="Cork", mode="selout", darkness=0.55)
        s("set_pixels", layer="FX", pixels=[[7, 11, "#FFFFFF"], [7, 12, "#FFFFFF"], [8, 10, "#FFFFFF"], [14, 16, "#FFC2DA"], [11, 18, "#FFC2DA"]])
        s("draw_sparkle", layer="FX", points=[[19, 6, 2]], color="#FFE0F0")
    return s


# ═════════════════════════════════════════════════════════════════════════ oak tree (48×64)


def tree(path):
    s = Script(f"tree_{path}", 48, 64)
    if path == "C":
        s("voxel_model_create", name="Oak", width=40, height=56, depth=40)
        s("voxel_shape", shape="cone", a=[20, 0, 20], b=[20, 26, 20], radius=4.5, radius_b=2.2, color="#7A5236")
        for i, (x, y, z, r) in enumerate([(20, 34, 20, 13), (11, 29, 17, 8), (29, 30, 22, 8), (18, 43, 18, 9), (25, 39, 26, 8)]):
            s("voxel_shape", shape="ellipsoid", center=[x, y, z], radius=r, color="#4E9A45" if i % 2 else "#5BAA4E", jitter=0.35, seed=i)
        s("voxel_render_to_frames", directions=1, width=48, height=64, yaw=30, pitch=25, outline=True, pixel_palette=16, layer="Oak")
        return s
    s.layers("Shadow", "Trunk", "Leaves")
    s("draw_ellipse", layer="Shadow", x=8, y=56, width=32, height=7, color="#6A6E9A", filled=True)
    s("draw_polygon", layer="Trunk", points=[[20, 60], [22, 34], [26, 34], [29, 60]], color="#7A5236")
    for i, (x, y, w, h) in enumerate([(6, 14, 22, 20), (20, 10, 22, 22), (12, 4, 24, 18), (4, 26, 18, 14), (24, 24, 20, 16), (14, 22, 20, 16)]):
        s("draw_ellipse", layer="Leaves", x=x, y=y, width=w, height=h, color="#5BAA4E" if i % 2 == 0 or path == "A" else "#4E9A45", filled=True)
    if path == "B":
        s("layer_blend", layer="Shadow", mode="multiply")
        s("pattern_fill", layer="Trunk", pattern="plank", size=2, color="#5A3A26", seed=9)
        s("auto_shade", layer="Trunk", tones=3, cast_shadow=4, min_region=3)
        s("auto_shade", layer="Leaves", tones=4, bevel=4, contrast=0.6, cast_shadow=0)
        s("pattern_fill", layer="Leaves", pattern="grass", color="#8CCB63", seed=2)
        s("outline", layer="Trunk", mode="selout", darkness=0.55)
        s("outline", layer="Leaves", mode="selout", darkness=0.6)
        s("pixel_cleanup", layer="Leaves")
    return s


# ═════════════════════════════════════════════════════════════════════════ effects (48×48, 8 frames)

FIRE = ["#FFF9D6", "#FFE066", "#FFA431", "#F2581F", "#B72727"]


def explosion(path):
    n = 8
    s = Script(f"explosion_{path}", 48, 48, frames=n)
    s.layers("Smoke", "Fire", "Sparks")
    for f in range(n):
        t = f / (n - 1)
        r = 4 + 18 * math.sqrt(t)
        if path == "A":
            s("draw_ellipse", layer="Fire", frame=f, x=round(24 - r), y=round(24 - r), width=round(r * 2), height=round(r * 2), color="#FF8A30", filled=True)
            continue
        bands = [(1.0, 4), (0.78, 3), (0.55, 2), (0.34, 1), (0.16, 0)]
        for k, (scale, ci) in enumerate(bands):
            rr = r * scale * (1 - 0.55 * t if ci <= 1 else 1 - 0.25 * t)
            if rr < 1:
                continue
            pts = ring(24, 24, rr, rr * 0.92, 14, phase=f * 0.4 + k)
            pts = [[x + math.sin(i * 2.3 + f) * rr * 0.12, y + math.cos(i * 1.7 + f) * rr * 0.12] for i, (x, y) in enumerate(pts)]
            s("draw_polygon", layer="Fire", frame=f, points=pts, color=FIRE[ci])
        if t > 0.85:
            # Dissipation: the fire is spent, only thinning smoke remains.
            for i in range(6):
                a = i / 6 * math.tau + f * 0.2
                sr = 3 + 3 * (1 - t)
                s("draw_ellipse", layer="Smoke", frame=f, x=round(24 + math.cos(a) * r - sr), y=round(24 + math.sin(a) * r - sr - 6), width=round(sr * 2), height=round(sr * 2), color="#8E8496", filled=True)
            s("dither_fill", layer="Smoke", frame=f, color="#8E8496", color2="transparent", level=10, only_opaque=True)
            continue
        if t > 0.35:
            for i in range(6):
                a = i / 6 * math.tau + f * 0.2
                d = r * 0.85
                sr = 3 + 5 * t
                s("draw_ellipse", layer="Smoke", frame=f, x=round(24 + math.cos(a) * d - sr), y=round(24 + math.sin(a) * d - sr - 6 * t), width=round(sr * 2), height=round(sr * 2), color="#6E6476", filled=True)
            s("dither_fill", layer="Smoke", frame=f, color="#6E6476", color2="#8E8496", level=int(4 + 8 * t), only_opaque=True)
        if f < 3:
            s("draw_ellipse", layer="Sparks", frame=f, x=round(24 - r - 3), y=round(24 - r - 3), width=round(r * 2 + 6), height=round(r * 2 + 6), color="#FFE9B0")
        s("draw_sparkle", layer="Sparks", frame=f, points=[[round(24 + math.cos(i * 1.9 + f * 0.3) * (r + 4)), round(24 + math.sin(i * 1.9) * (r + 4)), 1 + (i + f) % 2] for i in range(6)], color="#FFD27A")
    if path == "B":
        s("auto_shade", layer="Smoke", scope="layer", tones=3, contrast=0.5, cast_shadow=0, min_region=4)
        s("outline", layer="Smoke", mode="selout", darkness=0.45)
        s("layer_blend", layer="Sparks", mode="add")
    return s


def magic(path):
    n = 8
    s = Script(f"magic_{path}", 48, 48, frames=n)
    s.layers("Glow", "Runes", "Sparks")
    for f in range(n):
        t = f / n
        if path == "A":
            s("draw_ellipse", layer="Runes", frame=f, x=8, y=8, width=32, height=32, color="#9A7BFF")
            continue
        s("draw_ellipse", layer="Glow", frame=f, x=6, y=6, width=36, height=36, color="#2A1A55", filled=True)
        s("dither_fill", layer="Glow", frame=f, color="#2A1A55", color2="#4A2E8F", level=6 + (f % 4), only_opaque=True)
        s("draw_ellipse", layer="Runes", frame=f, x=8, y=8, width=32, height=32, color="#B9A2FF")
        s("draw_ellipse", layer="Runes", frame=f, x=13, y=13, width=22, height=22, color="#8E6CFF")
        pts = ring(24, 24, 15, 15, 5, phase=t * math.tau / 5 - math.pi / 2)
        star = [pts[i * 2 % 5] for i in range(5)]
        s("draw_polygon", layer="Runes", frame=f, points=star, color="#E6DBFF", filled=False)
        s("draw_sparkle", layer="Sparks", frame=f, points=[[round(24 + math.cos(t * math.tau + i * 1.257) * 19), round(24 + math.sin(t * math.tau + i * 1.257) * 19), 2 + (i + f) % 3] for i in range(5)], color="#D8C8FF", core="#FFFFFF")
    if path == "B":
        s("layer_blend", layer="Glow", mode="screen")
        s("layer_blend", layer="Sparks", mode="add")
    return s


FX_FRAMES = {"fire": 16, "campfire": 16, "slash": 8, "hit": 8, "magic": 12, "smoke": 8, "lightning": 8, "heal": 12, "explosion": 12,
             "shockwave": 10, "beam": 12, "portal": 12, "ice": 10, "poison": 12, "splash": 10, "dash": 8, "shield": 12, "levelup": 14,
             "pickup": 8, "dust": 8, "holy": 12, "shadow": 16, "teleport": 12}
FX_LOOPS = {"fire", "campfire", "heal", "portal", "poison", "shield", "pickup", "shadow"}


def generated(kind, size=48, frames=None, glow=False, suffix=""):
    def make():
        s = Script(f"{kind}{suffix}_G", size, size, frames=1)
        s.layers("FX")
        args = dict(kind=kind, layer="FX", tag=kind)
        if frames:
            args["frames"] = frames
        if glow:
            args["glow"] = True
        s("fx_generate", **args)
        s.frames = frames or FX_FRAMES[kind]
        s.looped = kind in FX_LOOPS
        return s
    return make


def particles(name, frames=12, **args):
    def make():
        s = Script(f"{name}_P", 64, 64, frames=1)
        s.layers("FX")
        s("fx_particles", layer="FX", frames=frames, tag=name, **args)
        s.frames = frames
        s.looped = args.get("emit") == "stream"
        return s
    return make


# Secondary motion: every still asset becomes an ≥ 8-frame loop with one or two calls.
MOTION = {
    "Cottage (house)": "smoke",
    "Ranger (character)": dict(kind="breathe", amount=1),
    "Slime (monster)": dict(kind="squash", amount=0.12),
    "Treasure chest": dict(kind="float", amount=1),
    "Potion": dict(kind="float", amount=1),
    "Oak tree": dict(kind="sway", amount=2),
}


def animated(title, make):
    def build():
        s = make()
        if s.frames >= 8:
            return s
        motion = MOTION[title]
        if motion == "smoke":
            s("layer_create", name="Smoke")
            s("fx_particles", layer="Smoke", frames=8, emit="stream", count=14, origin=[45, 6], area=[1, 0], angle=-90, spread=30,
              speed=[0.4, 0.8], gravity=-0.02, drag=0.0, size=[2, 4], life=[6, 8], colors=["#E9E4E6", "#C5BDC2", "#9A9098", "#6E6470"], seed=4)
            if "Glow" in getattr(s, "layer_names", ()):
                s("anim_generate", layer="Glow", kind="pulse", amount=0.4)
        else:
            s("anim_generate", **motion)
        s.frames = 8
        s.looped = True
        return s
    return build


SUITE = [
    ("Cottage (house)", "building", [("A", animated("Cottage (house)", cottage_raw)), ("B", animated("Cottage (house)", cottage_craft)),
                                     ("C", animated("Cottage (house)", cottage_voxel))]),
    ("Ranger (character)", "character", [("A", animated("Ranger (character)", ranger_raw)), ("B", animated("Ranger (character)", ranger_craft)),
                                         ("C", animated("Ranger (character)", ranger_voxel))]),
    ("Slime (monster)", "character", [("A", animated("Slime (monster)", lambda: slime("A"))), ("B", animated("Slime (monster)", lambda: slime("B")))]),
    ("Treasure chest", "prop", [("A", animated("Treasure chest", lambda: chest("A"))), ("B", animated("Treasure chest", lambda: chest("B")))]),
    ("Potion", "item", [("A", animated("Potion", lambda: potion("A"))), ("B", animated("Potion", lambda: potion("B")))]),
    ("Oak tree", "nature", [("A", animated("Oak tree", lambda: tree("A"))), ("B", animated("Oak tree", lambda: tree("B"))),
                            ("C", animated("Oak tree", lambda: tree("C")))]),
    ("Explosion (8f)", "effect", [("A", lambda: explosion("A")), ("B", lambda: explosion("B")), ("G", generated("explosion", frames=8))]),
    ("Magic circle (8f)", "effect", [("A", lambda: magic("A")), ("B", lambda: magic("B")), ("G", generated("magic", frames=8))]),
]

# The VFX pack: every generator kind at 64×64 (≥ 8 frames), "lit" variants with the additive
# glow pass, and custom effects built with the particle emitter.
FIRE_RAMP = ["#FFF9D6", "#FFE066", "#FFA431", "#F2581F", "#B72727"]
VFX_SUITE = [(k, generated(k, size=64)) for k in FX_FRAMES] + [
    ("fire (lit)", generated("fire", size=64, glow=True, suffix="_lit")),
    ("portal (lit)", generated("portal", size=64, glow=True, suffix="_lit")),
    ("beam (lit)", generated("beam", size=64, glow=True, suffix="_lit")),
    ("holy (lit)", generated("holy", size=64, glow=True, suffix="_lit")),
    ("rain", particles("rain", frames=8, emit="stream", count=40, origin=[32, -4], area=[40, 0], angle=100, spread=4, speed=[4, 5],
                       gravity=0.1, drag=0, life=[8, 8], size=[2, 2], shape="streak", colors=["#C9E6FF", "#8FB8E8"], seed=2)),
    ("snow", particles("snow", frames=16, emit="stream", count=28, origin=[32, -2], area=[36, 0], angle=95, spread=40, speed=[3.2, 4.6],
                       gravity=0.0, drag=0.0, life=[16, 16], size=[2, 1], shape="dot", colors=["#FFFFFF", "#DDEBFF"], seed=3)),
    ("embers", particles("embers", frames=12, emit="stream", count=20, origin=[32, 60], area=[14, 0], angle=-90, spread=40, speed=[3.0, 4.5],
                         gravity=-0.05, drag=0.0, life=[12, 12], size=[2, 1], shape="dot", colors=FIRE_RAMP, seed=5)),
    ("confetti", particles("confetti", frames=12, count=40, origin=[32, 40], angle=-90, spread=120, speed=[2, 4], gravity=0.22, drag=0.04,
                           life=[10, 12], size=[2, 2], shape="square", colors=["#FF6B8A", "#FFD24A", "#6FE3B0", "#7AA8FF", "#C68BFF"], seed=6)),
    ("coin burst", particles("coin_burst", frames=10, count=14, origin=[32, 44], angle=-90, spread=140, speed=[2, 3.5], gravity=0.3, drag=0.02,
                             life=[9, 10], size=[3, 2], shape="star", colors=["#FFFFFF", "#FFE58A", "#FFC23D", "#C8862B"], seed=7)),
    ("magic dust", particles("magic_dust", frames=12, emit="stream", count=18, origin=[32, 32], area=[18, 18], angle=-90, spread=360,
                             speed=[0.2, 0.6], gravity=-0.02, drag=0.05, life=[6, 10], size=[3, 1], shape="star",
                             colors=["#FFFFFF", "#E7C9FF", "#B57BFF", "#7A3FE0"], seed=8)),
]


# ═════════════════════════════════════════════════════════════════════════ measurement


def metrics(img):
    """Pixel-art quality measurements of an RGBA PIL image (transparent background)."""
    w, h = img.size
    px = img.load()
    opaque = [(x, y) for y in range(h) for x in range(w) if px[x, y][3] > 0]
    colors = {px[x, y][:3] for x, y in opaque}
    lum = lambda c: 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]  # noqa: E731
    # Shade depth: distinct tones per hue family (families holding ≥ 6 % of the pixels).
    fam = {}
    for x, y in opaque:
        r, g, b = (v / 255 for v in px[x, y][:3])
        hh, ll, ss = colorsys.rgb_to_hls(r, g, b)
        key = "grey" if ss < 0.12 else int(((hh * 360 + 15) % 360) // 30)
        fam.setdefault(key, []).append(px[x, y][:3])
    big = [v for v in fam.values() if len(v) >= 0.06 * len(opaque)]
    depth = sum(len(set(v)) for v in big) / max(1, len(big))
    # Silhouette definition: the edge pixel's tone differs from the first interior pixel found
    # by walking inward (sel-out may make lit edges lighter — that still defines the shape).
    def is_edge(x, y):
        return any(not (0 <= x + dx < w and 0 <= y + dy < h) or px[x + dx, y + dy][3] == 0 for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))

    edge, dark = 0, 0
    for x, y in opaque:
        outs = [(dx, dy) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)) if not (0 <= x + dx < w and 0 <= y + dy < h) or px[x + dx, y + dy][3] == 0]
        if not outs:
            continue
        edge += 1
        dx, dy = -sum(o[0] for o in outs), -sum(o[1] for o in outs)
        if dx == 0 and dy == 0:
            dark += 1  # one-pixel-thin feature: it is all edge
            continue
        dx, dy = (dx > 0) - (dx < 0), (dy > 0) - (dy < 0)
        ix, iy = x + dx, y + dy
        for _ in range(3):
            if not (0 <= ix < w and 0 <= iy < h) or px[ix, iy][3] == 0 or not is_edge(ix, iy):
                break
            ix, iy = ix + dx, iy + dy
        if 0 <= ix < w and 0 <= iy < h and px[ix, iy][3] > 0 and abs(lum(px[x, y]) - lum(px[ix, iy])) >= 6:
            dark += 1
    return dict(colors=len(colors), depth=round(depth, 1), outline=round(100 * dark / max(1, edge)), pixels=len(opaque))


BARS = dict(depth=3.0, outline=80, colors=64)


def temporal(frames):
    """Motion metrics over the frame sequence: frame count, mean change between consecutive
    frames, loop seam (last → first change) and the coverage envelope."""
    def changed(a, b):
        pa, pb = a.load(), b.load()
        diff = union = 0
        for y in range(a.height):
            for x in range(a.width):
                ca, cb = pa[x, y], pb[x, y]
                if ca[3] or cb[3]:
                    union += 1
                    diff += ca != cb
        return diff / max(1, union)

    steps = [changed(frames[i], frames[i + 1]) for i in range(len(frames) - 1)]
    cover = [sum(1 for p in f.getdata() if p[3]) for f in frames]
    return dict(frames=len(frames), motion=round(sum(steps) / max(1, len(steps)), 3),
                seam=round(changed(frames[-1], frames[0]), 3), peak=cover.index(max(cover)), end=round(cover[-1] / max(1, max(cover)), 2))


def run(s):
    """Replays a script, exporting every frame; returns (wall ms, frame images)."""
    from PIL import Image

    lines = list(s.lines)
    if "Shadow" in getattr(s, "layer_names", ()):
        lines.append(json.dumps({"op": "layer_visibility", "layer": "Shadow", "visible": False}))
    lines.append(json.dumps({"op": "export_png", "frame": s.frames // 2, "scale": 1, "_save": f"{s.name}_measure.png"}))
    if "Shadow" in getattr(s, "layer_names", ()):
        lines.append(json.dumps({"op": "layer_visibility", "layer": "Shadow", "visible": True}))
    for k in range(s.frames):
        lines.append(json.dumps({"op": "export_png", "frame": k, "scale": 1, "_save": f"frames/{s.name}_{k:02d}.png"}))
    lines.append(json.dumps({"op": "export_gif", "scale": 3, "_save": f"{s.name}.gif"}))
    script = os.path.join(OUT, f"{s.name}.jsonl")
    with open(script, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    t0 = time.perf_counter()
    r = subprocess.run([RUNNER, script, OUT], capture_output=True, text=True)
    ms = (time.perf_counter() - t0) * 1000
    if r.returncode != 0:
        print(r.stdout, r.stderr)
        raise SystemExit(f"{s.name} failed")
    frames = [Image.open(os.path.join(OUT, "frames", f"{s.name}_{k:02d}.png")).convert("RGBA") for k in range(s.frames)]
    return ms, frames, Image.open(os.path.join(OUT, f"{s.name}_measure.png")).convert("RGBA")


def motion_ok(t, looped, effect):
    """≥ 8 frames, something moves, stills stay coherent, loops close, one-shots dissipate."""
    if t["frames"] < 8 or t["motion"] < 0.005:
        return False
    if not effect and t["motion"] > 0.5:
        return False
    if looped:
        return t["seam"] <= max(2.5 * t["motion"], 0.1)
    return (not effect) or t["end"] < 0.95 or t["peak"] not in (0, t["frames"] - 1)


def main():
    from PIL import Image, ImageDraw

    if not os.path.exists(RUNNER):
        subprocess.run(["cargo", "build", "--release", "--example", "run_script"], cwd=os.path.join(ROOT, "Native"), check=True)
    os.makedirs(os.path.join(OUT, "frames"), exist_ok=True)
    rows, cells = [], []
    for title, kind, paths in SUITE:
        for path, make in paths:
            s = make()
            ms, frames, measure = run(s)
            m = metrics(measure)
            t = temporal(frames)
            looped = getattr(s, "looped", kind != "effect" or title.startswith("Magic"))
            if kind == "effect":
                # Effects use one hot→cold ramp across hues: judge brightness bands, not hue families.
                ok = path == "A" or (3 <= m["colors"] <= 24)
            else:
                ok = path == "A" or (m["depth"] >= BARS["depth"] and m["colors"] <= BARS["colors"] and m["outline"] >= BARS["outline"])
            if path == "G":
                ok = ok and s.calls == 1
            ok = path == "A" or (ok and motion_ok(t, looped, kind == "effect"))
            rows.append(dict(asset=title, kind=kind, path=path, calls=s.calls, kb=round(s.payload() / 1024, 1), ms=round(ms), ok=ok, **m, **t))
            cells.append((title, path, frames[len(frames) // 2]))
            print(f"{title:22} {path}  calls {s.calls:3}  {ms:6.0f} ms  colors {m['colors']:3}  depth {m['depth']:4}  outline {m['outline']:3}%"
                  f"  frames {t['frames']:2} motion {t['motion']:.3f} seam {t['seam']:.3f}  {'PASS' if ok else 'FAIL'}")

    vfx = []
    for title, make in VFX_SUITE:
        s = make()
        ms, frames, measure = run(s)
        m, t = metrics(measure), temporal(frames)
        # Particles of one burst share an age (one color per frame): count colors over the clip.
        m["colors"] = len({p[:3] for f in frames for p in f.getdata() if p[3]})
        ok = s.calls == 1 and 2 <= m["colors"] <= 24 and motion_ok(t, s.looped, True)
        vfx.append(dict(asset=title, calls=s.calls, ms=round(ms), ok=ok, loop=s.looped, colors=m["colors"], frames_img=frames, **t))
        print(f"VFX {title:16} frames {t['frames']:2}  colors {m['colors']:2}  motion {t['motion']:.3f}  seam {t['seam']:.3f}  "
              f"peak {t['peak']:2} end {t['end']:.2f}  {'loop' if s.looped else 'once'}  {'PASS' if ok else 'FAIL'}")

    # Comparison sheet: one row per asset, columns A / B / C / G at 4× (middle frame).
    titles = [t for t, _, _ in SUITE]
    cw, ch, sc = 64 * 4 + 16, 64 * 4 + 28, 4
    sheet = Image.new("RGBA", (180 + cw * 4, ch * len(titles) + 30), (34, 36, 52, 255))
    d = ImageDraw.Draw(sheet)
    for j, lab in enumerate(["A  raw primitives", "B  craft pipeline", "C  voxel -> pixels", "G  fx_generate (1 call)"]):
        d.text((180 + j * cw + 8, 8), lab, fill=(220, 220, 240, 255))
    for i, t in enumerate(titles):
        y0 = 30 + i * ch
        d.text((10, y0 + ch // 2 - 6), t, fill=(220, 220, 240, 255))
        for title, path, img in cells:
            if title != t:
                continue
            j = "ABCG".index(path)
            big = img.resize((img.width * sc, img.height * sc), Image.NEAREST)
            bg = Image.new("RGBA", (cw - 16, ch - 28), (196, 214, 236, 255))
            bg.alpha_composite(big, ((bg.width - big.width) // 2, (bg.height - big.height) // 2))
            sheet.alpha_composite(bg, (180 + j * cw + 8, y0 + 4))
            r = next(r for r in rows if r["asset"] == t and r["path"] == path)
            d.text((180 + j * cw + 8, y0 + ch - 22), f"{r['calls']} calls · {r['frames']}f · {r['colors']} col · outline {r['outline']}%", fill=(200, 200, 220, 255))
    sheet.save(os.path.join(OUT, "comparison.png"))

    # VFX pack film strips: 8 evenly spaced frames per effect at 2×.
    fw, rh = 64 * 2 + 4, 64 * 2 + 8
    strip = Image.new("RGBA", (150 + fw * 8, rh * len(vfx) + 8), (20, 20, 32, 255))
    d = ImageDraw.Draw(strip)
    for i, v in enumerate(vfx):
        y0 = 4 + i * rh
        d.text((8, y0 + 40), v["asset"], fill=(220, 220, 240, 255))
        d.text((8, y0 + 56), f"{v['frames']}f {'loop' if v['loop'] else 'once'}", fill=(150, 150, 180, 255))
        fr = v["frames_img"]
        for j in range(8):
            img = fr[round(j * (len(fr) - 1) / 7)]
            strip.alpha_composite(img.resize((128, 128), Image.NEAREST), (150 + j * fw, y0 + 2))
    strip.save(os.path.join(OUT, "vfx_pack.png"))

    with open(os.path.join(OUT, "report.md"), "w") as fh:
        fh.write("| Asset | Path | Calls | Frames | Payload KB | ms | Colors | Shade depth | Outline % | Motion | Loop seam | Result |\n"
                 "|---|---|---|---|---|---|---|---|---|---|---|---|\n")
        for r in rows:
            fh.write(f"| {r['asset']} | {r['path']} | {r['calls']} | {r['frames']} | {r['kb']} | {r['ms']} | {r['colors']} | {r['depth']} | "
                     f"{r['outline']} | {r['motion']} | {r['seam']} | {'✅' if r['ok'] else '❌'} |\n")
        fh.write("\n| VFX | Calls | Frames | Colors | Type | Motion | Loop seam | Peak frame | End/peak | Result |\n|---|---|---|---|---|---|---|---|---|---|\n")
        for v in vfx:
            fh.write(f"| {v['asset']} | {v['calls']} | {v['frames']} | {v['colors']} | {'loop' if v['loop'] else 'one-shot'} | {v['motion']} | "
                     f"{v['seam']} | {v['peak']} | {v['end']} | {'✅' if v['ok'] else '❌'} |\n")
    failed = [r for r in rows if not r["ok"]] + [v for v in vfx if not v["ok"]]
    print(f"\n{len(rows)} asset builds + {len(vfx)} VFX, {len(failed)} below the bars → {os.path.join(OUT, 'report.md')}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
