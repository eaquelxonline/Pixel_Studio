//! Argument decoding shared by all commands. Every accessor validates and produces a
//! precise error message an AI agent can act on.

use super::json::Json;
use super::{CmdError, CmdResult};
use crate::pixel::{Rgba, TRANSPARENT};
use crate::project::Document;
use crate::raster::Region;

pub fn opt_i64(a: &Json, key: &str) -> CmdResult<Option<i64>> {
    match a.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => v.as_i64().map(Some).ok_or_else(|| CmdError::invalid(format!("'{key}' must be an integer"))),
    }
}

pub fn req_i64(a: &Json, key: &str) -> CmdResult<i64> {
    opt_i64(a, key)?.ok_or_else(|| CmdError::invalid(format!("missing required integer '{key}'")))
}

/// Integer coordinates are bounded so arithmetic can never overflow.
pub fn coord(a: &Json, key: &str) -> CmdResult<i64> {
    let v = req_i64(a, key)?;
    if v.abs() > 1_000_000 {
        return Err(CmdError::invalid(format!("'{key}' is out of range")));
    }
    Ok(v)
}

pub fn opt_bool(a: &Json, key: &str) -> CmdResult<Option<bool>> {
    match a.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| CmdError::invalid(format!("'{key}' must be a boolean"))),
    }
}

pub fn opt_str<'a>(a: &'a Json, key: &str) -> CmdResult<Option<&'a str>> {
    match a.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or_else(|| CmdError::invalid(format!("'{key}' must be a string"))),
    }
}

pub fn req_str<'a>(a: &'a Json, key: &str) -> CmdResult<&'a str> {
    opt_str(a, key)?.ok_or_else(|| CmdError::invalid(format!("missing required string '{key}'")))
}

/// Layer by index (0 = bottom) or by name; defaults to the active layer.
pub fn layer_index(doc: &Document, a: &Json, key: &str) -> CmdResult<usize> {
    match a.get(key) {
        None | Some(Json::Null) => doc.layer_index(doc.active_layer).ok_or_else(|| CmdError::not_found("no active layer")),
        Some(Json::Str(name)) => doc
            .layers
            .iter()
            .position(|l| l.name == *name)
            .or_else(|| doc.layers.iter().position(|l| l.name.eq_ignore_ascii_case(name)))
            // Function-calling adapters that only support string parameters send indices as text.
            .or_else(|| name.trim().parse::<usize>().ok().filter(|i| *i < doc.layers.len()))
            .ok_or_else(|| CmdError::not_found(format!("no layer named '{name}'"))),
        Some(v) => {
            let i = v.as_i64().ok_or_else(|| CmdError::invalid(format!("'{key}' must be a layer index or name")))?;
            if i < 0 || i as usize >= doc.layers.len() {
                return Err(CmdError::not_found(format!("layer index {i} out of range (0..{})", doc.layers.len())));
            }
            Ok(i as usize)
        }
    }
}

/// Frame by 0-based index; defaults to the active frame.
pub fn frame_index(doc: &Document, a: &Json, key: &str) -> CmdResult<usize> {
    match opt_i64(a, key)? {
        None => doc.frame_index(doc.active_frame).ok_or_else(|| CmdError::not_found("no active frame")),
        Some(i) if i < 0 || i as usize >= doc.frames.len() => {
            Err(CmdError::not_found(format!("frame index {i} out of range (0..{})", doc.frames.len())))
        }
        Some(i) => Ok(i as usize),
    }
}

/// Color as `#RRGGBB[AA]`/`transparent` (interned into the palette) or a palette index.
pub fn color_value(doc: &mut Document, v: &Json, key: &str) -> CmdResult<u16> {
    match v {
        Json::Str(s) => {
            let c = Rgba::parse_hex(s).ok_or_else(|| CmdError::invalid(format!("'{key}': '{s}' is not a #RRGGBB or #RRGGBBAA color")))?;
            doc.palette.intern(c).map_err(|e| CmdError::new("limit", e))
        }
        Json::Null => Ok(TRANSPARENT),
        v => {
            let i = v.as_i64().ok_or_else(|| CmdError::invalid(format!("'{key}' must be a hex color string or palette index")))?;
            if i < 0 || i as usize >= doc.palette.len() {
                return Err(CmdError::not_found(format!("palette index {i} does not exist (palette has {} entries)", doc.palette.len())));
            }
            Ok(i as u16)
        }
    }
}

pub fn color(doc: &mut Document, a: &Json, key: &str) -> CmdResult<u16> {
    let v = a.get(key).ok_or_else(|| CmdError::invalid(format!("missing required color '{key}'")))?.clone();
    color_value(doc, &v, key)
}

/// Optional x/y/width/height; defaults to the whole canvas.
pub fn region(doc: &Document, a: &Json) -> CmdResult<Region> {
    let x = opt_i64(a, "x")?.unwrap_or(0);
    let y = opt_i64(a, "y")?.unwrap_or(0);
    let w = opt_i64(a, "width")?.unwrap_or(doc.width as i64 - x);
    let h = opt_i64(a, "height")?.unwrap_or(doc.height as i64 - y);
    if w <= 0 || h <= 0 {
        return Err(CmdError::invalid("region width and height must be positive"));
    }
    for v in [x, y, w, h] {
        if v.abs() > 1_000_000 {
            return Err(CmdError::invalid("region is out of range"));
        }
    }
    Ok(Region { x, y, w, h })
}

pub fn scale(a: &Json, default: u32) -> CmdResult<u32> {
    match opt_i64(a, "scale")? {
        None => Ok(default),
        Some(s) if (1..=64).contains(&s) => Ok(s as u32),
        Some(_) => Err(CmdError::invalid("'scale' must be between 1 and 64")),
    }
}

/// Resolves the target cel for drawing and refuses locked layers.
pub fn target(doc: &Document, a: &Json) -> CmdResult<(u32, u32)> {
    let li = layer_index(doc, a, "layer")?;
    let fi = frame_index(doc, a, "frame")?;
    let layer = &doc.layers[li];
    if layer.locked {
        return Err(CmdError::new("locked", format!("layer '{}' is locked", layer.name)));
    }
    Ok((layer.id, doc.frames[fi].id))
}
