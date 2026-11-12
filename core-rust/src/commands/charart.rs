//! Character-art commands: auto_shade, gradient_map, draw_sparkle, pixel_cleanup (and the
//! sel-out options of `outline`, which live in pixel2d).

use std::collections::HashMap;
use std::sync::Arc;

use super::args::{color_value, frame_index, layer_index, opt_f64, opt_i64, opt_str, target};
use super::json::Json;
use super::{CmdError, CmdResult, Outcome};
use crate::pixel::{PixelGrid, Rgba};
use crate::pixelart::charart::{self, ShadeSettings};
use crate::pixelart::{grid_to_image, image_to_grid};
use crate::project::Document;

pub fn light_arg(a: &Json, key: &str, default: (f32, f32)) -> CmdResult<(f32, f32)> {
    match a.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Str(s)) => Ok(match s.as_str() {
            "top-left" | "top_left" => (-1.0, -1.2),
            "top-right" | "top_right" => (1.0, -1.2),
            "top" => (0.0, -1.0),
            "left" => (-1.0, 0.0),
            "right" => (1.0, 0.0),
            "bottom" => (0.0, 1.0),
            "front" => (-0.3, -0.5),
            other => {
                return Err(CmdError::invalid(format!(
                    "unknown light '{other}' (top-left, top-right, top, left, right, bottom, front or [x, y])"
                )));
            }
        }),
        Some(Json::Arr(v)) if v.len() == 2 => {
            let x = v[0].as_f64().ok_or_else(|| CmdError::invalid("light must be [x, y]"))? as f32;
            let y = v[1].as_f64().ok_or_else(|| CmdError::invalid("light must be [x, y]"))? as f32;
            if x == 0.0 && y == 0.0 {
                return Err(CmdError::invalid("light direction cannot be [0, 0]"));
            }
            Ok((x, y))
        }
        Some(_) => Err(CmdError::invalid(format!("'{key}' must be a direction name or [x, y]"))),
    }
}

/// Cels addressed by scope (cel / layer / frame / all), skipping locked layers.
fn cels(doc: &Document, a: &Json) -> CmdResult<Vec<(usize, usize)>> {
    let li = layer_index(doc, a, "layer")?;
    let fi = frame_index(doc, a, "frame")?;
    let v: Vec<(usize, usize)> = match opt_str(a, "scope")?.unwrap_or("cel") {
        "cel" => vec![(li, fi)],
        "layer" => (0..doc.frames.len()).map(|f| (li, f)).collect(),
        "frame" => (0..doc.layers.len()).map(|l| (l, fi)).collect(),
        "all" => (0..doc.layers.len()).flat_map(|l| (0..doc.frames.len()).map(move |f| (l, f))).collect(),
        other => return Err(CmdError::invalid(format!("unknown scope '{other}' (cel, layer, frame, all)"))),
    };
    if v.len() == 1 && doc.layers[v[0].0].locked {
        return Err(CmdError::new("locked", format!("layer '{}' is locked", doc.layers[v[0].0].name)));
    }
    Ok(v.into_iter().filter(|(l, _)| !doc.layers[*l].locked).collect())
}

fn color_list(doc: &Document, v: Option<&Json>, key: &str) -> CmdResult<Vec<Rgba>> {
    match v {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Arr(list)) => list.iter().map(|c| color_value(doc, c, key).map(Rgba::from_u32)).collect(),
        Some(_) => Err(CmdError::invalid(format!("'{key}' must be an array of colors"))),
    }
}

fn edit_cel(doc: &mut Document, l: usize, f: usize, edit: impl FnOnce(&mut crate::renderer::Image) -> usize) -> u64 {
    let key = (doc.layers[l].id, doc.frames[f].id);
    let Some(cel) = doc.cels.get_mut(&key) else { return 0 };
    let grid: &mut PixelGrid = Arc::make_mut(cel);
    let mut img = grid_to_image(grid);
    if edit(&mut img) == 0 {
        return 0;
    }
    image_to_grid(&img, grid) as u64
}

