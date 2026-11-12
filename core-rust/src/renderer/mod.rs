//! Deterministic integer compositing of layers into straight-alpha RGBA8.
//!
//! The same input always produces bit-identical output on every platform, which is what
//! the cross-platform golden tests rely on.

use crate::pixel::{Rgba, TILE_SIZE};
use crate::project::Document;

#[derive(Debug, Clone)]
pub struct OnionSkin {
    pub before: usize,
    pub after: usize,
    /// Opacity of the nearest onion frame (0-255); further frames fade out.
    pub opacity: u8,
    pub tint: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Restrict to a single layer id (ignores visibility). `None` = all visible layers.
    pub only_layer: Option<u32>,
    pub onion: Option<OnionSkin>,
    pub background: Option<Rgba>,
}

/// An RGBA8 image, row-major, 4 bytes per pixel.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Image {
        Image { width, height, pixels: vec![0; width as usize * height as usize * 4] }
    }

    pub fn get(&self, x: u32, y: u32) -> Rgba {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Rgba::new(self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3])
    }

    /// Nearest-neighbour upscale by an integer factor.
    pub fn scaled(&self, factor: u32) -> Image {
        if factor <= 1 {
            return self.clone();
        }
        let (w, h) = (self.width * factor, self.height * factor);
        let mut out = Image::new(w, h);
        let src_row = self.width as usize * 4;
        for y in 0..self.height as usize {
            let mut row = Vec::with_capacity(w as usize * 4);
            for x in 0..self.width as usize {
                let p = &self.pixels[y * src_row + x * 4..y * src_row + x * 4 + 4];
                for _ in 0..factor {
                    row.extend_from_slice(p);
                }
            }
            for k in 0..factor as usize {
                let start = (y * factor as usize + k) * row.len();
                out.pixels[start..start + row.len()].copy_from_slice(&row);
            }
        }
        out
    }

    /// Copies `src` into this image at (x, y) without blending.
    pub fn blit(&mut self, src: &Image, x: u32, y: u32) {
        for row in 0..src.height {
            let dy = y + row;
            if dy >= self.height {
                break;
            }
            let w = src.width.min(self.width.saturating_sub(x)) as usize;
            let s = row as usize * src.width as usize * 4;
            let d = (dy as usize * self.width as usize + x as usize) * 4;
            self.pixels[d..d + w * 4].copy_from_slice(&src.pixels[s..s + w * 4]);
        }
    }

    /// FNV-1a 64-bit hash of size + pixels; used by golden tests.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |b: u8| {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        self.width.to_le_bytes().into_iter().for_each(&mut eat);
        self.height.to_le_bytes().into_iter().for_each(&mut eat);
        self.pixels.iter().copied().for_each(&mut eat);
        h
    }
}

#[inline]
fn div255(v: u32) -> u32 {
    (v + 128 + ((v + 128) >> 8)) >> 8
}

/// Source-over blend of straight-alpha colors with an extra opacity factor.
#[inline]
pub fn blend_over(dst: &mut [u8], src: Rgba, opacity: u8) {
    let sa = div255(src.a as u32 * opacity as u32);
    if sa == 0 {
        return;
    }
    let da = dst[3] as u32;
    if sa == 255 || da == 0 {
        dst[0] = src.r;
        dst[1] = src.g;
        dst[2] = src.b;
        dst[3] = sa as u8;
        return;
    }
    let inv = 255 - sa;
    let out_a = sa + div255(da * inv);
    let mix = |s: u8, d: u8| -> u8 {
        let num = s as u32 * sa * 255 + d as u32 * da * inv;
        ((num + out_a * 255 / 2) / (out_a * 255)) as u8
    };
    dst[0] = mix(src.r, dst[0]);
    dst[1] = mix(src.g, dst[1]);
    dst[2] = mix(src.b, dst[2]);
    dst[3] = out_a as u8;
}

