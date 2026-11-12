//! Pixel-art craft tools: perceptual color (OKLab), hue-shifted ramps, palette library,
//! median-cut quantization with ordered dithering, selective outlines, dither fills and 2D
//! normal maps for dynamic sprite lighting.

pub mod charart;
pub mod palettes;

use crate::pixel::{PixelGrid, Rgba, TRANSPARENT};
use crate::renderer::Image;

// ---- color science ----------------------------------------------------------------------

fn to_lin(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn from_lin(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

/// OKLab (L, a, b) of an sRGB color.
pub fn oklab(c: Rgba) -> [f32; 3] {
    let (r, g, b) = (to_lin(c.r), to_lin(c.g), to_lin(c.b));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

pub fn from_oklab(lab: [f32; 3], alpha: u8) -> Rgba {
    let l = lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2];
    let m = lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2];
    let s = lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    Rgba::new(
        from_lin(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        from_lin(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
        from_lin(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
        alpha,
    )
}

pub(crate) fn lch(c: Rgba) -> [f32; 3] {
    let [l, a, b] = oklab(c);
    [l, (a * a + b * b).sqrt(), b.atan2(a).to_degrees().rem_euclid(360.0)]
}

fn lab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let l = lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2];
    let m = lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2];
    let s = lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// OKLCH → sRGB with gamut mapping by chroma reduction (keeps hue and lightness exact).
pub(crate) fn from_lch(l: f32, c: f32, h: f32, alpha: u8) -> Rgba {
    let hr = h.to_radians();
    let inside = |c: f32| lab_to_linear([l, c * hr.cos(), c * hr.sin()]).iter().all(|v| (-0.001..=1.001).contains(v));
    let mut chroma = c;
    if !inside(chroma) {
        let (mut lo, mut hi) = (0.0f32, c);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if inside(mid) { lo = mid } else { hi = mid }
        }
        chroma = lo;
    }
    from_oklab([l, chroma * hr.cos(), chroma * hr.sin()], alpha)
}

pub(crate) fn hue_toward(h: f32, target: f32, amount: f32) -> f32 {
    let d = ((target - h + 540.0) % 360.0) - 180.0;
    (h + d.clamp(-amount.abs(), amount.abs())).rem_euclid(360.0)
}

/// A hue-shifted color ramp around `base` (the base sits in the middle): shadows drift toward
/// cool purple-blue and gain saturation, highlights drift toward warm yellow and lose some
/// saturation — the classic pixel-art shading recipe.
pub fn ramp(base: Rgba, steps: usize, hue_shift: f32, contrast: f32) -> Vec<Rgba> {
    let steps = steps.clamp(2, 16);
    let [l0, c0, h0] = lch(base);
    let mid = (steps - 1) as f32 / 2.0;
    (0..steps)
        .map(|i| {
            let t = (i as f32 - mid) / mid.max(1.0); // -1 (dark) .. +1 (light)
            let l = (l0 + t * contrast * if t < 0.0 { l0.min(0.55) } else { (1.0 - l0).min(0.45) }).clamp(0.08, 0.98);
            let (h, c) = if t < 0.0 {
                (hue_toward(h0, 285.0, hue_shift * -t), c0 * (1.0 + 0.25 * -t))
            } else {
                (hue_toward(h0, 85.0, hue_shift * t), c0 * (1.0 - 0.35 * t))
            };
            from_lch(l, c, h, base.a)
        })
        .collect()
}

/// Darker, cooler companion color used for selective outlines.
pub fn outline_color(c: Rgba) -> Rgba {
    let [l, ch, h] = lch(c);
    from_lch((l * 0.42).max(0.12), (ch * 1.1).min(0.2), hue_toward(h, 285.0, 25.0), 255)
}

pub fn palette_by_name(name: &str) -> Option<Vec<Rgba>> {
    palettes::PALETTES.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)).map(|(_, _, c)| c.iter().map(|&v| Rgba::from_u32(v)).collect())
}

// ---- quantization -------------------------------------------------------------------------

