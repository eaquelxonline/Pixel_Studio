//! Sparse voxel models ("3D pixels") with per-voxel true color, material and bone.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::math::Vec3;
use super::rig::{Animation, Bone};

pub const CHUNK: i32 = 16;
pub const CHUNK_VOLUME: usize = (CHUNK * CHUNK * CHUNK) as usize;
pub const MAX_MODEL_SIZE: u32 = 512;
pub const MAX_BONES: usize = 255;

/// One 3D pixel. `rgba` alpha 0 = empty. `material` indexes the project's materials
/// (0 = default), `bone` indexes the model's skeleton (0 = root / unbound).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Voxel {
    pub rgba: u32,
    pub material: u8,
    pub bone: u8,
}

impl Voxel {
    pub const EMPTY: Voxel = Voxel { rgba: 0, material: 0, bone: 0 };

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.rgba & 0xFF == 0
    }
}

pub type Chunk = [Voxel; CHUNK_VOLUME];
pub type ChunkKey = (i32, i32, i32);

static VERSION: AtomicU64 = AtomicU64::new(1);

/// Globally unique edit stamp; render caches compare it to know when to rebuild.
pub fn next_version() -> u64 {
    VERSION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub struct VoxelModel {
    pub name: String,
    pub size: [u32; 3],
    pub chunks: HashMap<ChunkKey, Arc<Chunk>>,
    pub bones: Vec<Bone>,
    pub animations: Vec<Animation>,
    /// Model-space point placed at the bottom-center of rendered sprites (usually between the feet).
    pub origin: Vec3,
    pub version: u64,
}

impl VoxelModel {
    pub fn new(name: &str, size: [u32; 3]) -> VoxelModel {
        VoxelModel {
            name: name.into(),
            size,
            chunks: HashMap::new(),
            bones: Vec::new(),
            animations: Vec::new(),
            origin: Vec3::new(size[0] as f32 / 2.0, 0.0, size[2] as f32 / 2.0),
            version: next_version(),
        }
    }

    pub fn in_bounds(&self, x: i32, y: i32, z: i32) -> bool {
        x >= 0 && y >= 0 && z >= 0 && (x as u32) < self.size[0] && (y as u32) < self.size[1] && (z as u32) < self.size[2]
    }

    #[inline]
    fn split(x: i32, y: i32, z: i32) -> (ChunkKey, usize) {
        let key = (x.div_euclid(CHUNK), y.div_euclid(CHUNK), z.div_euclid(CHUNK));
        let (lx, ly, lz) = (x.rem_euclid(CHUNK), y.rem_euclid(CHUNK), z.rem_euclid(CHUNK));
        (key, ((lz * CHUNK + ly) * CHUNK + lx) as usize)
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Voxel {
        if !self.in_bounds(x, y, z) {
            return Voxel::EMPTY;
        }
        let (key, i) = Self::split(x, y, z);
        self.chunks.get(&key).map(|c| c[i]).unwrap_or(Voxel::EMPTY)
    }

    /// Writes a voxel (clipped to bounds). Returns true when it changed.
    pub fn set(&mut self, x: i32, y: i32, z: i32, v: Voxel) -> bool {
        if !self.in_bounds(x, y, z) {
            return false;
        }
        let v = if v.is_empty() { Voxel::EMPTY } else { v };
        let (key, i) = Self::split(x, y, z);
        let changed = match self.chunks.get_mut(&key) {
            None if v.is_empty() => false,
            None => {
                let mut c = [Voxel::EMPTY; CHUNK_VOLUME];
                c[i] = v;
                self.chunks.insert(key, Arc::new(c));
                true
            }
            Some(c) => {
                if c[i] == v {
                    false
                } else {
                    Arc::make_mut(c)[i] = v;
                    true
                }
            }
        };
        if changed {
            self.version = next_version();
        }
        changed
    }

    pub fn touch(&mut self) {
        self.version = next_version();
    }

    /// Drops chunks that became empty.
    pub fn compact(&mut self) {
        self.chunks.retain(|_, c| c.iter().any(|v| !v.is_empty()));
    }

    pub fn for_each(&self, mut f: impl FnMut(i32, i32, i32, Voxel)) {
        let mut keys: Vec<&ChunkKey> = self.chunks.keys().collect();
        keys.sort();
        for key in keys {
            let c = &self.chunks[key];
            for (i, v) in c.iter().enumerate() {
                if v.is_empty() {
                    continue;
                }
                let i = i as i32;
                f(key.0 * CHUNK + i % CHUNK, key.1 * CHUNK + (i / CHUNK) % CHUNK, key.2 * CHUNK + i / (CHUNK * CHUNK), *v);
            }
        }
    }

    pub fn count(&self) -> usize {
        self.chunks.values().map(|c| c.iter().filter(|v| !v.is_empty()).count()).sum()
    }

    /// Inclusive min/max corner of filled voxels.
    pub fn bounds(&self) -> Option<([i32; 3], [i32; 3])> {
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        let mut any = false;
        self.for_each(|x, y, z, _| {
            any = true;
            for (k, v) in [x, y, z].into_iter().enumerate() {
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        });
        any.then_some((lo, hi))
    }

    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }
}

/// Primitive volumes for sculpting. All coordinates are in voxel units; a voxel is inside when
/// its center is inside the shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Box {
        min: Vec3,
        max: Vec3,
    },
    Ellipsoid {
        center: Vec3,
        radii: Vec3,
    },
    /// Tapered capsule from `a` (radius `ra`) to `b` (radius `rb`) — limbs, horns, fingers.
    Capsule {
        a: Vec3,
        b: Vec3,
        ra: f32,
        rb: f32,
    },
    /// Cylinder from `a` to `b` with radius `r` (flat caps).
    Cylinder {
        a: Vec3,
        b: Vec3,
        r: f32,
    },
}

