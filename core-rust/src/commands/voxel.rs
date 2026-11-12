//! 3D pixel (voxel) commands: modeling, PBR materials and textures, rigging, animation and
//! rendering (including baking 8-direction sprite animations into the 2D project).

use std::collections::HashMap;
use std::sync::Arc;

use super::args::{color_value, frame_index, layer_index, opt_bool, opt_f64, opt_i64, opt_str, opt_vec3, req_i64, req_str, req_vec3};
use super::json::Json;
use super::{CmdError, CmdResult, Outcome, Query};
use crate::animation::validate_duration;
use crate::export;
use crate::pixel::{PixelGrid, Rgba};
use crate::project::{Document, MAX_FRAMES, validate_size};
use crate::voxel::material::{MAP_SLOTS, MapSlot, TEXTURE_KINDS, generate_set, slot_by_name};
use crate::voxel::model::{MAX_BONES, MAX_MODEL_SIZE, hash01};
use crate::voxel::render::{Camera, Outline, Pass, PointLight, Prepared, RenderSettings, pick, prepare, render, view_extent};
use crate::voxel::rig::{Animation, Bone, CycleParams, Key, auto_bind, idle_cycle, locomotion_cycle};
use crate::voxel::{Material, Shape, Texture, Vec3, Voxel, VoxelModel};

pub const MAX_BATCH_VOXELS: usize = 1_000_000;

/// Prepared geometry keyed by model version (shared by queries and bakes).
#[derive(Default)]
pub struct RenderCache {
    entries: Vec<Arc<Prepared>>,
}

impl RenderCache {
    pub fn get(&mut self, model: &VoxelModel) -> Arc<Prepared> {
        if let Some(p) = self.entries.iter().find(|p| p.version == model.version) {
            return Arc::clone(p);
        }
        let p = Arc::new(prepare(model));
        self.entries.push(Arc::clone(&p));
        if self.entries.len() > 6 {
            self.entries.remove(0);
        }
        p
    }
}

// ---- argument helpers -------------------------------------------------------------------

fn model_index(doc: &Document, a: &Json) -> CmdResult<usize> {
    if doc.models.is_empty() {
        return Err(CmdError::not_found("the project has no 3D models; create one with voxel_model_create"));
    }
    match a.get("model") {
        None | Some(Json::Null) => Ok(0),
        Some(Json::Str(name)) => doc
            .models
            .iter()
            .position(|m| m.name == *name)
            .or_else(|| name.parse::<usize>().ok().filter(|i| *i < doc.models.len()))
            .ok_or_else(|| CmdError::not_found(format!("no model named '{name}'"))),
        Some(v) => v
            .as_i64()
            .filter(|i| *i >= 0 && (*i as usize) < doc.models.len())
            .map(|i| i as usize)
            .ok_or_else(|| CmdError::not_found("model index out of range")),
    }
}

fn material_arg(doc: &Document, a: &Json) -> CmdResult<Option<u8>> {
    match a.get("material") {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(name)) => doc
            .materials
            .iter()
            .position(|m| m.name == *name)
            .map(|i| Some(i as u8))
            .ok_or_else(|| CmdError::not_found(format!("no material named '{name}'"))),
        Some(v) => v
            .as_i64()
            .filter(|i| *i >= 0 && (*i as usize) < doc.materials.len())
            .map(|i| Some(i as u8))
            .ok_or_else(|| CmdError::not_found("material index out of range")),
    }
}

fn bone_arg(model: &VoxelModel, a: &Json, key: &str) -> CmdResult<Option<u8>> {
    match a.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(name)) => {
            model.bone_index(name).map(|i| Some(i as u8)).ok_or_else(|| CmdError::not_found(format!("no bone named '{name}'")))
        }
        Some(v) => v
            .as_i64()
            .filter(|i| *i >= 0 && (*i as usize) < model.bones.len().max(1))
            .map(|i| Some(i as u8))
            .ok_or_else(|| CmdError::not_found("bone index out of range")),
    }
}

fn anim_index(model: &VoxelModel, a: &Json, key: &str) -> CmdResult<Option<usize>> {
    match a.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(name)) => model
            .animations
            .iter()
            .position(|x| x.name == *name)
            .map(Some)
            .ok_or_else(|| CmdError::not_found(format!("no animation named '{name}'"))),
        Some(v) => v
            .as_i64()
            .filter(|i| *i >= 0 && (*i as usize) < model.animations.len())
            .map(|i| Some(i as usize))
            .ok_or_else(|| CmdError::not_found("animation index out of range")),
    }
}

fn name_arg(a: &Json, key: &str, max: usize) -> CmdResult<String> {
    let n = req_str(a, key)?.trim();
    if n.is_empty() || n.chars().count() > max || n.chars().any(char::is_control) {
        return Err(CmdError::invalid(format!("'{key}' must be 1-{max} printable characters")));
    }
    Ok(n.to_string())
}

fn v3(a: &Json, key: &str) -> CmdResult<Vec3> {
    req_vec3(a, key).map(Vec3::from_f64)
}

fn base64_decode(s: &str) -> CmdResult<Vec<u8>> {
    let s = s.split_once("base64,").map(|(_, b)| b).unwrap_or(s);
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return Err(CmdError::invalid("invalid base64 data")),
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// Binary input: the call payload or a `data_base64` argument.
fn binary_input(a: &Json, payload: &[u8]) -> CmdResult<Vec<u8>> {
    if !payload.is_empty() {
        return Ok(payload.to_vec());
    }
    match opt_str(a, "data_base64")? {
        Some(b) => base64_decode(b),
        None => Err(CmdError::invalid("provide the PNG as 'data_base64' (or as the binary payload)")),
    }
}

// ---- modeling -----------------------------------------------------------------------------

pub fn model_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = name_arg(a, "name", 128)?;
    if doc.models.iter().any(|m| m.name == name) {
        return Err(CmdError::invalid(format!("model '{name}' already exists")));
    }
    let dim = |k: &str| -> CmdResult<u32> {
        let v = opt_i64(a, k)?.unwrap_or(64);
        if !(1..=MAX_MODEL_SIZE as i64).contains(&v) {
            return Err(CmdError::invalid(format!("'{k}' must be 1-{MAX_MODEL_SIZE}")));
        }
        Ok(v as u32)
    };
    let size = [dim("width")?, dim("height")?, dim("depth")?];
    let mut m = VoxelModel::new(&name, size);
    if let Some(o) = opt_vec3(a, "origin")? {
        m.origin = Vec3::from_f64(o);
    }
    doc.models.push(m);
    Ok(Outcome::new(
        Json::obj()
            .with("model", doc.models.len() - 1)
            .with("name", name)
            .with("size", Json::Arr(size.iter().map(|&d| Json::from(d)).collect())),
        0,
    ))
}

/// Duplicates a model (voxels, rig, animations) — the basis of frame-by-frame 3D animation and VFX.
pub fn model_clone(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let name = name_arg(a, "name", 128)?;
    if doc.models.iter().any(|m| m.name == name) {
        return Err(CmdError::invalid(format!("model '{name}' already exists")));
    }
    if doc.models.len() >= 4096 {
        return Err(CmdError::new("limit", "at most 4096 models"));
    }
    let mut copy = doc.models[i].clone();
    copy.name = name.clone();
    if opt_bool(a, "empty")?.unwrap_or(false) {
        copy.chunks.clear();
    }
    copy.touch();
    doc.models.push(copy);
    Ok(Outcome::new(Json::obj().with("model", doc.models.len() - 1).with("name", name), 0))
}

