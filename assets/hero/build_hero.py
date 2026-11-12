#!/usr/bin/env python3
"""Builds the Pixel Studio 3D pixel hero ("Aurelion, the Dawn Knight") as engine commands.

Everything is made with the same tools an AI agent uses over MCP: voxel_shape, materials,
procedural PBR textures, rig_bone_add, anim3d_generate, voxel_render... No external assets.

    python3 build_hero.py            # writes hero.jsonl and hero_showcase.jsonl next to this file
    cargo run --release --example run_script -- ../../assets/hero/hero_showcase.jsonl out/

Coordinate system: 512^3 voxel grid, y up, the hero faces +z, ground at y = 0,
origin (256, 0, 256) between the feet. The character's left side is +x. Height ~300 voxels.
"""

import json
import os

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

here = os.path.dirname(os.path.abspath(__file__))
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
