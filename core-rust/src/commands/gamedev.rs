//! Game-production commands: blend modes, slices (hitbox/hurtbox/pivot), palette swaps,
//! RotSprite, tilesets/tilemaps with blob autotiling, normal-mapped 2D light previews and
//! engine exports (Aseprite-compatible atlas JSON, Godot SpriteFrames, Tiled TMJ).

use std::sync::Arc;

use super::args::{color_value, frame_index, layer_index, opt_bool, opt_f64, opt_i64, opt_str, req_i64, req_str, scale, target};
use super::json::Json;
use super::{CmdError, CmdResult, Outcome, Query};
use crate::export;
use crate::gamedev::atlas::{self, Placement};
use crate::gamedev::light2d::{self, Light2D, LightSettings};
use crate::gamedev::tiles::{self, BlobStyle, Tilemap, Tileset, TilesetKind};
use crate::gamedev::{Slice, SliceKey, rotsprite};
use crate::layers::Blend;
use crate::pixel::{PixelGrid, Rgba, TRANSPARENT};
use crate::pixelart;
use crate::project::Document;
use crate::renderer::{Image, RenderOptions, render};

const MAX_TILE_SIDE: i64 = 256;
const MAX_TILES: usize = 4096;
const MAX_MAP_SIDE: i64 = 4096;

fn png_query(img: &Image, result: Json) -> Query {
    let png = export::png::encode(img);
    Query {
        result: result.with("width", img.width).with("height", img.height).with("mimeType", "image/png").with("bytes", png.len()),
        binary: png,
    }
}

fn hex_or(doc: &Document, a: &Json, key: &str, default: Rgba) -> CmdResult<Rgba> {
    match a.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(v) => Ok(Rgba::from_u32(color_value(doc, v, key)?)),
    }
}

// ---- layers & slices ---------------------------------------------------------------------------

pub fn layer_blend(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let li = layer_index(doc, a, "layer")?;
    let mode = req_str(a, "mode")?;
    let names: Vec<&str> = Blend::ALL.iter().map(|(n, _)| *n).collect();
    let blend = Blend::parse(mode).ok_or_else(|| CmdError::invalid(format!("unknown blend mode '{mode}' ({})", names.join(", "))))?;
    doc.layers[li].blend = blend;
    Ok(Outcome::new(Json::obj().with("layer", li).with("mode", blend.as_str()), 0))
}

/// Creates or updates a slice key. Without `frame` the key starts at the first frame, so it
/// applies to the whole animation until a later key overrides it.
pub fn slice_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = req_str(a, "name")?.trim().to_string();
    if name.is_empty() || name.len() > 128 {
        return Err(CmdError::invalid("slice 'name' must be 1-128 characters"));
    }
    let fi = if a.get("frame").is_some_and(|v| !v.is_null()) { frame_index(doc, a, "frame")? } else { 0 };
    let frame_id = doc.frames[fi].id;
    let (x, y) = (req_i64(a, "x")?, req_i64(a, "y")?);
    let (w, h) = (req_i64(a, "width")?, req_i64(a, "height")?);
    if !(1..=65_536).contains(&w) || !(1..=65_536).contains(&h) || x.abs() > 65_536 || y.abs() > 65_536 {
        return Err(CmdError::invalid("slice rectangle out of range (width/height ≥ 1)"));
    }
    let pivot = match a.get("pivot") {
        None | Some(Json::Null) => None,
        Some(Json::Arr(p)) if p.len() == 2 => {
            let px = p[0].as_i64().ok_or_else(|| CmdError::invalid("'pivot' must be [x, y]"))?;
            let py = p[1].as_i64().ok_or_else(|| CmdError::invalid("'pivot' must be [x, y]"))?;
            Some((px.clamp(-65_536, 65_536) as i32, py.clamp(-65_536, 65_536) as i32))
        }
        Some(_) => return Err(CmdError::invalid("'pivot' must be [x, y] relative to the slice")),
    };
    let default_color = match name.to_ascii_lowercase() {
        n if n.contains("hurt") => Rgba::new(64, 160, 255, 255),
        n if n.contains("hit") || n.contains("attack") => Rgba::new(255, 64, 64, 255),
        n if n.contains("pivot") || n.contains("origin") => Rgba::new(255, 220, 64, 255),
        _ => Rgba::new(0, 0, 255, 255),
    };
    let color = hex_or(doc, a, "color", default_color)?.to_u32();
    let key = SliceKey { frame_id, x: x as i32, y: y as i32, w: w as u32, h: h as u32, pivot };
    let created = match doc.slices.iter_mut().find(|s| s.name == name) {
        Some(s) => {
            if a.get("color").is_some() {
                s.color = color;
            }
            match s.keys.iter_mut().find(|k| k.frame_id == frame_id) {
                Some(k) => *k = key,
                None => s.keys.push(key),
            }
            false
        }
        None => {
            doc.slices.push(Slice { name: name.clone(), color, keys: vec![key] });
            true
        }
    };
    Ok(Outcome::new(Json::obj().with("slice", name).with("frame", fi).with("created", created), 0))
}

pub fn slice_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = req_str(a, "name")?;
    let before = doc.slices.len();
    match a.get("frame").filter(|v| !v.is_null()) {
        Some(_) => {
            let id = doc.frames[frame_index(doc, a, "frame")?].id;
            let s = doc.slices.iter_mut().find(|s| s.name == name).ok_or_else(|| CmdError::not_found(format!("no slice '{name}'")))?;
            s.keys.retain(|k| k.frame_id != id);
            doc.slices.retain(|s| !s.keys.is_empty());
        }
        None => {
            doc.slices.retain(|s| s.name != name);
            if doc.slices.len() == before {
                return Err(CmdError::not_found(format!("no slice '{name}'")));
            }
        }
    }
    Ok(Outcome::new(Json::obj().with("deleted", name), 0))
}

