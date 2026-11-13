#!/usr/bin/env python3
"""Pixel Studio 3D voxel assets: the voxkit helpers, the hero, the knight diorama (Assets/Knight.jsonl,
bundled by the apps) and the voxel VFX sequences. Each builder writes engine command scripts
(*.jsonl + *_showcase.jsonl with renders) to Assets/Build/; replay them with Native/Script.rs."""

import json
import math
import os
import random
import sys

# ════ voxkit: voxel authoring helpers ═══════════════════════════════════════════════════════════

def hexc(c):
    c = c.lstrip("#")
    return tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))


def tohex(rgb):
    return "#%02X%02X%02X" % tuple(max(0, min(255, int(round(v)))) for v in rgb)


def shade(c, k):
    """Multiply brightness (k < 1 darker) with a slight hue shift: shadows cooler, lights warmer."""
    r, g, b = hexc(c)
    if k < 1:
        return tohex((r * k * 0.97, g * k * 0.98, b * k + (1 - k) * 18))
    return tohex((r * k + (k - 1) * 20, g * k + (k - 1) * 12, b * k))


def mix(a, b, t):
    a, b = hexc(a), hexc(b)
    return tohex(tuple(a[i] + (b[i] - a[i]) * t for i in range(3)))


class Grid:
    def __init__(self, seed=1):
        self.v = {}  # (x, y, z) -> (color, material, bone)
        self.rng = random.Random(seed)

    # ---- primitives (inclusive integer bounds) ------------------------------------------------
    def put(self, x, y, z, color, mat="default", bone=None):
        self.v[(int(x), int(y), int(z))] = (color, mat, bone)

    def erase(self, x, y, z):
        self.v.pop((int(x), int(y), int(z)), None)

    def color_of(self, color, x, y, z):
        return color(x, y, z) if callable(color) else color

    def box(self, x0, y0, z0, x1, y1, z1, color, mat="default", bone=None):
        for x in range(min(x0, x1), max(x0, x1) + 1):
            for y in range(min(y0, y1), max(y0, y1) + 1):
                for z in range(min(z0, z1), max(z0, z1) + 1):
                    self.put(x, y, z, self.color_of(color, x, y, z), mat, bone)

    def ellipsoid(self, cx, cy, cz, rx, ry, rz, color, mat="default", bone=None, cut=None):
        for x in range(int(cx - rx - 1), int(cx + rx + 2)):
            for y in range(int(cy - ry - 1), int(cy + ry + 2)):
                for z in range(int(cz - rz - 1), int(cz + rz + 2)):
                    d = ((x + .5 - cx) / rx) ** 2 + ((y + .5 - cy) / ry) ** 2 + ((z + .5 - cz) / rz) ** 2
                    if d <= 1 and (cut is None or cut(x, y, z)):
                        self.put(x, y, z, self.color_of(color, x, y, z), mat, bone)

    def capsule(self, a, b, ra, rb, color, mat="default", bone=None):
        ax, ay, az = a
        bx, by, bz = b
        r = max(ra, rb)
        abx, aby, abz = bx - ax, by - ay, bz - az
        l2 = abx * abx + aby * aby + abz * abz or 1e-6
        for x in range(int(min(ax, bx) - r - 1), int(max(ax, bx) + r + 2)):
            for y in range(int(min(ay, by) - r - 1), int(max(ay, by) + r + 2)):
                for z in range(int(min(az, bz) - r - 1), int(max(az, bz) + r + 2)):
                    px, py, pz = x + .5, y + .5, z + .5
                    t = max(0, min(1, ((px - ax) * abx + (py - ay) * aby + (pz - az) * abz) / l2))
                    rr = ra + (rb - ra) * t
                    dx, dy, dz = px - (ax + abx * t), py - (ay + aby * t), pz - (az + abz * t)
                    if dx * dx + dy * dy + dz * dz <= rr * rr:
                        self.put(x, y, z, self.color_of(color, x, y, z), mat, bone)

    def mirror_x(self, cx, pred=None):
        """Mirror voxels with x >= cx onto the other side around plane x = cx - 0.5 (bones _l → _r)."""
        add = []
        for (x, y, z), (c, m, b) in self.v.items():
            if x >= cx and (pred is None or pred(x, y, z)):
                nb = b[:-2] + "_r" if isinstance(b, str) and b.endswith("_l") else b
                add.append(((2 * cx - 1 - x, y, z), (c, m, nb)))
        for k, val in add:
            self.v[k] = val

    def surface(self, x, y, z):
        return any((x + dx, y + dy, z + dz) not in self.v for dx, dy, dz in ((1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)))

    def recolor(self, pred, color, mat=None):
        for k, (c, m, b) in list(self.v.items()):
            if pred(*k):
                self.v[k] = (self.color_of(color, *k) if color else c, mat or m, b)

    def jitter(self, pred, amount, seed=3):
        """Per-voxel tonal variation (hand-placed pixel art feel)."""
        rng = random.Random(seed)
        for k, (c, m, b) in sorted(self.v.items()):
            if pred(*k, c):
                f = 1 + (rng.random() - 0.5) * 2 * amount
                self.v[k] = (shade(c, f), m, b)

    def merge(self, other, dx=0, dy=0, dz=0, bone_override=None):
        for (x, y, z), (c, m, b) in other.v.items():
            self.v[(x + dx, y + dy, z + dz)] = (c, m, bone_override if bone_override is not None else b)

    # ---- output ---------------------------------------------------------------------------
    def commands(self, model, actor="asset-builder", batch=60000):
        groups = {}
        for (x, y, z), (c, m, b) in sorted(self.v.items()):
            groups.setdefault((m, b), []).append([x, y, z, c])
        out = []
        for (m, b), vox in sorted(groups.items(), key=lambda kv: (kv[0][0], str(kv[0][1]))):
            for i in range(0, len(vox), batch):
                args = {"op": "voxel_set_many", "actor": actor, "model": model, "voxels": vox[i:i + batch], "material": m}
                if b is not None:
                    args["bone"] = b
                out.append(args)
        return out


