#!/usr/bin/env python3
"""Sir Valen, the Dawn Knight — a chunky 3D pixel (voxel) hero in the classic diorama style.

Builds two models with Pixel Studio's public tools:
  * "Knight"   — rigged and animated (walk / run / idle), for 8-direction sprites.
  * "Diorama"  — the knight on a floating grass island with rocks, flowers, a glowing crystal
                 and drifting sparkles, for the hero shot.

    python3 build_knight.py   ->  knight.jsonl (models only) and knight_showcase.jsonl (+ renders)
"""

import math
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from voxkit import Grid, mix, noise2, shade, write_jsonl  # noqa: E402

CX, CZ = 48, 48
STEEL_L, STEEL, STEEL_D = "#D3DEF2", "#9AAED6", "#61709F"
GOLD_L, GOLD, GOLD_D = "#FFE48A", "#F4BC33", "#B97A1B"
RED_L, RED, RED_D = "#F0505A", "#CF2638", "#7E1222"
DARK = "#1E2036"
UNDER = "#2D3358"
LEATHER = "#6E4024"
CYAN, CYAN_L = "#46E6FF", "#E2FFFF"
WING, WING_D = "#F6F8FF", "#BFC8E4"

cmds = []


def cmd(op, **a):
    cmds.append({"op": op, "actor": "knight-builder", **a})


# ------------------------------------------------------------------ materials
cmd("project_rename", name="Sir Valen - Dawn Knight")
cmd("material_set", name="steel", roughness=0.32, metallic=0.78)
cmd("material_set", name="gold", roughness=0.28, metallic=1.0)
cmd("material_set", name="cloth", roughness=0.88, metallic=0.0)
cmd("material_set", name="leather", roughness=0.7, metallic=0.0)
cmd("material_set", name="matte", roughness=0.9, metallic=0.0)
cmd("material_set", name="rune", roughness=0.15, metallic=0.2, emission=1.5)
cmd("material_set", name="gem", roughness=0.08, metallic=0.1, emission=1.5)
cmd("material_set", name="crystal", roughness=0.05, metallic=0.0, emission=1.4)
cmd("material_set", name="sparkle", roughness=0.2, metallic=0.0, emission=2.6)
cmd("material_set", name="grass", roughness=0.92, metallic=0.0)
cmd("material_set", name="dirt", roughness=0.95, metallic=0.0)
cmd("material_set", name="stone", roughness=0.8, metallic=0.0)

# ------------------------------------------------------------------ knight geometry
k = Grid(seed=7)


def steel_shade(x, y, z):
    return STEEL


# Legs (left; mirrored later). Left = +x.
LX = CX + 5
k.box(LX - 3, 0, CZ - 3, LX + 3, 0, CZ + 6, DARK, "steel", "foot_l")                 # sole
k.box(LX - 3, 1, CZ - 3, LX + 3, 3, CZ + 5, STEEL, "steel", "foot_l")                # sabaton
k.box(LX - 2, 1, CZ + 5, LX + 2, 2, CZ + 6, GOLD, "gold", "foot_l")                  # gold toe cap
k.box(LX - 3, 3, CZ + 3, LX + 3, 3, CZ + 5, STEEL_L, "steel", "foot_l")
k.capsule((LX + .5, 4, CZ + .5), (LX + .5, 13, CZ + .5), 3.2, 3.4, STEEL, "steel", "lower_leg_l")
k.box(LX, 5, CZ + 3, LX, 12, CZ + 4, GOLD, "gold", "lower_leg_l")                     # greave ridge
k.ellipsoid(LX + .5, 14.5, CZ + 2, 3.3, 2.8, 2.6, GOLD, "gold", "lower_leg_l")         # knee cop rim
k.ellipsoid(LX + .5, 14.5, CZ + 2.8, 2.4, 2.0, 2.2, STEEL_L, "steel", "lower_leg_l")   # knee cop
k.capsule((LX + .5, 16, CZ + .5), (LX + .5, 25, CZ + .5), 3.5, 3.8, UNDER, "cloth", "upper_leg_l")
k.ellipsoid(LX + .5, 20.5, CZ + 2.2, 3.6, 4.4, 2.2, STEEL, "steel", "upper_leg_l")     # cuisse
k.box(LX - 3, 16, CZ + 3, LX + 3, 16, CZ + 4, GOLD, "gold", "upper_leg_l")