fn slice_json(doc: &Document, s: &Slice, scale: u32) -> Json {
    let keys = s
        .keys
        .iter()
        .filter_map(|k| {
            let fi = doc.frame_index(k.frame_id)?;
            let sc = scale as i32;
            let mut j = Json::obj()
                .with("frame", fi)
                .with("bounds", Json::obj().with("x", k.x * sc).with("y", k.y * sc).with("w", k.w * scale).with("h", k.h * scale));
            if let Some((px, py)) = k.pivot {
                j.set("pivot", Json::obj().with("x", px * sc).with("y", py * sc));
            }
            Some(j)
        })
        .collect();
    Json::obj().with("name", s.name.as_str()).with("color", Rgba::from_u32(s.color).to_hex()).with("keys", Json::Arr(keys))
}

// ---- recoloring & rotation -----------------------------------------------------------------------

fn scope_cels(doc: &Document, a: &Json) -> CmdResult<Vec<(u32, u32)>> {
    let scope = opt_str(a, "scope")?.unwrap_or("cel");
    let li = layer_index(doc, a, "layer")?;
    let fi = frame_index(doc, a, "frame")?;
    Ok(match scope {
        "cel" => vec![(doc.layers[li].id, doc.frames[fi].id)],
        "layer" => doc.frames.iter().map(|f| (doc.layers[li].id, f.id)).collect(),
        "frame" => doc.layers.iter().map(|l| (l.id, doc.frames[fi].id)).collect(),
        "all" => doc.layers.iter().flat_map(|l| doc.frames.iter().map(move |f| (l.id, f.id))).collect(),
        other => return Err(CmdError::invalid(format!("unknown scope '{other}' (cel, layer, frame, all)"))),
    })
}

/// Remaps colors (RGB match, alpha kept): team colors, palette variants, day/night swaps.
pub fn palette_swap(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let mut pairs: Vec<(u32, Rgba)> = Vec::new();
    match (a.get("map"), a.get("from"), a.get("to")) {
        (Some(Json::Obj(entries)), _, _) => {
            for (k, v) in entries {
                let from = Rgba::parse_hex(k).ok_or_else(|| CmdError::invalid(format!("map key '{k}' is not a hex color")))?;
                pairs.push((from.to_u32() | 0xFF, Rgba::from_u32(color_value(doc, v, "map")?)));
            }
        }
        (_, Some(Json::Arr(from)), Some(Json::Arr(to))) if from.len() == to.len() => {
            for (f, t) in from.iter().zip(to) {
                let f = Rgba::from_u32(color_value(doc, f, "from")?);
                pairs.push((f.to_u32() | 0xFF, Rgba::from_u32(color_value(doc, t, "to")?)));
            }
        }
        _ => return Err(CmdError::invalid("give 'map' {\"#from\": \"#to\"} or equally long 'from' and 'to' arrays")),
    }
    let cels = scope_cels(doc, a)?;
    let mut changed = 0u64;
    for key in cels {
        if doc.layers.iter().any(|l| l.id == key.0 && l.locked) {
            continue;
        }
        let Some(cel) = doc.cels.get_mut(&key) else { continue };
        let mut writes = Vec::new();
        cel.for_each_opaque(|x, y, v| {
            if let Some((_, to)) = pairs.iter().find(|(f, _)| *f == v | 0xFF) {
                let alpha = ((v & 0xFF) * to.a as u32 / 255) as u8;
                writes.push((x, y, Rgba::new(to.r, to.g, to.b, alpha).to_pixel()));
            }
        });
        let grid: &mut PixelGrid = Arc::make_mut(cel);
        for (x, y, v) in writes {
            changed += grid.set(x as i64, y as i64, v) as u64;
        }
    }
    Ok(Outcome::new(Json::obj().with("changed", changed).with("mappings", pairs.len()), changed))
}

/// Pixel-art rotation (Scale2x×3 + nearest) that keeps lines unbroken and adds no colors.
pub fn rotsprite_cmd(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let degrees = opt_f64(a, "degrees")?.ok_or_else(|| CmdError::invalid("missing required number 'degrees'"))? as f32;
    let (l, f) = target(doc, a)?;
    let (w, h) = (doc.width, doc.height);
    if (w as u64) * (h as u64) > 1024 * 1024 {
        return Err(CmdError::new("limit", "rotsprite works on canvases up to 1024×1024"));
    }
    let grid = doc.cel_mut(l, f);
    let (cx, cy) = match a.get("center") {
        Some(Json::Arr(c)) if c.len() == 2 => (c[0].as_f64().unwrap_or(0.0) as f32, c[1].as_f64().unwrap_or(0.0) as f32),
        _ => match grid.bounds() {
            Some((x, y, bw, bh)) => (x as f32 + bw as f32 / 2.0, y as f32 + bh as f32 / 2.0),
            None => return Ok(Outcome::new(Json::obj().with("changed", 0), 0)),
        },
    };
    let mut src = vec![0u32; (w * h) as usize];
    grid.for_each_opaque(|x, y, v| src[(y * w + x) as usize] = v);
    let out = rotsprite::rotate(&src, w as usize, h as usize, degrees, cx, cy);
    let mut changed = 0u64;
    for y in 0..h {
        for x in 0..w {
            changed += grid.set(x as i64, y as i64, out[(y * w + x) as usize]) as u64;
        }
    }
    Ok(Outcome::new(Json::obj().with("changed", changed).with("center", Json::Arr(vec![(cx as f64).into(), (cy as f64).into()])), changed))
}

// ---- tilesets & tilemaps -------------------------------------------------------------------------

fn by_name_or_index<T>(items: &[Arc<T>], name: impl Fn(&T) -> &str, v: Option<&Json>, what: &str) -> CmdResult<usize> {
    match v {
        Some(Json::Str(s)) => items
            .iter()
            .position(|t| name(t) == s)
            .or_else(|| s.parse::<usize>().ok().filter(|i| *i < items.len()))
            .ok_or_else(|| CmdError::not_found(format!("no {what} named '{s}'"))),
        Some(v) => {
            let i = v.as_i64().ok_or_else(|| CmdError::invalid(format!("'{what}' must be a name or index")))?;
            if i < 0 || i as usize >= items.len() {
                return Err(CmdError::not_found(format!("{what} index {i} out of range (0..{})", items.len())));
            }
            Ok(i as usize)
        }
        None if items.len() == 1 => Ok(0),
        None => Err(CmdError::invalid(format!("missing '{what}' (name or index)"))),
    }
}