impl Shape {
    pub fn aabb(&self) -> (Vec3, Vec3) {
        match *self {
            Shape::Box { min, max } => (min, max),
            Shape::Ellipsoid { center, radii } => (center - radii, center + radii),
            Shape::Capsule { a, b, ra, rb } => {
                let r = ra.max(rb);
                (
                    Vec3::new(a.x.min(b.x) - r, a.y.min(b.y) - r, a.z.min(b.z) - r),
                    Vec3::new(a.x.max(b.x) + r, a.y.max(b.y) + r, a.z.max(b.z) + r),
                )
            }
            Shape::Cylinder { a, b, r } => (
                Vec3::new(a.x.min(b.x) - r, a.y.min(b.y) - r, a.z.min(b.z) - r),
                Vec3::new(a.x.max(b.x) + r, a.y.max(b.y) + r, a.z.max(b.z) + r),
            ),
        }
    }

    pub fn contains(&self, p: Vec3) -> bool {
        match *self {
            Shape::Box { min, max } => p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y && p.z >= min.z && p.z <= max.z,
            Shape::Ellipsoid { center, radii } => {
                let d = p - center;
                let (rx, ry, rz) = (radii.x.max(0.5), radii.y.max(0.5), radii.z.max(0.5));
                (d.x / rx).powi(2) + (d.y / ry).powi(2) + (d.z / rz).powi(2) <= 1.0
            }
            Shape::Capsule { a, b, ra, rb } => {
                let ab = b - a;
                let len2 = ab.dot(ab);
                let t = if len2 < 1e-6 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
                let r = ra + (rb - ra) * t;
                (p - (a + ab * t)).length() <= r.max(0.5)
            }
            Shape::Cylinder { a, b, r } => {
                let ab = b - a;
                let len2 = ab.dot(ab);
                if len2 < 1e-6 {
                    return false;
                }
                let t = (p - a).dot(ab) / len2;
                (0.0..=1.0).contains(&t) && (p - (a + ab * t)).length() <= r.max(0.5)
            }
        }
    }

    /// 0..1 position of a point along the shape's main axis (for gradients).
    pub fn axis_t(&self, p: Vec3, axis: usize) -> f32 {
        let (lo, hi) = self.aabb();
        let (l, h, v) = match axis {
            0 => (lo.x, hi.x, p.x),
            1 => (lo.y, hi.y, p.y),
            _ => (lo.z, hi.z, p.z),
        };
        if h - l < 1e-6 { 0.0 } else { ((v - l) / (h - l)).clamp(0.0, 1.0) }
    }
}

/// Integer hash → [0, 1). Deterministic noise for color variation.
pub fn hash01(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ (z as u32).wrapping_mul(0xCB1A_B31F)
        ^ seed.wrapping_mul(0x9E37_79B9);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h & 0x00FF_FFFF) as f32 / 16_777_216.0
}

/// Smooth 3D value noise in [0, 1).
pub fn value_noise(p: Vec3, seed: u32) -> f32 {
    let (x0, y0, z0) = (p.x.floor(), p.y.floor(), p.z.floor());
    let (fx, fy, fz) = (p.x - x0, p.y - y0, p.z - z0);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy, sz) = (s(fx), s(fy), s(fz));
    let (ix, iy, iz) = (x0 as i32, y0 as i32, z0 as i32);
    let mut acc = 0.0;
    for dz in 0..2 {
        for dy in 0..2 {
            for dx in 0..2 {
                let w =
                    (if dx == 1 { sx } else { 1.0 - sx }) * (if dy == 1 { sy } else { 1.0 - sy }) * (if dz == 1 { sz } else { 1.0 - sz });
                acc += w * hash01(ix + dx, iy + dy, iz + dz, seed);
            }
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_voxels() {
        let mut m = VoxelModel::new("t", [64, 64, 64]);
        let v = Voxel { rgba: 0xFF0000FF, material: 1, bone: 2 };
        let before = m.version;
        assert!(m.set(17, 3, 40, v));
        assert!(m.version > before);
        assert!(!m.set(17, 3, 40, v));
        assert_eq!(m.get(17, 3, 40), v);
        assert_eq!(m.get(64, 0, 0), Voxel::EMPTY);
        assert!(!m.set(-1, 0, 0, v));
        assert_eq!(m.count(), 1);
        assert_eq!(m.bounds(), Some(([17, 3, 40], [17, 3, 40])));
        let mut seen = vec![];
        m.for_each(|x, y, z, _| seen.push((x, y, z)));
        assert_eq!(seen, vec![(17, 3, 40)]);
        m.set(17, 3, 40, Voxel::EMPTY);
        m.compact();
        assert!(m.chunks.is_empty());
    }

    #[test]
    fn shapes() {
        let cap = Shape::Capsule { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0), ra: 2.0, rb: 1.0 };
        assert!(cap.contains(Vec3::new(0.0, 1.9, 0.0)));
        assert!(!cap.contains(Vec3::new(10.0, 1.9, 0.0)));
        let e = Shape::Ellipsoid { center: Vec3::new(5.0, 5.0, 5.0), radii: Vec3::new(4.0, 2.0, 2.0) };
        assert!(e.contains(Vec3::new(8.5, 5.0, 5.0)));
        assert!(!e.contains(Vec3::new(5.0, 7.5, 5.0)));
        let n = value_noise(Vec3::new(1.3, 2.7, 0.2), 7);
        assert!((0.0..1.0).contains(&n));
    }
}
