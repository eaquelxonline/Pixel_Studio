//! Voxel renderer.
//!
//! * Rest-pose surface extraction: exposed faces with smooth normals, two-scale ambient occlusion
//!   and MagicaVoxel-style per-corner occlusion (soft darkening in creases and inner corners).
//! * Rigid bone skinning and orthographic rasterization with sub-voxel UVs.
//! * PBR (or toon) shading in view space: key/fill/rim lights with a shadow map (PCF), point lights
//!   (fires, windows, magic), studio reflections, emission with bloom.
//! * G-buffer passes, supersampling, selective outline, inner lines, gradient backgrounds and a
//!   ground shadow catcher for transparent sprites.

use std::sync::Arc;

use super::material::{Material, Texture, sample_surface};
use super::math::{Affine, Vec3};
use super::model::{Voxel, VoxelModel};
use super::rig::pose;
use crate::pixel::Rgba;
use crate::renderer::Image;

/// Face directions: +X, -X, +Y, -Y, +Z, -Z.
pub const DIRS: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];

/// One exposed voxel face in rest pose.
#[derive(Debug, Clone, Copy)]
pub struct Face {
    pub pos: [i32; 3],
    pub dir: u8,
    pub voxel: Voxel,
    /// Smooth surface normal (model space) of the voxel.
    pub smooth: Vec3,
    /// Neighbourhood ambient occlusion 0..1 (1 = open).
    pub ao: f32,
    /// Per-corner occlusion in `face_corners` order, interpolated across the face.
    pub corner_ao: [f32; 4],
}

/// Cached render geometry of a model version.
pub struct Prepared {
    pub version: u64,
    pub faces: Vec<Face>,
}

/// Dense occupancy bitset for fast neighbourhood queries.
struct Occupancy {
    size: [i32; 3],
    bits: Vec<u64>,
}

impl Occupancy {
    fn build(model: &VoxelModel) -> Occupancy {
        let size = [model.size[0] as i32, model.size[1] as i32, model.size[2] as i32];
        let total = (size[0] as usize) * (size[1] as usize) * (size[2] as usize);
        let mut occ = Occupancy { size, bits: vec![0; total.div_ceil(64)] };
        model.for_each(|x, y, z, _| {
            let i = occ.index(x, y, z);
            occ.bits[i >> 6] |= 1 << (i & 63);
        });
        occ
    }

    #[inline]
    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        ((z as usize * self.size[1] as usize) + y as usize) * self.size[0] as usize + x as usize
    }

    #[inline]
    fn get(&self, x: i32, y: i32, z: i32) -> bool {
        if x < 0 || y < 0 || z < 0 || x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return false;
        }
        let i = self.index(x, y, z);
        self.bits[i >> 6] >> (i & 63) & 1 == 1
    }
}

/// Coarse (4×) occupancy fractions for large-scale occlusion (armpits, under the cape, ...).
struct Coarse {
    size: [i32; 3],
    fill: Vec<f32>,
}

const COARSE: i32 = 4;

impl Coarse {
    fn build(occ: &Occupancy) -> Coarse {
        let size = [(occ.size[0] + COARSE - 1) / COARSE, (occ.size[1] + COARSE - 1) / COARSE, (occ.size[2] + COARSE - 1) / COARSE];
        let mut fill = vec![0f32; (size[0] * size[1] * size[2]) as usize];
        for cz in 0..size[2] {
            for cy in 0..size[1] {
                for cx in 0..size[0] {
                    let mut n = 0;
                    for z in 0..COARSE {
                        for y in 0..COARSE {
                            for x in 0..COARSE {
                                n += occ.get(cx * COARSE + x, cy * COARSE + y, cz * COARSE + z) as i32;
                            }
                        }
                    }
                    fill[((cz * size[1] + cy) * size[0] + cx) as usize] = n as f32 / (COARSE * COARSE * COARSE) as f32;
                }
            }
        }
        Coarse { size, fill }
    }

    fn get(&self, x: i32, y: i32, z: i32) -> f32 {
        if x < 0 || y < 0 || z < 0 || x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return 0.0;
        }
        self.fill[((z * self.size[1] + y) * self.size[0] + x) as usize]
    }
}

const CORNER_AO: [f32; 4] = [0.46, 0.66, 0.84, 1.0];

/// Extracts exposed faces with smooth normals and ambient occlusion.
pub fn prepare(model: &VoxelModel) -> Prepared {
    let occ = Occupancy::build(model);
    let coarse = Coarse::build(&occ);
    let mut faces = Vec::new();
    model.for_each(|x, y, z, v| {
        let exposed: Vec<u8> = (0..6u8)
            .filter(|&d| {
                let o = DIRS[d as usize];
                !occ.get(x + o[0], y + o[1], z + o[2])
            })
            .collect();
        if exposed.is_empty() {
            return;
        }
        // Smooth normal: points away from the occupied neighbourhood (radius 2).
        let mut g = Vec3::ZERO;
        for dz in -2..=2i32 {
            for dy in -2..=2i32 {
                for dx in -2..=2i32 {
                    if (dx, dy, dz) != (0, 0, 0) && occ.get(x + dx, y + dy, z + dz) {
                        let w = 1.0 / ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
                        g = g - Vec3::new(dx as f32, dy as f32, dz as f32) * w;
                    }
                }
            }
        }
        let face_sum = exposed.iter().fold(Vec3::ZERO, |a, &d| {
            let o = DIRS[d as usize];
            a + Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32)
        });
        let smooth =
            if g.length() < 0.5 { face_sum.normalized() } else { (g.normalized() * 0.8 + face_sum.normalized() * 0.2).normalized() };
        for d in exposed {
            let o = DIRS[d as usize];
            // Fine AO: occupancy of the 5×5×5 block in front of the face.
            let (fx, fy, fz) = (x + o[0] * 2, y + o[1] * 2, z + o[2] * 2);
            let mut n = 0;
            for dz in -2..=2 {
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        n += occ.get(fx + dx, fy + dy, fz + dz) as i32;
                    }
                }
            }
            let fine = 1.0 - (n as f32 / 125.0 * 1.6).min(0.85);
            // Coarse AO: 3×3×3 coarse cells around a point ~6 voxels in front of the face.
            let (cx, cy, cz) = ((x + o[0] * 6).div_euclid(COARSE), (y + o[1] * 6).div_euclid(COARSE), (z + o[2] * 6).div_euclid(COARSE));
            let mut c = 0.0;
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        c += coarse.get(cx + dx, cy + dy, cz + dz);
                    }
                }
            }
            let broad = 1.0 - (c / 27.0 * 1.4).min(0.7);
            // Per-corner AO (classic voxel rule: two sides + diagonal of the cell in front).
            let front = [x + o[0], y + o[1], z + o[2]];
            let tangents: Vec<usize> = (0..3).filter(|&k| o[k] == 0).collect();
            let mut corner_ao = [1.0; 4];
            for (k, c) in face_corners(d).iter().enumerate() {
                let (ta, tb) = (tangents[0], tangents[1]);
                let sa = if c[ta] > 0.5 { 1 } else { -1 };
                let sb = if c[tb] > 0.5 { 1 } else { -1 };
                let at = |da: i32, db: i32| {
                    let mut p = front;
                    p[ta] += da;
                    p[tb] += db;
                    occ.get(p[0], p[1], p[2])
                };
                let (s1, s2, cn) = (at(sa, 0), at(0, sb), at(sa, sb));
                let level = if s1 && s2 { 0 } else { 3 - (s1 as usize + s2 as usize + cn as usize) };
                corner_ao[k] = CORNER_AO[level];
            }
            faces.push(Face { pos: [x, y, z], dir: d, voxel: v, smooth, ao: (fine * broad).clamp(0.08, 1.0), corner_ao });
        }
    });
    Prepared { version: model.version, faces }
}