pub fn model_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let m = doc.models.remove(i);
    Ok(Outcome::new(Json::obj().with("deleted", m.name), 0))
}

pub fn model_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    if a.get("name").is_some() {
        let name = name_arg(a, "name", 128)?;
        doc.models[i].name = name;
    }
    if let Some(o) = opt_vec3(a, "origin")? {
        doc.models[i].origin = Vec3::from_f64(o);
    }
    doc.models[i].touch();
    let m = &doc.models[i];
    Ok(Outcome::new(Json::obj().with("model", i).with("name", m.name.as_str()).with("origin", vec_json(m.origin)), 0))
}

fn vec_json(v: Vec3) -> Json {
    Json::Arr(vec![Json::from(v.x as f64), Json::from(v.y as f64), Json::from(v.z as f64)])
}

pub fn voxel_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let rgba = super::args::color(doc, a, "color")?;
    let material = material_arg(doc, a)?.unwrap_or(0);
    let bone = bone_arg(&doc.models[i], a, "bone")?.unwrap_or(0);
    let (x, y, z) = (req_i64(a, "x")?, req_i64(a, "y")?, req_i64(a, "z")?);
    let m = &mut doc.models[i];
    if !m.in_bounds(x as i32, y as i32, z as i32) {
        return Err(CmdError::invalid(format!("voxel ({x},{y},{z}) is outside the {:?} model", m.size)));
    }
    let changed = m.set(x as i32, y as i32, z as i32, Voxel { rgba, material, bone }) as u64;
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

/// Explicit voxel list: `[[x, y, z, color], {x, y, z, color, material, bone}, ...]`.
pub fn voxel_set_many(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let items = a.get("voxels").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'voxels' must be an array"))?;
    if items.len() > MAX_BATCH_VOXELS {
        return Err(CmdError::new("limit", format!("at most {MAX_BATCH_VOXELS} voxels per call")));
    }
    let default_color = a.get("color").map(|c| color_value(doc, c, "color")).transpose()?;
    let material = material_arg(doc, a)?.unwrap_or(0);
    let bone = bone_arg(&doc.models[i], a, "bone")?.unwrap_or(0);
    let mut resolved = Vec::with_capacity(items.len());
    for (k, item) in items.iter().enumerate() {
        let (x, y, z, c) = match item {
            Json::Arr(p) if p.len() == 3 || p.len() == 4 => (p[0].as_i64(), p[1].as_i64(), p[2].as_i64(), p.get(3)),
            Json::Obj(_) => (
                item.get("x").and_then(Json::as_i64),
                item.get("y").and_then(Json::as_i64),
                item.get("z").and_then(Json::as_i64),
                item.get("color"),
            ),
            _ => (None, None, None, None),
        };
        let (Some(x), Some(y), Some(z)) = (x, y, z) else {
            return Err(CmdError::invalid(format!("voxels[{k}] needs integer x, y, z")));
        };
        let rgba = match c {
            Some(c) => color_value(doc, c, &format!("voxels[{k}].color"))?,
            None => default_color.ok_or_else(|| CmdError::invalid(format!("voxels[{k}] has no color and no default 'color'")))?,
        };
        let mat = match item.get("material") {
            Some(_) => material_arg(doc, item)?.unwrap_or(material),
            None => material,
        };
        let b = match item.get("bone") {
            Some(_) => bone_arg(&doc.models[i], item, "bone")?.unwrap_or(bone),
            None => bone,
        };
        resolved.push((x, y, z, Voxel { rgba, material: mat, bone: b }));
    }
    let m = &mut doc.models[i];
    let mut changed = 0u64;
    for (x, y, z, v) in resolved {
        changed += m.set(x as i32, y as i32, z as i32, v) as u64;
    }
    Ok(Outcome::new(Json::obj().with("requested", items.len()).with("changed", changed), changed))
}

fn parse_shape(a: &Json) -> CmdResult<Shape> {
    let kind = req_str(a, "shape")?;
    let shape = match kind {
        "box" => {
            let (p, q) = (v3(a, "min")?, v3(a, "max")?);
            Shape::Box {
                min: Vec3::new(p.x.min(q.x), p.y.min(q.y), p.z.min(q.z)),
                max: Vec3::new(p.x.max(q.x) + 1.0, p.y.max(q.y) + 1.0, p.z.max(q.z) + 1.0),
            }
        }
        "ellipsoid" | "sphere" => {
            let center = v3(a, "center")?;
            let radii = match opt_vec3(a, "radii")? {
                Some(r) => Vec3::from_f64(r),
                None => {
                    let r =
                        opt_f64(a, "radius")?.ok_or_else(|| CmdError::invalid("ellipsoid needs 'radii' [rx, ry, rz] or 'radius'"))? as f32;
                    Vec3::new(r, r, r)
                }
            };
            Shape::Ellipsoid { center, radii }
        }
        "capsule" | "cone" => {
            let ra = opt_f64(a, "radius")?.ok_or_else(|| CmdError::invalid("capsule needs 'radius'"))? as f32;
            let rb = opt_f64(a, "radius_b")?.map(|r| r as f32).unwrap_or(if kind == "cone" { 0.5 } else { ra });
            Shape::Capsule { a: v3(a, "a")?, b: v3(a, "b")?, ra, rb }
        }
        "cylinder" => Shape::Cylinder {
            a: v3(a, "a")?,
            b: v3(a, "b")?,
            r: opt_f64(a, "radius")?.ok_or_else(|| CmdError::invalid("cylinder needs 'radius'"))? as f32,
        },
        other => return Err(CmdError::invalid(format!("unknown shape '{other}' (box, ellipsoid, capsule, cone, cylinder)"))),
    };
    let (lo, hi) = shape.aabb();
    if (hi - lo).x * (hi - lo).y * (hi - lo).z > 512.0 * 512.0 * 512.0 {
        return Err(CmdError::new("limit", "shape is too large"));
    }
    Ok(shape)
}

fn mirrored_bone(model: &VoxelModel, bone: u8) -> u8 {
    let Some(b) = model.bones.get(bone as usize) else { return bone };
    let swapped = if let Some(s) = b.name.strip_suffix("_l") {
        format!("{s}_r")
    } else if let Some(s) = b.name.strip_suffix("_r") {
        format!("{s}_l")
    } else {
        return bone;
    };
    model.bone_index(&swapped).map(|i| i as u8).unwrap_or(bone)
}

