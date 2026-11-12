//! Layers, frames, animation tags, palette and canvas structure.

use super::args::{color_value, frame_index, layer_index, opt_bool, opt_i64, opt_str, req_i64, req_str};
use super::json::Json;
use super::{CmdError, CmdResult, Outcome};
use crate::animation::{DEFAULT_FRAME_MS, Direction, Tag, validate_duration};
use crate::layers::{MAX_LAYER_NAME, validate_name};
use crate::pixel::{Rgba, TRANSPARENT};
use crate::project::{Document, MAX_FRAMES, MAX_LAYERS, validate_size};

fn ok(result: Json) -> CmdResult<Outcome> {
    Ok(Outcome::new(result, 0))
}

fn layer_json(doc: &Document, i: usize) -> Json {
    let l = &doc.layers[i];
    Json::obj().with("index", i).with("id", l.id).with("name", l.name.as_str())
}

pub fn layer_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    if doc.layers.len() >= MAX_LAYERS {
        return Err(CmdError::new("limit", format!("at most {MAX_LAYERS} layers")));
    }
    let name = match opt_str(a, "name")? {
        Some(n) => validate_name(n)?,
        None => format!("Layer {}", doc.next_layer_id),
    };
    let active = doc.layer_index(doc.active_layer).map(|i| i + 1).unwrap_or(doc.layers.len());
    let index = match opt_i64(a, "index")? {
        Some(i) if i < 0 || i as usize > doc.layers.len() => return Err(CmdError::invalid("layer index out of range")),
        Some(i) => i as usize,
        None => active,
    };
    let id = doc.insert_layer(&name, index);
    doc.active_layer = id;
    ok(layer_json(doc, index))
}

pub fn layer_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = layer_index(doc, a, "layer")?;
    if doc.layers.len() == 1 {
        return Err(CmdError::invalid("cannot delete the last layer"));
    }
    let removed = doc.remove_layer(i);
    ok(Json::obj().with("deleted", removed.name).with("id", removed.id))
}

pub fn layer_rename(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = layer_index(doc, a, "layer")?;
    let name = validate_name(req_str(a, "name")?)?;
    doc.layers[i].name = name;
    ok(layer_json(doc, i))
}

pub fn layer_visibility(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = layer_index(doc, a, "layer")?;
    let visible = opt_bool(a, "visible")?.ok_or_else(|| CmdError::invalid("missing required boolean 'visible'"))?;
    doc.layers[i].visible = visible;
    ok(layer_json(doc, i).with("visible", visible))
}

pub fn layer_lock(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = layer_index(doc, a, "layer")?;
    let locked = opt_bool(a, "locked")?.ok_or_else(|| CmdError::invalid("missing required boolean 'locked'"))?;
    doc.layers[i].locked = locked;
    ok(layer_json(doc, i).with("locked", locked))
}

pub fn layer_opacity(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = layer_index(doc, a, "layer")?;
    let o = req_i64(a, "opacity")?;
    if !(0..=255).contains(&o) {
        return Err(CmdError::invalid("'opacity' must be 0-255"));
    }
    doc.layers[i].opacity = o as u8;
    ok(layer_json(doc, i).with("opacity", o))
}

pub fn layer_move(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = layer_index(doc, a, "layer")?;
    let to = req_i64(a, "index")?;
    if to < 0 || to as usize >= doc.layers.len() {
        return Err(CmdError::invalid("target index out of range"));
    }
    let layer = doc.layers.remove(i);
    doc.layers.insert(to as usize, layer);
    ok(layer_json(doc, to as usize))
}

pub fn layer_select(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = layer_index(doc, a, "layer")?;
    doc.active_layer = doc.layers[i].id;
    ok(layer_json(doc, i))
}

fn frame_json(doc: &Document, i: usize) -> Json {
    let f = &doc.frames[i];
    Json::obj().with("index", i).with("id", f.id).with("duration", f.duration_ms)
}

fn duration_arg(a: &Json, default: u32) -> CmdResult<u32> {
    match opt_i64(a, "duration")? {
        Some(ms) => Ok(validate_duration(ms)?),
        None => Ok(default),
    }
}

