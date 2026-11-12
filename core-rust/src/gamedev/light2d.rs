//! Normal-mapped 2D lighting for pixel art (the technique behind dynamically lit pixel games):
//! sprite albedo × (ambient + Σ point lights · N·L), with optional light banding so the result
//! keeps crisp pixel-art shading steps instead of smooth gradients.

use crate::pixel::Rgba;
use crate::renderer::Image;

#[derive(Debug, Clone, Copy)]
pub struct Light2D {
    pub x: f32,
    pub y: f32,
    /// Height above the sprite plane in pixels (low = grazing light, high = overhead).
    pub z: f32,
    pub color: [f32; 3],
    pub intensity: f32,
    pub radius: f32,
}

pub struct LightSettings {
    pub ambient: [f32; 3],
    pub lights: Vec<Light2D>,
    /// 0 = smooth; N ≥ 2 quantizes light intensity into N bands.
    pub bands: u32,
    /// Strength of the normal map's influence (0 = flat).
    pub normal_strength: f32,
}

fn lin(c: u8) -> f32 {
    (c as f32 / 255.0).powf(2.2)
}

fn srgb(v: f32) -> u8 {
    (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8
}

/// `normals` is a tangent-space normal map of the same size (RGB = XYZ*0.5+0.5, y up).
pub fn light(albedo: &Image, normals: &Image, s: &LightSettings) -> Image {
    let mut out = Image::new(albedo.width, albedo.height);
    for y in 0..albedo.height {
        for x in 0..albedo.width {
            let a = albedo.get(x, y);
            if a.a == 0 {
                continue;
            }
            let nm = normals.get(x, y);
            let mut n = [(nm.r as f32 / 255.0) * 2.0 - 1.0, (nm.g as f32 / 255.0) * 2.0 - 1.0, (nm.b as f32 / 255.0) * 2.0 - 1.0];
            if nm.a == 0 {
                n = [0.0, 0.0, 1.0];
            }
            n[0] *= s.normal_strength;
            n[1] *= s.normal_strength;
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-5);
            let n = [n[0] / len, n[1] / len, n[2] / len];
            let mut acc = s.ambient;
            for l in &s.lights {
                // Image y grows downward; normal map y points up.
                let (dx, dy, dz) = (l.x - (x as f32 + 0.5), -(l.y - (y as f32 + 0.5)), l.z);
                let d = (dx * dx + dy * dy + dz * dz).sqrt().max(1e-3);
                let planar = (dx * dx + dy * dy).sqrt();
                if planar > l.radius {
                    continue;
                }
                let ndl = ((n[0] * dx + n[1] * dy + n[2] * dz) / d).max(0.0);
                let att = (1.0 - planar / l.radius).powi(2);
                let mut k = ndl * att * l.intensity;
                if s.bands >= 2 {
                    let b = s.bands as f32;
                    k = (k * b).ceil().min(b) / b;
                }
                for c in 0..3 {
                    acc[c] += l.color[c] * k;
                }
            }
            let px = [lin(a.r) * acc[0], lin(a.g) * acc[1], lin(a.b) * acc[2]];
            let o = ((y * out.width + x) * 4) as usize;
            out.pixels[o..o + 4].copy_from_slice(&[srgb(px[0]), srgb(px[1]), srgb(px[2]), a.a]);
        }
    }
    out
}

pub fn rgb_lin(c: Rgba) -> [f32; 3] {
    [lin(c.r), lin(c.g), lin(c.b)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lights_brighten_near_and_facing_pixels() {
        let mut img = Image::new(16, 16);
        for p in img.pixels.as_chunks_mut::<4>().0 {
            *p = [128, 128, 128, 255];
        }
        let flat = {
            let mut n = Image::new(16, 16);
            for p in n.pixels.as_chunks_mut::<4>().0 {
                *p = [128, 128, 255, 255];
            }
            n
        };
        let s = LightSettings {
            ambient: [0.05, 0.05, 0.08],
            lights: vec![Light2D { x: 2.0, y: 8.0, z: 6.0, color: [1.0, 0.6, 0.3], intensity: 2.0, radius: 12.0 }],
            bands: 4,
            normal_strength: 1.0,
        };
        let out = light(&img, &flat, &s);
        assert!(out.get(2, 8).r > out.get(13, 8).r);
        assert!(out.get(2, 8).r > out.get(2, 8).b, "warm light");
        let distinct: std::collections::HashSet<u8> = (0..16).map(|x| out.get(x, 8).r).collect();
        assert!(distinct.len() <= 6, "banded light keeps few steps");
    }
}
