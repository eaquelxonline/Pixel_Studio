//! Pixel-art craft commands: ASCII drawing, selective outlines, hue-shifted ramps, palette
//! library, quantization with dithering, dither fills and sprite normal maps.

use std::sync::Arc;

use super::args::{color, color_value, frame_index, layer_index, opt_bool, opt_f64, opt_i64, opt_str, region, scale, target};
use super::json::Json;
use super::{CmdError, CmdResult, Outcome, Query};
use crate::export;
use crate::pixel::{Palette, PixelGrid, Rgba, TRANSPARENT};
use crate::pixelart::{self, Dither, palettes::PALETTES};
use crate::project::Document;
use crate::renderer::{RenderOptions, render};

/// Draws ASCII art: one string per row, a legend maps characters to colors. The format
/// matches `read_region` (ascii), so agents can read, edit and write sprites as text.
pub fn draw_ascii(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let rows = a.get("rows").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'rows' must be an array of strings"))?;
    let legend = match a.get("legend") {
        Some(Json::Obj(entries)) => entries,
        _ => return Err(CmdError::invalid("'legend' must map single characters to colors, e.g. {\"o\": \"#1B1B2F\"}")),
    };
    let mut map: Vec<(char, u32)> = Vec::new();
    for (k, v) in legend {
        let mut chars = k.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return Err(CmdError::invalid(format!("legend key '{k}' must be one character")));
        };
        map.push((ch, color_value(doc, v, &format!("legend.{k}"))?));
    }
    let skip = opt_str(a, "transparent")?.unwrap_or(".").chars().next().unwrap_or('.');
    let erase = opt_bool(a, "erase_transparent")?.unwrap_or(false);
    let (x0, y0) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
    let flip = opt_bool(a, "flip_x")?.unwrap_or(false);
    let (l, f) = target(doc, a)?;
    let mut writes = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        let s = row.as_str().ok_or_else(|| CmdError::invalid(format!("rows[{y}] must be a string")))?;
        let chars: Vec<char> = s.chars().collect();
        let w = chars.len();
        for (x, ch) in chars.into_iter().enumerate() {
            let xx = if flip { w - 1 - x } else { x };
            if ch == skip || ch == ' ' {
                if erase {
                    writes.push((x0 + xx as i64, y0 + y as i64, TRANSPARENT));
                }
                continue;
            }
            let v = map
                .iter()
                .find(|(c, _)| *c == ch)
                .map(|(_, v)| *v)
                .ok_or_else(|| CmdError::invalid(format!("character '{ch}' in rows[{y}] is not in the legend")))?;
            writes.push((x0 + xx as i64, y0 + y as i64, v));
        }
    }
    let grid = doc.cel_mut(l, f);
    let mut changed = 0u64;
    for (x, y, v) in writes {
        changed += grid.set(x, y, v) as u64;
    }
    Ok(Outcome::new(Json::obj().with("changed", changed).with("rows", rows.len()), changed))
}

fn with_cel_image(doc: &mut Document, a: &Json, f: impl FnOnce(&mut crate::renderer::Image)) -> CmdResult<u64> {
    let (l, fr) = target(doc, a)?;
    let grid = doc.cel_mut(l, fr);
    let mut img = pixelart::grid_to_image(grid);
    f(&mut img);
    Ok(pixelart::image_to_grid(&img, grid) as u64)
}

