#!/usr/bin/env python3
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

import colorsys
import json
import os

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

here = os.path.dirname(os.path.abspath(__file__))
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