// ---- camera, lights, settings ---------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// Turntable angle in degrees. 0 = the model's front (+Z) faces the viewer.
    pub yaw: f32,
    /// Elevation in degrees (0 = side view, 90 = top-down).
    pub pitch: f32,
    /// Pixels per voxel.
    pub scale: f32,
    pub width: u32,
    pub height: u32,
    /// Screen position (pixels) of the model's origin.
    pub anchor_x: f32,
    pub anchor_y: f32,
}

impl Camera {
    /// Rotation from model space (relative to the origin) into view space (x right, y up, z toward viewer).
    pub fn view_rotation(&self) -> Affine {
        let yaw = Affine::rotation_euler(Vec3::new(0.0, self.yaw, 0.0));
        let pitch = Affine::rotation_euler(Vec3::new(self.pitch, 0.0, 0.0));
        pitch.then(&yaw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Lit,
    Albedo,
    Normal,
    Depth,
    Occlusion,
    Roughness,
    Metallic,
    Emission,
}

impl Pass {
    pub fn parse(s: &str) -> Option<Pass> {
        Some(match s.to_ascii_lowercase().as_str() {
            "lit" | "beauty" | "final" => Pass::Lit,
            "albedo" | "basecolor" | "color" => Pass::Albedo,
            "normal" | "normals" => Pass::Normal,
            "depth" => Pass::Depth,
            "ao" | "occlusion" => Pass::Occlusion,
            "roughness" => Pass::Roughness,
            "metallic" | "metalness" => Pass::Metallic,
            "emission" | "emissive" => Pass::Emission,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outline {
    None,
    /// Selective outline: each edge pixel is a darkened, cooler version of the color it borders.
    Selective,
    Color(Rgba),
}

#[derive(Debug, Clone, Copy)]
pub struct Light {
    /// Direction the light comes FROM, in view space.
    pub dir: Vec3,
    pub color: Vec3,
    pub intensity: f32,
}

/// Local light in model space (campfires, lanterns, windows, magic).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    pub pos: Vec3,
    /// Linear RGB.
    pub color: Vec3,
    pub intensity: f32,
    /// Distance (voxels) at which the light fades to zero.
    pub radius: f32,
}

#[derive(Debug, Clone)]
pub struct Lighting {
    pub lights: Vec<Light>,
    pub points: Vec<PointLight>,
    pub sky: Vec3,
    pub ground: Vec3,
    pub exposure: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            lights: vec![
                Light { dir: Vec3::new(-0.55, 0.7, 0.55).normalized(), color: Vec3::new(1.0, 0.94, 0.85), intensity: 3.2 },
                Light { dir: Vec3::new(0.75, 0.15, 0.4).normalized(), color: Vec3::new(0.55, 0.65, 1.0), intensity: 0.9 },
                Light { dir: Vec3::new(0.3, 0.45, -0.9).normalized(), color: Vec3::new(0.85, 0.9, 1.0), intensity: 2.0 },
            ],
            points: Vec::new(),
            sky: Vec3::new(0.32, 0.36, 0.48),
            ground: Vec3::new(0.16, 0.13, 0.12),
            exposure: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderSettings {
    pub camera: Camera,
    /// (animation index, time in frames)
    pub animation: Option<(usize, f32)>,
    pub pass: Pass,
    /// 0 = flat voxel faces ("3D pixel" look), 1 = fully smooth normals (sprite look).
    pub smoothness: f32,
    /// 0 = physically based; N ≥ 2 = toon shading with N light bands.
    pub toon_steps: u32,
    pub outline: Outline,
    /// Darken depth discontinuities inside the silhouette.
    pub inner_lines: bool,
    pub supersample: u32,
    pub background: Option<Rgba>,
    /// Vertical background gradient (top, bottom) with a soft vignette.
    pub background_gradient: Option<(Rgba, Rgba)>,
    pub lighting: Lighting,
    /// Cast shadows from the key light (0 = off, 1 = full strength).
    pub shadows: f32,
    /// Ground shadow under the model for transparent sprites (alpha 0..1, 0 = off).
    pub ground_shadow: f32,
    /// Glow strength around emissive surfaces (0 = off).
    pub bloom: f32,
    /// Glow radius in output pixels (0 = automatic).
    pub bloom_radius: f32,
}

impl RenderSettings {
    pub fn new(camera: Camera) -> RenderSettings {
        RenderSettings {
            camera,
            animation: None,
            pass: Pass::Lit,
            smoothness: 0.6,
            toon_steps: 0,
            outline: Outline::None,
            inner_lines: false,
            supersample: 1,
            background: None,
            background_gradient: None,
            lighting: Lighting::default(),
            shadows: 0.0,
            ground_shadow: 0.0,
            bloom: 0.0,
            bloom_radius: 0.0,
        }
    }
}

// ---- rasterization --------------------------------------------------------------------

struct Target {
    w: usize,
    h: usize,
    depth: Vec<f32>,
    face: Vec<u32>,
    uv: Vec<[f32; 2]>,
}

const NO_FACE: u32 = u32::MAX;

impl Target {
    fn new(w: usize, h: usize, with_uv: bool) -> Target {
        Target {
            w,
            h,
            depth: vec![f32::NEG_INFINITY; w * h],
            face: vec![NO_FACE; w * h],
            uv: if with_uv { vec![[0.0; 2]; w * h] } else { Vec::new() },
        }
    }

    #[inline]
    fn edge(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
        (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
    }

    fn triangle(&mut self, p: [(f32, f32, f32); 3], uv: [[f32; 2]; 3], id: u32) {
        let area = Self::edge((p[0].0, p[0].1), (p[1].0, p[1].1), (p[2].0, p[2].1));
        if area.abs() < 1e-9 {
            return;
        }
        let minx = p.iter().map(|v| v.0).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let maxx = p.iter().map(|v| v.0).fold(f32::MIN, f32::max).ceil().min(self.w as f32 - 1.0) as i32;
        let miny = p.iter().map(|v| v.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let maxy = p.iter().map(|v| v.1).fold(f32::MIN, f32::max).ceil().min(self.h as f32 - 1.0) as i32;
        if minx > maxx || miny > maxy {
            return;
        }
        let inv = 1.0 / area;
        let with_uv = !self.uv.is_empty();
        for y in miny..=maxy {
            for x in minx..=maxx {
                let c = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = Self::edge((p[1].0, p[1].1), (p[2].0, p[2].1), c) * inv;
                let w1 = Self::edge((p[2].0, p[2].1), (p[0].0, p[0].1), c) * inv;
                let w2 = 1.0 - w0 - w1;
                // Small epsilon keeps shared edges watertight.
                if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                    continue;
                }
                let z = w0 * p[0].2 + w1 * p[1].2 + w2 * p[2].2;
                let i = y as usize * self.w + x as usize;
                if z > self.depth[i] {
                    self.depth[i] = z;
                    self.face[i] = id;
                    if with_uv {
                        self.uv[i] = [
                            (w0 * uv[0][0] + w1 * uv[1][0] + w2 * uv[2][0]).clamp(0.0, 1.0),
                            (w0 * uv[0][1] + w1 * uv[1][1] + w2 * uv[2][1]).clamp(0.0, 1.0),
                        ];
                    }
                }
            }
        }
    }
}

/// Face corner offsets (unit cube) for each direction. The four corners form a parallelogram:
/// corner 0 = uv (0,0), 1 = (1,0), 2 = (1,1), 3 = (0,1).
fn face_corners(dir: u8) -> [[f32; 3]; 4] {
    match dir {
        0 => [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]],
        1 => [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]],
        2 => [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]],
        3 => [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
        4 => [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]],
        _ => [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
    }
}

const QUAD_UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// Model-space point on a face at (u, v).
fn face_point(f: &Face, u: f32, v: f32) -> Vec3 {
    let c = face_corners(f.dir);
    let at = |k: usize| Vec3::new(c[k][0], c[k][1], c[k][2]);
    let base = Vec3::new(f.pos[0] as f32, f.pos[1] as f32, f.pos[2] as f32);
    base + at(0) + (at(1) - at(0)) * u + (at(3) - at(0)) * v
}

/// Camera rotation composed with the origin shift (model space → view space).
fn camera_view(model: &VoxelModel, settings: &RenderSettings) -> Affine {
    settings.camera.view_rotation().then(&Affine::translation(-model.origin))
}

/// Per-bone transform from model space into view space (before projection).
fn bone_views(model: &VoxelModel, settings: &RenderSettings) -> Vec<Affine> {
    let view = camera_view(model, settings);
    let anim = settings.animation.and_then(|(i, t)| model.animations.get(i).map(|a| (a, t)));
    let world = pose(&model.bones, anim.map(|(a, _)| a), anim.map(|(_, t)| t).unwrap_or(0.0));
    if world.is_empty() { vec![view] } else { world.iter().map(|w| view.then(w)).collect() }
}

/// Screen-space extent of the posed model: (min x, max x, min y, max y) in view units.
pub fn view_extent(model: &VoxelModel, prepared: &Prepared, settings: &RenderSettings) -> Option<(f32, f32, f32, f32)> {
    let views = bone_views(model, settings);
    let mut e = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    let mut any = false;
    for f in &prepared.faces {
        let m = &views[(f.voxel.bone as usize).min(views.len() - 1)];
        let c = m.apply(Vec3::new(f.pos[0] as f32 + 0.5, f.pos[1] as f32 + 0.5, f.pos[2] as f32 + 0.5));
        any = true;
        e = (e.0.min(c.x - 0.9), e.1.max(c.x + 0.9), e.2.min(c.y - 0.9), e.3.max(c.y + 0.9));
    }
    any.then_some(e)
}

fn rasterize(model: &VoxelModel, prepared: &Prepared, settings: &RenderSettings, ss: u32, with_uv: bool) -> (Target, Vec<Affine>) {
    let cam = &settings.camera;
    let (w, h) = ((cam.width * ss) as usize, (cam.height * ss) as usize);
    let mut target = Target::new(w, h, with_uv);
    let views = bone_views(model, settings);
    let s = cam.scale * ss as f32;
    let (ax, ay) = (cam.anchor_x * ss as f32, cam.anchor_y * ss as f32);
    for (id, f) in prepared.faces.iter().enumerate() {
        let m = &views[(f.voxel.bone as usize).min(views.len() - 1)];
        let o = DIRS[f.dir as usize];
        let n = m.apply_vector(Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32));
        if n.z <= 1e-4 {
            continue;
        }
        let base = Vec3::new(f.pos[0] as f32, f.pos[1] as f32, f.pos[2] as f32);
        let corners = face_corners(f.dir).map(|c| {
            let p = m.apply(base + Vec3::new(c[0], c[1], c[2]));
            (ax + p.x * s, ay - p.y * s, p.z)
        });
        target.triangle([corners[0], corners[1], corners[2]], [QUAD_UV[0], QUAD_UV[1], QUAD_UV[2]], id as u32);
        target.triangle([corners[0], corners[2], corners[3]], [QUAD_UV[0], QUAD_UV[2], QUAD_UV[3]], id as u32);
    }
    (target, views)
}

// ---- shadow map -------------------------------------------------------------------------

/// Orthographic depth map from the key light, in view space.
struct ShadowMap {
    lx: Vec3,
    ly: Vec3,
    lz: Vec3,
    min_x: f32,
    min_y: f32,
    texel: f32,
    target: Target,
}

impl ShadowMap {
    fn build(prepared: &Prepared, views: &[Affine], light_dir: Vec3) -> Option<ShadowMap> {
        let lz = light_dir.normalized();
        let up = if lz.y.abs() > 0.95 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
        let lx = up.cross(lz).normalized();
        let ly = lz.cross(lx);
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for f in &prepared.faces {
            let m = &views[(f.voxel.bone as usize).min(views.len() - 1)];
            let c = m.apply(Vec3::new(f.pos[0] as f32 + 0.5, f.pos[1] as f32 + 0.5, f.pos[2] as f32 + 0.5));
            let (x, y) = (c.dot(lx), c.dot(ly));
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
        if x0 > x1 {
            return None;
        }
        let (x0, y0) = (x0 - 2.0, y0 - 2.0);
        let extent = (x1 + 2.0 - x0).max(y1 + 2.0 - y0);
        // Two texels per voxel, capped for very large models.
        let texel = (extent / 2048.0).max(0.5);
        let n = (extent / texel).ceil() as usize + 2;
        let mut target = Target::new(n, n, false);
        for (id, f) in prepared.faces.iter().enumerate() {
            let m = &views[(f.voxel.bone as usize).min(views.len() - 1)];
            let base = Vec3::new(f.pos[0] as f32, f.pos[1] as f32, f.pos[2] as f32);
            let c = face_corners(f.dir).map(|c| {
                let p = m.apply(base + Vec3::new(c[0], c[1], c[2]));
                ((p.dot(lx) - x0) / texel, (p.dot(ly) - y0) / texel, p.dot(lz))
            });
            target.triangle([c[0], c[1], c[2]], [[0.0; 2]; 3], id as u32);
            target.triangle([c[0], c[2], c[3]], [[0.0; 2]; 3], id as u32);
        }
        Some(ShadowMap { lx, ly, lz, min_x: x0, min_y: y0, texel, target })
    }

    /// 0 = fully shadowed, 1 = lit. 3×3 percentage-closer filtering.
    fn visibility(&self, p: Vec3) -> f32 {
        let fx = (p.dot(self.lx) - self.min_x) / self.texel;
        let fy = (p.dot(self.ly) - self.min_y) / self.texel;
        let depth = p.dot(self.lz);
        let (cx, cy) = (fx.floor() as i64, fy.floor() as i64);
        let mut lit = 0.0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || y < 0 || x >= self.target.w as i64 || y >= self.target.h as i64 {
                    lit += 1.0;
                    continue;
                }
                let occluder = self.target.depth[y as usize * self.target.w + x as usize];
                if !occluder.is_finite() || depth >= occluder - 0.9 {
                    lit += 1.0;
                }
            }
        }
        lit / 9.0
    }
}

// ---- shading ------------------------------------------------------------------------------

#[inline]
fn srgb_to_linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[inline]
fn linear_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

fn aces(x: f32) -> f32 {
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

/// Studio environment for reflections, in view space (y up, z toward the viewer).
fn environment(r: Vec3, rough: f32, l: &Lighting) -> Vec3 {
    let up = r.y;
    let horizon = Vec3::new(0.85, 0.72, 0.58);
    let base = if up >= 0.0 { horizon.lerp(l.sky * 2.2, up.powf(0.6)) } else { horizon.lerp(l.ground * 0.6, (-up).powf(0.5)) };
    let key = Vec3::new(-0.45, 0.75, 0.48).normalized();
    let sharp = 1.0 - rough;
    let soft = r.dot(key).max(0.0).powf(4.0 + 60.0 * sharp * sharp) * (1.0 + 6.0 * sharp);
    let rim = r.dot(Vec3::new(0.6, 0.35, -0.72).normalized()).max(0.0).powf(6.0 + 30.0 * sharp) * 1.5;
    base + Vec3::new(1.0, 0.95, 0.88) * soft + Vec3::new(0.55, 0.7, 1.0) * rim
}

struct Brdf {
    n: Vec3,
    albedo: Vec3,
    rough: f32,
    metal: f32,
    f0: Vec3,
    ndv: f32,
    toon: u32,
}

impl Brdf {
    /// Reflected radiance for light arriving from direction `l` with radiance `radiance`.
    fn light(&self, l: Vec3, radiance: Vec3) -> Vec3 {
        let view_dir = Vec3::new(0.0, 0.0, 1.0);
        let mut ndl = self.n.dot(l).max(0.0);
        if self.toon >= 2 {
            let steps = self.toon as f32;
            ndl = ((ndl * steps).floor() / (steps - 1.0)).min(1.0);
        }
        if ndl <= 0.0 {
            return Vec3::ZERO;
        }
        let a = self.rough * self.rough;
        let a2 = a * a;
        let k = (self.rough + 1.0).powi(2) / 8.0;
        let h = (l + view_dir).normalized();
        let ndh = self.n.dot(h).max(0.0);
        let vdh = view_dir.dot(h).max(0.0);
        let d = a2 / (std::f32::consts::PI * ((ndh * ndh) * (a2 - 1.0) + 1.0).powi(2)).max(1e-6);
        let g = (self.ndv / (self.ndv * (1.0 - k) + k)) * (ndl / (ndl * (1.0 - k) + k));
        let f = self.f0 + (Vec3::new(1.0, 1.0, 1.0) - self.f0) * (1.0 - vdh).powi(5);
        let spec = f * (d * g / (4.0 * self.ndv * ndl + 1e-4));
        let spec = if self.toon >= 2 { spec * 0.6 } else { spec };
        let kd = (Vec3::new(1.0, 1.0, 1.0) - f) * (1.0 - self.metal);
        let diffuse = kd.mul_elem(self.albedo) * (1.0 / std::f32::consts::PI);
        (diffuse + spec).mul_elem(radiance) * ndl
    }
}

/// Everything known about one visible pixel.
struct Fragment<'a> {
    face: &'a Face,
    view: &'a Affine,
    u: f32,
    v: f32,
    /// View-space position.
    pos: Vec3,
    /// Key light visibility (1 = lit).
    shadow: f32,
    depth01: f32,
}

/// Returns (sRGB color, linear glow for bloom).
fn shade(
    fr: &Fragment,
    materials: &[Material],
    textures: &[Arc<Texture>],
    settings: &RenderSettings,
    points_view: &[PointLight],
) -> ([u8; 4], Vec3) {
    let face = fr.face;
    let v = face.voxel;
    let c = Rgba::from_u32(v.rgba);
    let mat_default = Material::new("default");
    let mat = materials.get(v.material as usize).unwrap_or(materials.first().unwrap_or(&mat_default));
    let axis = (face.dir / 2) as usize;
    let surface_point = face_point(face, fr.u, fr.v);
    let surf = sample_surface(mat, textures, surface_point.to_array(), axis);
    let o = DIRS[face.dir as usize];
    let face_n = Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32);
    let geo = (face_n * (1.0 - settings.smoothness) + face.smooth * settings.smoothness).normalized();
    let (t, b) = match axis {
        0 => (Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 0.0)),
        1 => (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0)),
        _ => (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
    };
    let nt = surf.normal_ts;
    let n_model = (t * nt.x + b * nt.y + geo * nt.z).normalized();
    let n = fr.view.apply_vector(n_model).normalized();
    let albedo = Vec3::new(srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b)).mul_elem(surf.albedo_mul);
    let ca = face.corner_ao;
    let (u, w) = (fr.u, fr.v);
    let corner = ca[0] * (1.0 - u) * (1.0 - w) + ca[1] * u * (1.0 - w) + ca[2] * u * w + ca[3] * (1.0 - u) * w;
    let ao = face.ao.powf(0.6) * corner * surf.occlusion;
    let emission = if surf.has_emission_map { surf.emission } else { albedo * mat.emission };
    let enc = |x: f32| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let gray = |x: f32| {
        let g = enc(x);
        [g, g, g, 255]
    };
    let rgba = match settings.pass {
        Pass::Albedo => [linear_to_srgb(albedo.x), linear_to_srgb(albedo.y), linear_to_srgb(albedo.z), 255],
        Pass::Normal => [enc(n.x * 0.5 + 0.5), enc(n.y * 0.5 + 0.5), enc(n.z * 0.5 + 0.5), 255],
        Pass::Depth => gray(fr.depth01),
        Pass::Occlusion => gray(ao),
        Pass::Roughness => gray(surf.roughness),
        Pass::Metallic => gray(surf.metallic),
        Pass::Emission => [linear_to_srgb(emission.x), linear_to_srgb(emission.y), linear_to_srgb(emission.z), 255],
        Pass::Lit => {
            let view_dir = Vec3::new(0.0, 0.0, 1.0);
            let ndv = n.dot(view_dir).max(1e-3);
            let brdf = Brdf {
                n,
                albedo,
                rough: surf.roughness,
                metal: surf.metallic,
                f0: Vec3::new(0.04, 0.04, 0.04).lerp(albedo, surf.metallic),
                ndv,
                toon: settings.toon_steps,
            };
            let mut col = Vec3::ZERO;
            for (i, light) in settings.lighting.lights.iter().enumerate() {
                let vis = if i == 0 { 1.0 - settings.shadows * (1.0 - fr.shadow) } else { 1.0 };
                col = col + brdf.light(light.dir, light.color * (light.intensity * vis * (0.55 + 0.45 * ao)));
            }
            for p in points_view {
                let d = p.pos - fr.pos;
                let dist = d.length();
                if dist < p.radius && dist > 1e-3 {
                    let fall = (1.0 - dist / p.radius).powi(2);
                    col = col + brdf.light(d * (1.0 / dist), p.color * (p.intensity * fall)) * (0.35 + 0.65 * ao);
                }
            }
            let hemi = settings.lighting.sky * (0.5 + 0.5 * n.y) + settings.lighting.ground * (0.5 - 0.5 * n.y);
            let ambient_diffuse = albedo.mul_elem(hemi) * (1.0 - surf.metallic);
            let r = (n * (2.0 * ndv) - view_dir).normalized();
            let env = environment(r, surf.roughness, &settings.lighting);
            let fr_env = brdf.f0 + (Vec3::new(1.0, 1.0, 1.0) - brdf.f0) * ((1.0 - ndv).powi(5) * (1.0 - surf.roughness).powi(2));
            let ambient_spec = fr_env.mul_elem(env) * (1.0 - 0.6 * surf.roughness);
            col = col + (ambient_diffuse + ambient_spec) * ao + emission;
            let e = settings.lighting.exposure;
            [linear_to_srgb(aces(col.x * e)), linear_to_srgb(aces(col.y * e)), linear_to_srgb(aces(col.z * e)), c.a]
        }
    };
    (rgba, emission)
}