/// Sculpt or paint with a primitive. Modes: add (fill), paint (recolor existing voxels only),
/// erase. Optional gradient (`color_to` along `gradient_axis`), `jitter` (0..1 tonal noise),
/// `mirror_x` (also apply mirrored across the model origin's X, swapping _l/_r bones).
pub fn voxel_shape(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let shape = parse_shape(a)?;
    let mode = opt_str(a, "mode")?.unwrap_or("add");
    if !matches!(mode, "add" | "paint" | "erase") {
        return Err(CmdError::invalid("'mode' must be add, paint or erase"));
    }
    let color = match a.get("color") {
        Some(c) => Some(color_value(doc, c, "color")?),
        None if mode == "erase" => None,
        None => return Err(CmdError::invalid("missing required color 'color'")),
    };
    let color_to = a.get("color_to").map(|c| color_value(doc, c, "color_to")).transpose()?;
    let axis = match opt_str(a, "gradient_axis")?.unwrap_or("y") {
        "x" => 0,
        "y" => 1,
        "z" => 2,
        _ => return Err(CmdError::invalid("'gradient_axis' must be x, y or z")),
    };
    let jitter = opt_f64(a, "jitter")?.unwrap_or(0.0).clamp(0.0, 1.0) as f32;
    let seed = opt_i64(a, "seed")?.unwrap_or(1) as u32;
    let material = material_arg(doc, a)?;
    let bone = bone_arg(&doc.models[i], a, "bone")?;
    let mirror = opt_bool(a, "mirror_x")?.unwrap_or(false);
    let model = &mut doc.models[i];
    let (lo, hi) = shape.aabb();
    let mut changed = 0u64;
    let mut writes: Vec<(i32, i32, i32, Voxel)> = Vec::new();
    for z in lo.z.floor() as i32..=hi.z.ceil() as i32 {
        for y in lo.y.floor() as i32..=hi.y.ceil() as i32 {
            for x in lo.x.floor() as i32..=hi.x.ceil() as i32 {
                let p = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                if !shape.contains(p) || !model.in_bounds(x, y, z) {
                    continue;
                }
                let existing = model.get(x, y, z);
                let out = match mode {
                    "erase" => Voxel::EMPTY,
                    "paint" if existing.is_empty() => continue,
                    _ => {
                        let mut c = Rgba::from_u32(color.unwrap_or(0));
                        if let Some(to) = color_to {
                            let t = shape.axis_t(p, axis);
                            let to = Rgba::from_u32(to);
                            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
                            c = Rgba::new(mix(c.r, to.r), mix(c.g, to.g), mix(c.b, to.b), mix(c.a, to.a));
                        }
                        if jitter > 0.0 {
                            let n = 1.0 + (hash01(x, y, z, seed) - 0.5) * 2.0 * jitter * 0.35;
                            let j = |v: u8| (v as f32 * n).clamp(0.0, 255.0) as u8;
                            c = Rgba::new(j(c.r), j(c.g), j(c.b), c.a);
                        }
                        let base = if existing.is_empty() { Voxel::EMPTY } else { existing };
                        Voxel { rgba: c.to_pixel(), material: material.unwrap_or(base.material), bone: bone.unwrap_or(base.bone) }
                    }
                };
                writes.push((x, y, z, out));
            }
        }
    }
    let ox = model.origin.x;
    for (x, y, z, v) in writes {
        changed += model.set(x, y, z, v) as u64;
        if mirror {
            let mx = (2.0 * ox - (x as f32 + 0.5) - 0.5).round() as i32;
            if mx != x {
                let mv = if v.is_empty() { v } else { Voxel { bone: mirrored_bone(model, v.bone), ..v } };
                if mode != "paint" || !model.get(mx, y, z).is_empty() {
                    changed += model.set(mx, y, z, mv) as u64;
                }
            }
        }
    }
    Ok(Outcome::new(Json::obj().with("changed", changed).with("voxels", model.count()), changed))
}

/// Copies one half of the model onto the other across the origin's X plane.
pub fn voxel_mirror(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let from_positive = match opt_str(a, "from")?.unwrap_or("+") {
        "+" | "right" | "positive" => true,
        "-" | "left" | "negative" => false,
        _ => return Err(CmdError::invalid("'from' must be '+' or '-'")),
    };
    let model = &mut doc.models[i];
    let ox = model.origin.x;
    let mut writes = Vec::new();
    model.for_each(|x, y, z, v| {
        let cx = x as f32 + 0.5;
        if (from_positive && cx > ox) || (!from_positive && cx < ox) {
            writes.push(((2.0 * ox - cx - 0.5).round() as i32, y, z, v));
        }
    });
    let mut changed = 0;
    for (x, y, z, v) in writes {
        let v = Voxel { bone: mirrored_bone(model, v.bone), ..v };
        changed += model.set(x, y, z, v) as u64;
    }
    Ok(Outcome::new(Json::obj().with("changed", changed), changed))
}

// ---- materials & textures -----------------------------------------------------------------

fn texture_index(doc: &Document, v: &Json) -> CmdResult<Option<usize>> {
    match v {
        Json::Null => Ok(None),
        Json::Str(name) => doc
            .textures
            .iter()
            .position(|t| t.name == *name)
            .map(Some)
            .ok_or_else(|| CmdError::not_found(format!("no texture named '{name}'"))),
        v => v
            .as_i64()
            .filter(|i| *i >= 0 && (*i as usize) < doc.textures.len())
            .map(|i| Some(i as usize))
            .ok_or_else(|| CmdError::not_found("texture index out of range")),
    }
}

/// Creates or updates a PBR material. `maps` = {albedo|normal|roughness|metallic|occlusion|height|emission: texture name | null}.
pub fn material_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = name_arg(a, "name", 64)?;
    let idx = match doc.materials.iter().position(|m| m.name == name) {
        Some(i) => i,
        None => {
            if doc.materials.len() >= 256 {
                return Err(CmdError::new("limit", "at most 256 materials"));
            }
            doc.materials.push(Material::new(&name));
            doc.materials.len() - 1
        }
    };
    let mut m = doc.materials[idx].clone();
    let unit = |k: &str| -> CmdResult<Option<f32>> { Ok(opt_f64(a, k)?.map(|v| v.clamp(0.0, 1.0) as f32)) };
    if let Some(v) = unit("roughness")? {
        m.roughness = v;
    }
    if let Some(v) = unit("metallic")? {
        m.metallic = v;
    }
    if let Some(v) = opt_f64(a, "emission")? {
        m.emission = v.clamp(0.0, 100.0) as f32;
    }
    if let Some(v) = opt_f64(a, "uv_scale")? {
        m.uv_scale = v.clamp(0.01, 64.0) as f32;
    }
    if let Some(v) = opt_f64(a, "normal_strength")? {
        m.normal_strength = v.clamp(0.0, 4.0) as f32;
    }
    if let Some(Json::Obj(entries)) = a.get("maps") {
        for (k, v) in entries {
            let slot = slot_by_name(k).ok_or_else(|| CmdError::invalid(format!("unknown map slot '{k}'")))?;
            m.maps[slot as usize] = texture_index(doc, v)?;
        }
    }
    doc.materials[idx] = m;
    for model in doc.models.iter_mut() {
        model.touch();
    }
    Ok(Outcome::new(Json::obj().with("material", idx).with("name", name), 0))
}

