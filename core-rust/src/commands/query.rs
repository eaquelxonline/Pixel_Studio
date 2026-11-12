//! Read-only commands: inspection, pixel reads, rendering and export.

use super::args::{coord, frame_index, layer_index, opt_bool, opt_i64, opt_str, region, scale};
use super::json::Json;
use super::{CmdError, CmdResult, Query};
use crate::animation::{Direction, playback_order, timing};
use crate::export::{self, SheetFrame, gif::GifFrame};
use crate::history::History;
use crate::pixel::{Rgba, TRANSPARENT};
use crate::project::Document;
use crate::renderer::{Image, OnionSkin, RenderOptions, render};
use crate::storage;

pub const MAX_READ_AREA: i64 = 128 * 128;
const MAX_OUTPUT_SIDE: u32 = 8192;
/// Symbols for the compact text view; '.' is transparent.
const ASCII_KEYS: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz#$%&*+=?@^~<>/|";

fn q(result: Json) -> CmdResult<Query> {
    Ok(Query { result, binary: Vec::new() })
}

pub fn run_query(
    doc: &Document,
    history: &History,
    revision: u64,
    op: &str,
    a: &Json,
    cache: &mut super::voxel::RenderCache,
) -> CmdResult<Query> {
    match op {
        "palette_list" => Ok(super::pixel2d::palette_list()),
        "export_normal_map" => super::pixel2d::export_normal_map(doc, a),
        "voxel_inspect" => super::voxel::voxel_inspect(doc, a),
        "voxel_get" => super::voxel::voxel_get(doc, a),
        "voxel_render" => super::voxel::voxel_render(doc, cache, a),
        "voxel_render_sheet" => super::voxel::voxel_render_sheet(doc, cache, a),
        "voxel_pick" => super::voxel::voxel_pick(doc, cache, a),
        "gamedev_inspect" => super::gamedev::gamedev_inspect(doc),
        "render_lit" => super::gamedev::render_lit(doc, a),
        "export_atlas" => super::gamedev::export_atlas(doc, a),
        "export_godot_spriteframes" => super::gamedev::export_godot(doc, a),
        "export_tileset" => super::gamedev::export_tileset(doc, a),
        "export_tiled" => super::gamedev::export_tiled(doc, a),
        "tilemap_render" => super::gamedev::tilemap_render(doc, a),
        "status" => q(Json::obj()
            .with("engine", "pixcore")
            .with("version", env!("CARGO_PKG_VERSION"))
            .with("revision", revision)
            .with("project", doc.name.as_str())
            .with("undo", history.can_undo())
            .with("redo", history.can_redo())
            .with("abiVersion", crate::ffi::ABI_VERSION)
            .with("operations", Json::Arr(super::OPERATIONS.iter().map(|(name, _)| Json::from(*name)).collect()))),
        "canvas_inspect" | "project_inspect" => q(inspect(doc, history, revision, op == "project_inspect")),
        "get_pixel" => get_pixel(doc, a),
        "read_region" => read_region(doc, a),
        "frame_get" => frame_get(doc, a),
        "palette_get" => q(Json::obj().with("size", doc.palette.len()).with(
            "colors",
            Json::Arr(
                doc.palette
                    .colors()
                    .iter()
                    .enumerate()
                    .skip(1)
                    .map(|(i, c)| Json::obj().with("index", i).with("color", c.to_hex()))
                    .collect(),
            ),
        )),
        "animation_info" => animation_info(doc, a),
        "history_list" => history_list(history, a),
        "render_frame" => render_frame(doc, a),
        "canvas_snapshot" => snapshot(doc, revision, a),
        "export_png" => export_png(doc, a),
        "export_gif" => export_gif(doc, a),
        "export_spritesheet" => export_sheet(doc, a),
        "project_save" => {
            let log: Vec<_> = history.log_entries().cloned().collect();
            let bytes = storage::save(doc, &log);
            Ok(Query { result: Json::obj().with("format", "pxs").with("bytes", bytes.len()), binary: bytes })
        }
        _ => Err(CmdError::new("unknown_op", format!("unknown operation '{op}'"))),
    }
}