/// Renders a model.
pub fn render(
    model: &VoxelModel,
    prepared: &Prepared,
    materials: &[Material],
    textures: &[Arc<Texture>],
    settings: &RenderSettings,
) -> Image {
    let ss = settings.supersample.clamp(1, 4);
    let (target, views) = rasterize(model, prepared, settings, ss, true);
    let cam_view = camera_view(model, settings);
    let lit = settings.pass == Pass::Lit;
    let key = settings.lighting.lights.first().map(|l| l.dir);
    let shadow_map = match key {
        Some(dir) if lit && (settings.shadows > 0.0 || settings.ground_shadow > 0.0) => ShadowMap::build(prepared, &views, dir),
        _ => None,
    };
    let points_view: Vec<PointLight> = settings.lighting.points.iter().map(|p| PointLight { pos: cam_view.apply(p.pos), ..*p }).collect();
    let (zmin, zmax) = target.depth.iter().filter(|z| z.is_finite()).fold((f32::MAX, f32::MIN), |(a, b), &z| (a.min(z), b.max(z)));
    let zrange = (zmax - zmin).max(1e-3);
    let mut hi = vec![[0u8; 4]; target.w * target.h];
    let mut glow = vec![Vec3::ZERO; if settings.bloom > 0.0 { target.w * target.h } else { 0 }];
    for (i, &fid) in target.face.iter().enumerate() {
        if fid == NO_FACE {
            continue;
        }
        let f = &prepared.faces[fid as usize];
        let view = &views[(f.voxel.bone as usize).min(views.len() - 1)];
        let [u, v] = target.uv[i];
        let pos = view.apply(face_point(f, u, v));
        let shadow = match &shadow_map {
            Some(sm) if settings.shadows > 0.0 => {
                let o = DIRS[f.dir as usize];
                let n = view.apply_vector(Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32));
                sm.visibility(pos + n * 0.6)
            }
            _ => 1.0,
        };
        let fr = Fragment { face: f, view, u, v, pos, shadow, depth01: (target.depth[i] - zmin) / zrange };
        let (rgba, emission) = shade(&fr, materials, textures, settings, &points_view);
        hi[i] = rgba;
        if !glow.is_empty() {
            glow[i] = emission;
        }
    }
    let (w, h) = (settings.camera.width as usize, settings.camera.height as usize);
    let mut img = Image::new(w as u32, h as u32);
    let mut depth = vec![f32::NEG_INFINITY; w * h];
    let mut glow_out = vec![Vec3::ZERO; if glow.is_empty() { 0 } else { w * h }];
    let ssu = ss as usize;
    for y in 0..h {
        for x in 0..w {
            let (mut acc, mut n, mut zmax_px, mut g) = ([0u32; 4], 0u32, f32::NEG_INFINITY, Vec3::ZERO);
            for sy in 0..ssu {
                for sx in 0..ssu {
                    let i = (y * ssu + sy) * target.w + x * ssu + sx;
                    if target.face[i] != NO_FACE {
                        for k in 0..4 {
                            acc[k] += hi[i][k] as u32;
                        }
                        n += 1;
                        zmax_px = zmax_px.max(target.depth[i]);
                        if !glow.is_empty() {
                            g = g + glow[i];
                        }
                    }
                }
            }
            if !glow_out.is_empty() {
                glow_out[y * w + x] = g * (1.0 / (ssu * ssu) as f32);
            }
            // Sharp silhouette: a pixel is solid when at least half of its samples are covered.
            if n * 2 >= (ssu * ssu) as u32 && n > 0 {
                let o = (y * w + x) * 4;
                for k in 0..4 {
                    img.pixels[o + k] = (acc[k] / n) as u8;
                }
                depth[y * w + x] = zmax_px;
            }
        }
    }
    if lit {
        if settings.inner_lines {
            inner_lines(&mut img, &depth, settings.camera.scale);
        }
        match settings.outline {
            Outline::None => {}
            Outline::Selective => outline(&mut img, None),
            Outline::Color(c) => outline(&mut img, Some(c)),
        }
        if settings.ground_shadow > 0.0
            && let Some(sm) = &shadow_map
        {
            ground_shadow(&mut img, model, settings, sm);
        }
        if settings.bloom > 0.0 {
            bloom(&mut img, glow_out, settings);
        }
    }
    composite_background(&mut img, settings);
    img
}

