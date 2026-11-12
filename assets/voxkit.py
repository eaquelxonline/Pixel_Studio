"""voxkit — tiny helper for authoring Pixel Studio voxel assets as engine command scripts.

Voxels are placed in a Python dict first (hand-detail friendly: patterns, trims, dithering),
then emitted as `voxel_set_many` commands grouped by material and bone. Everything goes through
the public command protocol, exactly like an AI agent using the MCP tools.
"""

import json
import math
import random


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