fn inspect(doc: &Document, history: &History, revision: u64, detailed: bool) -> Json {
    let active_frame = doc.frame_index(doc.active_frame).unwrap_or(0);
    let active_layer = doc.layer_index(doc.active_layer).unwrap_or(0);
    let layers = doc
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let mut j = Json::obj()
                .with("index", i)
                .with("id", l.id)
                .with("name", l.name.as_str())
                .with("visible", l.visible)
                .with("locked", l.locked)
                .with("opacity", l.opacity)
                .with("blend", l.blend.as_str());
            if detailed {
                let (n, _) = doc.cel_stats(l.id, doc.active_frame);
                j.set("pixelsInActiveFrame", n);
            }
            j
        })
        .collect();
    let frames =
        doc.frames.iter().enumerate().map(|(i, f)| Json::obj().with("index", i).with("id", f.id).with("duration", f.duration_ms)).collect();
    let durations: Vec<u32> = doc.frames.iter().map(|f| f.duration_ms).collect();
    let t = timing(&durations);
    let mut j = Json::obj()
        .with("name", doc.name.as_str())
        .with("width", doc.width)
        .with("height", doc.height)
        .with("revision", revision)
        .with("activeLayer", active_layer)
        .with("activeFrame", active_frame)
        .with("layers", Json::Arr(layers))
        .with("frameCount", doc.frames.len())
        .with("frames", Json::Arr(frames))
        .with("tags", Json::Arr(doc.tags.iter().map(|t| tag_json(doc, t)).collect()))
        .with("paletteSize", doc.palette.len())
        .with("totalDurationMs", t.total_ms)
        .with("averageFps", (t.average_fps * 100.0).round() / 100.0)
        .with("undoAvailable", history.can_undo())
        .with("redoAvailable", history.can_redo())
        .with("slices", Json::Arr(doc.slices.iter().map(|s| Json::from(s.name.as_str())).collect()))
        .with("tilesets", doc.tilesets.len())
        .with("tilemaps", doc.tilemaps.len());
    if detailed {
        let tiles: usize = doc.cels.values().map(|c| c.allocated_tiles()).sum();
        j.set("celCount", doc.cels.len());
        j.set("allocatedTiles", tiles);
    }
    j
}

fn tag_json(doc: &Document, t: &crate::animation::Tag) -> Json {
    Json::obj()
        .with("name", t.name.as_str())
        .with("from", doc.frame_index(t.from_id))
        .with("to", doc.frame_index(t.to_id))
        .with("direction", t.direction.as_str())
        .with("repeat", t.repeat)
}

/// Colors of a rectangle either from one layer's cel or from the composited frame.
fn read_colors(doc: &Document, a: &Json, x: i64, y: i64, w: i64, h: i64) -> CmdResult<Vec<Option<Rgba>>> {
    let fi = frame_index(doc, a, "frame")?;
    if opt_bool(a, "composite")?.unwrap_or(false) {
        let img = render(doc, fi, &RenderOptions::default());
        let mut out = Vec::with_capacity((w * h) as usize);
        for yy in y..y + h {
            for xx in x..x + w {
                let inside = xx >= 0 && yy >= 0 && xx < doc.width as i64 && yy < doc.height as i64;
                let c = if inside { img.get(xx as u32, yy as u32) } else { Rgba::TRANSPARENT };
                out.push((c.a > 0).then_some(c));
            }
        }
        return Ok(out);
    }
    let li = layer_index(doc, a, "layer")?;
    let grid = doc.cel(doc.layers[li].id, doc.frames[fi].id);
    let mut idx = Vec::with_capacity((w * h) as usize);
    for yy in y..y + h {
        for xx in x..x + w {
            idx.push(grid.map(|g| g.get(xx, yy)).unwrap_or(TRANSPARENT));
        }
    }
    Ok(idx.iter().map(|&p| if p == TRANSPARENT { None } else { Some(Rgba::from_u32(p)) }).collect())
}

fn get_pixel(doc: &Document, a: &Json) -> CmdResult<Query> {
    let (x, y) = (coord(a, "x")?, coord(a, "y")?);
    if x < 0 || y < 0 || x >= doc.width as i64 || y >= doc.height as i64 {
        return Err(CmdError::invalid(format!("pixel ({x},{y}) is outside the {}x{} canvas", doc.width, doc.height)));
    }
    let colors = read_colors(doc, a, x, y, 1, 1)?;
    q(Json::obj().with("x", x).with("y", y).with("color", colors[0].map(|c| c.to_hex())).with("transparent", colors[0].is_none()))
}