def write_jsonl(path, cmds):
    with open(path, "w") as f:
        for c in cmds:
            f.write(json.dumps(c, separators=(",", ":")) + "\n")


def noise2(x, y, seed=0):
    """Smooth periodic-friendly value noise (0..1)."""
    def h(i, j):
        n = (i * 374761393 + j * 668265263 + seed * 2246822519) & 0xFFFFFFFF
        n = (n ^ (n >> 13)) * 1274126177 & 0xFFFFFFFF
        return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0
    xi, yi = math.floor(x), math.floor(y)
    fx, fy = x - xi, y - yi
    sx, sy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a = h(xi, yi) * (1 - sx) + h(xi + 1, yi) * sx
    b = h(xi, yi + 1) * (1 - sx) + h(xi + 1, yi + 1) * sx
    return a * (1 - sy) + b * sy


# ════ build_hero ════════════════════════════════════════════════════════════════════════════════

def build_hero(out):
    """Builds the Pixel Studio 3D pixel hero ("Aurelion, the Dawn Knight") as engine commands.

    Everything is made with the same tools an AI agent uses over MCP: voxel_shape, materials,
    procedural PBR textures, rig_bone_add, anim3d_generate, voxel_render... No external assets.

        python3 build_hero.py            # writes hero.jsonl and hero_showcase.jsonl next to this file
        cargo run --release --example run_script -- ../../assets/hero/hero_showcase.jsonl out/

    Coordinate system: 512^3 voxel grid, y up, the hero faces +z, ground at y = 0,
    origin (256, 0, 256) between the feet. The character's left side is +x. Height ~300 voxels.
    """
    CX, CZ = 256, 256
    cmds = []


    def cmd(op, **args):
        cmds.append({"op": op, "actor": "hero-builder", **args})


    def X(dx):
        """Model x for an offset from the center line (positive = character's left)."""
        return CX + dx


    def shape(kind, color, bone, material=None, mirror=False, **geo):
        args = {"shape": kind, "color": color, "bone": bone}
        if material:
            args["material"] = material
        if mirror:
            args["mirror_x"] = True
        args.update(geo)
        cmd("voxel_shape", **args)


    def paint(kind, color, material=None, mirror=False, **geo):
        args = {"shape": kind, "color": color, "mode": "paint"}
        if material:
            args["material"] = material
        if mirror:
            args["mirror_x"] = True
        args.update(geo)
        cmd("voxel_shape", **args)


    # ---------------------------------------------------------------- project & materials
    cmd("project_rename", name="Aurelion - Dawn Knight")
    cmd("voxel_model_create", name="Aurelion", width=512, height=512, depth=512, origin=[CX, 0, CZ])

    cmd("texture_generate", name="steel", kind="brushed_metal", size=64, cells=8, seed=11, roughness=0.3, contrast=0.1, metallic=0.95, uv_scale=1)
    cmd("material_set", name="steel", normal_strength=0.35)
    cmd("texture_generate", name="gold", kind="hammered_metal", size=64, cells=10, seed=5, roughness=0.26, contrast=0.1, metallic=1.0, uv_scale=1)
    cmd("material_set", name="gold", normal_strength=0.4)
    cmd("texture_generate", name="cloth", kind="fabric", size=32, cells=8, seed=3, roughness=0.85, contrast=0.16, uv_scale=1)
    cmd("material_set", name="cloth", normal_strength=0.6)
    cmd("texture_generate", name="leather", kind="leather", size=64, cells=6, seed=9, roughness=0.6, contrast=0.15, uv_scale=1)
    cmd("material_set", name="leather", normal_strength=0.45, uv_scale=0.4)
    cmd("texture_generate", name="mail", kind="chainmail", size=32, cells=8, seed=2, roughness=0.42, contrast=0.55, metallic=0.85, uv_scale=1)
    cmd("material_set", name="skin", roughness=0.55, metallic=0.0)
    cmd("material_set", name="hair", roughness=0.45, metallic=0.0)
    cmd("material_set", name="eye", roughness=0.15, metallic=0.0)
    cmd("material_set", name="rune", roughness=0.2, metallic=0.3, emission=2.4)
    cmd("material_set", name="gem", roughness=0.08, metallic=0.2, emission=1.6)
    cmd("material_set", name="blade", roughness=0.16, metallic=1.0)

    # ---------------------------------------------------------------- skeleton
    def bone(name, parent, head, tail):
        a = {"name": name, "head": head, "tail": tail}
        if parent:
            a["parent"] = parent
        cmd("rig_bone_add", **a)


    bone("hips", None, [CX, 150, CZ], [CX, 166, CZ])
    bone("spine", "hips", [CX, 166, CZ], [CX, 200, CZ])
    bone("chest", "spine", [CX, 200, CZ], [CX, 246, CZ])
    bone("neck", "chest", [CX, 246, CZ + 1], [CX, 260, CZ + 2])
    bone("head", "neck", [CX, 260, CZ + 2], [CX, 302, CZ + 2])
    for side, s in (("l", 1), ("r", -1)):
        bone(f"upper_arm_{side}", "chest", [X(s * 51), 238, CZ], [X(s * 57), 180, CZ + 1])
        bone(f"lower_arm_{side}", f"upper_arm_{side}", [X(s * 57), 180, CZ + 1], [X(s * 61), 128, CZ + 6])
        bone(f"hand_{side}", f"lower_arm_{side}", [X(s * 61), 128, CZ + 6], [X(s * 62), 104, CZ + 9])
        bone(f"upper_leg_{side}", "hips", [X(s * 22), 148, CZ], [X(s * 24), 84, CZ + 2])
        bone(f"lower_leg_{side}", f"upper_leg_{side}", [X(s * 24), 84, CZ + 2], [X(s * 23), 16, CZ])
        bone(f"foot_{side}", f"lower_leg_{side}", [X(s * 23), 16, CZ], [X(s * 23), 5, CZ + 22])
    bone("cape", "chest", [CX, 244, CZ - 30], [CX, 80, CZ - 34])

    # ---------------------------------------------------------------- palette
    SKIN, SKIN_D = "#EDBB92", "#C98A62"
    HAIR, HAIR_L = "#5A2E17", "#B2703A"
    STEEL, STEEL_D = "#D5DEE8", "#8592A3"
    GOLD, GOLD_D = "#F2C54B", "#B07A1E"
    BLUE, BLUE_D = "#3557C2", "#1E2F78"
    RED, RED_D = "#C7303A", "#6E1220"
    LEATHER, LEATHER_D = "#7A4B2A", "#4A2C18"
    PANTS = "#2E2630"
    MAIL = "#A4ADB8"
    RUNE = "#7FF3FF"

    # ---------------------------------------------------------------- legs (left, mirrored to right)
    for m in (True,):
        # thigh (dark trousers) + steel cuisse
        shape("capsule", PANTS, "upper_leg_l", "cloth", m, a=[X(22), 150, CZ], b=[X(24), 86, CZ + 2], radius=12.5, radius_b=10.5, jitter=0.15)
        shape("ellipsoid", STEEL, "upper_leg_l", "steel", m, center=[X(23), 118, CZ + 6], radii=[12.5, 23, 8], color_to=STEEL_D, gradient_axis="y")
        paint("box", GOLD, "gold", m, min=[X(9), 94, CZ + 8], max=[X(38), 96, CZ + 18])
        # shin + greave
        shape("capsule", PANTS, "lower_leg_l", "cloth", m, a=[X(24), 84, CZ + 2], b=[X(23), 26, CZ], radius=11, radius_b=9.5)
        shape("capsule", STEEL, "lower_leg_l", "steel", m, a=[X(24), 76, CZ + 4], b=[X(23), 38, CZ + 3], radius=11.5, radius_b=10.5, color_to=STEEL_D, gradient_axis="y")
        paint("box", GOLD, "gold", m, min=[X(10), 74, CZ + 10], max=[X(38), 76, CZ + 18])
        # knee cop with gold rim
        shape("ellipsoid", STEEL, "lower_leg_l", "steel", m, center=[X(24), 86, CZ + 11], radii=[12, 11, 8])
        paint("ellipsoid", GOLD, "gold", m, center=[X(24), 86, CZ + 19], radii=[13, 12, 3])
        shape("ellipsoid", STEEL, "lower_leg_l", "steel", m, center=[X(24), 86, CZ + 16], radii=[6, 6, 4])
        # boot: shaft, cuff, foot, sole, toe cap
        shape("capsule", LEATHER, "lower_leg_l", "leather", m, a=[X(23), 40, CZ + 1], b=[X(23), 10, CZ + 1], radius=12, radius_b=11, color_to=LEATHER_D, gradient_axis="y")
        shape("ellipsoid", "#9A6238", "lower_leg_l", "leather", m, center=[X(23), 40, CZ + 1], radii=[14, 5, 14])
        shape("ellipsoid", LEATHER, "foot_l", "leather", m, center=[X(23), 9, CZ + 10], radii=[11, 9, 19], color_to=LEATHER_D, gradient_axis="y")
        shape("box", "#24160D", "foot_l", "leather", m, min=[X(12), 0, CZ - 9], max=[X(34), 3, CZ + 28])
        shape("ellipsoid", STEEL, "foot_l", "steel", m, center=[X(23), 10, CZ + 22], radii=[9, 7, 8])

    # ---------------------------------------------------------------- hips, belt, faulds, tabard
    shape("ellipsoid", PANTS, "hips", "cloth", center=[CX, 152, CZ], radii=[33, 18, 23])
    shape("capsule", STEEL, "hips", "steel", a=[CX, 160, CZ], b=[CX, 142, CZ], radius=27, radius_b=31, color_to=STEEL_D, gradient_axis="y")
    paint("ellipsoid", STEEL_D, "steel", center=[CX, 150, CZ], radii=[50, 0.8, 50])
    paint("box", GOLD, "gold", min=[X(-45), 141, CZ - 34], max=[X(45), 142, CZ + 34])
    shape("ellipsoid", LEATHER_D, "hips", "leather", center=[CX, 164, CZ], radii=[33, 4.5, 27])
    shape("box", GOLD, "hips", "gold", min=[X(-6), 160, CZ + 26], max=[X(6), 170, CZ + 30])
    shape("box", RUNE, "hips", "gem", min=[X(-2), 163, CZ + 30], max=[X(2), 167, CZ + 31])
    # tabard front panel with gold trim and sun emblem
    shape("box", BLUE, "hips", "cloth", min=[X(-17), 108, CZ + 22], max=[X(17), 160, CZ + 27], color_to=BLUE_D, gradient_axis="y", jitter=0.25)
    paint("box", GOLD, "gold", min=[X(-18), 108, CZ + 22], max=[X(-15), 160, CZ + 28])
    paint("box", GOLD, "gold", min=[X(15), 108, CZ + 22], max=[X(18), 160, CZ + 28])
    paint("box", GOLD, "gold", min=[X(-18), 108, CZ + 22], max=[X(18), 111, CZ + 28])
    shape("ellipsoid", GOLD, "hips", "gold", center=[CX, 133, CZ + 28], radii=[7, 7, 2])
    for dx, dy in ((0, 12), (0, -12), (12, 0), (-12, 0), (8, 8), (-8, 8), (8, -8), (-8, -8)):
        shape("capsule", GOLD, "hips", "gold", a=[CX, 133, CZ + 28], b=[CX + dx, 133 + dy, CZ + 28], radius=1.6, radius_b=0.8)
    # back panel
    shape("box", BLUE_D, "hips", "cloth", min=[X(-16), 112, CZ - 27], max=[X(16), 158, CZ - 22])

    # ---------------------------------------------------------------- torso
    shape("ellipsoid", MAIL, "spine", "mail", center=[CX, 182, CZ], radii=[28, 22, 22])
    shape("ellipsoid", STEEL, "chest", "steel", center=[CX, 216, CZ + 1], radii=[37, 30, 25], color_to=STEEL_D, gradient_axis="y")
    shape("ellipsoid", STEEL, "chest", "steel", center=[X(14), 222, CZ + 11], radii=[16, 14, 15], color_to=STEEL_D, gradient_axis="y", mirror_x=True)
    shape("capsule", STEEL, "spine", "steel", a=[CX, 196, CZ + 1], b=[CX, 184, CZ + 1], radius=27, radius_b=28, color_to=STEEL_D, gradient_axis="y")
    paint("box", GOLD, "gold", min=[X(-45), 184, CZ - 40], max=[X(45), 185, CZ + 40])
    # breastplate ridge, gold neckline, sun gem
    shape("capsule", GOLD, "chest", "gold", a=[CX, 192, CZ + 26], b=[CX, 236, CZ + 25], radius=1.6)
    paint("ellipsoid", GOLD, "gold", center=[CX, 244, CZ + 4], radii=[26, 8, 30])
    shape("ellipsoid", "#FF4466", "chest", "gem", center=[CX, 226, CZ + 28], radii=[4, 5, 3])
    shape("ellipsoid", GOLD, "chest", "gold", center=[CX, 226, CZ + 26], radii=[7, 8, 2.5])
    # gorget
    shape("ellipsoid", STEEL, "chest", "steel", center=[CX, 247, CZ + 1], radii=[21, 7, 19], color_to=STEEL_D, gradient_axis="y")
    # cape clasps
    shape("ellipsoid", GOLD, "chest", "gold", center=[X(28), 243, CZ + 18], radii=[5, 5, 4], mirror_x=True)
    shape("ellipsoid", "#FF4466", "chest", "gem", center=[X(28), 243, CZ + 22], radii=[2, 2, 1.5], mirror_x=True)

    # ---------------------------------------------------------------- cape (flares toward the hem)
    for i, y in enumerate(range(244, 76, -8)):
        t = i / 20.0
        half = 34 + 18 * t
        z0 = CZ - 31 - 3 * t
        shade = RED if t < 0.5 else RED
        shape("box", shade, "cape", "cloth", min=[X(-half), y - 8, z0 - 5], max=[X(half), y, z0], color_to=RED_D, gradient_axis="y", jitter=0.18, seed=40 + i)
    paint("box", GOLD, "gold", min=[X(-60), 78, CZ - 45], max=[X(60), 81, CZ - 25])
    # cape folds: darker vertical grooves
    for fx in (-34, -17, 0, 17, 34):
        paint("box", RED_D, "cloth", min=[X(fx - 1), 78, CZ - 46], max=[X(fx + 1), 200, CZ - 41])

    # ---------------------------------------------------------------- arms (left, mirrored to right)
    m = True
    shape("capsule", MAIL, "upper_arm_l", "mail", m, a=[X(50), 236, CZ], b=[X(57), 182, CZ + 1], radius=9.5, radius_b=8.5)
    # pauldron: three lames with gold rims
    shape("ellipsoid", STEEL, "upper_arm_l", "steel", m, center=[X(53), 242, CZ], radii=[19, 13, 19], color_to=STEEL_D, gradient_axis="y")
    paint("box", GOLD, "gold", m, min=[X(30), 230, CZ - 22], max=[X(76), 232, CZ + 22])
    shape("ellipsoid", STEEL, "upper_arm_l", "steel", m, center=[X(57), 228, CZ], radii=[16, 8, 17], color_to=STEEL_D, gradient_axis="y")
    paint("box", GOLD, "gold", m, min=[X(36), 221, CZ - 22], max=[X(78), 222, CZ + 22])
    shape("ellipsoid", GOLD, "upper_arm_l", "gold", m, center=[X(56), 253, CZ], radii=[5, 3, 5])
    # elbow, vambrace, bracer, gauntlet
    shape("ellipsoid", STEEL, "lower_arm_l", "steel", m, center=[X(57), 180, CZ - 2], radii=[9, 9, 9])
    shape("capsule", LEATHER, "lower_arm_l", "leather", m, a=[X(57), 178, CZ + 1], b=[X(61), 130, CZ + 6], radius=8.5, radius_b=7.5)
    shape("ellipsoid", STEEL, "lower_arm_l", "steel", m, center=[X(59), 154, CZ + 5], radii=[9, 20, 8], color_to=STEEL_D, gradient_axis="y")
    paint("box", GOLD, "gold", m, min=[X(46), 134, CZ - 10], max=[X(74), 136, CZ + 20])
    shape("ellipsoid", STEEL, "hand_l", "steel", m, center=[X(61), 117, CZ + 8], radii=[9, 12, 9], color_to=STEEL_D, gradient_axis="y")
    shape("ellipsoid", LEATHER_D, "hand_l", "leather", m, center=[X(60), 110, CZ + 13], radii=[7, 6, 6])

    # ---------------------------------------------------------------- sword (right hand, rune-lit blade)
    hx, hy, hz = X(-61), 116, CZ + 10
    d = (0.0, 0.86, 0.51)


    def along(t, dx=0.0):
        return [hx + dx, hy + d[1] * t, hz + d[2] * t]


    shape("ellipsoid", GOLD, "hand_r", "gold", center=along(-19), radii=[5, 5, 5])
    shape("capsule", LEATHER_D, "hand_r", "leather", a=along(-17), b=along(12), radius=3.2)
    shape("capsule", GOLD, "hand_r", "gold", a=along(14, -16), b=along(14, 16), radius=3.2)
    shape("ellipsoid", "#55E0FF", "hand_r", "gem", center=along(14), radii=[3, 3, 3])
    for dx, length, r in ((-3.4, 112, 2.3), (0.0, 124, 2.5), (3.4, 112, 2.3)):
        shape("capsule", "#E9F1FA", "hand_r", "blade", a=along(17, dx), b=along(length, dx * 0.3), radius=r, radius_b=0.8, color_to="#AFC0D2", gradient_axis="y")
    paint("capsule", RUNE, "rune", a=along(30), b=along(100), radius=1.2, radius_b=0.8)

    # ---------------------------------------------------------------- head
    shape("capsule", SKIN, "neck", "skin", a=[CX, 244, CZ + 2], b=[CX, 262, CZ + 3], radius=9.5)
    shape("ellipsoid", SKIN, "head", "skin", center=[CX, 278, CZ + 3], radii=[17, 21, 18], color_to=SKIN_D, gradient_axis="y")
    shape("ellipsoid", SKIN, "head", "skin", center=[CX, 268, CZ + 6], radii=[14, 10, 14])
    shape("ellipsoid", SKIN, "head", "skin", center=[X(17), 276, CZ + 2], radii=[3, 6, 4], mirror_x=True)
    shape("ellipsoid", SKIN_D, "head", "skin", center=[CX, 274, CZ + 21], radii=[3, 5, 3])
    # eyes: white, iris, pupil, highlight
    shape("ellipsoid", "#F4F1EA", "head", "eye", center=[X(7), 280, CZ + 18], radii=[4.2, 2.6, 2.2], mirror_x=True)
    shape("ellipsoid", "#2E7BE0", "head", "eye", center=[X(7), 280, CZ + 20], radii=[2.2, 2.4, 1.2], mirror_x=True)
    shape("ellipsoid", "#0E1530", "head", "eye", center=[X(7), 280, CZ + 21], radii=[1, 1.2, 0.8], mirror_x=True)
    cmd("voxel_set", x=X(6), y=281, z=CZ + 21, color="#FFFFFF", bone="head", material="eye")
    cmd("voxel_set", x=X(-8), y=281, z=CZ + 21, color="#FFFFFF", bone="head", material="eye")
    shape("capsule", HAIR, "head", "hair", a=[X(3), 285, CZ + 19], b=[X(12), 286, CZ + 16], radius=1.4, mirror_x=True)
    shape("capsule", "#A55548", "head", "skin", a=[X(-4), 266, CZ + 19], b=[X(4), 266, CZ + 19], radius=1.0)
    shape("ellipsoid", "#F2C9A6", "head", "skin", center=[X(10), 271, CZ + 16], radii=[3, 2, 2], mirror_x=True)
    # hair: cap, bangs, side locks, back
    shape("ellipsoid", HAIR, "head", "hair", center=[CX, 289, CZ - 1], radii=[19.5, 15, 20], color_to=HAIR_L, gradient_axis="y", jitter=0.45, seed=7)
    for i, (dx, dz, ln) in enumerate(((-12, 15, 14), (-5, 18, 12), (3, 18, 13), (10, 15, 15))):
        shape("capsule", HAIR, "head", "hair", a=[X(dx), 300, CZ + dz - 6], b=[X(dx - 3), 300 - ln, CZ + dz + 1], radius=4, radius_b=1.5, color_to=HAIR_L, gradient_axis="y", jitter=0.4, seed=20 + i)
    shape("capsule", HAIR, "head", "hair", a=[X(17), 292, CZ - 2], b=[X(19), 262, CZ + 1], radius=5, radius_b=2.5, color_to=HAIR_L, gradient_axis="y", jitter=0.4, mirror_x=True)
    shape("ellipsoid", HAIR, "head", "hair", center=[CX, 274, CZ - 10], radii=[17, 18, 10], color_to=HAIR_L, gradient_axis="y", jitter=0.45, seed=9)
    # gold circlet with a gem
    paint("box", GOLD, "gold", min=[X(-22), 292, CZ - 25], max=[X(22), 294, CZ + 25])
    shape("ellipsoid", "#55E0FF", "head", "gem", center=[CX, 294, CZ + 20], radii=[2.5, 2.5, 1.5])

    # Anything left unbound follows the nearest bone.
    cmd("rig_bind", only_unbound=True)

    # ---------------------------------------------------------------- animations
    cmd("anim3d_generate", name="walk", kind="walk", frames=8, frame_ms=110, stride=26, knee=40, arm_swing=22, bob=3, lean=3)
    cmd("anim3d_generate", name="idle", kind="idle", frames=8)
    cmd("anim3d_generate", name="run", kind="run", frames=8, frame_ms=75)

    here = out
    with open(os.path.join(here, "hero.jsonl"), "w") as f:
        for c in cmds:
            f.write(json.dumps(c, separators=(",", ":")) + "\n")

    # ---------------------------------------------------------------- showcase renders
    show = list(cmds)
    look = {"pitch": 25, "supersample": 2, "outline": True, "inner_lines": True, "smoothness": 0.9, "exposure": 1.15}


    def out(op, save, **args):
        show.append({"op": op, "_save": save, **args})


    out("voxel_inspect", "hero_inspect.txt", _json=True)
    out("voxel_render", "hero_512_front.png", width=512, yaw=20, **{**look, "pitch": 15})
    out("voxel_render", "hero_512_three_quarter.png", width=512, yaw=-35, **{**look, "pitch": 15})
    out("voxel_render", "hero_512_back.png", width=512, yaw=160, **{**look, "pitch": 15})
    out("voxel_render", "hero_512_voxels_closeup.png", width=512, height=512, yaw=25, pitch=10, scale=5, smoothness=0.0, supersample=1, focus=[CX, 262, CZ + 10])
    for pas in ("albedo", "normal", "occlusion", "roughness", "metallic", "emission", "depth"):
        out("voxel_render", f"passes/hero_{pas}.png", width=512, yaw=20, pitch=15, pas="lit", **{"pass": pas})
    out("voxel_render_sheet", "sprites/walk_8dir_256.png", animation="walk", directions=8, width=256, _json=True, **look)
    out("voxel_render_sheet", "sprites/walk_8dir_256_normal.png", animation="walk", directions=8, width=256, pitch=25, supersample=2, **{"pass": "normal"})
    out("voxel_render_sheet", "sprites/walk_8dir_256_albedo.png", animation="walk", directions=8, width=256, pitch=25, supersample=2, **{"pass": "albedo"})
    out("voxel_render_sheet", "sprites/run_8dir_256.png", animation="run", directions=8, width=256, **look)
    out("voxel_render_sheet", "sprites/idle_8dir_256.png", animation="idle", directions=8, width=256, **look)
    # Bake the 8-direction walk into editable 2D frames, then export per-direction GIFs.
    show.append({"op": "voxel_render_to_frames", "animation": "walk", "directions": 8, "width": 256, "height": 256, **look})
    for dname in ("S", "SE", "E", "NE", "N", "NW", "W", "SW"):
        out("export_gif", f"gifs/walk_{dname}.gif", tag=f"walk_{dname}", scale=2)
    out("export_spritesheet", "sprites/walk_8dir_2d_project.png", columns=8, _json=True)
    out("project_save", "Aurelion.pxs")

    for c in show:
        if "pas" in c:
            del c["pas"]
    with open(os.path.join(here, "hero_showcase.jsonl"), "w") as f:
        for c in show:
            f.write(json.dumps(c, separators=(",", ":")) + "\n")
    print(f"{len(cmds)} build commands, {len(show) - len(cmds)} showcase outputs")