fn add_texture(doc: &mut Document, tex: Texture) -> CmdResult<usize> {
    if let Some(i) = doc.textures.iter().position(|t| t.name == tex.name) {
        doc.textures[i] = Arc::new(tex);
        return Ok(i);
    }
    if doc.textures.len() >= 1024 {
        return Err(CmdError::new("limit", "at most 1024 textures"));
    }
    doc.textures.push(Arc::new(tex));
    Ok(doc.textures.len() - 1)
}

/// Imports a PNG as a texture map (any PBR slot can reference it).
pub fn texture_import(doc: &mut Document, a: &Json, payload: &[u8]) -> CmdResult<Outcome> {
    let name = name_arg(a, "name", 64)?;
    let png = binary_input(a, payload)?;
    let img = export::png::decode(&png).map_err(|e| CmdError::new("invalid_file", e))?;
    if img.width > 4096 || img.height > 4096 {
        return Err(CmdError::new("limit", "textures are limited to 4096x4096"));
    }
    let (w, h) = (img.width, img.height);
    let idx = add_texture(doc, Texture { name: name.clone(), width: w, height: h, data: img.pixels })?;
    Ok(Outcome::new(Json::obj().with("texture", idx).with("name", name).with("width", w).with("height", h), 0))
}

/// Generates a tileable PBR texture set (albedo, normal, roughness, occlusion, height) and,
/// by default, a material using it.
pub fn texture_generate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let name = name_arg(a, "name", 48)?;
    let kind = opt_str(a, "kind")?.unwrap_or("noise");
    if !TEXTURE_KINDS.contains(&kind) {
        return Err(CmdError::invalid(format!("unknown kind '{kind}' ({})", TEXTURE_KINDS.join(", "))));
    }
    let size = opt_i64(a, "size")?.unwrap_or(64).clamp(8, 1024) as u32;
    let cells = opt_i64(a, "cells")?.unwrap_or(4).clamp(1, 64) as i32;
    let seed = opt_i64(a, "seed")?.unwrap_or(1) as u32;
    let roughness = opt_f64(a, "roughness")?.unwrap_or(0.7).clamp(0.02, 1.0) as f32;
    let contrast = opt_f64(a, "contrast")?.unwrap_or(0.35).clamp(0.0, 1.0) as f32;
    let set = generate_set(&name, kind, size, cells, seed, roughness, contrast);
    let mut ids = Json::obj();
    let mut maps = [None; 7];
    for (tex, slot, label) in [
        (set.albedo, MapSlot::Albedo, "albedo"),
        (set.normal, MapSlot::Normal, "normal"),
        (set.roughness, MapSlot::Roughness, "roughness"),
        (set.occlusion, MapSlot::Occlusion, "occlusion"),
        (set.height, MapSlot::Height, "height"),
    ] {
        let tname = tex.name.clone();
        let idx = add_texture(doc, tex)?;
        maps[slot as usize] = Some(idx);
        ids.set(label, tname);
    }
    let mut result = Json::obj().with("textures", ids).with("kind", kind).with("size", size);
    if opt_bool(a, "material")?.unwrap_or(true) {
        let idx = match doc.materials.iter().position(|m| m.name == name) {
            Some(i) => i,
            None => {
                doc.materials.push(Material::new(&name));
                doc.materials.len() - 1
            }
        };
        let m = &mut doc.materials[idx];
        m.maps = maps;
        m.roughness = roughness;
        if let Some(v) = opt_f64(a, "metallic")? {
            m.metallic = v.clamp(0.0, 1.0) as f32;
        }
        if let Some(v) = opt_f64(a, "uv_scale")? {
            m.uv_scale = v.clamp(0.01, 64.0) as f32;
        }
        result.set("material", idx);
        for model in doc.models.iter_mut() {
            model.touch();
        }
    }
    Ok(Outcome::new(result, 0))
}

// ---- rigging & animation --------------------------------------------------------------------

pub fn bone_add(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let name = name_arg(a, "name", 64)?;
    let model = &mut doc.models[i];
    if model.bones.len() >= MAX_BONES {
        return Err(CmdError::new("limit", format!("at most {MAX_BONES} bones")));
    }
    if model.bone_index(&name).is_some() {
        return Err(CmdError::invalid(format!("bone '{name}' already exists")));
    }
    let parent = match a.get("parent") {
        None | Some(Json::Null) => None,
        Some(Json::Str(p)) => Some(model.bone_index(p).ok_or_else(|| CmdError::not_found(format!("no bone named '{p}'")))?),
        Some(v) => Some(
            v.as_i64()
                .filter(|p| *p >= 0 && (*p as usize) < model.bones.len())
                .ok_or_else(|| CmdError::not_found("parent bone out of range"))? as usize,
        ),
    };
    let head = v3(a, "head")?;
    let tail = opt_vec3(a, "tail")?.map(Vec3::from_f64).unwrap_or(head);
    model.bones.push(Bone { name: name.clone(), parent, head, tail });
    model.touch();
    Ok(Outcome::new(Json::obj().with("bone", model.bones.len() - 1).with("name", name), 0))
}

pub fn rig_clear(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let model = &mut doc.models[i];
    model.bones.clear();
    model.animations.clear();
    let mut writes = Vec::new();
    model.for_each(|x, y, z, v| {
        if v.bone != 0 {
            writes.push((x, y, z, Voxel { bone: 0, ..v }));
        }
    });
    for (x, y, z, v) in writes {
        model.set(x, y, z, v);
    }
    model.touch();
    Ok(Outcome::new(Json::obj().with("cleared", true), 0))
}

pub fn rig_bind(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let only_unbound = opt_bool(a, "only_unbound")?.unwrap_or(false);
    let n = auto_bind(&mut doc.models[i], only_unbound);
    Ok(Outcome::new(Json::obj().with("rebound", n), n as u64))
}

pub fn anim_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let name = name_arg(a, "name", 64)?;
    let frames = opt_i64(a, "frames")?.unwrap_or(8).clamp(1, 10_000) as u32;
    let frame_ms = validate_duration(opt_i64(a, "frame_ms")?.unwrap_or(100))?;
    let model = &mut doc.models[i];
    let mut anim = Animation::new(&name, frames, frame_ms);
    anim.looped = opt_bool(a, "loop")?.unwrap_or(true);
    model.animations.retain(|x| x.name != name);
    model.animations.push(anim);
    Ok(Outcome::new(Json::obj().with("animation", model.animations.len() - 1).with("name", name), 0))
}

pub fn anim_key(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let model = &doc.models[i];
    let ai = anim_index(model, a, "animation")?.ok_or_else(|| CmdError::invalid("missing 'animation'"))?;
    let bone = bone_arg(model, a, "bone")?.ok_or_else(|| CmdError::invalid("missing 'bone'"))? as usize;
    let frame = opt_f64(a, "frame")?.ok_or_else(|| CmdError::invalid("missing 'frame'"))? as f32;
    let rot = opt_vec3(a, "rotation")?.map(Vec3::from_f64).unwrap_or(Vec3::ZERO);
    let pos = opt_vec3(a, "position")?.map(Vec3::from_f64).unwrap_or(Vec3::ZERO);
    let anim = &mut doc.models[i].animations[ai];
    if frame < 0.0 || frame > anim.frames as f32 {
        return Err(CmdError::invalid("'frame' is outside the animation"));
    }
    anim.set_key(Key { bone, frame, rot, pos });
    Ok(Outcome::new(Json::obj().with("keys", anim.keys.len()), 0))
}

