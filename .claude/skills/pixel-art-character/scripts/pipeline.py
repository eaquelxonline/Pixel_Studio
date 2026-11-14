#!/usr/bin/env python3
"""Pixel Studio character pipeline from a shell (no gateway needed).

  pipeline.py import CONCEPT.png|jpg [--out DIR] [--colors 32] [--cell N] [--canvas 128]
      concept_import -> art_review -> sprite.png, sprite_x4.png, review.png, sprite.pxs

  pipeline.py turnaround SHEET.png|jpg [--palette-from CONCEPT.png|jpg] [--names S,SE,E,NE,N,NW,W,SW]
      concept_import -> split_sheet (frames, one baseline) -> quantize to the concept's colours
      -> one PNG per direction, spritesheet.png/.json, review per direction, turnaround.pxs

Both commands print a short report (grid cell, confidence, sprite size, critic score, issues).
The engine runner is built on first use (cargo, release mode).
"""
import argparse
import base64
import json
import pathlib
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = pathlib.Path(__file__).resolve().parents[4]
NATIVE = ROOT / "Native"
RUNNER = NATIVE / "target" / "release" / "examples" / "run_script"
DIRS = ["S", "SE", "E", "NE", "N", "NW", "W", "SW"]


def runner():
    if not RUNNER.exists():
        print("building the engine runner (one time)…", file=sys.stderr)
        subprocess.run(["cargo", "build", "--release", "--example", "run_script"], cwd=NATIVE, check=True)
    return RUNNER


def run(cmds, out):
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as fh:
        for c in cmds:
            fh.write(json.dumps(c) + "\n")
        script = fh.name
    try:
        r = subprocess.run([str(runner()), script, str(out)], capture_output=True, text=True)
    finally:
        pathlib.Path(script).unlink(missing_ok=True)
    if r.returncode != 0:
        sys.exit(f"engine failed:\n{r.stderr or r.stdout}")


def b64(path):
    return base64.b64encode(pathlib.Path(path).read_bytes()).decode()


def png_colors(path):
    """Opaque colours of an 8-bit, non-interlaced PNG (what the engine exports)."""
    data = pathlib.Path(path).read_bytes()
    pos, idat, plte, trns = 8, b"", b"", b""
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
        elif kind == b"PLTE":
            plte = body
        elif kind == b"tRNS":
            trns = body
        elif kind == b"IDAT":
            idat += body
        pos += n + 12
    if depth != 8 or interlace:
        sys.exit(f"{path}: only 8-bit non-interlaced PNGs are supported here")
    bpp = {2: 3, 6: 4, 3: 1, 0: 1, 4: 2}[ctype]
    raw, stride, prev, colors = zlib.decompress(idat), w * bpp, bytearray(w * bpp), set()
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b, c = prev[i], prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        prev = line
        for x in range(w):
            px = line[x * bpp:(x + 1) * bpp]
            if ctype == 6:
                rgb, alpha = px[:3], px[3]
            elif ctype == 2:
                rgb, alpha = px, 255
            elif ctype == 3:
                rgb, alpha = plte[px[0] * 3:px[0] * 3 + 3], trns[px[0]] if px[0] < len(trns) else 255
            else:
                rgb, alpha = bytes([px[0]] * 3), px[1] if ctype == 4 else 255
            if alpha >= 128:
                colors.add("#%02X%02X%02X" % tuple(rgb))
    return sorted(colors)


def report(title, imp, rev):
    print(f"== {title}")
    if imp:
        s = imp.get("sprite", {})
        print(f"   grid cell {imp.get('cell')} (confidence {imp.get('gridConfidence', 0):.2f}), "
              f"sprite {s.get('width')}x{s.get('height')}, {imp.get('colors')} colours, "
              f"background removed {imp.get('backgroundPixels')} px, specks cleaned {imp.get('cleanedPixels')} px")
        if imp.get("gridConfidence", 1) < 0.2 and imp.get("cell", 1) > 1:
            print("   ! weak grid: the image may not be true pixel art — re-roll with the grid wording or pass --cell")
    if rev:
        v = rev.get("values", {})
        print(f"   critic score {rev.get('score')}/100, value span {v.get('span')}")
        for i in rev.get("issues", []):
            print(f"   - [{i['severity']}] {i['code']}: {i['message']}")