/// Returns pixels as hex rows or a compact character map with a color legend.
fn read_region(doc: &Document, a: &Json) -> CmdResult<Query> {
    let r = region(doc, a)?;
    if r.w * r.h > MAX_READ_AREA {
        return Err(CmdError::new("limit", format!("read at most {MAX_READ_AREA} pixels per call (e.g. 128x128); split the region")));
    }
    let format = opt_str(a, "format")?.unwrap_or("ascii");
    let colors = read_colors(doc, a, r.x, r.y, r.w, r.h)?;
    let mut j = Json::obj().with("x", r.x).with("y", r.y).with("width", r.w).with("height", r.h).with("format", format);
    match format {
        "hex" => {
            let rows =
                colors.chunks(r.w as usize).map(|row| Json::Arr(row.iter().map(|c| Json::from(c.map(|c| c.to_hex()))).collect())).collect();
            j.set("rows", Json::Arr(rows));
        }
        "ascii" => {
            let keys: Vec<char> = ASCII_KEYS.chars().collect();
            let mut legend: Vec<(Rgba, char)> = Vec::new();
            let mut rows = Vec::new();
            for row in colors.chunks(r.w as usize) {
                let mut s = String::with_capacity(row.len());
                for c in row {
                    match c {
                        None => s.push('.'),
                        Some(c) => {
                            let k = match legend.iter().find(|(lc, _)| lc == c) {
                                Some((_, k)) => *k,
                                None => {
                                    let k = *keys.get(legend.len()).ok_or_else(|| {
                                        CmdError::new("limit", "too many distinct colors for ascii format; use format 'hex'")
                                    })?;
                                    legend.push((*c, k));
                                    k
                                }
                            };
                            s.push(k);
                        }
                    }
                }
                rows.push(Json::Str(s));
            }
            let mut lg = Json::obj().with(".", "transparent");
            for (c, k) in legend {
                lg.set(&k.to_string(), c.to_hex());
            }
            j.set("legend", lg);
            j.set("rows", Json::Arr(rows));
        }
        other => return Err(CmdError::invalid(format!("unknown format '{other}' (ascii, hex)"))),
    }
    q(j)
}

fn frame_get(doc: &Document, a: &Json) -> CmdResult<Query> {
    let fi = frame_index(doc, a, "frame")?;
    let f = &doc.frames[fi];
    let cels = doc
        .layers
        .iter()
        .enumerate()
        .map(|(li, l)| {
            let (n, b) = doc.cel_stats(l.id, f.id);
            let bounds = b.map(|(x, y, w, h)| Json::obj().with("x", x).with("y", y).with("width", w).with("height", h));
            Json::obj().with("layer", li).with("name", l.name.as_str()).with("pixels", n).with("bounds", bounds)
        })
        .collect();
    let tags: Vec<Json> = doc
        .tags
        .iter()
        .filter(|t| {
            let (a, b) = (doc.frame_index(t.from_id).unwrap_or(0), doc.frame_index(t.to_id).unwrap_or(0));
            fi >= a && fi <= b
        })
        .map(|t| Json::from(t.name.as_str()))
        .collect();
    q(Json::obj()
        .with("index", fi)
        .with("id", f.id)
        .with("duration", f.duration_ms)
        .with("tags", Json::Arr(tags))
        .with("cels", Json::Arr(cels)))
}

/// Frame range (and direction) from `tag` or `from`/`to`, defaulting to all frames.
fn sequence(doc: &Document, a: &Json) -> CmdResult<(Vec<usize>, Option<u32>)> {
    if let Some(name) = opt_str(a, "tag")? {
        let t = doc.tags.iter().find(|t| t.name == name).ok_or_else(|| CmdError::not_found(format!("no tag named '{name}'")))?;
        let (from, to) = (doc.frame_index(t.from_id).unwrap_or(0), doc.frame_index(t.to_id).unwrap_or(0));
        return Ok((playback_order(from, to, t.direction), Some(t.repeat)));
    }
    let from = opt_i64(a, "from")?.unwrap_or(0);
    let to = opt_i64(a, "to")?.unwrap_or(doc.frames.len() as i64 - 1);
    if from < 0 || to >= doc.frames.len() as i64 || from > to {
        return Err(CmdError::invalid("invalid frame range"));
    }
    let dir = match opt_str(a, "direction")? {
        Some(d) => Direction::parse(d).ok_or_else(|| CmdError::invalid("'direction' must be forward, reverse or pingpong"))?,
        None => Direction::Forward,
    };
    Ok((playback_order(from as usize, to as usize, dir), None))
}