/// Procedural humanoid cycles from standard bone names (walk, run, idle).
pub fn anim_generate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
    let i = model_index(doc, a)?;
    let kind = opt_str(a, "kind")?.unwrap_or("walk");
    let frames = opt_i64(a, "frames")?.unwrap_or(8).clamp(2, 240) as u32;
    let name = opt_str(a, "name")?.unwrap_or(kind).to_string();
    let model = &doc.models[i];
    if model.bones.is_empty() {
        return Err(CmdError::invalid("the model has no skeleton; add bones (hips, upper_leg_l, ...) first"));
    }
    let mut anim = match kind {
        "walk" | "run" => {
            let mut p = if kind == "walk" { CycleParams::walk(frames) } else { CycleParams::run(frames) };
            if let Some(v) = opt_f64(a, "stride")? {
                p.stride_deg = v.clamp(0.0, 80.0) as f32;
            }
            if let Some(v) = opt_f64(a, "arm_swing")? {
                p.arm_swing_deg = v.clamp(0.0, 80.0) as f32;
            }
            if let Some(v) = opt_f64(a, "bob")? {
                p.bob = v.clamp(0.0, 40.0) as f32;
            }
            if let Some(v) = opt_f64(a, "knee")? {
                p.knee_deg = v.clamp(0.0, 120.0) as f32;
            }
            if let Some(v) = opt_f64(a, "lean")? {
                p.lean_deg = v.clamp(-30.0, 30.0) as f32;
            }
            locomotion_cycle(model, &name, &p)
        }
        "idle" => idle_cycle(model, &name, frames),
        other => return Err(CmdError::invalid(format!("unknown kind '{other}' (walk, run, idle)"))),
    };
    if let Some(ms) = opt_i64(a, "frame_ms")? {
        anim.frame_ms = validate_duration(ms)?;
    }
    let keys = anim.keys.len();
    let model = &mut doc.models[i];
    model.animations.retain(|x| x.name != name);
    model.animations.push(anim);
    Ok(Outcome::new(
        Json::obj().with("animation", model.animations.len() - 1).with("name", name).with("frames", frames).with("keys", keys),
        0,
    ))
}

// ---- rendering --------------------------------------------------------------------------------

pub const DIRECTIONS_8: [(&str, f32); 8] =
    [("S", 0.0), ("SE", 45.0), ("E", 90.0), ("NE", 135.0), ("N", 180.0), ("NW", 225.0), ("W", 270.0), ("SW", 315.0)];

fn directions(n: i64) -> CmdResult<Vec<(&'static str, f32)>> {
    Ok(match n {
        1 => vec![DIRECTIONS_8[0]],
        2 => vec![DIRECTIONS_8[0], DIRECTIONS_8[4]],
        4 => vec![DIRECTIONS_8[0], DIRECTIONS_8[2], DIRECTIONS_8[4], DIRECTIONS_8[6]],
        8 => DIRECTIONS_8.to_vec(),
        16 => (0..16)
            .map(|i| {
                (["S", "SSE", "SE", "ESE", "E", "ENE", "NE", "NNE", "N", "NNW", "NW", "WNW", "W", "WSW", "SW", "SSW"][i], i as f32 * 22.5)
            })
            .collect(),
        _ => return Err(CmdError::invalid("'directions' must be 1, 2, 4, 8 or 16")),
    })
}

/// Common render options from arguments (camera size/angles are filled in by the caller).
fn settings_from(doc: &Document, model: &VoxelModel, a: &Json, width: u32, height: u32) -> CmdResult<RenderSettings> {
    let mut s = RenderSettings::new(Camera {
        yaw: 0.0,
        pitch: 30.0,
        scale: 1.0,
        width,
        height,
        anchor_x: width as f32 / 2.0,
        anchor_y: height as f32,
    });
    s.camera.yaw = opt_f64(a, "yaw")?.unwrap_or(0.0) as f32;
    s.camera.pitch = opt_f64(a, "pitch")?.unwrap_or(30.0).clamp(-89.0, 89.0) as f32;
    if let Some(p) = opt_str(a, "pass")? {
        s.pass = Pass::parse(p)
            .ok_or_else(|| CmdError::invalid("'pass' must be lit, albedo, normal, depth, occlusion, roughness, metallic or emission"))?;
    }
    s.smoothness = opt_f64(a, "smoothness")?.unwrap_or(0.55).clamp(0.0, 1.0) as f32;
    s.toon_steps = opt_i64(a, "toon_steps")?.unwrap_or(0).clamp(0, 16) as u32;
    s.inner_lines = opt_bool(a, "inner_lines")?.unwrap_or(false);
    s.supersample = opt_i64(a, "supersample")?.unwrap_or(1).clamp(1, 4) as u32;
    s.outline = match a.get("outline") {
        None | Some(Json::Null) | Some(Json::Bool(false)) => Outline::None,
        Some(Json::Bool(true)) => Outline::Selective,
        Some(Json::Str(v)) if v == "selective" || v == "selout" => Outline::Selective,
        Some(Json::Str(v)) if v == "none" => Outline::None,
        Some(v) => Outline::Color(Rgba::from_u32(color_value(doc, v, "outline")?)),
    };
    if let Some(bg) = opt_str(a, "background")? {
        s.background = Some(Rgba::parse_hex(bg).ok_or_else(|| CmdError::invalid("'background' must be a hex color"))?);
    }
    if let Some(e) = opt_f64(a, "exposure")? {
        s.lighting.exposure = e.clamp(0.05, 8.0) as f32;
    }
    s.shadows = match a.get("shadows") {
        Some(Json::Bool(b)) => *b as u8 as f32,
        _ => opt_f64(a, "shadows")?.unwrap_or(0.0).clamp(0.0, 1.0) as f32,
    };
    s.ground_shadow = opt_f64(a, "ground_shadow")?.unwrap_or(0.0).clamp(0.0, 1.0) as f32;
    s.bloom = opt_f64(a, "bloom")?.unwrap_or(0.0).clamp(0.0, 4.0) as f32;
    s.bloom_radius = opt_f64(a, "bloom_radius")?.unwrap_or(0.0).clamp(0.0, 256.0) as f32;
    if let Some(dir) = opt_vec3(a, "key_light")? {
        s.lighting.lights[0].dir = Vec3::from_f64(dir).normalized();
    }
    if let Some(v) = opt_f64(a, "key_intensity")? {
        s.lighting.lights[0].intensity = v.clamp(0.0, 20.0) as f32;
    }
    if let Some(v) = opt_f64(a, "ambient")? {
        let k = v.clamp(0.0, 4.0) as f32;
        s.lighting.sky = s.lighting.sky * k;
        s.lighting.ground = s.lighting.ground * k;
    }
    if let Some(Json::Arr(g)) = a.get("background_gradient") {
        if g.len() != 2 {
            return Err(CmdError::invalid("'background_gradient' must be [top, bottom] colors"));
        }
        let top = Rgba::from_u32(color_value(doc, &g[0], "background_gradient")?);
        let bottom = Rgba::from_u32(color_value(doc, &g[1], "background_gradient")?);
        s.background_gradient = Some((top, bottom));
    }
    if let Some(Json::Arr(lights)) = a.get("lights") {
        if lights.len() > 32 {
            return Err(CmdError::new("limit", "at most 32 point lights"));
        }
        for (i, l) in lights.iter().enumerate() {
            let pos = Vec3::from_f64(req_vec3(l, "position").map_err(|e| CmdError::invalid(format!("lights[{i}]: {}", e.message)))?);
            let c = Rgba::from_u32(match l.get("color") {
                Some(c) => color_value(doc, c, "lights.color")?,
                None => Rgba::new(255, 170, 90, 255).to_pixel(),
            });
            let lin = |v: u8| (v as f32 / 255.0).powf(2.2);
            s.lighting.points.push(PointLight {
                pos,
                color: Vec3::new(lin(c.r), lin(c.g), lin(c.b)),
                intensity: opt_f64(l, "intensity")?.unwrap_or(4.0).clamp(0.0, 200.0) as f32,
                radius: opt_f64(l, "radius")?.unwrap_or(48.0).clamp(1.0, 2048.0) as f32,
            });
        }
    }
    s.animation = match anim_index(model, a, "animation")? {
        Some(ai) => Some((ai, opt_f64(a, "frame")?.unwrap_or(0.0) as f32)),
        None => None,
    };
    Ok(s)
}

