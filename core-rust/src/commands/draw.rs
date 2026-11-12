//! Pixel-level drawing commands.

use super::args::{color, color_value, coord, frame_index, layer_index, opt_bool, opt_i64, opt_str, region, req_i64, target};
use super::json::Json;
use super::{CmdError, CmdResult, Outcome};
use crate::pixel::TRANSPARENT;
use crate::project::Document;
use crate::raster::{self, PasteMode, Region};

pub const MAX_BATCH_PIXELS: usize = 262_144;

fn hex_of(doc: &Document, index: u16) -> Json {
    match doc.palette.get(index) {
        Some(c) if index != TRANSPARENT => Json::Str(c.to_hex()),
        _ => Json::Null,
    }
}

pub fn set_pixel(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let (x, y) = (coord(a, "x")?, coord(a, "y")?);
    let (l, f) = target(doc, a)?;
    let v = color(doc, a, "color")?;
    if x < 0 || y < 0 || x >= doc.width as i64 || y >= doc.height as i64 {
        return Err(CmdError::invalid(format!("pixel ({x},{y}) is outside the {}x{} canvas", doc.width, doc.height)));
    }
    let changed = doc.cel_mut(l, f).set(x, y, v) as u64;
    Ok(Outcome::new(
        Json::obj().with("x", x).with("y", y).with("color", hex_of(doc, v)).with("paletteIndex", v).with("changed", changed),
        changed,
    ))
}

pub fn erase_pixel(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let (x, y) = (coord(a, "x")?, coord(a, "y")?);
    let (l, f) = target(doc, a)?;
    let changed = doc.cel_mut(l, f).set(x, y, TRANSPARENT) as u64;
    Ok(Outcome::new(Json::obj().with("x", x).with("y", y).with("changed", changed), changed))
}

/// Batch of explicit pixels: `[{"x":1,"y":2,"color":"#fff"}, [3,4,"#000"], ...]`.
/// Every pixel is still an individual coordinate+color, so the operation stays auditable.
pub fn set_pixels(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let items = a.get("pixels").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'pixels' must be an array"))?;
    if items.len() > MAX_BATCH_PIXELS {
        return Err(CmdError::new("limit", format!("at most {MAX_BATCH_PIXELS} pixels per call")));
    }
    let (l, f) = target(doc, a)?;
    let default = match a.get("color") {
        Some(v) => Some(color_value(doc, &v.clone(), "color")?),
        None => None,
    };
    let mut resolved = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let (x, y, c) = match item {
            Json::Arr(p) if p.len() == 2 || p.len() == 3 => (p[0].as_i64(), p[1].as_i64(), p.get(2).cloned()),
            Json::Obj(_) => (item.get("x").and_then(Json::as_i64), item.get("y").and_then(Json::as_i64), item.get("color").cloned()),
            _ => (None, None, None),
        };
        let (Some(x), Some(y)) = (x, y) else {
            return Err(CmdError::invalid(format!("pixels[{i}] needs integer x and y")));
        };
        let v = match c {
            Some(c) => color_value(doc, &c, &format!("pixels[{i}].color"))?,
            None => default.ok_or_else(|| CmdError::invalid(format!("pixels[{i}] has no color and no default 'color' was given")))?,
        };
        resolved.push((x, y, v));
    }
    let grid = doc.cel_mut(l, f);
    let (mut changed, mut clipped) = (0u64, 0u64);
    for (x, y, v) in resolved {
        if !grid.in_bounds(x, y) {
            clipped += 1;
            continue;
        }
        changed += grid.set(x, y, v) as u64;
    }
    Ok(Outcome::new(Json::obj().with("requested", items.len()).with("changed", changed).with("outOfBounds", clipped), changed))
}