/// 1-px outline around a layer's pixels: `color` or selective (darker, cooler bordering color).
pub fn outline(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let col = match a.get("color") {
        None | Some(Json::Null) => None,
        Some(Json::Str(s)) if s == "selective" => None,
        Some(v) => Some(Rgba::from_u32(color_value(doc, v, "color")?)),
    };
    let corners = opt_bool(a, "corners")?.unwrap_or(false);
    let changed = with_cel_image(doc, a, |img| pixelart::outline_image(img, col, corners))?;
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

/// Builds a hue-shifted ramp and (by default) adds it to the swatches.
pub fn color_ramp(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let base = Rgba::from_u32(color(doc, a, "base")?);
    let steps = opt_i64(a, "steps")?.unwrap_or(5).clamp(2, 16) as usize;
    let hue = opt_f64(a, "hue_shift")?.unwrap_or(18.0).clamp(0.0, 90.0) as f32;
    let contrast = opt_f64(a, "contrast")?.unwrap_or(0.75).clamp(0.05, 1.0) as f32;
    let colors = pixelart::ramp(base, steps, hue, contrast);
    if opt_bool(a, "add_to_palette")?.unwrap_or(true) {
        for c in &colors {
            doc.palette.intern(*c).map_err(|e| CmdError::new("limit", e))?;
        }
    }
    Ok(Outcome::new(Json::obj().with("ramp", Json::Arr(colors.iter().map(|c| Json::from(c.to_hex())).collect())), 0))
}

pub fn palette_list() -> Query {
    let list = PALETTES.iter().map(|(n, credit, c)| Json::obj().with("name", *n).with("credit", *credit).with("colors", c.len())).collect();
    Query { result: Json::obj().with("palettes", Json::Arr(list)), binary: Vec::new() }
}

/// Loads a library palette (or explicit colors) into the swatches.
pub fn palette_load(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let colors: Vec<Rgba> = match (opt_str(a, "name")?, a.get("colors")) {
        (Some(n), _) => {
            pixelart::palette_by_name(n).ok_or_else(|| CmdError::not_found(format!("unknown palette '{n}' (see palette_list)")))?
        }
        (None, Some(Json::Arr(list))) => {
            list.iter().map(|c| color_value(doc, c, "colors").map(Rgba::from_u32)).collect::<CmdResult<_>>()?
        }
        _ => return Err(CmdError::invalid("give 'name' (library palette) or 'colors'")),
    };
    if opt_bool(a, "replace")?.unwrap_or(true) {
        let mut all = vec![Rgba::TRANSPARENT];
        all.extend(colors.iter().copied());
        doc.palette = Palette::from_colors(all).map_err(CmdError::invalid)?;
    } else {
        for c in &colors {
            doc.palette.intern(*c).map_err(|e| CmdError::new("limit", e))?;
        }
    }
    Ok(Outcome::new(Json::obj().with("loaded", colors.len()).with("paletteSize", doc.palette.len()), 0))
}

/// Resolves the target palette for quantization: a library name, "swatches", or N colors.
pub fn quantize_palette(doc: &Document, a: &Json, sample: &[(Rgba, u32)]) -> CmdResult<Vec<Rgba>> {
    match a.get("palette") {
        Some(Json::Str(s)) if s == "swatches" => Ok(doc.palette.colors().iter().skip(1).copied().collect()),
        Some(Json::Str(s)) => pixelart::palette_by_name(s).ok_or_else(|| CmdError::not_found(format!("unknown palette '{s}'"))),
        Some(Json::Arr(list)) => list.iter().map(|c| color_value(doc, c, "palette").map(Rgba::from_u32)).collect(),
        _ => {
            let n = opt_i64(a, "colors")?.unwrap_or(16).clamp(2, 256) as usize;
            Ok(pixelart::median_cut(sample, n))
        }
    }
}

pub fn dither_arg(a: &Json) -> CmdResult<Dither> {
    match opt_str(a, "dither")? {
        None => Ok(Dither::None),
        Some(d) => Dither::parse(d).ok_or_else(|| CmdError::invalid("'dither' must be none, bayer2, bayer4 or bayer8")),
    }
}

/// Reduces colors of a cel, a frame (all layers), a layer (all frames) or everything to a
/// limited palette — the step that turns renders and photos into readable pixel art.
pub fn quantize(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let scope = opt_str(a, "scope")?.unwrap_or("cel");
    let li = layer_index(doc, a, "layer")?;
    let fi = frame_index(doc, a, "frame")?;
    let cels: Vec<(u32, u32)> = match scope {
        "cel" => vec![(doc.layers[li].id, doc.frames[fi].id)],
        "layer" => doc.frames.iter().map(|f| (doc.layers[li].id, f.id)).collect(),
        "frame" => doc.layers.iter().map(|l| (l.id, doc.frames[fi].id)).collect(),
        "all" => doc.layers.iter().flat_map(|l| doc.frames.iter().map(move |f| (l.id, f.id))).collect(),
        other => return Err(CmdError::invalid(format!("unknown scope '{other}'"))),
    };
    let dither = dither_arg(a)?;
    let strength = opt_f64(a, "dither_strength")?.unwrap_or(28.0).clamp(0.0, 255.0) as f32;
    // One shared palette across the whole scope keeps animation frames consistent.
    let mut samples: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for key in &cels {
        if let Some(g) = doc.cels.get(key) {
            g.for_each_opaque(|_, _, v| *samples.entry(v | 0xFF).or_default() += 1);
        }
    }
    let mut sample: Vec<(Rgba, u32)> = samples.into_iter().map(|(c, n)| (Rgba::from_u32(c), n)).collect();
    sample.sort_by_key(|(c, _)| c.to_u32());
    let pal = quantize_palette(doc, a, &sample)?;
    if pal.is_empty() {
        return Ok(Outcome::new(Json::obj().with("changed", 0), 0));
    }
    let mut changed = 0u64;
    for key in cels {
        if doc.layers.iter().any(|l| l.id == key.0 && l.locked) {
            continue;
        }
        let Some(cel) = doc.cels.get_mut(&key) else { continue };
        let grid: &mut PixelGrid = Arc::make_mut(cel);
        let mut img = pixelart::grid_to_image(grid);
        pixelart::quantize_image(&mut img, &pal, dither, strength);
        changed += pixelart::image_to_grid(&img, grid) as u64;
    }
    if opt_bool(a, "add_to_palette")?.unwrap_or(false) {
        for c in &pal {
            let _ = doc.palette.intern(*c);
        }
    }
    Ok(Outcome::new(
        Json::obj().with("changed", changed).with("palette", Json::Arr(pal.iter().map(|c| Json::from(c.to_hex())).collect())),
        changed,
    ))
}

/// Fills a region with an ordered-dither blend: level 0 = all `color`, 16 = all `color2`.
pub fn dither_fill(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let r = region(doc, a)?;
    let c1 = color(doc, a, "color")?;
    let c2 = color(doc, a, "color2")?;
    let level = opt_i64(a, "level")?.unwrap_or(8).clamp(0, 16) as u32;
    let only_opaque = opt_bool(a, "only_opaque")?.unwrap_or(false);
    let (l, f) = target(doc, a)?;
    let grid = doc.cel_mut(l, f);
    let mut changed = 0u64;
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            if only_opaque && grid.get(x, y) == TRANSPARENT {
                continue;
            }
            let v = if pixelart::dither_pick(x, y, level) { c2 } else { c1 };
            changed += grid.set(x, y, v) as u64;
        }
    }
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

/// Normal map of a frame (or one layer) for 2D dynamic lighting (Godot/Unity sprite normals).
pub fn export_normal_map(doc: &Document, a: &Json) -> CmdResult<Query> {
    let fi = frame_index(doc, a, "frame")?;
    let mut opts = RenderOptions::default();
    if a.get("layer").is_some_and(|v| !v.is_null()) {
        opts.only_layer = Some(doc.layers[layer_index(doc, a, "layer")?].id);
    }
    let img = render(doc, fi, &opts);
    let strength = opt_f64(a, "strength")?.unwrap_or(3.0).clamp(0.1, 20.0) as f32;
    let bevel = opt_f64(a, "bevel")?.unwrap_or(3.0).clamp(0.5, 32.0) as f32;
    let n = pixelart::normal_map(&img, strength, bevel).scaled(scale(a, 1)?);
    let png = export::png::encode(&n);
    Ok(Query { result: Json::obj().with("width", n.width).with("height", n.height).with("mimeType", "image/png"), binary: png })
}
