//! PBR materials and textures for voxels.
//!
//! Every voxel has its own true color (albedo) and a material index. A material adds the PBR
//! parameters (roughness, metallic, emission) and optional texture maps. Textures are mapped onto
//! voxel faces with planar UVs per face axis (X faces use z/y, Y faces x/z, Z faces x/y), so a
//! texture "wraps" a voxel model like a UV-unwrapped box, `uv_scale` texels per voxel.

use std::sync::Arc;

use super::math::Vec3;
use super::model::{hash01, value_noise};

/// Texture map slots of a material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapSlot {
    /// Multiplies the voxel color (RGB) — color variation, painted detail.
    Albedo = 0,
    /// Tangent-space normal map (RGB = XYZ * 0.5 + 0.5).
    Normal = 1,
    /// Roughness multiplier (R channel).
    Roughness = 2,
    /// Metallic multiplier (R channel).
    Metallic = 3,
    /// Ambient occlusion multiplier (R channel).
    Occlusion = 4,
    /// Height (R channel) — deepens crevices in the occlusion term.
    Height = 5,
    /// Emission color (RGB), scaled by the material's emission strength.
    Emission = 6,
}

pub const MAP_SLOTS: [(&str, MapSlot); 7] = [
    ("albedo", MapSlot::Albedo),
    ("normal", MapSlot::Normal),
    ("roughness", MapSlot::Roughness),
    ("metallic", MapSlot::Metallic),
    ("occlusion", MapSlot::Occlusion),
    ("height", MapSlot::Height),
    ("emission", MapSlot::Emission),
];

pub fn slot_by_name(name: &str) -> Option<MapSlot> {
    let n = name.to_ascii_lowercase();
    let n = match n.as_str() {
        "ao" | "ambient_occlusion" => "occlusion",
        "basecolor" | "base_color" | "diffuse" | "color" => "albedo",
        "metalness" => "metallic",
        "displacement" | "bump" => "height",
        "emissive" => "emission",
        other => other,
    };
    MAP_SLOTS.iter().find(|(s, _)| *s == n).map(|(_, slot)| *slot)
}

/// RGBA8 image used as a material map.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Texture {
    /// Nearest sample with wrapping; returns RGBA in 0..1.
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let x = (u.floor() as i64).rem_euclid(self.width as i64) as usize;
        let y = (v.floor() as i64).rem_euclid(self.height as i64) as usize;
        let i = (y * self.width as usize + x) * 4;
        let d = &self.data[i..i + 4];
        [d[0] as f32 / 255.0, d[1] as f32 / 255.0, d[2] as f32 / 255.0, d[3] as f32 / 255.0]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub roughness: f32,
    pub metallic: f32,
    /// Emission strength (0 = none). Emission color = emission map or the voxel color.
    pub emission: f32,
    /// Texels per voxel for all maps.
    pub uv_scale: f32,
    /// Normal map strength (0..2).
    pub normal_strength: f32,
    /// Texture index (into the project's textures) per slot.
    pub maps: [Option<usize>; 7],
}

impl Material {
    pub fn new(name: &str) -> Material {
        Material { name: name.into(), roughness: 0.7, metallic: 0.0, emission: 0.0, uv_scale: 1.0, normal_strength: 1.0, maps: [None; 7] }
    }
}

/// Everything the shader needs for one voxel face.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceSample {
    pub albedo_mul: Vec3,
    pub normal_ts: Vec3,
    pub roughness: f32,
    pub metallic: f32,
    pub occlusion: f32,
    pub emission: Vec3,
    pub has_emission_map: bool,
}

/// Samples all maps of a material at a voxel face. `axis` = face axis (0 x, 1 y, 2 z).
pub fn sample_surface(mat: &Material, textures: &[Arc<Texture>], pos: [i32; 3], axis: usize) -> SurfaceSample {
    let (u, v) = match axis {
        0 => (pos[2] as f32, -(pos[1] as f32)),
        1 => (pos[0] as f32, pos[2] as f32),
        _ => (pos[0] as f32, -(pos[1] as f32)),
    };
    let (u, v) = (u * mat.uv_scale, v * mat.uv_scale);
    let tex = |slot: MapSlot| mat.maps[slot as usize].and_then(|i| textures.get(i)).map(|t| t.sample(u, v));
    let albedo_mul = tex(MapSlot::Albedo).map(|c| Vec3::new(c[0], c[1], c[2])).unwrap_or(Vec3::new(1.0, 1.0, 1.0));
    let normal_ts = tex(MapSlot::Normal)
        .map(|c| {
            let n = Vec3::new((c[0] * 2.0 - 1.0) * mat.normal_strength, (c[1] * 2.0 - 1.0) * mat.normal_strength, c[2] * 2.0 - 1.0);
            n.normalized()
        })
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    let roughness = (mat.roughness * tex(MapSlot::Roughness).map(|c| c[0] * 2.0).unwrap_or(1.0)).clamp(0.03, 1.0);
    let metallic = (mat.metallic * tex(MapSlot::Metallic).map(|c| c[0] * 2.0).unwrap_or(1.0)).clamp(0.0, 1.0);
    let mut occlusion = tex(MapSlot::Occlusion).map(|c| c[0]).unwrap_or(1.0);
    if let Some(h) = tex(MapSlot::Height) {
        occlusion *= 0.75 + 0.25 * h[0];
    }
    let em = tex(MapSlot::Emission);
    SurfaceSample {
        albedo_mul,
        normal_ts,
        roughness,
        metallic,
        occlusion,
        emission: em.map(|c| Vec3::new(c[0], c[1], c[2])).unwrap_or(Vec3::new(1.0, 1.0, 1.0)) * mat.emission,
        has_emission_map: em.is_some(),
    }
}