fn tileset_idx(doc: &Document, a: &Json) -> CmdResult<usize> {
    by_name_or_index(&doc.tilesets, |t| &t.name, a.get("tileset"), "tileset")
}

fn tilemap_idx(doc: &Document, a: &Json) -> CmdResult<usize> {
    by_name_or_index(&doc.tilemaps, |t| &t.name, a.get("tilemap"), "tilemap")
}

fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> CmdResult<String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(CmdError::invalid("'name' must be 1-128 characters"));
    }
    if taken(name) {
        return Err(CmdError::new("conflict", format!("'{name}' already exists")));
    }
    Ok(name.to_string())
}

fn tile_size(a: &Json) -> CmdResult<(u32, u32)> {
    let s = opt_i64(a, "tile_size")?.unwrap_or(16);
    let w = opt_i64(a, "tile_width")?.unwrap_or(s);
    let h = opt_i64(a, "tile_height")?.unwrap_or(s);
    if !(1..=MAX_TILE_SIDE).contains(&w) || !(1..=MAX_TILE_SIDE).contains(&h) {
        return Err(CmdError::invalid(format!("tile size must be 1-{MAX_TILE_SIDE}")));
    }
    Ok((w as u32, h as u32))
}

pub fn tileset_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = unique_name(req_str(a, "name")?, |n| doc.tilesets.iter().any(|t| t.name == n))?;
    let (tile_w, tile_h) = tile_size(a)?;
    doc.tilesets.push(Arc::new(Tileset { name: name.clone(), tile_w, tile_h, kind: TilesetKind::Plain, tiles: Vec::new() }));
    Ok(Outcome::new(Json::obj().with("tileset", doc.tilesets.len() - 1).with("name", name), 0))
}

fn render_source(doc: &Document, a: &Json) -> CmdResult<Image> {
    let fi = frame_index(doc, a, "frame")?;
    let mut opts = RenderOptions::default();
    if a.get("layer").is_some_and(|v| !v.is_null()) {
        opts.only_layer = Some(doc.layers[layer_index(doc, a, "layer")?].id);
    }
    Ok(render(doc, fi, &opts))
}

fn cut(img: &Image, x: i64, y: i64, w: u32, h: u32) -> Vec<u32> {
    let mut t = vec![0u32; (w * h) as usize];
    for ty in 0..h as i64 {
        for tx in 0..w as i64 {
            let (sx, sy) = (x + tx, y + ty);
            if sx < 0 || sy < 0 || sx >= img.width as i64 || sy >= img.height as i64 {
                continue;
            }
            let c = img.get(sx as u32, sy as u32);
            t[(ty * w as i64 + tx) as usize] = if c.a == 0 { 0 } else { c.to_pixel() };
        }
    }
    t
}

/// Cuts a grid of tiles from the canvas into a tileset (duplicates are skipped by default).
pub fn tileset_add_tiles(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let ti = tileset_idx(doc, a)?;
    let img = render_source(doc, a)?;
    let (x0, y0) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
    let ts = &doc.tilesets[ti];
    let (tw, th) = (ts.tile_w, ts.tile_h);
    let cols = opt_i64(a, "columns")?.unwrap_or((img.width / tw) as i64).clamp(1, 256);
    let rows = opt_i64(a, "rows")?.unwrap_or((img.height / th) as i64).clamp(1, 256);
    let dedupe = opt_bool(a, "dedupe")?.unwrap_or(true);
    let skip_empty = opt_bool(a, "skip_empty")?.unwrap_or(true);
    let ts = Arc::make_mut(&mut doc.tilesets[ti]);
    let mut indices = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let t = cut(&img, x0 + c * tw as i64, y0 + r * th as i64, tw, th);
            if skip_empty && t.iter().all(|&v| v == 0) {
                continue;
            }
            if let Some(i) = dedupe.then(|| ts.tiles.iter().position(|e| *e == t)).flatten() {
                indices.push(Json::from(i));
                continue;
            }
            if ts.tiles.len() >= MAX_TILES {
                return Err(CmdError::new("limit", format!("a tileset holds at most {MAX_TILES} tiles")));
            }
            ts.tiles.push(t);
            indices.push(Json::from(ts.tiles.len() - 1));
        }
    }
    Ok(Outcome::new(Json::obj().with("tiles", Json::Arr(indices)).with("tileCount", ts.tiles.len()), 0))
}

fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x.wrapping_mul(0x8DA6_B343) ^ y.wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^ (h >> 15)
}

/// Seamless procedural fill: base color with small wrapped clusters of the accent colors
/// (grass blades, pebbles, cracks) — never a single noisy pixel soup.
fn procedural_fill(w: u32, h: u32, base: Rgba, accents: &[Rgba], density: f32, seed: u32) -> Vec<u32> {
    let mut t = vec![base.to_pixel(); (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let r = hash(x, y, seed);
            if (r % 1000) as f32 >= density * 1000.0 || accents.is_empty() {
                continue;
            }
            let c = accents[(r / 1000) as usize % accents.len()].to_pixel();
            t[(y * w + x) as usize] = c;
            // Two-pixel vertical cluster (wraps → seamless).
            if r & 0x10000 != 0 {
                t[(((y + h - 1) % h) * w + x) as usize] = c;
            }
        }
    }
    t
}