fn composite_frame(doc: &Document, frame_index: usize, opts: &RenderOptions, img: &mut Image, opacity_scale: u8, tint: Option<Rgba>) {
    let Some(frame) = doc.frames.get(frame_index) else { return };
    for layer in &doc.layers {
        let include = match opts.only_layer {
            Some(id) => id == layer.id,
            None => layer.visible,
        };
        if !include || layer.opacity == 0 {
            continue;
        }
        let Some(grid) = doc.cel(layer.id, frame.id) else { continue };
        let opacity = div255(layer.opacity as u32 * opacity_scale as u32) as u8;
        for ty in 0..grid.tiles_y() {
            for tx in 0..grid.tiles_x() {
                let Some(tile) = grid.tile(tx, ty) else { continue };
                for (i, &v) in tile.iter().enumerate() {
                    if v == 0 {
                        continue;
                    }
                    let x = tx * TILE_SIZE + i as u32 % TILE_SIZE;
                    let y = ty * TILE_SIZE + i as u32 / TILE_SIZE;
                    if x >= img.width || y >= img.height {
                        continue;
                    }
                    let mut c = Rgba::from_u32(v);
                    if let Some(t) = tint {
                        c = Rgba::new(
                            ((c.r as u16 + t.r as u16) / 2) as u8,
                            ((c.g as u16 + t.g as u16) / 2) as u8,
                            ((c.b as u16 + t.b as u16) / 2) as u8,
                            c.a,
                        );
                    }
                    let o = (y as usize * img.width as usize + x as usize) * 4;
                    if layer.blend == crate::layers::Blend::Normal || img.pixels[o + 3] == 0 {
                        blend_over(&mut img.pixels[o..o + 4], c, opacity);
                    } else {
                        let px = &img.pixels[o..o + 4];
                        let mixed = Rgba::new(
                            layer.blend.channel(px[0] as u32, c.r as u32) as u8,
                            layer.blend.channel(px[1] as u32, c.g as u32) as u8,
                            layer.blend.channel(px[2] as u32, c.b as u32) as u8,
                            c.a,
                        );
                        blend_over(&mut img.pixels[o..o + 4], mixed, opacity);
                    }
                }
            }
        }
    }
}

/// Renders one frame. Onion frames are drawn underneath the current frame.
pub fn render(doc: &Document, frame_index: usize, opts: &RenderOptions) -> Image {
    let mut img = Image::new(doc.width, doc.height);
    if let Some(bg) = opts.background {
        for px in img.pixels.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&[bg.r, bg.g, bg.b, bg.a]);
        }
    }
    if let Some(onion) = &opts.onion {
        let red = Rgba::new(255, 64, 64, 255);
        let blue = Rgba::new(64, 128, 255, 255);
        let fade = |dist: usize, max: usize| -> u8 { (onion.opacity as usize * (max + 1 - dist) / (max + 1).max(1)) as u8 };
        for d in (1..=onion.before).rev() {
            if let Some(i) = frame_index.checked_sub(d) {
                composite_frame(doc, i, opts, &mut img, fade(d, onion.before), onion.tint.then_some(red));
            }
        }
        for d in (1..=onion.after).rev() {
            let i = frame_index + d;
            if i < doc.frames.len() {
                composite_frame(doc, i, opts, &mut img, fade(d, onion.after), onion.tint.then_some(blue));
            }
        }
    }
    composite_frame(doc, frame_index, opts, &mut img, 255, None);
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_rules() {
        let mut d = [0u8, 0, 0, 0];
        blend_over(&mut d, Rgba::new(10, 20, 30, 255), 255);
        assert_eq!(d, [10, 20, 30, 255]);
        let mut d = [0u8, 0, 255, 255];
        blend_over(&mut d, Rgba::new(255, 0, 0, 255), 128);
        assert_eq!(d, [128, 0, 127, 255]);
        let mut d = [9u8, 9, 9, 255];
        blend_over(&mut d, Rgba::new(255, 0, 0, 0), 255);
        assert_eq!(d, [9, 9, 9, 255]);
    }

    #[test]
    fn layer_order_and_visibility() {
        let mut doc = Document::new(4, 4);
        let bottom = doc.active_layer;
        let top = doc.insert_layer("top", 1);
        let f = doc.active_frame;
        let red = Rgba::new(255, 0, 0, 255).to_pixel();
        let blue = Rgba::new(0, 0, 255, 255).to_pixel();
        doc.cel_mut(bottom, f).set(1, 1, red);
        doc.cel_mut(top, f).set(1, 1, blue);
        let img = render(&doc, 0, &RenderOptions::default());
        assert_eq!(img.get(1, 1), Rgba::new(0, 0, 255, 255));
        doc.layers[1].visible = false;
        let img = render(&doc, 0, &RenderOptions::default());
        assert_eq!(img.get(1, 1), Rgba::new(255, 0, 0, 255));
        assert_eq!(img.scaled(3).get(5, 5), Rgba::new(255, 0, 0, 255));
    }
}