// ---- procedural PBR texture sets ------------------------------------------------------

pub const TEXTURE_KINDS: [&str; 9] =
    ["noise", "brushed_metal", "hammered_metal", "leather", "fabric", "scales", "stone", "wood", "chainmail"];

/// Tileable value noise on a `period`-sized lattice.
fn periodic_noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let p = period.max(1);
    let h = |ix: i32, iy: i32| hash01(ix.rem_euclid(p), iy.rem_euclid(p), 0, seed);
    let (ix, iy) = (x0 as i32, y0 as i32);
    let a = h(ix, iy) * (1.0 - sx) + h(ix + 1, iy) * sx;
    let b = h(ix, iy + 1) * (1.0 - sx) + h(ix + 1, iy + 1) * sx;
    a * (1.0 - sy) + b * sy
}

fn fbm(x: f32, y: f32, period: i32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut f) = (0.0, 0.5, 0.0, 1.0);
    for o in 0..octaves {
        sum += amp * periodic_noise(x * f, y * f, period * f as i32, seed.wrapping_add(o * 101));
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

/// Tileable Worley (cellular) distance to the nearest feature point.
fn worley(x: f32, y: f32, period: i32, seed: u32) -> (f32, f32) {
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let (mut d1, mut d2) = (f32::MAX, f32::MAX);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (gx, gy) = (cx + dx, cy + dy);
            let (wx, wy) = (gx.rem_euclid(period), gy.rem_euclid(period));
            let px = gx as f32 + hash01(wx, wy, 1, seed);
            let py = gy as f32 + hash01(wx, wy, 2, seed);
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2)
}

/// Height field (0..1) of a procedural surface at texel (x, y).
fn height_at(kind: &str, x: f32, y: f32, size: f32, cells: i32, seed: u32) -> f32 {
    let c = cells as f32;
    let (u, v) = (x / size * c, y / size * c);
    match kind {
        "brushed_metal" => 0.6 * fbm(u * 0.25, v * 8.0, (cells / 4).max(1), seed, 3) + 0.4 * periodic_noise(u * 0.1, v * 16.0, 1, seed + 9),
        "hammered_metal" => {
            let (d1, _) = worley(u, v, cells, seed);
            1.0 - (d1 * 1.2).min(1.0).powf(0.6) * 0.8
        }
        "leather" => {
            let (d1, d2) = worley(u * 1.5, v * 1.5, (cells as f32 * 1.5) as i32, seed);
            let crease = ((d2 - d1) * 4.0).min(1.0);
            0.7 * crease + 0.3 * fbm(u * 2.0, v * 2.0, cells * 2, seed + 3, 3)
        }
        "fabric" => {
            let threads = 4.0;
            let wx = ((u * threads * std::f32::consts::PI).sin() * 0.5 + 0.5).powf(0.7);
            let wy = ((v * threads * std::f32::consts::PI).sin() * 0.5 + 0.5).powf(0.7);
            let over = ((u * threads).floor() as i32 + (v * threads).floor() as i32) & 1 == 0;
            0.85 * if over { wx } else { wy } + 0.15 * fbm(u * 4.0, v * 4.0, cells * 4, seed, 2)
        }
        "scales" => {
            let row = (v * 2.0).floor();
            let su = u * 2.0 + if row as i32 & 1 == 1 { 0.5 } else { 0.0 };
            let (fu, fv) = (su.fract() - 0.5, (v * 2.0).fract());
            let d = (fu * fu + (fv - 0.15).powi(2)).sqrt();
            (1.0 - d * 1.6).clamp(0.0, 1.0)
        }
        "stone" => {
            let (d1, d2) = worley(u, v, cells, seed);
            let edge = ((d2 - d1) * 3.0).min(1.0);
            0.65 * edge + 0.35 * fbm(u * 3.0, v * 3.0, cells * 3, seed + 5, 4)
        }
        "wood" => {
            let n = fbm(u * 0.5, v * 4.0, (cells / 2).max(1), seed, 3);
            ((u * 3.0 + n * 4.0) * std::f32::consts::PI).sin() * 0.35 + 0.5 + (n - 0.5) * 0.3
        }
        "chainmail" => {
            let (fu, fv) = ((u * 2.0).fract() - 0.5, (v * 2.0 + if (u * 2.0).floor() as i32 & 1 == 1 { 0.5 } else { 0.0 }).fract() - 0.5);
            let r = (fu * fu + fv * fv).sqrt();
            (1.0 - ((r - 0.32).abs() * 9.0)).clamp(0.0, 1.0)
        }
        _ => fbm(u, v, cells, seed, 5),
    }
}