/// Generates the complete 47-tile blob terrain set from a fill (canvas region or procedural).
pub fn tileset_generate_blob(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = unique_name(req_str(a, "name")?, |n| doc.tilesets.iter().any(|t| t.name == n))?;
    let (tw, th) = tile_size(a)?;
    let fill: Vec<u32> = if a.get("source_x").is_some() {
        let img = render_source(doc, a)?;
        cut(&img, req_i64(a, "source_x")?, opt_i64(a, "source_y")?.unwrap_or(0), tw, th)
    } else {
        let base = hex_or(doc, a, "fill", Rgba::new(72, 140, 64, 255))?;
        let accents = match a.get("accents") {
            Some(Json::Arr(list)) => {
                list.iter().map(|c| color_value(doc, c, "accents").map(Rgba::from_u32)).collect::<CmdResult<Vec<_>>>()?
            }
            _ => {
                let r = pixelart::ramp(base, 5, 20.0, 0.6);
                vec![r[3], r[1]]
            }
        };
        let density = opt_f64(a, "density")?.unwrap_or(0.12).clamp(0.0, 1.0) as f32;
        procedural_fill(tw, th, base, &accents, density, opt_i64(a, "seed")?.unwrap_or(7) as u32)
    };
    let opaque: Vec<Rgba> = fill.iter().filter(|&&v| v != 0).map(|&v| Rgba::from_u32(v)).collect();
    if opaque.is_empty() {
        return Err(CmdError::invalid("the fill tile is empty"));
    }
    let n = opaque.len() as u32;
    let sum = opaque.iter().fold([0u32; 3], |s, c| [s[0] + c.r as u32, s[1] + c.g as u32, s[2] + c.b as u32]);
    let avg = Rgba::new((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, 255);
    let ramp = pixelart::ramp(avg, 7, 24.0, 0.8);
    let style = BlobStyle {
        inset: opt_i64(a, "inset")?.unwrap_or(1).clamp(0, (tw.min(th) / 4) as i64) as u32,
        edge: opt_i64(a, "edge")?.unwrap_or(1).clamp(0, (tw.min(th) / 3) as i64) as u32,
        light: hex_or(doc, a, "light", ramp[5])?,
        dark: hex_or(doc, a, "dark", ramp[2])?,
        outline: hex_or(doc, a, "outline", pixelart::outline_color(avg))?,
        lip: opt_i64(a, "lip")?.unwrap_or(0).clamp(0, (th / 2) as i64) as u32,
        lip_color: hex_or(doc, a, "lip_color", Rgba::new(110, 72, 52, 255))?,
        rounded: opt_bool(a, "rounded")?.unwrap_or(true),
    };
    let tiles = tiles::synthesize_blob(&fill, tw, th, &style);
    doc.tilesets.push(Arc::new(Tileset { name: name.clone(), tile_w: tw, tile_h: th, kind: TilesetKind::Blob, tiles }));
    Ok(Outcome::new(Json::obj().with("tileset", doc.tilesets.len() - 1).with("name", name).with("tileCount", 47), 0))
}

pub fn tileset_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let ti = tileset_idx(doc, a)?;
    if let Some(m) = doc.tilemaps.iter().find(|m| m.tileset == ti) {
        return Err(CmdError::new("conflict", format!("tilemap '{}' still uses this tileset", m.name)));
    }
    let ts = doc.tilesets.remove(ti);
    for m in &mut doc.tilemaps {
        if m.tileset > ti {
            Arc::make_mut(m).tileset -= 1;
        }
    }
    Ok(Outcome::new(Json::obj().with("deleted", ts.name.as_str()), 0))
}

pub fn tilemap_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = unique_name(req_str(a, "name")?, |n| doc.tilemaps.iter().any(|t| t.name == n))?;
    let ti = tileset_idx(doc, a)?;
    let (cols, rows) = (req_i64(a, "columns")?, req_i64(a, "rows")?);
    if !(1..=MAX_MAP_SIDE).contains(&cols) || !(1..=MAX_MAP_SIDE).contains(&rows) || cols * rows > 1 << 22 {
        return Err(CmdError::invalid(format!("tilemap size must be 1-{MAX_MAP_SIDE} per side")));
    }
    doc.tilemaps.push(Arc::new(Tilemap::new(&name, ti, cols as u32, rows as u32)));
    Ok(Outcome::new(Json::obj().with("tilemap", doc.tilemaps.len() - 1).with("name", name), 0))
}

pub fn tilemap_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let mi = tilemap_idx(doc, a)?;
    let m = doc.tilemaps.remove(mi);
    Ok(Outcome::new(Json::obj().with("deleted", m.name.as_str()), 0))
}

/// Places explicit tiles: `cells` = [[x, y, tile], …] (tile -1/null = empty) or x/y/tile, or
/// `rows` of space-separated tile numbers.
pub fn tilemap_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let mi = tilemap_idx(doc, a)?;
    let count = doc.tilesets[doc.tilemaps[mi].tileset].tiles.len() as i64;
    let mut writes: Vec<(i64, i64, i64)> = Vec::new();
    let tile_of = |v: &Json| -> CmdResult<i64> {
        match v {
            Json::Null => Ok(-1),
            v => v.as_i64().ok_or_else(|| CmdError::invalid("tile must be an index or null")),
        }
    };
    if let Some(Json::Arr(cells)) = a.get("cells") {
        for c in cells {
            match c.as_array() {
                Some([x, y, t]) => writes.push((
                    x.as_i64().ok_or_else(|| CmdError::invalid("cells: x must be an integer"))?,
                    y.as_i64().ok_or_else(|| CmdError::invalid("cells: y must be an integer"))?,
                    tile_of(t)?,
                )),
                _ => return Err(CmdError::invalid("each cell must be [x, y, tile]")),
            }
        }
    } else if let Some(Json::Arr(rows)) = a.get("rows") {
        let (ox, oy) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
        for (y, row) in rows.iter().enumerate() {
            let s = row.as_str().ok_or_else(|| CmdError::invalid("rows must be strings of tile numbers"))?;
            for (x, tok) in s.split_whitespace().enumerate() {
                let t = if tok == "." || tok == "-" {
                    -1
                } else {
                    tok.parse::<i64>().map_err(|_| CmdError::invalid(format!("'{tok}' is not a tile")))?
                };
                writes.push((ox + x as i64, oy + y as i64, t));
            }
        }
    } else {
        writes.push((req_i64(a, "x")?, req_i64(a, "y")?, tile_of(a.get("tile").unwrap_or(&Json::Null))?));
    }
    let m = Arc::make_mut(&mut doc.tilemaps[mi]);
    let mut changed = 0u64;
    for (x, y, t) in writes {
        if !m.in_bounds(x, y) {
            return Err(CmdError::invalid(format!("cell ({x}, {y}) is outside the {}×{} map", m.cols, m.rows)));
        }
        if t >= count {
            return Err(CmdError::not_found(format!("tile {t} does not exist (tileset has {count})")));
        }
        let i = (y as u32 * m.cols + x as u32) as usize;
        let v = (t + 1).max(0) as u32;
        if m.cells[i] != v {
            m.cells[i] = v;
            changed += 1;
        }
    }
    Ok(Outcome::new(Json::obj().with("changedCells", changed), 0))
}