pub fn draw_line(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let (x0, y0, x1, y1) = (coord(a, "x0")?, coord(a, "y0")?, coord(a, "x1")?, coord(a, "y1")?);
    let (l, f) = target(doc, a)?;
    let v = color(doc, a, "color")?;
    let changed = raster::line(doc.cel_mut(l, f), x0, y0, x1, y1, v) as u64;
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

fn shape(doc: &mut Document, a: &Json, ellipse: bool) -> CmdResult<Outcome> {
    let r = Region { x: coord(a, "x")?, y: coord(a, "y")?, w: coord(a, "width")?, h: coord(a, "height")? };
    if r.w <= 0 || r.h <= 0 {
        return Err(CmdError::invalid("width and height must be positive"));
    }
    let filled = opt_bool(a, "filled")?.unwrap_or(false);
    let (l, f) = target(doc, a)?;
    let v = color(doc, a, "color")?;
    let grid = doc.cel_mut(l, f);
    let changed = if ellipse { raster::ellipse(grid, r, v, filled) } else { raster::rectangle(grid, r, v, filled) } as u64;
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

pub fn draw_rectangle(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    shape(doc, a, false)
}

pub fn draw_ellipse(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    shape(doc, a, true)
}

pub fn fill_region(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let (x, y) = (coord(a, "x")?, coord(a, "y")?);
    let contiguous = opt_bool(a, "contiguous")?.unwrap_or(true);
    let (l, f) = target(doc, a)?;
    let v = color(doc, a, "color")?;
    let changed = raster::fill(doc.cel_mut(l, f), x, y, v, contiguous) as u64;
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

/// Replaces one color with another in a cel, a whole layer, a whole frame or everywhere.
pub fn replace_color(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let from = color(doc, a, "from")?;
    let to = color(doc, a, "to")?;
    let scope = opt_str(a, "scope")?.unwrap_or("cel");
    let li = layer_index(doc, a, "layer")?;
    let fi = frame_index(doc, a, "frame")?;
    let cels: Vec<(u32, u32)> = match scope {
        "cel" => vec![(doc.layers[li].id, doc.frames[fi].id)],
        "layer" => doc.frames.iter().map(|f| (doc.layers[li].id, f.id)).collect(),
        "frame" => doc.layers.iter().map(|l| (l.id, doc.frames[fi].id)).collect(),
        "all" => doc.layers.iter().flat_map(|l| doc.frames.iter().map(move |f| (l.id, f.id))).collect(),
        other => return Err(CmdError::invalid(format!("unknown scope '{other}' (cel, layer, frame, all)"))),
    };
    let mut changed = 0u64;
    for (l, f) in cels {
        let locked = doc.layers.iter().any(|x| x.id == l && x.locked);
        if locked || doc.cel(l, f).is_none() {
            continue;
        }
        let grid = doc.cel_mut(l, f);
        let full = Region::full(grid);
        changed += raster::replace(grid, full, from, to) as u64;
    }
    Ok(Outcome::new(Json::obj().with("changed", changed).with("scope", scope), changed))
}

pub fn flip(doc: &mut Document, a: &Json, horizontal: bool) -> CmdResult<Outcome> {
    let r = region(doc, a)?;
    let (l, f) = target(doc, a)?;
    let grid = doc.cel_mut(l, f);
    let changed = if horizontal { raster::flip_horizontal(grid, r) } else { raster::flip_vertical(grid, r) } as u64;
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

pub fn rotate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let degrees = req_i64(a, "degrees")?;
    let r = region(doc, a)?;
    let (l, f) = target(doc, a)?;
    let changed = raster::rotate(doc.cel_mut(l, f), r, degrees)? as u64;
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

fn paste_mode(a: &Json) -> CmdResult<PasteMode> {
    match opt_str(a, "mode")?.unwrap_or("over") {
        "over" => Ok(PasteMode::Over),
        "replace" => Ok(PasteMode::Replace),
        other => Err(CmdError::invalid(format!("unknown mode '{other}' (over, replace)"))),
    }
}

pub fn move_region(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let r = region(doc, a)?;
    let (dx, dy) = (coord(a, "dx")?, coord(a, "dy")?);
    let mode = paste_mode(a)?;
    let (l, f) = target(doc, a)?;
    let grid = doc.cel_mut(l, f);
    let data = raster::extract(grid, r);
    let mut changed = raster::clear_region(grid, r) as u64;
    changed += raster::paste(grid, r.x + dx, r.y + dy, r.w, r.h, &data, mode) as u64;
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

/// Copies a region, optionally between layers and frames — the workhorse for building
/// animation frames from a previous pose.
pub fn copy_region(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let r = region(doc, a)?;
    let src_l = doc.layers[layer_index(doc, a, "layer")?].id;
    let src_f = doc.frames[frame_index(doc, a, "frame")?].id;
    let dst_li = match a.get("toLayer") {
        Some(_) => layer_index(doc, a, "toLayer")?,
        None => doc.layer_index(src_l).unwrap(),
    };
    let dst_fi = match a.get("toFrame") {
        Some(_) => frame_index(doc, a, "toFrame")?,
        None => doc.frame_index(src_f).unwrap(),
    };
    let dst_layer = &doc.layers[dst_li];
    if dst_layer.locked {
        return Err(CmdError::new("locked", format!("layer '{}' is locked", dst_layer.name)));
    }
    let (dst_l, dst_f) = (dst_layer.id, doc.frames[dst_fi].id);
    let to_x = opt_i64(a, "toX")?.unwrap_or(r.x);
    let to_y = opt_i64(a, "toY")?.unwrap_or(r.y);
    let mode = paste_mode(a)?;
    let data = match doc.cel(src_l, src_f) {
        Some(g) => raster::extract(g, r),
        None => vec![TRANSPARENT; (r.w * r.h) as usize],
    };
    let changed = raster::paste(doc.cel_mut(dst_l, dst_f), to_x, to_y, r.w, r.h, &data, mode) as u64;
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

pub fn clear(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let r = region(doc, a)?;
    let (l, f) = target(doc, a)?;
    let grid = doc.cel_mut(l, f);
    let changed = raster::clear_region(grid, r) as u64;
    grid.compact();
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}