# Hips, tassets, belt, tabard
k.ellipsoid(CX, 27, CZ + .5, 7.2, 3.6, 4.6, UNDER, "cloth", "hips")
k.ellipsoid(CX + 6.5, 24.5, CZ + .5, 3.2, 4.5, 4.8, STEEL, "steel", "hips")             # tasset
k.recolor(lambda x, y, z: x >= CX + 4 and y == 21 and (x, y, z) in k.v, GOLD, "gold")
k.ellipsoid(CX, 29.5, CZ + .5, 7.8, 1.3, 5.4, LEATHER, "leather", "hips")              # belt
k.box(CX - 1, 28, CZ + 5, CX, 30, CZ + 6, GOLD, "gold", "hips")
k.put(CX - 1, 29, CZ + 7, CYAN, "gem", "hips")
k.put(CX, 29, CZ + 7, CYAN, "gem", "hips")
for y in range(13, 29):
    for x in range(CX - 4, CX + 4):
        edge = x in (CX - 4, CX + 3) or y == 13
        c = GOLD if edge else (RED if (y + x) % 7 else RED_D if y < 20 else RED)
        k.put(x, y, CZ + 6, c, "gold" if edge else "cloth", "hips")
        k.put(x, y, CZ - 6, RED_D if not edge else GOLD_D, "cloth", "hips")
k.box(CX - 1, 18, CZ + 7, CX, 22, CZ + 7, GOLD, "gold", "hips")                          # tabard emblem
k.box(CX - 2, 20, CZ + 7, CX + 1, 20, CZ + 7, GOLD, "gold", "hips")

# Torso
k.ellipsoid(CX, 33, CZ + .5, 6.6, 3.5, 4.6, STEEL_D, "steel", "spine")
k.ellipsoid(CX, 39.5, CZ + .8, 8.6, 6.0, 5.6, STEEL, "steel", "chest")
k.ellipsoid(CX, 41.5, CZ + 3.2, 6.8, 3.8, 3.4, STEEL_L, "steel", "chest")               # pectoral plate
for (x, y, z), (c, m, b) in list(k.v.items()):
    if b == "chest" and z >= CZ + 4 and k.surface(x, y, z):
        if abs(abs(x + .5 - CX) - (y - 34) * 0.85) < 0.55 and 34 <= y <= 43:
            k.v[(x, y, z)] = (GOLD, "gold", b)                                           # V trim
        if y == 34:
            k.v[(x, y, z)] = (GOLD, "gold", b)
k.box(CX - 1, 37, CZ + 6, CX, 39, CZ + 6, CYAN, "gem", "chest")                           # chest gem
k.ellipsoid(CX, 45.2, CZ + .5, 5.3, 1.4, 4.4, GOLD, "gold", "chest")                     # collar

# Shoulders and arms
k.ellipsoid(CX + 10, 43, CZ + .5, 4.8, 3.6, 5.0, STEEL, "steel", "upper_arm_l")
k.recolor(lambda x, y, z: x >= CX + 7 and y in (40, 41) and k.v[(x, y, z)][2] == "upper_arm_l" and k.surface(x, y, z), GOLD, "gold")
k.ellipsoid(CX + 11, 46, CZ + .5, 1.6, 1.2, 1.6, GOLD, "gold", "upper_arm_l")
k.capsule((CX + 10.5, 41, CZ + .5), (CX + 11.5, 34, CZ + .5), 2.3, 2.2, UNDER, "cloth", "upper_arm_l")
k.ellipsoid(CX + 11.5, 33.5, CZ, 2.4, 2.2, 2.4, STEEL_L, "steel", "lower_arm_l")
k.capsule((CX + 11.5, 33, CZ + .8), (CX + 12, 27, CZ + 1.5), 2.5, 2.3, STEEL, "steel", "lower_arm_l")
k.recolor(lambda x, y, z: y == 27 and k.v[(x, y, z)][2] == "lower_arm_l", GOLD, "gold")
k.box(CX + 11, 23, CZ, CX + 13, 26, CZ + 3, STEEL_D, "steel", "hand_l")