/// Paints terrain and re-autotiles: `rows` as ASCII ('#' = terrain), a rectangle, or cells.
pub fn tilemap_terrain(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let mi = tilemap_idx(doc, a)?;
    if doc.tilesets[doc.tilemaps[mi].tileset].kind != TilesetKind::Blob {
        return Err(CmdError::invalid("terrain painting needs a blob tileset (tileset_generate_blob)"));
    }
    let value = opt_str(a, "mode")?.unwrap_or("add") != "remove";
    let mut cells: Vec<(i64, i64, bool)> = Vec::new();
    if let Some(Json::Arr(rows)) = a.get("rows") {
        let (ox, oy) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
        let replace = opt_bool(a, "replace")?.unwrap_or(true);
        for (y, row) in rows.iter().enumerate() {
            let s = row.as_str().ok_or_else(|| CmdError::invalid("rows must be strings"))?;
            for (x, ch) in s.chars().enumerate() {
                let on = matches!(ch, '#' | 'X' | 'x' | '1' | '█');
                if on || replace {
                    cells.push((ox + x as i64, oy + y as i64, on));
                }
            }
        }
    } else if let Some(Json::Arr(list)) = a.get("cells") {
        for c in list {
            match c.as_array() {
                Some([x, y]) => cells.push((x.as_i64().unwrap_or(-1), y.as_i64().unwrap_or(-1), value)),
                _ => return Err(CmdError::invalid("each cell must be [x, y]")),
            }
        }
    } else {
        let (x, y) = (req_i64(a, "x")?, req_i64(a, "y")?);
        let (w, h) = (opt_i64(a, "width")?.unwrap_or(1), opt_i64(a, "height")?.unwrap_or(1));
        if w < 1 || h < 1 || w > MAX_MAP_SIDE || h > MAX_MAP_SIDE {
            return Err(CmdError::invalid("width/height must be 1-4096"));
        }
        for yy in y..y + h {
            for xx in x..x + w {
                cells.push((xx, yy, value));
            }
        }
    }
    let m = Arc::make_mut(&mut doc.tilemaps[mi]);
    let mut changed = 0u64;
    for (x, y, on) in cells {
        if !m.in_bounds(x, y) {
            continue;
        }
        let i = (y as u32 * m.cols + x as u32) as usize;
        changed += (m.terrain[i] != on) as u64;
        m.terrain[i] = on;
    }
    m.autotile();
    Ok(Outcome::new(Json::obj().with("changedCells", changed).with("terrainCells", m.terrain.iter().filter(|&&t| t).count()), 0))
}

/// Draws a tilemap into a layer cel (so it composites, lights and exports with the scene).
pub fn tilemap_bake(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let mi = tilemap_idx(doc, a)?;
    let map = Arc::clone(&doc.tilemaps[mi]);
    let ts = Arc::clone(&doc.tilesets[map.tileset]);
    let img = map.render(&ts);
    let (ox, oy) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
    let replace = opt_bool(a, "replace")?.unwrap_or(false);
    let (l, f) = target(doc, a)?;
    let grid = doc.cel_mut(l, f);
    let mut changed = 0u64;
    for y in 0..img.height {
        for x in 0..img.width {
            let c = img.get(x, y);
            if c.a == 0 && !replace {
                continue;
            }
            let v = if c.a == 0 { TRANSPARENT } else { c.to_pixel() };
            changed += grid.set(ox + x as i64, oy + y as i64, v) as u64;
        }
    }
    Ok(Outcome::new(Json::obj().with("changed", changed).with("width", img.width).with("height", img.height), changed))
}

// ---- queries --------------------------------------------------------------------------------------

pub fn gamedev_inspect(doc: &Document) -> CmdResult<Query> {
    let tilesets = doc
        .tilesets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            Json::obj()
                .with("index", i)
                .with("name", t.name.as_str())
                .with("tileWidth", t.tile_w)
                .with("tileHeight", t.tile_h)
                .with("kind", if t.kind == TilesetKind::Blob { "blob47" } else { "plain" })
                .with("tileCount", t.tiles.len())
        })
        .collect();
    let tilemaps = doc
        .tilemaps
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut j = Json::obj()
                .with("index", i)
                .with("name", m.name.as_str())
                .with("tileset", doc.tilesets[m.tileset].name.as_str())
                .with("columns", m.cols)
                .with("rows", m.rows)
                .with("filledCells", m.cells.iter().filter(|&&c| c != 0).count());
            if m.cols <= 96 && m.rows <= 64 {
                let rows = (0..m.rows)
                    .map(|y| {
                        Json::from((0..m.cols).map(|x| if m.terrain[(y * m.cols + x) as usize] { '#' } else { '.' }).collect::<String>())
                    })
                    .collect();
                j.set("terrain", Json::Arr(rows));
            }
            j
        })
        .collect();
    let layers = doc.layers.iter().map(|l| Json::obj().with("name", l.name.as_str()).with("blend", l.blend.as_str())).collect();
    Ok(Query {
        result: Json::obj()
            .with("slices", Json::Arr(doc.slices.iter().map(|s| slice_json(doc, s, 1)).collect()))
            .with("tilesets", Json::Arr(tilesets))
            .with("tilemaps", Json::Arr(tilemaps))
            .with("layerBlendModes", Json::Arr(layers))
            .with("blendModes", Json::Arr(Blend::ALL.iter().map(|(n, _)| Json::from(*n)).collect())),
        binary: Vec::new(),
    })
}