/// Picks one scale/anchor that fits every requested direction and frame (sprite consistency).
fn fit(
    model: &VoxelModel,
    prepared: &Prepared,
    base: &RenderSettings,
    yaws: &[f32],
    frames: &[f32],
    margin: f32,
    scale: Option<f32>,
) -> CmdResult<(f32, f32, f32)> {
    let (mut half_w, mut lo_y, mut hi_y) = (0.0f32, f32::MAX, f32::MIN);
    for &yaw in yaws {
        for &t in frames {
            let mut s = base.clone();
            s.camera.yaw = yaw;
            s.animation = s.animation.map(|(ai, _)| (ai, t));
            if let Some((x0, x1, y0, y1)) = view_extent(model, prepared, &s) {
                half_w = half_w.max(x0.abs()).max(x1.abs());
                lo_y = lo_y.min(y0);
                hi_y = hi_y.max(y1);
            }
        }
    }
    if half_w == 0.0 {
        return Err(CmdError::invalid("the model is empty"));
    }
    let (w, h) = (base.camera.width as f32, base.camera.height as f32);
    let s = scale.unwrap_or_else(|| ((w / 2.0 - margin) / half_w).min((h - 2.0 * margin) / (hi_y - lo_y))).max(0.05);
    let anchor_y = h - margin + lo_y * s;
    Ok((s, w / 2.0, anchor_y))
}

fn render_dims(a: &Json, default: u32) -> CmdResult<(u32, u32)> {
    let w = opt_i64(a, "width")?.unwrap_or(default as i64);
    let h = opt_i64(a, "height")?.unwrap_or(w);
    if !(8..=2048).contains(&w) || !(8..=2048).contains(&h) {
        return Err(CmdError::invalid("render width/height must be 8-2048"));
    }
    Ok((w as u32, h as u32))
}