/// Soft shadow on the ground plane (y = origin) for pixels not covered by the model.
fn ground_shadow(img: &mut Image, model: &VoxelModel, settings: &RenderSettings, sm: &ShadowMap) {
    let cam = &settings.camera;
    let r = cam.view_rotation().m;
    let det = r[0][0] * r[1][2] - r[0][2] * r[1][0];
    if det.abs() < 1e-3 {
        return;
    }
    let _ = model;
    let up = Vec3::new(r[0][1], r[1][1], r[2][1]) * 0.5;
    let (w, h) = (img.width as usize, img.height as usize);
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            if img.pixels[o + 3] != 0 {
                continue;
            }
            let vx = (x as f32 + 0.5 - cam.anchor_x) / cam.scale;
            let vy = (cam.anchor_y - (y as f32 + 0.5)) / cam.scale;
            let gx = (vx * r[1][2] - vy * r[0][2]) / det;
            let gz = (vy * r[0][0] - vx * r[1][0]) / det;
            let vz = r[2][0] * gx + r[2][2] * gz;
            let vis = sm.visibility(Vec3::new(vx, vy, vz) + up);
            let a = settings.ground_shadow * (1.0 - vis);
            if a > 0.01 {
                img.pixels[o..o + 4].copy_from_slice(&[12, 10, 22, (a * 255.0) as u8]);
            }
        }
    }
}