/// A generated set of PBR maps (all tileable).
pub struct TextureSet {
    pub albedo: Texture,
    pub normal: Texture,
    pub roughness: Texture,
    pub occlusion: Texture,
    pub height: Texture,
}

/// Generates a tileable PBR texture set. `cells` = feature count across the tile.
pub fn generate_set(name: &str, kind: &str, size: u32, cells: i32, seed: u32, base_roughness: f32, contrast: f32) -> TextureSet {
    let n = size as usize;
    let mut h = vec![0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            h[y * n + x] = height_at(kind, x as f32 + 0.5, y as f32 + 0.5, size as f32, cells.max(1), seed).clamp(0.0, 1.0);
        }
    }
    let at = |x: i64, y: i64| h[(y.rem_euclid(n as i64) as usize) * n + x.rem_euclid(n as i64) as usize];
    let img = |f: &dyn Fn(usize, usize) -> [u8; 4], suffix: &str| {
        let mut data = Vec::with_capacity(n * n * 4);
        for y in 0..n {
            for x in 0..n {
                data.extend_from_slice(&f(x, y));
            }
        }
        Texture { name: format!("{name}_{suffix}"), width: size, height: size, data }
    };
    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let albedo = img(
        &|x, y| {
            let v = 1.0 - contrast * 0.5
                + contrast * at(x as i64, y as i64)
                + (value_noise(Vec3::new(x as f32 * 0.7, y as f32 * 0.7, 0.0), seed + 77) - 0.5) * 0.06;
            let g = to8(v * 0.92);
            [g, g, g, 255]
        },
        "albedo",
    );
    let normal = img(
        &|x, y| {
            let (xi, yi) = (x as i64, y as i64);
            let dx = (at(xi + 1, yi - 1) + 2.0 * at(xi + 1, yi) + at(xi + 1, yi + 1))
                - (at(xi - 1, yi - 1) + 2.0 * at(xi - 1, yi) + at(xi - 1, yi + 1));
            let dy = (at(xi - 1, yi + 1) + 2.0 * at(xi, yi + 1) + at(xi + 1, yi + 1))
                - (at(xi - 1, yi - 1) + 2.0 * at(xi, yi - 1) + at(xi + 1, yi - 1));
            let nrm = Vec3::new(-dx * 1.5, dy * 1.5, 1.0).normalized();
            [to8(nrm.x * 0.5 + 0.5), to8(nrm.y * 0.5 + 0.5), to8(nrm.z * 0.5 + 0.5), 255]
        },
        "normal",
    );
    let roughness = img(
        &|x, y| {
            let r = to8((base_roughness * (0.8 + 0.4 * (1.0 - at(x as i64, y as i64)))) / 2.0);
            [r, r, r, 255]
        },
        "roughness",
    );
    let occlusion = img(
        &|x, y| {
            let (xi, yi) = (x as i64, y as i64);
            let mut blur = 0.0;
            for dy in -2..=2 {
                for dx in -2..=2 {
                    blur += at(xi + dx, yi + dy);
                }
            }
            let cavity = (blur / 25.0 - at(xi, yi)).max(0.0);
            let a = to8(1.0 - cavity * 2.5);
            [a, a, a, 255]
        },
        "occlusion",
    );
    let height = img(
        &|x, y| {
            let v = to8(at(x as i64, y as i64));
            [v, v, v, 255]
        },
        "height",
    );
    TextureSet { albedo, normal, roughness, occlusion, height }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_sets_tile_and_vary() {
        for kind in TEXTURE_KINDS {
            let set = generate_set("t", kind, 32, 4, 7, 0.6, 0.5);
            assert_eq!(set.albedo.data.len(), 32 * 32 * 4);
            let first = set.height.data[0];
            assert!(set.height.data.chunks(4).any(|p| p[0] != first), "{kind} must not be flat");
            // Sampling wraps.
            assert_eq!(set.normal.sample(32.0, 0.0), set.normal.sample(0.0, 0.0));
        }
        assert_eq!(slot_by_name("AO"), Some(MapSlot::Occlusion));
        assert_eq!(slot_by_name("basecolor"), Some(MapSlot::Albedo));
    }

    #[test]
    fn surface_sampling_uses_maps() {
        let set = generate_set("m", "stone", 16, 2, 1, 0.8, 0.6);
        let textures = vec![Arc::new(set.albedo), Arc::new(set.normal)];
        let mut mat = Material::new("stone");
        mat.maps[MapSlot::Albedo as usize] = Some(0);
        mat.maps[MapSlot::Normal as usize] = Some(1);
        let a = sample_surface(&mat, &textures, [3, 5, 1], 2);
        let b = sample_surface(&mat, &textures, [9, 2, 1], 2);
        assert!(a.albedo_mul != b.albedo_mul || a.normal_ts != b.normal_ts);
        assert!((a.normal_ts.length() - 1.0).abs() < 1e-3);
    }
}