fn animation_info(doc: &Document, a: &Json) -> CmdResult<Query> {
    let (order, repeat) = sequence(doc, a)?;
    let durations: Vec<u32> = order.iter().map(|&i| doc.frames[i].duration_ms).collect();
    let t = timing(&durations);
    let seq = order.iter().zip(&durations).map(|(&i, &d)| Json::obj().with("frame", i).with("duration", d)).collect();
    q(Json::obj()
        .with("sequence", Json::Arr(seq))
        .with("totalMs", t.total_ms)
        .with("averageFps", (t.average_fps * 100.0).round() / 100.0)
        .with("repeat", repeat.unwrap_or(0))
        .with("tags", Json::Arr(doc.tags.iter().map(|t| tag_json(doc, t)).collect())))
}

fn history_list(history: &History, a: &Json) -> CmdResult<Query> {
    let limit = opt_i64(a, "limit")?.unwrap_or(50).clamp(1, 1000) as usize;
    let entries = history
        .log_entries()
        .rev()
        .take(limit)
        .map(|e| {
            Json::obj()
                .with("revision", e.revision)
                .with("actor", e.actor.as_str())
                .with("op", e.op.as_str())
                .with("operationId", e.operation_id.clone())
                .with("changedPixels", e.changed_pixels)
                .with("timestamp", e.timestamp_ms)
        })
        .collect();
    q(Json::obj().with("entries", Json::Arr(entries)).with("undoAvailable", history.can_undo()).with("redoAvailable", history.can_redo()))
}

fn options(doc: &Document, a: &Json) -> CmdResult<RenderOptions> {
    let mut opts = RenderOptions::default();
    if a.get("layer").is_some_and(|v| !v.is_null()) {
        opts.only_layer = Some(doc.layers[layer_index(doc, a, "layer")?].id);
    }
    if let Some(bg) = opt_str(a, "background")? {
        opts.background = Some(Rgba::parse_hex(bg).ok_or_else(|| CmdError::invalid("'background' must be a hex color"))?);
    }
    if let Some(o) = a.get("onion").filter(|o| !o.is_null()) {
        let n = |k: &str, d: i64| opt_i64(o, k).map(|v| v.unwrap_or(d));
        let (before, after, opacity) = (n("before", 1)?, n("after", 0)?, n("opacity", 96)?);
        if !(0..=8).contains(&before) || !(0..=8).contains(&after) || !(0..=255).contains(&opacity) {
            return Err(CmdError::invalid("onion: before/after 0-8, opacity 0-255"));
        }
        opts.onion = Some(OnionSkin {
            before: before as usize,
            after: after as usize,
            opacity: opacity as u8,
            tint: opt_bool(o, "tint")?.unwrap_or(true),
        });
    }
    Ok(opts)
}

fn check_output(doc: &Document, s: u32) -> CmdResult<()> {
    if doc.width * s > MAX_OUTPUT_SIDE || doc.height * s > MAX_OUTPUT_SIDE {
        return Err(CmdError::new("limit", format!("scaled output would exceed {MAX_OUTPUT_SIDE}px; lower 'scale'")));
    }
    Ok(())
}

fn render_frame(doc: &Document, a: &Json) -> CmdResult<Query> {
    let fi = frame_index(doc, a, "frame")?;
    let s = scale(a, 1)?;
    check_output(doc, s)?;
    let img = render(doc, fi, &options(doc, a)?).scaled(s);
    Ok(Query {
        result: Json::obj().with("width", img.width).with("height", img.height).with("format", "rgba8").with("frame", fi),
        binary: img.pixels,
    })
}

fn auto_scale(doc: &Document) -> u32 {
    (512 / doc.width.max(doc.height)).clamp(1, 16)
}