/// Adds a blurred glow of emissive light; glow spills onto transparent pixels as a halo.
fn bloom(img: &mut Image, glow: Vec<Vec3>, settings: &RenderSettings) {
    let (w, h) = (img.width as usize, img.height as usize);
    let radius = if settings.bloom_radius > 0.0 { settings.bloom_radius } else { (w.max(h) as f32 / 48.0).max(2.0) };
    let r = radius.round().max(1.0) as usize;
    let mut buf = glow;
    let mut tmp = vec![Vec3::ZERO; w * h];
    // Three box blurs ≈ Gaussian.
    for _ in 0..3 {
        for y in 0..h {
            let mut acc = Vec3::ZERO;
            let row = y * w;
            for x in 0..(r.min(w)) {
                acc = acc + buf[row + x];
            }
            for x in 0..w {
                if x + r < w {
                    acc = acc + buf[row + x + r];
                }
                if x > r {
                    acc = acc - buf[row + x - r - 1];
                }
                tmp[row + x] = acc * (1.0 / (2 * r + 1) as f32);
            }
        }
        for x in 0..w {
            let mut acc = Vec3::ZERO;
            for y in 0..(r.min(h)) {
                acc = acc + tmp[y * w + x];
            }
            for y in 0..h {
                if y + r < h {
                    acc = acc + tmp[(y + r) * w + x];
                }
                if y > r {
                    acc = acc - tmp[(y - r - 1) * w + x];
                }
                buf[y * w + x] = acc * (1.0 / (2 * r + 1) as f32);
            }
        }
    }
    let strength = settings.bloom * 1.2;
    for (i, g) in buf.iter().enumerate() {
        let g = *g * strength;
        let o = i * 4;
        let p = &mut img.pixels[o..o + 4];
        if p[3] > 0 {
            let lin = |c: u8, add: f32| linear_to_srgb(srgb_to_linear(c) + add);
            p[0] = lin(p[0], g.x);
            p[1] = lin(p[1], g.y);
            p[2] = lin(p[2], g.z);
        } else {
            let a = g.x.max(g.y).max(g.z).min(1.0);
            if a > 1.0 / 255.0 {
                p.copy_from_slice(&[linear_to_srgb(g.x / a), linear_to_srgb(g.y / a), linear_to_srgb(g.z / a), (a * 255.0) as u8]);
            }
        }
    }
}