/// Renders a model to PNG from any angle/frame/pass.
pub fn voxel_render(doc: &Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Query> {
    let mi = model_index(doc, a)?;
    let model = &doc.models[mi];
    let (w, h) = render_dims(a, 256)?;
    let mut s = settings_from(doc, model, a, w, h)?;
    let prepared = cache.get(model);
    let margin = opt_f64(a, "margin")?.unwrap_or(8.0) as f32;
    let explicit = opt_f64(a, "scale")?.map(|v| v.clamp(0.05, 64.0) as f32);
    let t = s.animation.map(|(_, t)| t).unwrap_or(0.0);
    let (scale, mut ax, mut ay) = fit(model, &prepared, &s, &[s.camera.yaw], &[t], margin, explicit)?;
    // `focus` = model point to center in the image (close-ups); use with an explicit `scale`.
    if let Some(f) = opt_vec3(a, "focus")? {
        let v = s.camera.view_rotation().apply(Vec3::from_f64(f) - model.origin);
        ax = w as f32 / 2.0 - v.x * scale;
        ay = h as f32 / 2.0 + v.y * scale;
    }
    s.camera.scale = scale;
    s.camera.anchor_x = ax;
    s.camera.anchor_y = ay;
    let img = render(model, &prepared, &doc.materials, &doc.textures, &s);
    let png = export::png::encode(&img);
    Ok(Query {
        result: Json::obj()
            .with("model", model.name.as_str())
            .with("width", w)
            .with("height", h)
            .with("scale", scale as f64)
            .with("faces", prepared.faces.len())
            .with("mimeType", "image/png")
            .with("bytes", png.len()),
        binary: png,
    })
}

struct Bake {
    images: Vec<(String, f32, u32, crate::renderer::Image)>,
    frame_ms: u32,
    frames: u32,
    scale: f32,
    anim_name: String,
}

/// Frame-by-frame 3D animation: every listed model is one frame (VFX, morphs, hand-keyed poses).
fn bake_sequence(doc: &Document, cache: &mut RenderCache, a: &Json, list: &[Json]) -> CmdResult<Bake> {
    if list.is_empty() || list.len() > 1024 {
        return Err(CmdError::invalid("'models' must list 1-1024 models"));
    }
    let mut indices = Vec::with_capacity(list.len());
    for m in list {
        indices.push(model_index(doc, &Json::obj().with("model", m.clone()))?);
    }
    let (w, h) = render_dims(a, 256)?;
    let dirs = directions(opt_i64(a, "directions")?.unwrap_or(1))?;
    let frame_ms = validate_duration(opt_i64(a, "frame_ms")?.unwrap_or(66))?;
    let margin = opt_f64(a, "margin")?.unwrap_or(6.0) as f32;
    let explicit = opt_f64(a, "scale")?.map(|v| v.clamp(0.05, 64.0) as f32);
    // One shared camera for the whole sequence so the effect does not jitter: fit the union of
    // every frame's extent, keep the origin centered and the ground line fixed.
    let (mut half_w, mut lo_y, mut hi_y) = (0.0f32, f32::MAX, f32::MIN);
    for &mi in &indices {
        let model = &doc.models[mi];
        let prepared = cache.get(model);
        for (_, yaw) in &dirs {
            let mut s = settings_from(doc, model, a, w, h)?;
            s.camera.yaw = *yaw;
            if let Some((x0, x1, y0, y1)) = view_extent(model, &prepared, &s) {
                half_w = half_w.max(x0.abs()).max(x1.abs());
                lo_y = lo_y.min(y0);
                hi_y = hi_y.max(y1);
            }
        }
    }
    if half_w == 0.0 {
        return Err(CmdError::invalid("all models in the sequence are empty"));
    }
    let scale = explicit.unwrap_or_else(|| ((w as f32 / 2.0 - margin) / half_w).min((h as f32 - 2.0 * margin) / (hi_y - lo_y))).max(0.05);
    let (ax, ay) = (w as f32 / 2.0, h as f32 - margin + lo_y * scale);
    let mut images = Vec::new();
    for (dname, yaw) in &dirs {
        for (f, &mi) in indices.iter().enumerate() {
            let model = &doc.models[mi];
            let mut s = settings_from(doc, model, a, w, h)?;
            s.camera.yaw = *yaw;
            s.camera.scale = scale;
            s.camera.anchor_x = ax;
            s.camera.anchor_y = ay;
            if let Some(f) = opt_vec3(a, "focus")? {
                let v = s.camera.view_rotation().apply(Vec3::from_f64(f) - model.origin);
                s.camera.anchor_x = w as f32 / 2.0 - v.x * scale;
                s.camera.anchor_y = h as f32 / 2.0 + v.y * scale;
            }
            let prepared = cache.get(model);
            images.push((dname.to_string(), *yaw, f as u32, render(model, &prepared, &doc.materials, &doc.textures, &s)));
        }
    }
    let name = opt_str(a, "name")?.unwrap_or("sequence").to_string();
    Ok(Bake { images, frame_ms, frames: indices.len() as u32, scale, anim_name: name })
}

fn bake(doc: &Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Bake> {
    if let Some(Json::Arr(list)) = a.get("models") {
        return bake_sequence(doc, cache, a, list);
    }
    let mi = model_index(doc, a)?;
    let model = &doc.models[mi];
    let (w, h) = render_dims(a, 256)?;
    let mut base = settings_from(doc, model, a, w, h)?;
    let dirs = directions(opt_i64(a, "directions")?.unwrap_or(8))?;
    let ai = anim_index(model, a, "animation")?;
    let (frames, frame_ms, anim_name) = match ai {
        Some(i) => {
            let an = &model.animations[i];
            (an.frames, an.frame_ms, an.name.clone())
        }
        None => (1, 100, "pose".to_string()),
    };
    if dirs.len() as u64 * frames as u64 > 4096 {
        return Err(CmdError::new("limit", "too many sprites in one bake (max 4096)"));
    }
    base.animation = ai.map(|i| (i, 0.0));
    let prepared = cache.get(model);
    let margin = opt_f64(a, "margin")?.unwrap_or(6.0) as f32;
    let explicit = opt_f64(a, "scale")?.map(|v| v.clamp(0.05, 64.0) as f32);
    let times: Vec<f32> = (0..frames).map(|f| f as f32).collect();
    let yaws: Vec<f32> = dirs.iter().map(|d| d.1).collect();
    let (scale, ax, ay) = fit(model, &prepared, &base, &yaws, &times, margin, explicit)?;
    base.camera.scale = scale;
    base.camera.anchor_x = ax;
    base.camera.anchor_y = ay;
    let mut images = Vec::with_capacity(dirs.len() * frames as usize);
    for (dname, yaw) in &dirs {
        for f in 0..frames {
            let mut s = base.clone();
            s.camera.yaw = *yaw;
            s.animation = ai.map(|i| (i, f as f32));
            images.push((dname.to_string(), *yaw, f, render(model, &prepared, &doc.materials, &doc.textures, &s)));
        }
    }
    Ok(Bake { images, frame_ms, frames, scale, anim_name })
}

/// Renders directions × frames into one sprite sheet (rows = directions) for any G-buffer pass.
pub fn voxel_render_sheet(doc: &Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Query> {
    let b = bake(doc, cache, a)?;
    let (cw, ch) = (b.images[0].3.width, b.images[0].3.height);
    let rows = b.images.len() as u32 / b.frames;
    if cw * b.frames > 16384 || ch * rows > 16384 {
        return Err(CmdError::new("limit", "sprite sheet would exceed 16384px"));
    }
    let mut sheet = crate::renderer::Image::new(cw * b.frames, ch * rows);
    let mut cells = Vec::new();
    for (k, (dir, yaw, f, img)) in b.images.iter().enumerate() {
        let (x, y) = ((k as u32 % b.frames) * cw, (k as u32 / b.frames) * ch);
        sheet.blit(img, x, y);
        cells.push(
            Json::obj()
                .with("direction", dir.as_str())
                .with("yaw", *yaw as f64)
                .with("frame", *f)
                .with("x", x)
                .with("y", y)
                .with("w", cw)
                .with("h", ch)
                .with("duration", b.frame_ms),
        );
    }
    let png = export::png::encode(&sheet);
    Ok(Query {
        result: Json::obj()
            .with("animation", b.anim_name.as_str())
            .with("pass", opt_str(a, "pass")?.unwrap_or("lit"))
            .with("columns", b.frames)
            .with("rows", rows)
            .with("cellWidth", cw)
            .with("cellHeight", ch)
            .with("scale", b.scale as f64)
            .with("cells", Json::Arr(cells))
            .with("mimeType", "image/png")
            .with("bytes", png.len()),
        binary: png,
    })
}

/// Bakes directions × frames into the 2D project as frames on one layer, tagged per direction
/// (e.g. walk_S, walk_SE, ...), so the sprites can be polished pixel by pixel.
pub fn voxel_render_to_frames(doc: &mut Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Outcome> {
    let b = bake(doc, cache, a)?;
    let (w, h) = (b.images[0].3.width, b.images[0].3.height);
    if doc.frames.len() + b.images.len() > MAX_FRAMES {
        return Err(CmdError::new("limit", "frame safety limit reached"));
    }
    let resize = opt_bool(a, "resize_canvas")?.unwrap_or(true);
    if (doc.width, doc.height) != (w, h) {
        if !resize {
            return Err(CmdError::invalid(format!("canvas is {}x{} but the render is {w}x{h}; set resize_canvas", doc.width, doc.height)));
        }
        let (w2, h2) = validate_size(w as i64, h as i64)?;
        doc.resize(w2, h2, 0, 0);
    }
    let layer_name = opt_str(a, "layer")?.unwrap_or("3D Render").to_string();
    let layer_id = match doc.layers.iter().find(|l| l.name == layer_name) {
        Some(l) => l.id,
        None => doc.insert_layer(&layer_name, doc.layers.len()),
    };
    // Reuse a single, completely empty starting frame instead of leaving a blank frame in front.
    let reuse_first = doc.frames.len() == 1 && doc.cels.values().all(|c| c.is_empty());
    let start = if reuse_first { 0 } else { doc.frames.len() };
    let mut tag_ranges: HashMap<String, (u32, u32)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for (k, (dir, _, _, img)) in b.images.iter().enumerate() {
        let index = start + k;
        let fid = if reuse_first && k == 0 {
            doc.frames[0].duration_ms = b.frame_ms;
            doc.frames[0].id
        } else {
            doc.insert_frame(index, b.frame_ms)
        };
        let mut grid = PixelGrid::new(doc.width, doc.height);
        for y in 0..img.height {
            for x in 0..img.width {
                let c = img.get(x, y);
                if c.a > 0 {
                    grid.set(x as i64, y as i64, c.to_pixel());
                }
            }
        }
        doc.cels.insert((layer_id, fid), Arc::new(grid));
        let tag = format!("{}_{}", b.anim_name, dir);
        let e = tag_ranges.entry(tag.clone()).or_insert((fid, fid));
        e.1 = fid;
        if !order.contains(&tag) {
            order.push(tag);
        }
    }
    let mut tags = Vec::new();
    for name in order {
        let (from, to) = tag_ranges[&name];
        let mut unique = name.clone();
        let mut n = 2;
        while doc.tags.iter().any(|t| t.name == unique) {
            unique = format!("{name}_{n}");
            n += 1;
        }
        doc.tags.push(crate::animation::Tag {
            name: unique.clone(),
            from_id: from,
            to_id: to,
            direction: crate::animation::Direction::Forward,
            repeat: 0,
        });
        tags.push(Json::from(unique));
    }
    doc.active_layer = layer_id;
    doc.active_frame = doc.frames[start].id;
    let n = b.images.len();
    Ok(Outcome::new(
        Json::obj()
            .with("framesAdded", n)
            .with("firstFrame", start)
            .with("tags", Json::Arr(tags))
            .with("width", w)
            .with("height", h)
            .with("scale", b.scale as f64)
            .with("layer", layer_name),
        (n as u64) * (w as u64) * (h as u64),
    ))
}

pub fn voxel_pick(doc: &Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Query> {
    let mi = model_index(doc, a)?;
    let model = &doc.models[mi];
    let (w, h) = render_dims(a, 256)?;
    let mut s = settings_from(doc, model, a, w, h)?;
    let prepared = cache.get(model);
    let explicit = opt_f64(a, "scale")?.map(|v| v.clamp(0.05, 64.0) as f32);
    let t = s.animation.map(|(_, t)| t).unwrap_or(0.0);
    let (scale, ax, ay) = fit(model, &prepared, &s, &[s.camera.yaw], &[t], opt_f64(a, "margin")?.unwrap_or(8.0) as f32, explicit)?;
    s.camera.scale = scale;
    s.camera.anchor_x = ax;
    s.camera.anchor_y = ay;
    let (px, py) = (req_i64(a, "px")?, req_i64(a, "py")?);
    let hit = if px < 0 || py < 0 { None } else { pick(model, &prepared, &s, px as u32, py as u32) };
    Ok(Query {
        result: match hit {
            Some((p, n)) => {
                let v = model.get(p[0], p[1], p[2]);
                Json::obj()
                    .with("hit", true)
                    .with("voxel", Json::Arr(p.iter().map(|&c| Json::from(c)).collect()))
                    .with("normal", Json::Arr(n.iter().map(|&c| Json::from(c)).collect()))
                    .with("color", Rgba::from_u32(v.rgba).to_hex())
                    .with("material", v.material)
                    .with("bone", model.bones.get(v.bone as usize).map(|b| b.name.as_str()))
            }
            None => Json::obj().with("hit", false),
        },
        binary: Vec::new(),
    })
}

pub fn voxel_inspect(doc: &Document, a: &Json) -> CmdResult<Query> {
    let models: Vec<Json> = doc
        .models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut j = Json::obj()
                .with("index", i)
                .with("name", m.name.as_str())
                .with("size", Json::Arr(m.size.iter().map(|&d| Json::from(d)).collect()))
                .with("origin", vec_json(m.origin))
                .with("bones", m.bones.len())
                .with(
                    "animations",
                    Json::Arr(
                        m.animations
                            .iter()
                            .map(|x| Json::obj().with("name", x.name.as_str()).with("frames", x.frames).with("frameMs", x.frame_ms))
                            .collect(),
                    ),
                );
            if a.get("model").is_some() || doc.models.len() == 1 {
                j.set("voxels", m.count());
                if let Some((lo, hi)) = m.bounds() {
                    j.set(
                        "bounds",
                        Json::obj()
                            .with("min", Json::Arr(lo.iter().map(|&v| Json::from(v)).collect()))
                            .with("max", Json::Arr(hi.iter().map(|&v| Json::from(v)).collect())),
                    );
                }
                j.set(
                    "skeleton",
                    Json::Arr(
                        m.bones
                            .iter()
                            .map(|b| {
                                Json::obj()
                                    .with("name", b.name.as_str())
                                    .with("parent", b.parent.map(|p| m.bones[p].name.as_str()))
                                    .with("head", vec_json(b.head))
                                    .with("tail", vec_json(b.tail))
                            })
                            .collect(),
                    ),
                );
            }
            j
        })
        .collect();
    let materials = doc
        .materials
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut maps = Json::obj();
            for (name, slot) in MAP_SLOTS {
                if let Some(t) = m.maps[slot as usize] {
                    maps.set(name, doc.textures[t].name.as_str());
                }
            }
            Json::obj()
                .with("index", i)
                .with("name", m.name.as_str())
                .with("roughness", m.roughness as f64)
                .with("metallic", m.metallic as f64)
                .with("emission", m.emission as f64)
                .with("uvScale", m.uv_scale as f64)
                .with("maps", maps)
        })
        .collect();
    let textures = doc
        .textures
        .iter()
        .enumerate()
        .map(|(i, t)| Json::obj().with("index", i).with("name", t.name.as_str()).with("width", t.width).with("height", t.height))
        .collect();
    Ok(Query {
        result: Json::obj().with("models", Json::Arr(models)).with("materials", Json::Arr(materials)).with("textures", Json::Arr(textures)),
        binary: Vec::new(),
    })
}

pub fn voxel_get(doc: &Document, a: &Json) -> CmdResult<Query> {
    let mi = model_index(doc, a)?;
    let m = &doc.models[mi];
    let (x, y, z) = (req_i64(a, "x")?, req_i64(a, "y")?, req_i64(a, "z")?);
    let v = m.get(x as i32, y as i32, z as i32);
    let result = if v.is_empty() {
        Json::obj().with("empty", true)
    } else {
        Json::obj()
            .with("empty", false)
            .with("color", Rgba::from_u32(v.rgba).to_hex())
            .with("material", doc.materials.get(v.material as usize).map(|m| m.name.as_str()))
            .with("bone", m.bones.get(v.bone as usize).map(|b| b.name.as_str()))
    };
    Ok(Query { result, binary: Vec::new() })
}

/// Pastes a PNG into a 2D cel (reference images, imported art).
pub fn image_import(doc: &mut Document, a: &Json, payload: &[u8]) -> CmdResult<Outcome> {
    let png = binary_input(a, payload)?;
    let img = export::png::decode(&png).map_err(|e| CmdError::new("invalid_file", e))?;
    let li = layer_index(doc, a, "layer")?;
    let fi = frame_index(doc, a, "frame")?;
    if doc.layers[li].locked {
        return Err(CmdError::new("locked", "layer is locked"));
    }
    let (lid, fid) = (doc.layers[li].id, doc.frames[fi].id);
    let (ox, oy) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
    let grid = doc.cel_mut(lid, fid);
    let mut changed = 0u64;
    for y in 0..img.height {
        for x in 0..img.width {
            let c = img.get(x, y);
            if c.a > 0 {
                changed += grid.set(ox + x as i64, oy + y as i64, c.to_pixel()) as u64;
            }
        }
    }
    Ok(Outcome::new(Json::obj().with("width", img.width).with("height", img.height).with("changed", changed), changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64() {
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(base64_decode("data:image/png;base64,SGk=").unwrap(), b"Hi");
        assert!(base64_decode("!!").is_err());
    }
}