/// The agent's "eyes": a PNG plus a structured description of what is on the canvas.
fn snapshot(doc: &Document, revision: u64, a: &Json) -> CmdResult<Query> {
    let fi = frame_index(doc, a, "frame")?;
    let s = scale(a, auto_scale(doc))?;
    check_output(doc, s)?;
    let img = render(doc, fi, &options(doc, a)?);
    let mut used: Vec<(Rgba, u32)> = Vec::new();
    let (mut x0, mut y0, mut x1, mut y1, mut any) = (u32::MAX, u32::MAX, 0, 0, false);
    for y in 0..img.height {
        for x in 0..img.width {
            let c = img.get(x, y);
            if c.a == 0 {
                continue;
            }
            any = true;
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            match used.iter_mut().find(|(u, _)| *u == c) {
                Some((_, n)) => *n += 1,
                None => used.push((c, 1)),
            }
        }
    }
    used.sort_by_key(|u| std::cmp::Reverse(u.1));
    let colors = used.iter().take(64).map(|(c, n)| Json::obj().with("color", c.to_hex()).with("pixels", *n)).collect();
    let bounds = any.then(|| Json::obj().with("x", x0).with("y", y0).with("width", x1 - x0 + 1).with("height", y1 - y0 + 1));
    let png = export::png::encode(&img.scaled(s));
    let result = Json::obj()
        .with(
            "canvas",
            Json::obj()
                .with("width", doc.width)
                .with("height", doc.height)
                .with("activeFrame", fi)
                .with("frameCount", doc.frames.len())
                .with("revision", revision),
        )
        .with("contentBounds", bounds)
        .with("usedColors", Json::Arr(colors))
        .with("distinctColors", used.len())
        .with("imageScale", s)
        .with("mimeType", "image/png");
    Ok(Query { result, binary: png })
}

fn export_png(doc: &Document, a: &Json) -> CmdResult<Query> {
    let fi = frame_index(doc, a, "frame")?;
    let s = scale(a, 1)?;
    check_output(doc, s)?;
    let img = render(doc, fi, &options(doc, a)?).scaled(s);
    let png = export::png::encode(&img);
    Ok(Query {
        result: Json::obj().with("width", img.width).with("height", img.height).with("mimeType", "image/png").with("bytes", png.len()),
        binary: png,
    })
}

fn rendered_sequence(doc: &Document, a: &Json) -> CmdResult<(Vec<usize>, Vec<Image>, Option<u32>)> {
    let (order, repeat) = sequence(doc, a)?;
    let s = scale(a, 1)?;
    check_output(doc, s)?;
    let mut opts = options(doc, a)?;
    opts.onion = None;
    let images = order.iter().map(|&i| render(doc, i, &opts).scaled(s)).collect();
    Ok((order, images, repeat))
}

fn export_gif(doc: &Document, a: &Json) -> CmdResult<Query> {
    let (order, images, repeat) = rendered_sequence(doc, a)?;
    let frames: Vec<GifFrame> =
        order.iter().zip(&images).map(|(&i, img)| GifFrame { image: img, duration_ms: doc.frames[i].duration_ms }).collect();
    let (w, h) = (images[0].width, images[0].height);
    let loops = opt_i64(a, "loop")?.or(repeat.map(|r| r as i64)).unwrap_or(0).clamp(0, 65535) as u16;
    let gif = export::gif::encode(w, h, &frames, loops);
    Ok(Query {
        result: Json::obj()
            .with("width", w)
            .with("height", h)
            .with("frames", frames.len())
            .with("mimeType", "image/gif")
            .with("bytes", gif.len()),
        binary: gif,
    })
}

fn export_sheet(doc: &Document, a: &Json) -> CmdResult<Query> {
    let (order, images, _) = rendered_sequence(doc, a)?;
    let columns = opt_i64(a, "columns")?.unwrap_or(0).clamp(0, 10_000) as usize;
    let padding = opt_i64(a, "padding")?.unwrap_or(0).clamp(0, 64) as u32;
    let frames: Vec<SheetFrame> =
        order.iter().zip(&images).map(|(&i, img)| SheetFrame { index: i, image: img, duration_ms: doc.frames[i].duration_ms }).collect();
    let sheet = export::sprite_sheet(&frames, columns, padding).map_err(|e| CmdError::new("limit", e))?;
    let png = export::png::encode(&sheet.image);
    let meta = sheet
        .metadata
        .with("tags", Json::Arr(doc.tags.iter().map(|t| tag_json(doc, t)).collect()))
        .with("generator", "Pixel Studio")
        .with("mimeType", "image/png")
        .with("bytes", png.len());
    Ok(Query { result: meta, binary: png })
}