fn rgb_arg(doc: &Document, v: Option<&Json>, key: &str, default: [f32; 3]) -> CmdResult<[f32; 3]> {
    match v {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Arr(c)) if c.len() == 3 => {
            let mut out = [0.0; 3];
            for (i, x) in c.iter().enumerate() {
                out[i] = x.as_f64().ok_or_else(|| CmdError::invalid(format!("'{key}' must be [r, g, b] or a hex color")))? as f32;
            }
            Ok(out)
        }
        Some(v) => Ok(light2d::rgb_lin(Rgba::from_u32(color_value(doc, v, key)?))),
    }
}

/// Renders a frame with normal-mapped 2D point lights (torches, campfires, spells) — the
/// in-editor preview of what Godot/Unity 2D lights will do with the exported normal map.
pub fn render_lit(doc: &Document, a: &Json) -> CmdResult<Query> {
    let fi = frame_index(doc, a, "frame")?;
    let unlit: Vec<u32> = match a.get("unlit_layers") {
        Some(Json::Arr(list)) => list
            .iter()
            .map(|v| layer_index(doc, &Json::obj().with("l", v.clone()), "l").map(|i| doc.layers[i].id))
            .collect::<CmdResult<_>>()?,
        _ => Vec::new(),
    };
    let normal_layer = match a.get("normal_layer") {
        Some(v) if !v.is_null() => Some(doc.layers[layer_index(doc, &Json::obj().with("l", v.clone()), "l")?].id),
        _ => None,
    };
    let mut lit_doc = doc.clone();
    for l in &mut lit_doc.layers {
        if unlit.contains(&l.id) || Some(l.id) == normal_layer {
            l.visible = false;
        }
    }
    let albedo = render(&lit_doc, fi, &RenderOptions::default());
    let normals = match normal_layer {
        Some(id) => render(doc, fi, &RenderOptions { only_layer: Some(id), ..Default::default() }),
        None => {
            let strength = opt_f64(a, "strength")?.unwrap_or(3.0).clamp(0.1, 20.0) as f32;
            let bevel = opt_f64(a, "bevel")?.unwrap_or(3.0).clamp(0.5, 32.0) as f32;
            pixelart::normal_map(&albedo, strength, bevel)
        }
    };
    let mut lights = Vec::new();
    if let Some(Json::Arr(list)) = a.get("lights") {
        for l in list.iter().take(64) {
            let x = opt_f64(l, "x")?.ok_or_else(|| CmdError::invalid("each light needs x and y"))? as f32;
            let y = opt_f64(l, "y")?.ok_or_else(|| CmdError::invalid("each light needs x and y"))? as f32;
            lights.push(Light2D {
                x,
                y,
                z: opt_f64(l, "z")?.unwrap_or(24.0).clamp(0.5, 4096.0) as f32,
                color: rgb_arg(doc, l.get("color"), "color", [1.0, 0.75, 0.45])?,
                intensity: opt_f64(l, "intensity")?.unwrap_or(1.5).clamp(0.0, 64.0) as f32,
                radius: opt_f64(l, "radius")?.unwrap_or(64.0).clamp(1.0, 8192.0) as f32,
            });
        }
    }
    let settings = LightSettings {
        ambient: rgb_arg(doc, a.get("ambient"), "ambient", [0.18, 0.18, 0.26])?,
        lights,
        bands: opt_i64(a, "bands")?.unwrap_or(0).clamp(0, 32) as u32,
        normal_strength: opt_f64(a, "normal_strength")?.unwrap_or(1.0).clamp(0.0, 8.0) as f32,
    };
    let mut out = light2d::light(&albedo, &normals, &settings);
    if !unlit.is_empty() {
        let mut glow = doc.clone();
        for l in &mut glow.layers {
            l.visible = unlit.contains(&l.id);
        }
        let g = render(&glow, fi, &RenderOptions::default());
        for y in 0..g.height {
            for x in 0..g.width {
                let c = g.get(x, y);
                if c.a > 0 {
                    let o = ((y * out.width + x) * 4) as usize;
                    crate::renderer::blend_over(&mut out.pixels[o..o + 4], c, 255);
                }
            }
        }
    }
    if let Some(bg) = opt_str(a, "background")? {
        let bg = Rgba::parse_hex(bg).ok_or_else(|| CmdError::invalid("'background' must be a hex color"))?;
        let mut base = Image::new(out.width, out.height);
        for p in base.pixels.as_chunks_mut::<4>().0 {
            *p = [bg.r, bg.g, bg.b, 255];
        }
        for y in 0..out.height {
            for x in 0..out.width {
                let o = ((y * out.width + x) * 4) as usize;
                crate::renderer::blend_over(&mut base.pixels[o..o + 4], out.get(x, y), 255);
            }
        }
        out = base;
    }
    let s = scale(a, 1)?;
    if out.width * s > 8192 || out.height * s > 8192 {
        return Err(CmdError::new("limit", "scaled output would exceed 8192px"));
    }
    Ok(png_query(&out.scaled(s), Json::obj().with("frame", fi).with("lights", settings.lights.len())))
}

/// Frames to export (timeline order): a tag's range or every frame.
fn export_frames(doc: &Document, a: &Json) -> CmdResult<Vec<usize>> {
    match opt_str(a, "tag")? {
        Some(name) => {
            let t = doc.tags.iter().find(|t| t.name == name).ok_or_else(|| CmdError::not_found(format!("no tag '{name}'")))?;
            let (from, to) = (doc.frame_index(t.from_id).unwrap_or(0), doc.frame_index(t.to_id).unwrap_or(0));
            Ok((from.min(to)..=from.max(to)).collect())
        }
        None => Ok((0..doc.frames.len()).collect()),
    }
}

struct Packed {
    frames: Vec<usize>,
    atlas: atlas::Atlas,
    scale: u32,
}

