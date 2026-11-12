//! Small, dependency-free 3D math: vectors and rigid affine transforms.

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    pub const fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    pub fn from_f64(v: [f64; 3]) -> Vec3 {
        Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)
    }

    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn normalized(self) -> Vec3 {
        let l = self.length();
        if l > 1e-8 { self * (1.0 / l) } else { Vec3::new(0.0, 0.0, 1.0) }
    }

    pub fn lerp(self, o: Vec3, t: f32) -> Vec3 {
        self + (o - self) * t
    }

    pub fn mul_elem(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    pub fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// Affine transform: 3×3 linear part (row-major) + translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub m: [[f32; 3]; 3],
    pub t: Vec3,
}

impl Default for Affine {
    fn default() -> Self {
        Affine::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: Vec3::ZERO };

    pub fn translation(t: Vec3) -> Affine {
        Affine { t, ..Affine::IDENTITY }
    }

    /// Rotation from Euler angles in degrees, applied X then Y then Z (R = Rz·Ry·Rx).
    pub fn rotation_euler(deg: Vec3) -> Affine {
        let (sx, cx) = deg.x.to_radians().sin_cos();
        let (sy, cy) = deg.y.to_radians().sin_cos();
        let (sz, cz) = deg.z.to_radians().sin_cos();
        let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
        let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
        let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
        Affine { m: mat_mul(rz, mat_mul(ry, rx)), t: Vec3::ZERO }
    }

    /// Rotation by `deg` around `pivot`.
    pub fn rotation_about(pivot: Vec3, deg: Vec3) -> Affine {
        Affine::translation(pivot).then(&Affine::rotation_euler(deg)).then(&Affine::translation(-pivot))
    }

    /// `self ∘ other`: apply `other` first, then `self`.
    pub fn then(&self, other: &Affine) -> Affine {
        Affine { m: mat_mul(self.m, other.m), t: self.apply_vector(other.t) + self.t }
    }

    pub fn apply(&self, p: Vec3) -> Vec3 {
        self.apply_vector(p) + self.t
    }

    pub fn apply_vector(&self, v: Vec3) -> Vec3 {
        let m = &self.m;
        Vec3::new(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        )
    }
}

fn mat_mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations() {
        let r = Affine::rotation_euler(Vec3::new(0.0, 90.0, 0.0));
        let p = r.apply(Vec3::new(0.0, 0.0, 1.0));
        assert!((p.x - 1.0).abs() < 1e-5 && p.z.abs() < 1e-5);
        let about = Affine::rotation_about(Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 90.0));
        let q = about.apply(Vec3::new(2.0, 0.0, 0.0));
        assert!((q.x - 1.0).abs() < 1e-5 && (q.y - 1.0).abs() < 1e-5);
        let chain = Affine::translation(Vec3::new(5.0, 0.0, 0.0)).then(&about);
        assert!((chain.apply(Vec3::new(2.0, 0.0, 0.0)).x - 6.0).abs() < 1e-5);
    }
}