def load(out, name):
    p = out / f"{name}.json"
    return json.loads(p.read_text()) if p.exists() else None


def cmd_import(a):
    out = pathlib.Path(a.out)
    imp = {"op": "concept_import", "data_base64": b64(a.image), "colors": a.colors, "_save": "import.bin", "_json": True}
    if a.cell:
        imp["cell"] = a.cell
    run([
        {"op": "project_new", "width": a.canvas, "height": a.canvas, "name": a.name},
        imp,
        {"op": "art_review", "scale": 4, "_save": "review.png", "_json": True},
        {"op": "export", "format": "png", "scale": 1, "density": 1, "_save": "sprite.png"},
        {"op": "export", "format": "png", "scale": 4, "density": 1, "_save": "sprite_x4.png"},
        {"op": "project_save", "_save": "sprite.pxs"},
    ], out)
    (out / "import.bin").unlink(missing_ok=True)
    report(a.image, load(out, "import"), load(out, "review"))
    print(f"   files: {out}/sprite.png, sprite_x4.png, review.png, sprite.pxs")


def cmd_turnaround(a):
    out = pathlib.Path(a.out)
    names = [n.strip() for n in a.names.split(",") if n.strip()]
    palette = None
    if a.palette_from:
        pal_dir = out / "_palette"
        run([
            {"op": "project_new", "width": a.canvas, "height": a.canvas},
            {"op": "concept_import", "data_base64": b64(a.palette_from), "colors": a.colors},
            {"op": "export", "format": "png", "scale": 1, "density": 1, "_save": "concept.png"},
        ], pal_dir)
        palette = png_colors(pal_dir / "concept.png")
        print(f"concept palette: {len(palette)} colours")
    cmds = [
        {"op": "project_new", "width": a.canvas, "height": a.canvas, "name": a.name},
        {"op": "concept_import", "data_base64": b64(a.sheet), "colors": a.colors, "_save": "import.bin", "_json": True},
        {"op": "split_sheet", "into": "frames", "names": names, "_save": "split.bin", "_json": True},
    ]
    if palette:
        cmds.append({"op": "quantize", "scope": "all", "palette": palette})
    for i, n in enumerate(names):
        cmds += [
            {"op": "art_review", "frame": i, "scale": 4, "_save": f"review_{n}.png", "_json": True},
            {"op": "export", "format": "png", "frame": i, "scale": 1, "density": 1, "_save": f"{n}.png"},
        ]
    cmds += [
        {"op": "export", "format": "spritesheet", "scale": 1, "_save": "spritesheet.png", "_json": True},
        {"op": "project_save", "_save": "turnaround.pxs"},
    ]
    run(cmds, out)
    for junk in ("import.bin", "split.bin"):
        (out / junk).unlink(missing_ok=True)
    report(a.sheet, load(out, "import"), None)
    split = load(out, "split") or {}
    print(f"   split: {json.dumps(split)[:300]}")
    for n in names:
        if (out / f"review_{n}.json").exists():
            report(f"direction {n}", None, load(out, f"review_{n}"))
    print(f"   files: {out}/<direction>.png, spritesheet.png/.json, review_*.png, turnaround.pxs")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    i = sub.add_parser("import", help="concept image -> clean pixel sprite")
    i.add_argument("image")
    t = sub.add_parser("turnaround", help="turnaround sheet -> one frame per direction")
    t.add_argument("sheet")
    t.add_argument("--palette-from", help="concept image whose colours every view is mapped onto")
    t.add_argument("--names", default=",".join(DIRS))
    for s in (i, t):
        s.add_argument("--out", default="character_out")
        s.add_argument("--colors", type=int, default=32)
        s.add_argument("--canvas", type=int, default=128)
        s.add_argument("--name", default="Character")
    i.add_argument("--cell", type=float)
    a = p.parse_args()
    {"import": cmd_import, "turnaround": cmd_turnaround}[a.cmd](a)


if __name__ == "__main__":
    main()