fn pack_frames(doc: &Document, a: &Json) -> CmdResult<Packed> {
    let frames = export_frames(doc, a)?;
    let s = scale(a, 1)?;
    if doc.width * s > 4096 || doc.height * s > 4096 {
        return Err(CmdError::new("limit", "frames larger than 4096px; lower 'scale'"));
    }
    let mut opts = RenderOptions::default();
    if a.get("layer").is_some_and(|v| !v.is_null()) {
        opts.only_layer = Some(doc.layers[layer_index(doc, a, "layer")?].id);
    }
    let images: Vec<Image> = frames.iter().map(|&i| render(doc, i, &opts).scaled(s)).collect();
    let atlas = atlas::pack(
        &images,
        opt_bool(a, "trim")?.unwrap_or(true),
        opt_i64(a, "padding")?.unwrap_or(1).clamp(0, 64) as u32,
        opt_i64(a, "extrude")?.unwrap_or(0).clamp(0, 16) as u32,
        opt_bool(a, "power_of_two")?.unwrap_or(false),
    )
    .map_err(|e| CmdError::new("limit", e))?;
    Ok(Packed { frames, atlas, scale: s })
}

fn rect_json(x: u32, y: u32, w: u32, h: u32) -> Json {
    Json::obj().with("x", x).with("y", y).with("w", w).with("h", h)
}

/// Texture atlas + Aseprite-compatible JSON (array format: frames, frameTags, layers, slices),
/// readable by Phaser, PixiJS, Unity/Godot Aseprite importers and LibGDX tools.
pub fn export_atlas(doc: &Document, a: &Json) -> CmdResult<Query> {
    let p = pack_frames(doc, a)?;
    let image_name = opt_str(a, "image")?.unwrap_or("atlas.png");
    let base = doc.name.replace(['/', '\\'], "_");
    let frames: Vec<Json> = p
        .frames
        .iter()
        .zip(&p.atlas.placements)
        .enumerate()
        .map(|(k, (&fi, pl))| frame_entry(format!("{base} {k}.png"), pl, doc.frames[fi].duration_ms))
        .collect();
    let first = p.frames.first().copied().unwrap_or(0);
    let last = p.frames.last().copied().unwrap_or(0);
    let tags: Vec<Json> = doc
        .tags
        .iter()
        .filter_map(|t| {
            let (from, to) = (doc.frame_index(t.from_id)?, doc.frame_index(t.to_id)?);
            if from < first || to > last {
                return None;
            }
            let mut j = Json::obj()
                .with("name", t.name.as_str())
                .with("from", from - first)
                .with("to", to - first)
                .with("direction", t.direction.as_str())
                .with("color", "#000000ff");
            if t.repeat > 0 {
                j.set("repeat", t.repeat.to_string());
            }
            Some(j)
        })
        .collect();
    let layers = doc
        .layers
        .iter()
        .map(|l| Json::obj().with("name", l.name.as_str()).with("opacity", l.opacity).with("blendMode", l.blend.as_str()))
        .collect();
    let meta = Json::obj()
        .with("app", "Pixel Studio")
        .with("version", env!("CARGO_PKG_VERSION"))
        .with("image", image_name)
        .with("format", "RGBA8888")
        .with("size", Json::obj().with("w", p.atlas.image.width).with("h", p.atlas.image.height))
        .with("scale", p.scale.to_string())
        .with("frameTags", Json::Arr(tags))
        .with("layers", Json::Arr(layers))
        .with("slices", Json::Arr(doc.slices.iter().map(|s| slice_json(doc, s, p.scale)).collect()));
    let unique = p.atlas.placements.iter().filter(|pl| pl.same_as.is_none()).count();
    let data = Json::obj().with("frames", Json::Arr(frames)).with("meta", meta);
    Ok(png_query(&p.atlas.image, Json::obj().with("data", data).with("uniqueFrames", unique).with("frameCount", p.frames.len())))
}