# ════ build_knight ══════════════════════════════════════════════════════════════════════════════

def build_knight(out):
    """Sir Valen, the Dawn Knight — a chunky 3D pixel (voxel) hero in the classic diorama style.

    Builds two models with Pixel Studio's public tools:
      * "Knight"   — rigged and animated (walk / run / idle), for 8-direction sprites.
      * "Diorama"  — the knight on a floating grass island with rocks, flowers, a glowing crystal
                     and drifting sparkles, for the hero shot.

        python3 build_knight.py   ->  knight.jsonl (models only) and knight_showcase.jsonl (+ renders)
    """
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

    here = out
    write_jsonl(os.path.join(os.path.dirname(out), "Knight.jsonl"), cmds)  # bundled by the apps

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


# ════ build_vfx ═════════════════════════════════════════════════════════════════════════════════

def build_vfx(out):
    """Voxel VFX library: 16-frame campfire (seamless loop), arcane portal (loop) and explosion.

    Frame-by-frame 3D pixel animation: every frame is its own voxel model (the static base is
    built once and cloned with voxel_model_clone), then the sequence is rendered with one shared
    camera into sprite sheets, GIFs and editable 2D frames. All through public tools.

        python3 build_vfx.py  ->  vfx.jsonl (models) and vfx_showcase.jsonl (+ renders)
    """
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

    here = out
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


def main(argv):
    """Usage: python3 Assets/Voxel.py [hero|knight|vfx|all]   → command scripts in Assets/Build/."""
    import os
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "Build")
    os.makedirs(out, exist_ok=True)
    targets = {"hero": build_hero, "knight": build_knight, "vfx": build_vfx}
    wanted = argv[1:] or ["all"]
    for name, fn in targets.items():
        if "all" in wanted or name in wanted:
            print(f"[{name}]")
            fn(out)


if __name__ == "__main__":
    import sys

    main(sys.argv)