fn composite_background(img: &mut Image, settings: &RenderSettings) {
    if settings.background.is_none() && settings.background_gradient.is_none() {
        return;
    }
    let (w, h) = (img.width as f32, img.height as f32);
    for y in 0..img.height {
        for x in 0..img.width {
            let bg = match (settings.background_gradient, settings.background) {
                (Some((top, bottom)), _) => {
                    let t = y as f32 / h.max(1.0);
                    let (dx, dy) = (x as f32 / w - 0.5, y as f32 / h - 0.5);
                    let vignette = 1.0 - (dx * dx + dy * dy).sqrt() * 0.55;
                    let mix = |a: u8, b: u8| ((a as f32 + (b as f32 - a as f32) * t) * vignette).clamp(0.0, 255.0) as u8;
                    Rgba::new(mix(top.r, bottom.r), mix(top.g, bottom.g), mix(top.b, bottom.b), 255)
                }
                (None, Some(c)) => c,
                _ => unreachable!(),
            };
            let o = ((y * img.width + x) * 4) as usize;
            let px = &mut img.pixels[o..o + 4];
            let mut dst = [bg.r, bg.g, bg.b, bg.a];
            crate::renderer::blend_over(&mut dst, Rgba::new(px[0], px[1], px[2], px[3]), 255);
            px.copy_from_slice(&dst);
        }
    }
}