fn frame_entry(filename: String, pl: &Placement, duration: u32) -> Json {
    let f = pl.frame;
    Json::obj()
        .with("filename", filename)
        .with("frame", rect_json(f.x, f.y, f.w, f.h))
        .with("rotated", false)
        .with("trimmed", pl.trimmed && (f.w != pl.source_w || f.h != pl.source_h))
        .with("spriteSourceSize", rect_json(pl.offset_x, pl.offset_y, f.w, f.h))
        .with("sourceSize", Json::obj().with("w", pl.source_w).with("h", pl.source_h))
        .with("duration", duration)
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Godot 4 `SpriteFrames` resource (.tres text) over the packed atlas: one animation per tag
/// (or "default"), trimmed frames restored with AtlasTexture margins, per-frame durations.
pub fn export_godot(doc: &Document, a: &Json) -> CmdResult<Query> {
    let p = pack_frames(doc, a)?;
    let texture = opt_str(a, "texture_path")?.unwrap_or("res://atlas.png");
    let fps_base = p.frames.iter().map(|&i| doc.frames[i].duration_ms.max(1)).fold(0, gcd).max(1);
    let speed = 1000.0 / fps_base as f64;
    let mut out = String::from("[gd_resource type=\"SpriteFrames\" format=3]\n\n");
    out += &format!("[ext_resource type=\"Texture2D\" path=\"{}\" id=\"1_atlas\"]\n\n", texture.replace('"', ""));
    for (k, pl) in p.atlas.placements.iter().enumerate() {
        let f = pl.frame;
        out += &format!("[sub_resource type=\"AtlasTexture\" id=\"AtlasTexture_{k}\"]\natlas = ExtResource(\"1_atlas\")\n");
        out += &format!("region = Rect2({}, {}, {}, {})\n", f.x, f.y, f.w, f.h);
        if pl.trimmed && (f.w != pl.source_w || f.h != pl.source_h) {
            out += &format!("margin = Rect2({}, {}, {}, {})\n", pl.offset_x, pl.offset_y, pl.source_w - f.w, pl.source_h - f.h);
        }
        out += "\n";
    }
    let first = p.frames.first().copied().unwrap_or(0);
    let mut anims: Vec<(String, Vec<usize>, bool)> = Vec::new();
    for t in &doc.tags {
        let (Some(from), Some(to)) = (doc.frame_index(t.from_id), doc.frame_index(t.to_id)) else { continue };
        if from < first || to >= first + p.frames.len() {
            continue;
        }
        let mut seq: Vec<usize> = (from..=to).map(|i| i - first).collect();
        match t.direction {
            crate::animation::Direction::Reverse => seq.reverse(),
            crate::animation::Direction::PingPong if seq.len() > 2 => {
                let back: Vec<usize> = seq[1..seq.len() - 1].iter().rev().copied().collect();
                seq.extend(back);
            }
            _ => {}
        }
        anims.push((t.name.clone(), seq, t.repeat == 0));
    }
    if anims.is_empty() {
        anims.push(("default".into(), (0..p.frames.len()).collect(), true));
    }
    out += "[resource]\nanimations = [";
    for (ai, (name, seq, looping)) in anims.iter().enumerate() {
        if ai > 0 {
            out += ", ";
        }
        out += "{\n\"frames\": [";
        for (j, &k) in seq.iter().enumerate() {
            if j > 0 {
                out += ", ";
            }
            let dur = doc.frames[p.frames[k]].duration_ms as f64 / fps_base as f64;
            out += &format!("{{\n\"duration\": {dur:.1},\n\"texture\": SubResource(\"AtlasTexture_{k}\")\n}}");
        }
        let name = name.replace(['"', '\\'], "");
        out += &format!("],\n\"loop\": {looping},\n\"name\": &\"{name}\",\n\"speed\": {speed:.1}\n}}");
    }
    out += "]\n";
    Ok(png_query(&p.atlas.image, Json::obj().with("tres", out).with("animations", anims.len()).with("texturePath", texture)))
}

pub fn export_tileset(doc: &Document, a: &Json) -> CmdResult<Query> {
    let ts = &doc.tilesets[tileset_idx(doc, a)?];
    let cols = opt_i64(a, "columns")?.unwrap_or(if ts.kind == TilesetKind::Blob { 8 } else { 16 }).clamp(1, 256) as u32;
    let img = ts.atlas(cols);
    let s = scale(a, 1)?;
    if img.width * s > 8192 || img.height * s > 8192 {
        return Err(CmdError::new("limit", "scaled output would exceed 8192px"));
    }
    let masks = if ts.kind == TilesetKind::Blob {
        Json::Arr(tiles::blob_masks().iter().map(|&m| Json::from(m as u32)).collect())
    } else {
        Json::Null
    };
    Ok(png_query(&img.scaled(s), Json::obj().with("columns", cols).with("tileCount", ts.tiles.len()).with("blobMasks", masks)))
}

pub fn tilemap_render(doc: &Document, a: &Json) -> CmdResult<Query> {
    let m = &doc.tilemaps[tilemap_idx(doc, a)?];
    let img = m.render(&doc.tilesets[m.tileset]);
    let s = scale(a, 1)?;
    if img.width * s > 8192 || img.height * s > 8192 {
        return Err(CmdError::new("limit", "scaled output would exceed 8192px"));
    }
    Ok(png_query(&img.scaled(s), Json::obj().with("tilemap", m.name.as_str())))
}

/// Tiled map (TMJ JSON, embedded tileset) + the tileset PNG it references. Blob tilesets also
/// get a Wang set ("terrain") so the map stays editable with Tiled's terrain brush.
pub fn export_tiled(doc: &Document, a: &Json) -> CmdResult<Query> {
    let m = &doc.tilemaps[tilemap_idx(doc, a)?];
    let ts = &doc.tilesets[m.tileset];
    let cols = if ts.kind == TilesetKind::Blob { 8 } else { 16u32.min(ts.tiles.len().max(1) as u32) };
    let img = ts.atlas(cols);
    let image_name = opt_str(a, "image")?.unwrap_or("tileset.png");
    let mut tileset = Json::obj()
        .with("firstgid", 1)
        .with("name", ts.name.as_str())
        .with("image", image_name)
        .with("imagewidth", img.width)
        .with("imageheight", img.height)
        .with("tilewidth", ts.tile_w)
        .with("tileheight", ts.tile_h)
        .with("tilecount", ts.tiles.len())
        .with("columns", cols)
        .with("margin", 0)
        .with("spacing", 0);
    if ts.kind == TilesetKind::Blob {
        // Tiled corner+edge ("mixed") Wang IDs: [top, top-right, right, bottom-right, bottom, bottom-left, left, top-left].
        let bits = [tiles::N, tiles::NE, tiles::E, tiles::SE, tiles::S, tiles::SW, tiles::W, tiles::NW];
        let wangtiles = tiles::blob_masks()
            .iter()
            .enumerate()
            .map(|(i, &mask)| {
                let id = bits.iter().map(|&b| Json::from(if mask & b != 0 { 1 } else { 0 })).collect();
                Json::obj().with("tileid", i).with("wangid", Json::Arr(id))
            })
            .collect();
        tileset.set(
            "wangsets",
            Json::Arr(vec![
                Json::obj()
                    .with("name", ts.name.as_str())
                    .with("type", "mixed")
                    .with("tile", 46)
                    .with(
                        "colors",
                        Json::Arr(vec![
                            Json::obj().with("name", ts.name.as_str()).with("color", "#4caf50").with("tile", 46).with("probability", 1),
                        ]),
                    )
                    .with("wangtiles", Json::Arr(wangtiles)),
            ]),
        );
    }
    let layer = Json::obj()
        .with("id", 1)
        .with("name", m.name.as_str())
        .with("type", "tilelayer")
        .with("x", 0)
        .with("y", 0)
        .with("width", m.cols)
        .with("height", m.rows)
        .with("opacity", 1)
        .with("visible", true)
        .with("data", Json::Arr(m.cells.iter().map(|&c| Json::from(c)).collect()));
    let tmj = Json::obj()
        .with("type", "map")
        .with("version", "1.10")
        .with("tiledversion", "1.11.2")
        .with("orientation", "orthogonal")
        .with("renderorder", "right-down")
        .with("infinite", false)
        .with("width", m.cols)
        .with("height", m.rows)
        .with("tilewidth", ts.tile_w)
        .with("tileheight", ts.tile_h)
        .with("compressionlevel", -1)
        .with("nextlayerid", 2)
        .with("nextobjectid", 1)
        .with("layers", Json::Arr(vec![layer]))
        .with("tilesets", Json::Arr(vec![tileset]));
    Ok(png_query(&img, Json::obj().with("tmj", tmj).with("image", image_name)))
}