/// Median-cut palette of at most `n` colors from (color, weight) samples.
pub fn median_cut(samples: &[(Rgba, u32)], n: usize) -> Vec<Rgba> {
    if samples.is_empty() {
        return Vec::new();
    }
    let mut boxes: Vec<Vec<(Rgba, u32)>> = vec![samples.to_vec()];
    while boxes.len() < n {
        // Split the box with the widest channel range (weighted by population).
        let (bi, ch, range) = boxes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let mut best = (0usize, 0i32);
                for ch in 0..3 {
                    let get = |c: &Rgba| [c.r, c.g, c.b][ch] as i32;
                    let lo = b.iter().map(|(c, _)| get(c)).min().unwrap();
                    let hi = b.iter().map(|(c, _)| get(c)).max().unwrap();
                    if hi - lo > best.1 {
                        best = (ch, hi - lo);
                    }
                }
                let pop: u32 = b.iter().map(|(_, w)| w).sum();
                (i, best.0, best.1 as f32 * (pop as f32).sqrt())
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .unwrap();
        if range <= 0.0 || boxes[bi].len() < 2 {
            break;
        }
        let mut b = boxes.swap_remove(bi);
        b.sort_by_key(|(c, _)| [c.r, c.g, c.b][ch]);
        let total: u32 = b.iter().map(|(_, w)| w).sum();
        let mut acc = 0;
        let mut cut = 1;
        for (i, (_, w)) in b.iter().enumerate() {
            acc += w;
            if acc * 2 >= total {
                cut = (i + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let tail = b.split_off(cut);
        boxes.push(b);
        boxes.push(tail);
    }
    boxes
        .iter()
        .map(|b| {
            let total: f32 = b.iter().map(|(_, w)| *w as f32).sum();
            let mut acc = [0.0f32; 3];
            for (c, w) in b {
                let lab = oklab(*c);
                for k in 0..3 {
                    acc[k] += lab[k] * *w as f32;
                }
            }
            from_oklab([acc[0] / total, acc[1] / total, acc[2] / total], 255)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dither {
    None,
    Bayer2,
    Bayer4,
    Bayer8,
}

impl Dither {
    pub fn parse(s: &str) -> Option<Dither> {
        Some(match s {
            "none" | "off" => Dither::None,
            "bayer2" => Dither::Bayer2,
            "bayer4" | "ordered" => Dither::Bayer4,
            "bayer8" => Dither::Bayer8,
            _ => return None,
        })
    }

    /// Threshold in -0.5..0.5 at a pixel.
    fn threshold(self, x: u32, y: u32) -> f32 {
        let n = match self {
            Dither::None => return 0.0,
            Dither::Bayer2 => 2,
            Dither::Bayer4 => 4,
            Dither::Bayer8 => 8,
        };
        bayer(x % n, y % n, n) as f32 / (n * n) as f32 - 0.5 + 0.5 / (n * n) as f32
    }
}

/// Recursive Bayer matrix value at (x, y) for an n×n matrix (n power of two).
fn bayer(x: u32, y: u32, n: u32) -> u32 {
    if n == 1 {
        return 0;
    }
    let h = n / 2;
    let q = match (x >= h, y >= h) {
        (false, false) => 0,
        (true, true) => 1,
        (true, false) => 2,
        (false, true) => 3,
    };
    4 * bayer(x % h, y % h, h) + q
}

/// Maps a color to the perceptually nearest palette entry.
pub struct Mapper {
    palette: Vec<(Rgba, [f32; 3])>,
}

impl Mapper {
    pub fn new(palette: &[Rgba]) -> Mapper {
        Mapper { palette: palette.iter().filter(|c| c.a > 0).map(|&c| (c, oklab(c))).collect() }
    }

    pub fn nearest(&self, c: Rgba) -> Rgba {
        let lab = oklab(c);
        let mut best = (f32::MAX, c);
        for (p, pl) in &self.palette {
            let d = (lab[0] - pl[0]).powi(2) * 1.4 + (lab[1] - pl[1]).powi(2) + (lab[2] - pl[2]).powi(2);
            if d < best.0 {
                best = (d, *p);
            }
        }
        Rgba::new(best.1.r, best.1.g, best.1.b, c.a)
    }

    /// Nearest color after an ordered-dither offset (`strength` in sRGB steps, ~24 typical).
    pub fn dithered(&self, c: Rgba, x: u32, y: u32, dither: Dither, strength: f32) -> Rgba {
        let t = dither.threshold(x, y) * strength;
        let s = |v: u8| (v as f32 + t).clamp(0.0, 255.0) as u8;
        self.nearest(Rgba::new(s(c.r), s(c.g), s(c.b), c.a))
    }
}

/// Quantizes an RGBA image in place. Alpha is snapped to 0/255 (pixel art has no soft edges).
pub fn quantize_image(img: &mut Image, palette: &[Rgba], dither: Dither, strength: f32) {
    let mapper = Mapper::new(palette);
    for y in 0..img.height {
        for x in 0..img.width {
            let o = ((y * img.width + x) * 4) as usize;
            let p = &mut img.pixels[o..o + 4];
            if p[3] < 128 {
                p.copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let q = mapper.dithered(Rgba::new(p[0], p[1], p[2], 255), x, y, dither, strength);
            p.copy_from_slice(&[q.r, q.g, q.b, 255]);
        }
    }
}

pub fn image_samples(img: &Image) -> Vec<(Rgba, u32)> {
    let mut map: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for p in img.pixels.as_chunks::<4>().0 {
        if p[3] >= 128 {
            *map.entry(Rgba::new(p[0], p[1], p[2], 255).to_u32()).or_default() += 1;
        }
    }
    let mut v: Vec<(Rgba, u32)> = map.into_iter().map(|(c, n)| (Rgba::from_u32(c), n)).collect();
    v.sort_by_key(|(c, _)| c.to_u32());
    v
}

/// Selective 1-px outline around an RGBA image (exterior), optionally with diagonal corners.
pub fn outline_image(img: &mut Image, color: Option<Rgba>, corners: bool) {
    let (w, h) = (img.width as i32, img.height as i32);
    let src = img.pixels.clone();
    let alpha = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && src[((y * w + x) * 4 + 3) as usize] > 0;
    for y in 0..h {
        for x in 0..w {
            if alpha(x, y) {
                continue;
            }
            let mut best: Option<Rgba> = None;
            let n4 = [(1, 0), (-1, 0), (0, 1), (0, -1)];
            let n8 = [(1, 1), (-1, 1), (1, -1), (-1, -1)];
            for (dx, dy) in n4.iter().chain(if corners { n8.iter() } else { [].iter() }) {
                let (nx, ny) = (x + dx, y + dy);
                if alpha(nx, ny) {
                    let o = ((ny * w + nx) * 4) as usize;
                    let c = Rgba::new(src[o], src[o + 1], src[o + 2], 255);
                    if best.is_none_or(|b| lch(c)[0] < lch(b)[0]) {
                        best = Some(c);
                    }
                }
            }
            if let Some(b) = best {
                let c = color.unwrap_or_else(|| outline_color(b));
                let o = ((y * w + x) * 4) as usize;
                img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
            }
        }
    }
}

// ---- cel operations ---------------------------------------------------------------------------

pub fn grid_to_image(g: &PixelGrid) -> Image {
    let mut img = Image::new(g.width(), g.height());
    g.for_each_opaque(|x, y, v| {
        let c = Rgba::from_u32(v);
        let o = ((y * g.width() + x) * 4) as usize;
        img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
    });
    img
}

/// Writes an image back into a cel; returns the number of changed pixels.
pub fn image_to_grid(img: &Image, g: &mut PixelGrid) -> usize {
    let mut changed = 0;
    for y in 0..img.height.min(g.height()) {
        for x in 0..img.width.min(g.width()) {
            let o = ((y * img.width + x) * 4) as usize;
            let p = &img.pixels[o..o + 4];
            let v = if p[3] == 0 { TRANSPARENT } else { Rgba::new(p[0], p[1], p[2], p[3]).to_pixel() };
            changed += g.set(x as i64, y as i64, v) as usize;
        }
    }
    changed
}

/// Height from the distance to the sprite edge plus luminance, turned into a tangent-space
/// normal map (RGB = normal, A = sprite alpha) for 2D dynamic lighting in game engines.
pub fn normal_map(img: &Image, strength: f32, bevel: f32) -> Image {
    let (w, h) = (img.width as usize, img.height as usize);
    let solid = |i: usize| img.pixels[i * 4 + 3] > 0;
    // Chamfer distance to transparency.
    let mut dist: Vec<f32> = (0..w * h).map(|i| if solid(i) { 1e6 } else { 0.0 }).collect();
    for pass in 0..2 {
        let ys: Box<dyn Iterator<Item = usize>> = if pass == 0 { Box::new(0..h) } else { Box::new((0..h).rev()) };
        for y in ys {
            let xs: Vec<usize> = if pass == 0 { (0..w).collect() } else { (0..w).rev().collect() };
            for x in xs {
                let i = y * w + x;
                if dist[i] == 0.0 {
                    continue;
                }
                let mut d = dist[i];
                for (dx, dy, c) in [(-1i32, 0i32, 1.0f32), (0, -1, 1.0), (-1, -1, 1.414), (1, -1, 1.414)] {
                    let (dx, dy) = if pass == 0 { (dx, dy) } else { (-dx, -dy) };
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    let nd = if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 { 0.0 } else { dist[ny as usize * w + nx as usize] };
                    d = d.min(nd + c);
                }
                dist[i] = d;
            }
        }
    }
    let height = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            return 0.0;
        }
        let i = y as usize * w + x as usize;
        if !solid(i) {
            return 0.0;
        }
        let edge = (dist[i] / bevel.max(0.5)).min(1.0);
        let lum = (img.pixels[i * 4] as f32 * 0.3 + img.pixels[i * 4 + 1] as f32 * 0.59 + img.pixels[i * 4 + 2] as f32 * 0.11) / 255.0;
        edge.sqrt() * 0.8 + lum * 0.2
    };
    let mut out = Image::new(w as u32, h as u32);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let i = y as usize * w + x as usize;
            if !solid(i) {
                continue;
            }
            let dx = height(x + 1, y) - height(x - 1, y);
            let dy = height(x, y + 1) - height(x, y - 1);
            let (nx, ny, nz) = (-dx * strength, dy * strength, 1.0f32);
            let l = (nx * nx + ny * ny + nz * nz).sqrt();
            let enc = |v: f32| ((v / l) * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0;
            let o = i * 4;
            out.pixels[o..o + 4].copy_from_slice(&[enc(nx) as u8, enc(ny) as u8, enc(nz) as u8, img.pixels[o + 3]]);
        }
    }
    out
}

/// Ordered dither pattern test: true where `color2` should be used for a 0..16 density level.
pub fn dither_pick(x: i64, y: i64, level: u32) -> bool {
    bayer(x.rem_euclid(4) as u32, y.rem_euclid(4) as u32, 4) < level.min(16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_roundtrip_and_ramps() {
        for c in [Rgba::new(255, 0, 0, 255), Rgba::new(18, 200, 77, 255), Rgba::new(250, 250, 250, 255), Rgba::new(10, 10, 40, 255)] {
            let back = from_oklab(oklab(c), 255);
            for (a, b) in [(c.r, back.r), (c.g, back.g), (c.b, back.b)] {
                assert!((a as i32 - b as i32).abs() <= 1, "{c:?} -> {back:?}");
            }
        }
        let r = ramp(Rgba::parse_hex("#3FA34D").unwrap(), 5, 20.0, 0.8);
        assert_eq!(r.len(), 5);
        let l: Vec<f32> = r.iter().map(|c| lch(*c)[0]).collect();
        assert!(l.windows(2).all(|w| w[0] < w[1]), "ramp goes dark → light: {l:?}");
        let (hd, hb, hl) = (lch(r[0])[2], lch(r[2])[2], lch(r[4])[2]);
        assert!(hd > hb && hl < hb, "shadows cooler, highlights warmer ({hd} {hb} {hl})");
        assert!(palette_by_name("endesga-32").unwrap().len() == 32);
        assert!(palette_by_name("Sweetie-16").is_some());
    }

    #[test]
    fn quantize_and_dither() {
        let mut img = Image::new(16, 1);
        for x in 0..16u32 {
            let v = (x * 17) as u8;
            img.pixels[(x * 4) as usize..(x * 4 + 4) as usize].copy_from_slice(&[v, v, v, 255]);
        }
        let pal = median_cut(&image_samples(&img), 4);
        assert!(pal.len() <= 4);
        let mut q = img.clone();
        quantize_image(&mut q, &pal, Dither::None, 0.0);
        let distinct: std::collections::HashSet<_> = q.pixels.chunks(4).map(|p| p.to_vec()).collect();
        assert!(distinct.len() <= 4);
        let mut d = img.clone();
        quantize_image(&mut d, &[Rgba::new(0, 0, 0, 255), Rgba::new(255, 255, 255, 255)], Dither::Bayer4, 255.0);
        assert!(d.pixels.chunks(4).any(|p| p[0] == 0) && d.pixels.chunks(4).any(|p| p[0] == 255));
        assert!(dither_pick(0, 0, 1) && !dither_pick(0, 0, 0));
    }

    #[test]
    fn outline_and_normals() {
        let mut img = Image::new(8, 8);
        for y in 2..6 {
            for x in 2..6 {
                let o = (y * 8 + x) * 4;
                img.pixels[o..o + 4].copy_from_slice(&[200, 80, 60, 255]);
            }
        }
        let n = normal_map(&img, 4.0, 2.0);
        assert!(n.get(2, 3).r < 128 && n.get(5, 3).r > 128, "left edge faces left, right edge faces right");
        outline_image(&mut img, None, false);
        assert_eq!(img.get(1, 3).a, 255);
        assert_eq!(img.get(1, 1).a, 0, "no corners by default");
        assert!(img.get(1, 3).r < 100);
    }
}