# Neck & helmet
k.box(CX - 2, 45, CZ - 1, CX + 1, 47, CZ + 2, DARK, "matte", "neck")
k.ellipsoid(CX, 52, CZ + .8, 5.6, 6.2, 5.6, STEEL, "steel", "head")
k.ellipsoid(CX, 54.5, CZ + .2, 5.0, 4.6, 5.2, STEEL_L, "steel", "head")
for (x, y, z), (c, m, b) in list(k.v.items()):
    if b == "head" and k.surface(x, y, z):
        if y == 54:
            k.v[(x, y, z)] = (GOLD, "gold", b)                                           # crown band
        if z >= CZ + 4 and ((49 <= y <= 51 and CX - 4 <= x <= CX + 3) or (46 <= y <= 51 and CX - 1 <= x <= CX)):
            k.v[(x, y, z)] = (DARK, "matte", b)                                          # T visor
k.put(CX - 1, 55, CZ + 6, "#FF3B55", "gem", "head")
k.put(CX, 55, CZ + 6, "#FF3B55", "gem", "head")
k.capsule((CX - .5, 58.5, CZ + 4), (CX - .5, 58.5, CZ - 4), 1.0, 1.0, GOLD, "gold", "head")  # crest
# wings (left, mirrored): a solid feathered fan sweeping up and back
for y in range(51, 61):
    rise = y - 51
    for z in range(CZ - rise - 2, CZ + 2 - rise // 2):
        if rise > 6 and z > CZ - rise + 1:
            continue
        feather = (z + y) % 3 == 0
        k.put(CX + 6, y, z, WING_D if feather else WING, "matte", "head")
        if rise < 6:
            k.put(CX + 5, y, z, WING_D, "matte", "head")
# plume
for t in range(12):
    a = t / 11
    y = 58 + 6 * math.sin(a * math.pi * 0.85)
    z = CZ + 2 - 11 * a
    r = 2.2 - 1.2 * a
    k.ellipsoid(CX, y, z, r, r * 1.2, r, mix(RED_L, RED_D, a * 0.8), "cloth", "head")

# Cape
for y in range(8, 46):
    t = (45 - y) / 37
    half = int(7 + 5 * t)
    zb = CZ - 6 - int(2.5 * t)
    for x in range(CX - half, CX + half):
        fold = (x - CX + 40) % 5 == 0
        c = RED_D if fold else mix(RED, RED_D, t * 0.6)
        if y == 8:
            c = GOLD
        k.put(x, y, zb, c, "gold" if y == 8 else "cloth", "cape")
        k.put(x, y, zb + 1, shade(RED_D, 0.7), "cloth", "cape")

k.mirror_x(CX)

# Sword in the right hand (−x side): glowing rune blade pointing forward-down.
hx, hy, hz = CX - 13, 24.5, CZ + 1.5
d = (-0.62, -0.2, 0.76)
p = (0.0, 1.0, 0.0)


def at(s, w=0.0):
    return (hx + d[0] * s + p[0] * w, hy + d[1] * s + p[1] * w, hz + d[2] * s + p[2] * w)


for s in range(-5, 3):
    x, y, z = at(s)
    k.put(round(x), round(y), round(z), LEATHER if s > -5 else GOLD, "leather", "hand_r")
for w in range(-4, 5):
    x, y, z = at(3, w)
    k.put(round(x), round(y), round(z), GOLD if abs(w) < 4 else GOLD_L, "gold", "hand_r")
x, y, z = at(3)
k.put(round(x), round(y) + 1, round(z), CYAN, "gem", "hand_r")
for s10 in range(40, 260):
    s = s10 / 10
    for w in (-1, 0, 1):
        if s > 23 and w != 0:
            continue
        x, y, z = at(s, w * 0.9)
        core = w == 0
        k.put(round(x), round(y), round(z), CYAN_L if core else CYAN, "rune", "hand_r")

# Kite shield on the left forearm, angled to face forward-left.
sc = (CX + 15.5, 30, CZ + 6)
ang = math.radians(32)
right = (math.cos(ang), 0, -math.sin(ang))
normal = (math.sin(ang), 0, math.cos(ang))
for u2 in range(-14, 15):
    for v2 in range(-22, 19):
        u, v = u2 / 2, v2 / 2
        width = 6.2 if v >= -2 else 6.2 * (v + 11) / 9
        if abs(u) > width or v > 9:
            continue
        border = abs(u) > width - 1.2 or v > 7.8 or (v < -2 and abs(u) > width - 1.6)
        emblem = (u * u + (v - 1) ** 2) < 6.5 or (abs(u) < 0.6 and -6 < v < 7) or (abs(v - 1) < 0.6 and abs(u) < 4.5)
        for wv in (0, 1):
            x = sc[0] + right[0] * u + normal[0] * wv
            y = sc[1] + v
            z = sc[2] + right[2] * u + normal[2] * wv
            if wv == 1:
                col, mat = (GOLD, "gold") if border or emblem else (STEEL if (u2 + v2) % 9 else STEEL_L, "steel")
            else:
                col, mat = STEEL_D, "steel"
            k.put(round(x), round(y), round(z), col, mat, "lower_arm_l")

# Subtle hand-placed tonal variation on cloth and steel (pixel-art texture).
k.jitter(lambda x, y, z, c: c in (RED, STEEL, UNDER), 0.06, seed=11)

# ------------------------------------------------------------------ knight model + rig
cmd("voxel_model_create", name="Knight", width=96, height=96, depth=96, origin=[CX, 0, CZ])


def bone(name, parent, head, tail):
    a = {"model": "Knight", "name": name, "head": head, "tail": tail}
    if parent:
        a["parent"] = parent
    cmd("rig_bone_add", **a)


bone("hips", None, [CX, 26, CZ], [CX, 30, CZ])
bone("spine", "hips", [CX, 30, CZ], [CX, 37, CZ])
bone("chest", "spine", [CX, 37, CZ], [CX, 45, CZ])
bone("neck", "chest", [CX, 45, CZ], [CX, 47, CZ])
bone("head", "neck", [CX, 47, CZ], [CX, 60, CZ])
for side, s in (("l", 1), ("r", -1)):
    bone(f"upper_arm_{side}", "chest", [CX + s * 10.5, 42, CZ], [CX + s * 11.5, 34, CZ])
    bone(f"lower_arm_{side}", f"upper_arm_{side}", [CX + s * 11.5, 34, CZ], [CX + s * 12, 27, CZ + 1])
    bone(f"hand_{side}", f"lower_arm_{side}", [CX + s * 12, 27, CZ + 1], [CX + s * 12, 23, CZ + 2])
    bone(f"upper_leg_{side}", "hips", [CX + s * 5.5, 25, CZ], [CX + s * 5.5, 14.5, CZ])
    bone(f"lower_leg_{side}", f"upper_leg_{side}", [CX + s * 5.5, 14.5, CZ], [CX + s * 5.5, 2, CZ])
    bone(f"foot_{side}", f"lower_leg_{side}", [CX + s * 5.5, 2, CZ], [CX + s * 5.5, 1, CZ + 5])
bone("cape", "chest", [CX, 45, CZ - 6], [CX, 8, CZ - 8])

cmds.extend(k.commands("Knight", actor="knight-builder"))
cmd("anim3d_generate", model="Knight", name="walk", kind="walk", frames=8, frame_ms=100, stride=30, knee=45, arm_swing=26, bob=0.8, lean=2)
cmd("anim3d_generate", model="Knight", name="run", kind="run", frames=8, frame_ms=70)
cmd("anim3d_generate", model="Knight", name="idle", kind="idle", frames=8)

# ------------------------------------------------------------------ diorama island
rng = random.Random(42)
I = Grid(seed=5)
OX, OZ, TOP = 64, 64, 24
GRASS = ["#6DBE45", "#62B33E", "#78C94F", "#5AA639"]
DIRT = ["#7B4A2B", "#6B3E22", "#86532F", "#5E361E"]
for x in range(OX - 40, OX + 41):
    for z in range(OZ - 40, OZ + 41):
        dx, dz = x + .5 - OX, z + .5 - OZ
        ang = math.atan2(dz, dx)
        R = 35 + 2.2 * math.sin(ang * 5 + 1) + 1.4 * math.sin(ang * 9)
        r = math.hypot(dx, dz)
        if r > R:
            continue
        rr = r / R
        bottom = int(TOP - 6 - 14 * (1 - rr ** 2) ** 0.8 - 3 * noise2(x * .15, z * .15, 3))
        for y in range(max(0, bottom), TOP + 1):
            if y >= TOP - 1:
                col, mat = GRASS[(x * 7 + z * 13 + (x * z) % 5) % 4 if rng.random() < 0.35 else 0], "grass"
            else:
                band = (TOP - y + int(2 * noise2(x * .3, z * .3, 9))) // 3
                col, mat = DIRT[band % 4], "dirt"
                if rng.random() < 0.035:
                    col, mat = "#7D7A80", "stone"
            I.put(x, y, z, col, mat)
        # grass drips over the rim
        if rr > 0.9:
            for y in range(TOP - 2 - rng.randint(0, 3), TOP - 1):
                I.put(x, y, z, GRASS[3], "grass")
# dirt path where the knight stands
for x in range(OX - 12, OX + 13):
    for z in range(OZ - 8, OZ + 14):
        if ((x - OX) / 12) ** 2 + ((z - OZ - 3) / 10) ** 2 + 0.25 * noise2(x * .4, z * .4, 2) < 1 and (x, TOP, z) in I.v:
            I.put(x, TOP, z, ["#A57E50", "#97714A", "#B08A5A"][(x + 2 * z) % 3], "dirt")
# rocks
for (rx, rz, s) in ((38, 50, 4), (90, 80, 5), (46, 92, 3), (84, 40, 4), (70, 96, 2)):
    for i in range(s + 2):
        bx, bz = rx + rng.randint(-s, s), rz + rng.randint(-s, s)
        h = rng.randint(1, s)
        for y in range(TOP + 1, TOP + 1 + h):
            for x in range(bx - 1, bx + 2):
                for z in range(bz - 1, bz + 2):
                    I.put(x, y, z, "#B6B5BC" if y == TOP + h else "#8D8B93", "stone")
# flowers and grass tufts
FLOW = ["#FF6FAE", "#B26BFF", "#FF4B4B", "#FFD84A", "#FFFFFF"]
for i in range(26):
    a, r = rng.uniform(0, 2 * math.pi), rng.uniform(14, 31)
    x, z = int(OX + r * math.cos(a)), int(OZ + r * math.sin(a))
    if (x, TOP, z) not in I.v:
        continue
    h = rng.randint(1, 3)
    for y in range(TOP + 1, TOP + 1 + h):
        I.put(x, y, z, "#3E8A2C", "grass")
    if i % 2 == 0:
        c = FLOW[i % len(FLOW)]
        y = TOP + 1 + h
        I.put(x, y, z, "#FFE14D", "matte")
        for ddx, ddz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            I.put(x + ddx, y, z + ddz, c, "matte")
# glowing crystal cluster
for (cx, cz, h, lean) in ((92, 62, 9, 0.25), (95, 66, 6, -0.3), (89, 67, 5, 0.4)):
    for y in range(h):
        w = 1.4 - y / h
        x0 = cx + lean * y
        I.ellipsoid(x0, TOP + 1 + y, cz, max(0.6, w + 0.4), 0.6, max(0.6, w + 0.4), mix("#9FF7FF", "#FFFFFF", y / h), "crystal")

D = Grid()
D.merge(I)
D.merge(k, OX - CX, TOP + 1, OZ - CZ, bone_override=None)
# floating sparkles (gold and cyan)
for i in range(34):
    a, r = rng.uniform(0, 2 * math.pi), rng.uniform(12, 40)
    x, z = int(OX + r * math.cos(a)), int(OZ + r * math.sin(a))
    y = rng.randint(TOP + 18, TOP + 80)
    D.put(x, y, z, "#FFE7A0" if i % 3 else "#7FF6FF", "sparkle")
# sparkles trailing the blade
for s10 in range(80, 260, 18):
    x, y, z = at(s10 / 10, rng.uniform(-3, 3))
    D.put(round(x) + OX - CX + rng.randint(-2, 2), round(y) + TOP + 1 + rng.randint(0, 4), round(z) + OZ - CZ, "#BFFBFF", "sparkle")

cmd("voxel_model_create", name="Diorama", width=128, height=128, depth=128, origin=[OX, 0, OZ])
for c in D.commands("Diorama", actor="knight-builder"):
    c.pop("bone", None)
    cmds.append(c)

here = os.path.dirname(os.path.abspath(__file__))
write_jsonl(os.path.join(here, "knight.jsonl"), cmds)

# ------------------------------------------------------------------ showcase
show = list(cmds)
tip = at(25)
hero_light = [
    {"position": [tip[0] + OX - CX, tip[1] + TOP + 1, tip[2] + OZ - CZ], "color": "#5FEFFF", "intensity": 7, "radius": 34},
    {"position": [92, TOP + 6, 64], "color": "#B8FBFF", "intensity": 5, "radius": 26},
]
shot = {"model": "Diorama", "pitch": 26, "smoothness": 0.0, "shadows": 1, "bloom": 0.55, "bloom_radius": 9, "supersample": 2,
        "background_gradient": ["#463C33", "#0E0D13"], "lights": hero_light, "exposure": 1.08}
sprite = {"model": "Knight", "pitch": 28, "smoothness": 0.15, "shadows": 1, "ground_shadow": 0.38, "bloom": 0.55,
          "supersample": 2, "outline": True, "inner_lines": False, "exposure": 1.05, "bloom_radius": 4}


def out(op, save, **a):
    show.append({"op": op, "_save": save, **a})


out("voxel_render", "knight_diorama_1024.png", width=1024, yaw=32, **shot)
out("voxel_render", "knight_diorama_back_1024.png", width=1024, yaw=210, **shot)
out("voxel_render", "knight_512_front.png", width=512, yaw=25, **{**sprite, "pitch": 18, "ground_shadow": 0.45})
out("voxel_render", "knight_512_closeup.png", width=512, yaw=30, scale=14, focus=[CX, 44, CZ],
    **{k2: v for k2, v in sprite.items() if k2 not in ("outline",)})
for pas in ("albedo", "normal", "occlusion", "emission", "depth"):
    out("voxel_render", f"passes/knight_{pas}.png", width=512, yaw=25, **{**sprite, "pass": pas})
out("voxel_render_sheet", "sprites/knight_walk_8dir_256.png", animation="walk", directions=8, width=256, _json=True, **sprite)
out("voxel_render_sheet", "sprites/knight_walk_8dir_256_normal.png", animation="walk", directions=8, width=256,
    **{**sprite, "pass": "normal", "outline": False, "ground_shadow": 0})
out("voxel_render_sheet", "sprites/knight_idle_8dir_256.png", animation="idle", directions=8, width=256, **sprite)
out("voxel_render_sheet", "sprites/knight_run_8dir_256.png", animation="run", directions=8, width=256, **sprite)
show.append({"op": "voxel_render_to_frames", "animation": "walk", "directions": 8, "width": 256, "height": 256, **sprite})
for dname in ("S", "SE", "E", "NE", "N", "NW", "W", "SW"):
    out("export_gif", f"gifs/knight_walk_{dname}.gif", tag=f"walk_{dname}", scale=2)
out("project_save", "SirValen.pxs")
write_jsonl(os.path.join(here, "knight_showcase.jsonl"), show)
print(f"knight voxels: {len(k.v)}, diorama voxels: {len(D.v)}, commands: {len(cmds)}")