/// Darkens pixels that sit in front of a large depth step (separates overlapping limbs).
fn inner_lines(img: &mut Image, depth: &[f32], scale: f32) {
    let (w, h) = (img.width as usize, img.height as usize);
    let threshold = 4.0 / scale.max(0.25) * scale.clamp(1.0, 4.0);
    let src = img.pixels.clone();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !depth[i].is_finite() {
                continue;
            }
            let mut step = false;
            for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if depth[j].is_finite() && depth[i] - depth[j] < -threshold {
                    step = true;
                }
            }
            if step {
                let o = i * 4;
                for k in 0..3 {
                    img.pixels[o + k] = (src[o + k] as f32 * 0.62) as u8;
                }
            }
        }
    }
}

/// 1-pixel exterior outline. `None` = selective outline derived from the bordering color.
fn outline(img: &mut Image, color: Option<Rgba>) {
    let (w, h) = (img.width as i32, img.height as i32);
    let src = img.pixels.clone();
    for y in 0..h {
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            if src[o + 3] != 0 {
                continue;
            }
            let mut best: Option<[u8; 4]> = None;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = ((ny * w + nx) * 4) as usize;
                if src[j + 3] > 0 {
                    let cand = [src[j], src[j + 1], src[j + 2], 255];
                    let lum = |p: [u8; 4]| p[0] as u32 * 3 + p[1] as u32 * 6 + p[2] as u32;
                    if best.is_none_or(|b| lum(cand) < lum(b)) {
                        best = Some(cand);
                    }
                }
            }
            if let Some(b) = best {
                let c = match color {
                    Some(c) => [c.r, c.g, c.b, c.a],
                    None => [(b[0] as f32 * 0.28) as u8, (b[1] as f32 * 0.26) as u8, (b[2] as f32 * 0.34 + 10.0).min(255.0) as u8, 255],
                };
                img.pixels[o..o + 4].copy_from_slice(&c);
            }
        }
    }
}