pub fn frame_add(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let count = opt_i64(a, "count")?.unwrap_or(1);
    if !(1..=10_000).contains(&count) || doc.frames.len() + count as usize > MAX_FRAMES {
        return Err(CmdError::new("limit", "'count' must be 1-10000 and stay within the frame safety limit"));
    }
    let active = doc.frame_index(doc.active_frame).unwrap_or(0);
    let duration = duration_arg(a, doc.frames.get(active).map(|f| f.duration_ms).unwrap_or(DEFAULT_FRAME_MS))?;
    let index = match opt_i64(a, "index")? {
        Some(i) if i < 0 || i as usize > doc.frames.len() => return Err(CmdError::invalid("frame index out of range")),
        Some(i) => i as usize,
        None => doc.frames.len(),
    };
    let mut added = Vec::new();
    for k in 0..count as usize {
        doc.insert_frame(index + k, duration);
        added.push(frame_json(doc, index + k));
    }
    doc.active_frame = doc.frames[index].id;
    ok(Json::obj().with("added", Json::Arr(added)).with("frameCount", doc.frames.len()))
}

pub fn frame_duplicate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    if doc.frames.len() >= MAX_FRAMES {
        return Err(CmdError::new("limit", "frame safety limit reached"));
    }
    let i = frame_index(doc, a, "frame")?;
    let to = match opt_i64(a, "index")? {
        Some(t) if t < 0 || t as usize > doc.frames.len() => return Err(CmdError::invalid("target index out of range")),
        Some(t) => t as usize,
        None => i + 1,
    };
    let id = doc.duplicate_frame(i, to);
    doc.active_frame = id;
    ok(frame_json(doc, to).with("source", i).with("frameCount", doc.frames.len()))
}

pub fn frame_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    if a.get("frame").is_none_or(Json::is_null) {
        return Err(CmdError::invalid("'frame' is required for frame_delete"));
    }
    let i = frame_index(doc, a, "frame")?;
    if doc.frames.len() == 1 {
        return Err(CmdError::invalid("cannot delete the last frame"));
    }
    let removed = doc.remove_frame(i);
    ok(Json::obj().with("deleted", i).with("id", removed.id).with("frameCount", doc.frames.len()))
}

pub fn frame_move(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = frame_index(doc, a, "frame")?;
    let to = req_i64(a, "index")?;
    if to < 0 || to as usize >= doc.frames.len() {
        return Err(CmdError::invalid("target index out of range"));
    }
    let f = doc.frames.remove(i);
    doc.frames.insert(to as usize, f);
    ok(frame_json(doc, to as usize))
}

/// `frame` may be an index, omitted (active frame) or the string "all".
pub fn frame_set_duration(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let ms = validate_duration(req_i64(a, "duration")?)?;
    if a.get("frame").and_then(Json::as_str) == Some("all") {
        doc.frames.iter_mut().for_each(|f| f.duration_ms = ms);
        return ok(Json::obj().with("frames", doc.frames.len()).with("duration", ms));
    }
    let i = frame_index(doc, a, "frame")?;
    doc.frames[i].duration_ms = ms;
    ok(frame_json(doc, i))
}

pub fn frame_select(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = frame_index(doc, a, "frame")?;
    doc.active_frame = doc.frames[i].id;
    ok(frame_json(doc, i))
}

fn direction_arg(a: &Json) -> CmdResult<Option<Direction>> {
    match opt_str(a, "direction")? {
        None => Ok(None),
        Some(s) => Direction::parse(s).map(Some).ok_or_else(|| CmdError::invalid("'direction' must be forward, reverse or pingpong")),
    }
}

fn repeat_arg(a: &Json) -> CmdResult<Option<u32>> {
    match opt_i64(a, "repeat")? {
        None => Ok(None),
        Some(r) if (0..=65_535).contains(&r) => Ok(Some(r as u32)),
        Some(_) => Err(CmdError::invalid("'repeat' must be 0 (forever) to 65535")),
    }
}