/// Automatic cel shading of flat-colored shapes (see pixelart::charart::auto_shade).
pub fn auto_shade(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let mut s = ShadeSettings {
        light: light_arg(a, "light", (-1.0, -1.2))?,
        tones: opt_i64(a, "tones")?.unwrap_or(3).clamp(2, 4) as u32,
        bevel: opt_f64(a, "bevel")?.unwrap_or(3.0).clamp(0.5, 32.0) as f32,
        form: opt_f64(a, "form")?.unwrap_or(0.45).clamp(0.0, 2.0) as f32,
        hue_shift: opt_f64(a, "hue_shift")?.unwrap_or(22.0).clamp(0.0, 90.0) as f32,
        contrast: opt_f64(a, "contrast")?.unwrap_or(0.5).clamp(0.05, 1.0) as f32,
        shadow_amount: opt_f64(a, "shadow")?.unwrap_or(0.0).clamp(-1.0, 1.0) as f32,
        min_region: opt_i64(a, "min_region")?.unwrap_or(6).clamp(1, 1_000_000) as usize,
        ..Default::default()
    };
    s.preserve = color_list(doc, a.get("preserve"), "preserve")?.iter().map(|c| c.to_u32() | 0xFF).collect();
    if let Some(Json::Obj(entries)) = a.get("ramps") {
        let mut ramps = HashMap::new();
        for (k, v) in entries {
            let base = Rgba::parse_hex(k).ok_or_else(|| CmdError::invalid(format!("ramps key '{k}' is not a hex color")))?;
            let tones = color_list(doc, Some(v), "ramps")?;
            let t: [Rgba; 4] = match tones.len() {
                2 => [tones[0], tones[0], base, tones[1]],
                3 => [tones[0], tones[0], tones[1], tones[2]],
                4 => [tones[0], tones[1], tones[2], tones[3]],
                _ => {
                    return Err(CmdError::invalid(
                        "each ramp needs [shadow, highlight], [shadow, base, highlight] or [deep, shadow, base, highlight]",
                    ));
                }
            };
            ramps.insert(base.to_u32() | 0xFF, t);
        }
        s.ramps = ramps;
    }
    if let Some(v) = a.get("rim").filter(|v| !v.is_null()) {
        let c = Rgba::from_u32(color_value(doc, v, "rim")?);
        s.rim = Some((c, opt_f64(a, "rim_strength")?.unwrap_or(0.55).clamp(0.0, 1.0) as f32));
    }
    let cast = opt_i64(a, "cast_shadow")?.unwrap_or(2).clamp(0, 64) as u32;
    let (w, h) = (doc.width as usize, doc.height as usize);
    let mut changed = 0u64;
    for (l, f) in cels(doc, a)? {
        // Pixels of visible layers above this one cast shadows onto it.
        let mut occ = vec![false; if cast > 0 { w * h } else { 0 }];
        if cast > 0 {
            for upper in &doc.layers[l + 1..] {
                if !upper.visible {
                    continue;
                }
                if let Some(g) = doc.cel(upper.id, doc.frames[f].id) {
                    g.for_each_opaque(|x, y, v| {
                        if v & 0xFF >= 128 {
                            occ[y as usize * w + x as usize] = true;
                        }
                    });
                }
            }
        }
        changed += edit_cel(doc, l, f, |img| charart::auto_shade(img, &s, &occ, cast));
    }
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

/// Color grading: luminance mapped onto a gradient (mood, time of day, flashbacks).
pub fn gradient_map(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let stops = color_list(doc, a.get("colors"), "colors")?;
    if stops.len() < 2 || stops.len() > 32 {
        return Err(CmdError::invalid("'colors' must list 2-32 colors from dark to light"));
    }
    let strength = opt_f64(a, "strength")?.unwrap_or(1.0).clamp(0.0, 1.0) as f32;
    let mut changed = 0u64;
    for (l, f) in cels(doc, a)? {
        changed += edit_cel(doc, l, f, |img| charart::gradient_map(img, &stops, strength));
    }
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

/// Stamps 4-point sparkles / stars (one or many).
pub fn draw_sparkle(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let color = Rgba::from_u32(color_value(doc, a.get("color").unwrap_or(&Json::Str("#FFE9B0".into())), "color")?);
    let core = Rgba::from_u32(color_value(doc, a.get("core").unwrap_or(&Json::Str("#FFFFFF".into())), "core")?);
    let style = opt_str(a, "style")?.unwrap_or("star4");
    if !["star4", "cross", "diamond", "dot"].contains(&style) {
        return Err(CmdError::invalid("'style' must be star4, cross, diamond or dot"));
    }
    let default_size = opt_i64(a, "size")?.unwrap_or(3);
    let mut points: Vec<(i64, i64, i64)> = Vec::new();
    match a.get("points") {
        Some(Json::Arr(list)) => {
            for p in list {
                match p.as_array() {
                    Some([x, y]) => points.push((x.as_i64().unwrap_or(0), y.as_i64().unwrap_or(0), default_size)),
                    Some([x, y, s]) => points.push((x.as_i64().unwrap_or(0), y.as_i64().unwrap_or(0), s.as_i64().unwrap_or(default_size))),
                    _ => return Err(CmdError::invalid("points must be [[x, y] or [x, y, size], …]")),
                }
            }
        }
        _ => points.push((opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0), default_size)),
    }
    if points.len() > 4096 {
        return Err(CmdError::new("limit", "at most 4096 sparkles per call"));
    }
    let (l, f) = target(doc, a)?;
    let grid = doc.cel_mut(l, f);
    let mut changed = 0u64;
    for (x, y, size) in points {
        for (dx, dy, is_core) in charart::sparkle_shape(size.clamp(1, 8) as u32, style) {
            let c = if is_core { core } else { color };
            changed += grid.set(x + dx as i64, y + dy as i64, c.to_pixel()) as u64;
        }
    }
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

pub fn pixel_cleanup(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let only = match a.get("color") {
        None | Some(Json::Null) => None,
        Some(v) => Some(Rgba::from_u32(color_value(doc, v, "color")?)),
    };
    let passes = opt_i64(a, "passes")?.unwrap_or(1).clamp(1, 4);
    let mut changed = 0u64;
    for (l, f) in cels(doc, a)? {
        for _ in 0..passes {
            changed += edit_cel(doc, l, f, |img| charart::pixel_cleanup(img, only));
        }
    }
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}