/// Voxel + face under a screen pixel (for click-to-edit in 3D viewports).
pub fn pick(model: &VoxelModel, prepared: &Prepared, settings: &RenderSettings, px: u32, py: u32) -> Option<([i32; 3], [i32; 3])> {
    if px >= settings.camera.width || py >= settings.camera.height {
        return None;
    }
    let (target, _) = rasterize(model, prepared, settings, 1, false);
    let fid = target.face[py as usize * target.w + px as usize];
    (fid != NO_FACE).then(|| {
        let f = &prepared.faces[fid as usize];
        (f.pos, DIRS[f.dir as usize])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::model::Shape;

    fn fill(m: &mut VoxelModel, s: Shape, rgba: Rgba, material: u8) {
        let (lo, hi) = s.aabb();
        for z in lo.z.floor() as i32..=hi.z.ceil() as i32 {
            for y in lo.y.floor() as i32..=hi.y.ceil() as i32 {
                for x in lo.x.floor() as i32..=hi.x.ceil() as i32 {
                    if s.contains(Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5)) {
                        m.set(x, y, z, Voxel { rgba: rgba.to_pixel(), material, bone: 0 });
                    }
                }
            }
        }
    }

    fn ball() -> VoxelModel {
        let mut m = VoxelModel::new("ball", [32, 32, 32]);
        fill(
            &mut m,
            Shape::Ellipsoid { center: Vec3::new(16.0, 16.0, 16.0), radii: Vec3::new(10.0, 10.0, 10.0) },
            Rgba::new(200, 60, 40, 255),
            0,
        );
        m.origin = Vec3::new(16.0, 16.0, 16.0);
        m
    }

    fn cam(yaw: f32) -> Camera {
        Camera { yaw, pitch: 30.0, scale: 3.0, width: 96, height: 96, anchor_x: 48.0, anchor_y: 48.0 }
    }

    fn lum(c: Rgba) -> u32 {
        c.r as u32 + c.g as u32 + c.b as u32
    }

    #[test]
    fn surface_is_shell_with_outward_normals_and_corner_ao() {
        let m = ball();
        let p = prepare(&m);
        assert!(!p.faces.is_empty());
        assert!(p.faces.len() < m.count() * 6);
        for f in &p.faces {
            let center = Vec3::new(f.pos[0] as f32 + 0.5 - 16.0, f.pos[1] as f32 + 0.5 - 16.0, f.pos[2] as f32 + 0.5 - 16.0);
            assert!(f.smooth.dot(center) > 0.0, "normal must point outward");
            assert!((0.0..=1.0).contains(&f.ao));
        }
        // Inner corner of an L shape gets occluded corners.
        let mut l = VoxelModel::new("l", [8, 8, 8]);
        fill(&mut l, Shape::Box { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(4.0, 1.0, 4.0) }, Rgba::new(255, 255, 255, 255), 0);
        fill(&mut l, Shape::Box { min: Vec3::new(0.0, 1.0, 0.0), max: Vec3::new(1.0, 4.0, 4.0) }, Rgba::new(255, 255, 255, 255), 0);
        let pl = prepare(&l);
        let floor = pl.faces.iter().find(|f| f.dir == 2 && f.pos == [1, 0, 2]).expect("floor face next to the wall");
        assert!(floor.corner_ao.iter().any(|&a| a < 1.0) && floor.corner_ao.contains(&1.0));
    }

    #[test]
    fn renders_lit_and_passes_deterministically() {
        let m = ball();
        let p = prepare(&m);
        let mats = vec![Material::new("default")];
        let mut s = RenderSettings::new(cam(0.0));
        let a = render(&m, &p, &mats, &[], &s);
        let b = render(&m, &p, &mats, &[], &s);
        assert_eq!(a, b);
        assert_eq!(a.get(48, 48).a, 255);
        assert_eq!(a.get(2, 2).a, 0);
        assert!(lum(a.get(38, 38)) > lum(a.get(60, 62)), "top-left key light");
        s.pass = Pass::Normal;
        let n = render(&m, &p, &mats, &[], &s);
        assert!(n.get(48, 48).b > 200, "center normal faces the viewer");
        s.pass = Pass::Lit;
        s.outline = Outline::Selective;
        s.supersample = 2;
        let o = render(&m, &p, &mats, &[], &s);
        let edge = (0..96).find(|&x| o.get(x, 48).a > 0).unwrap();
        assert!(lum(o.get(edge, 48)) < lum(o.get(edge + 3, 48)), "outline is darker than the interior");
    }

    #[test]
    fn cast_shadows_and_ground_shadow() {
        // A slab with a block hovering above it, lit from straight above.
        let mut m = VoxelModel::new("s", [40, 40, 40]);
        fill(&mut m, Shape::Box { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(40.0, 2.0, 40.0) }, Rgba::new(180, 180, 180, 255), 0);
        fill(&mut m, Shape::Box { min: Vec3::new(15.0, 12.0, 15.0), max: Vec3::new(25.0, 16.0, 25.0) }, Rgba::new(180, 180, 180, 255), 0);
        m.origin = Vec3::new(20.0, 0.0, 20.0);
        let p = prepare(&m);
        let mats = vec![Material::new("default")];
        let mut s =
            RenderSettings::new(Camera { yaw: 0.0, pitch: 89.0, scale: 2.0, width: 100, height: 100, anchor_x: 50.0, anchor_y: 50.0 });
        s.lighting.lights = vec![Light { dir: Vec3::new(0.0, 0.0, 1.0), color: Vec3::new(1.0, 1.0, 1.0), intensity: 3.0 }];
        s.shadows = 1.0;
        let img = render(&m, &p, &mats, &[], &s);
        // Top-down: the block is at the center; slab pixels just outside it are lit, the block casts on itself only.
        s.lighting.lights[0].dir = Vec3::new(0.7, 0.0, 0.7).normalized();
        let shadowed = render(&m, &p, &mats, &[], &s);
        s.shadows = 0.0;
        let unshadowed = render(&m, &p, &mats, &[], &s);
        let diff = (0..100 * 100).filter(|&i| shadowed.pixels[i * 4] + 8 < unshadowed.pixels[i * 4]).count();
        assert!(diff > 20, "the hovering block must cast a shadow ({diff} darker pixels)");
        assert_eq!(img.width, 100);
    }

    #[test]
    fn emission_blooms_into_a_halo() {
        let mut m = VoxelModel::new("glow", [16, 16, 16]);
        fill(
            &mut m,
            Shape::Ellipsoid { center: Vec3::new(8.0, 8.0, 8.0), radii: Vec3::new(3.0, 3.0, 3.0) },
            Rgba::new(255, 160, 40, 255),
            1,
        );
        m.origin = Vec3::new(8.0, 8.0, 8.0);
        let p = prepare(&m);
        let mut fire = Material::new("fire");
        fire.emission = 4.0;
        let mats = vec![Material::new("default"), fire];
        let mut s = RenderSettings::new(Camera { yaw: 0.0, pitch: 0.0, scale: 3.0, width: 64, height: 64, anchor_x: 32.0, anchor_y: 32.0 });
        let plain = render(&m, &p, &mats, &[], &s);
        s.bloom = 1.0;
        let glow = render(&m, &p, &mats, &[], &s);
        let halo = |img: &Image| (0..64 * 64).filter(|&i| img.pixels[i * 4 + 3] > 0).count();
        assert!(halo(&glow) > halo(&plain) + 50, "glow spills beyond the silhouette");
        s.lighting.points =
            vec![PointLight { pos: Vec3::new(8.0, 8.0, 14.0), color: Vec3::new(1.0, 0.5, 0.1), intensity: 8.0, radius: 20.0 }];
        s.bloom = 0.0;
        let warm = render(&m, &p, &[Material::new("default")], &[], &s);
        let cold = render(&m, &p, &[Material::new("default")], &[], &RenderSettings { lighting: Lighting::default(), ..s.clone() });
        assert!(warm.get(32, 32).r > cold.get(32, 32).r, "point light adds light");
    }

    #[test]
    fn picking_hits_the_front_face() {
        let m = ball();
        let p = prepare(&m);
        let s = RenderSettings { camera: Camera { pitch: 0.0, ..cam(0.0) }, ..RenderSettings::new(cam(0.0)) };
        let (pos, normal) = pick(&m, &p, &s, 48, 48).expect("hit");
        assert_eq!(normal, [0, 0, 1]);
        assert!(pos[2] >= 24);
        assert!(pick(&m, &p, &s, 1, 1).is_none());
    }
}