pub fn tag_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = req_str(a, "name")?.trim().to_string();
    if name.is_empty() || name.chars().count() > MAX_LAYER_NAME {
        return Err(CmdError::invalid("tag name must be 1-128 characters"));
    }
    if doc.tags.iter().any(|t| t.name == name) {
        return Err(CmdError::invalid(format!("tag '{name}' already exists")));
    }
    let from = frame_index(doc, a, "from")?;
    let to = frame_index(doc, a, "to")?;
    if from > to {
        return Err(CmdError::invalid("'from' must not be after 'to'"));
    }
    let tag = Tag {
        name: name.clone(),
        from_id: doc.frames[from].id,
        to_id: doc.frames[to].id,
        direction: direction_arg(a)?.unwrap_or(Direction::Forward),
        repeat: repeat_arg(a)?.unwrap_or(0),
    };
    let dir = tag.direction.as_str();
    doc.tags.push(tag);
    ok(Json::obj().with("name", name).with("from", from).with("to", to).with("direction", dir))
}

pub fn tag_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = req_str(a, "name")?;
    let before = doc.tags.len();
    doc.tags.retain(|t| t.name != name);
    if doc.tags.len() == before {
        return Err(CmdError::not_found(format!("no tag named '{name}'")));
    }
    ok(Json::obj().with("deleted", name))
}

/// Sets direction/repeat of a tag.
pub fn set_loop(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = req_str(a, "tag")?;
    let direction = direction_arg(a)?;
    let repeat = repeat_arg(a)?;
    let tag = doc.tags.iter_mut().find(|t| t.name == name).ok_or_else(|| CmdError::not_found(format!("no tag named '{name}'")))?;
    if let Some(d) = direction {
        tag.direction = d;
    }
    if let Some(r) = repeat {
        tag.repeat = r;
    }
    ok(Json::obj().with("tag", name).with("direction", tag.direction.as_str()).with("repeat", tag.repeat))
}

pub fn palette_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let index = req_i64(a, "index")?;
    let hex = req_str(a, "color")?;
    let c = Rgba::parse_hex(hex).ok_or_else(|| CmdError::invalid(format!("'{hex}' is not a color")))?;
    if index < 0 {
        return Err(CmdError::invalid("swatch index out of range"));
    }
    let old = doc.palette.set(index as usize, c).map_err(CmdError::invalid)?;
    // Swatch edits recolor the art that uses the old color (classic palette-swap workflow).
    let (from, to) = (old.to_pixel(), c.to_pixel());
    let mut changed = 0u64;
    let locked: Vec<u32> = doc.layers.iter().filter(|l| l.locked).map(|l| l.id).collect();
    for ((layer, _), cel) in doc.cels.iter_mut() {
        if from == TRANSPARENT || locked.contains(layer) {
            continue;
        }
        let grid = std::sync::Arc::make_mut(cel);
        let full = crate::raster::Region::full(grid);
        changed += crate::raster::replace(grid, full, from, to) as u64;
    }
    Ok(Outcome::new(
        Json::obj().with("index", index).with("color", c.to_hex()).with("previous", old.to_hex()).with("recolored", changed),
        changed,
    ))
}

pub fn palette_add(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let colors = a.get("colors").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'colors' must be an array of hex strings"))?;
    let mut indices = Vec::new();
    for (i, c) in colors.iter().enumerate() {
        if !matches!(c, Json::Str(_)) {
            return Err(CmdError::invalid(format!("colors[{i}] must be a hex string")));
        }
        let px = color_value(doc, c, &format!("colors[{i}]"))?;
        let idx = doc.palette.intern(Rgba::from_u32(px)).map_err(|e| CmdError::new("limit", e))?;
        indices.push(Json::from(idx));
    }
    ok(Json::obj().with("indices", Json::Arr(indices)).with("paletteSize", doc.palette.len()))
}

pub fn canvas_resize(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let (w, h) = validate_size(req_i64(a, "width")?, req_i64(a, "height")?)?;
    let ox = opt_i64(a, "offsetX")?.unwrap_or(0);
    let oy = opt_i64(a, "offsetY")?.unwrap_or(0);
    if ox.abs() > 8192 || oy.abs() > 8192 {
        return Err(CmdError::invalid("offset out of range"));
    }
    doc.resize(w, h, ox, oy);
    ok(Json::obj().with("width", w).with("height", h))
}

pub fn project_rename(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = req_str(a, "name")?.trim();
    if name.is_empty() || name.chars().count() > 256 || name.chars().any(char::is_control) {
        return Err(CmdError::invalid("project name must be 1-256 printable characters"));
    }
    doc.name = name.to_string();
    ok(Json::obj().with("name", name))
}
