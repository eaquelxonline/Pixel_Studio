//! Character-art finishing tools: automatic cel shading of flat color regions (with cast
//! shadows from the layers above and rim light), sel-out outlines, gradient-map color grading,
//! sparkle stamps and "pixel perfect" line cleanup.
//!
//! The intended workflow mirrors how anime pixel artists work: block the character in flat
//! colors on a few layers (hair back, body, face, hair front), let `auto_shade` produce
//! hue-shifted 2-4 tone shading that follows each shape, add sel-out outlines, then paint the
//! eyes and highlights by hand.

use std::collections::HashMap;

use super::{from_lch, hue_toward, lch, outline_color, ramp};
use crate::pixel::Rgba;
use crate::renderer::Image;

#[derive(Debug, Clone)]
pub struct ShadeSettings {
    /// Direction toward the light in image space (x right, y down), e.g. (-1, -1) = top-left.
    pub light: (f32, f32),
    /// 2 = base + shadow, 3 = + highlight, 4 = + deep shadow.
    pub tones: u32,
    /// Edge roundness in pixels (how far shading reaches into a shape).
    pub bevel: f32,
    /// Weight of the whole-shape gradient (big areas get a soft light→shadow side).
    pub form: f32,
    pub hue_shift: f32,
    pub contrast: f32,
    /// Shift of the shadow threshold: higher = more shadow.
    pub shadow_amount: f32,
    /// Regions smaller than this (pixels) are left untouched (eyes, buttons, details).
    pub min_region: usize,
    /// Explicit ramps: base color → [deep shadow, shadow, base, highlight].
    pub ramps: HashMap<u32, [Rgba; 4]>,
    /// Colors never shaded.
    pub preserve: Vec<u32>,
    /// Rim light on the silhouette edges facing away from the light: (color, strength 0-1).
    pub rim: Option<(Rgba, f32)>,
}

impl Default for ShadeSettings {
    fn default() -> Self {
        ShadeSettings {
            light: (-1.0, -1.2),
            tones: 3,
            bevel: 3.0,
            form: 0.45,
            hue_shift: 22.0,
            contrast: 0.5,
            shadow_amount: 0.0,
            min_region: 6,
            ramps: HashMap::new(),
            preserve: Vec::new(),
            rim: None,
        }
    }
}

fn key(c: Rgba) -> u32 {
    (c.r as u32) << 24 | (c.g as u32) << 16 | (c.b as u32) << 8 | 0xFF
}

/// Auto ramp [deep, shadow, base, highlight] around `base` — shadows lean violet and gain a
/// little saturation, highlights lean warm, as in the classic pastel anime palette.
pub fn auto_ramp(base: Rgba, hue_shift: f32, contrast: f32) -> [Rgba; 4] {
    let r = ramp(base, 7, hue_shift, contrast);
    [r[1], r[2], base, r[5]]
}

/// Cel-shades every flat color region of `img`. `occluders` (same size, may be empty) marks
/// pixels covered by layers drawn above; they cast shadows `cast` pixels away from the light.
pub fn auto_shade(img: &mut Image, s: &ShadeSettings, occluders: &[bool], cast: u32) -> usize {
    let (w, h) = (img.width as i32, img.height as i32);
    let n = (w * h) as usize;
    let px = |img: &Image, i: usize| Rgba::new(img.pixels[i * 4], img.pixels[i * 4 + 1], img.pixels[i * 4 + 2], img.pixels[i * 4 + 3]);
    // Connected regions of identical color (4-connectivity).
    let mut region = vec![u32::MAX; n];
    let mut regions: Vec<(Rgba, Vec<usize>)> = Vec::new();
    for start in 0..n {
        let c = px(img, start);
        if c.a == 0 || region[start] != u32::MAX {
            continue;
        }
        let id = regions.len() as u32;
        let mut members = vec![start];
        region[start] = id;
        let mut k = 0;
        while k < members.len() {
            let i = members[k];
            k += 1;
            let (x, y) = ((i as i32) % w, (i as i32) / w);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = (ny * w + nx) as usize;
                if region[j] == u32::MAX && px(img, j) == c {
                    region[j] = id;
                    members.push(j);
                }
            }
        }
        regions.push((c, members));
    }
    // Chamfer distance to the region border (3-4 metric / 3 ≈ pixels).
    let mut dist = vec![0f32; n];
    for (i, d) in dist.iter_mut().enumerate() {
        if region[i] != u32::MAX {
            *d = 1e6;
        }
    }
    let same = |a: usize, b: usize| region[a] == region[b];
    for pass in 0..2 {
        let ys: Vec<i32> = if pass == 0 { (0..h).collect() } else { (0..h).rev().collect() };
        for &y in &ys {
            let xs: Vec<i32> = if pass == 0 { (0..w).collect() } else { (0..w).rev().collect() };
            for &x in &xs {
                let i = (y * w + x) as usize;
                if region[i] == u32::MAX {
                    continue;
                }
                let sgn = if pass == 0 { -1 } else { 1 };
                let mut best = dist[i];
                for (dx, dy, cost) in
                    [(sgn, 0, 1.0f32), (0, sgn, 1.0), (sgn, sgn, std::f32::consts::SQRT_2), (-sgn, sgn, std::f32::consts::SQRT_2)]
                {
                    let (nx, ny) = (x + dx, y + dy);
                    let nd = if nx < 0 || ny < 0 || nx >= w || ny >= h || !same(i, (ny * w + nx) as usize) {
                        0.0
                    } else {
                        dist[(ny * w + nx) as usize]
                    };
                    best = best.min(nd + cost);
                }
                dist[i] = best;
            }
        }
    }
    let ll = (s.light.0 * s.light.0 + s.light.1 * s.light.1).sqrt().max(1e-3);
    let (lx, ly) = (s.light.0 / ll, s.light.1 / ll);
    let height = |i: usize| -> f32 {
        let t = (dist[i] / s.bevel.max(0.5)).min(1.0);
        (t * (2.0 - t)).sqrt()
    };
    let mut changed = 0;
    let src = img.pixels.clone();
    let mut tone_map = vec![0i8; n];
    for (rid, (base, members)) in regions.iter().enumerate() {
        let k = key(*base);
        if members.len() < s.min_region || s.preserve.contains(&k) {
            continue;
        }
        let tones = s.ramps.get(&k).copied().unwrap_or_else(|| auto_ramp(*base, s.hue_shift, s.contrast));
        // Shape centroid and extent for the whole-form gradient.
        let (mut cx, mut cy) = (0f32, 0f32);
        for &i in members {
            cx += (i as i32 % w) as f32;
            cy += (i as i32 / w) as f32;
        }
        cx /= members.len() as f32;
        cy /= members.len() as f32;
        let radius = members.iter().map(|&i| ((i as i32 % w) as f32 - cx).hypot((i as i32 / w) as f32 - cy)).fold(1.0f32, f32::max);
        for &i in members {
            let (x, y) = (i as i32 % w, i as i32 / w);
            let hh = |xx: i32, yy: i32| -> f32 {
                if xx < 0 || yy < 0 || xx >= w || yy >= h {
                    return 0.0;
                }
                let j = (yy * w + xx) as usize;
                if region[j] as usize != rid { 0.0 } else { height(j) }
            };
            let gx = hh(x + 1, y) - hh(x - 1, y);
            let gy = hh(x, y + 1) - hh(x, y - 1);
            // Outward normal (2D part) dotted with the light direction.
            let edge = -(gx * lx + gy * ly) * 0.9;
            let fx = (x as f32 - cx) / radius;
            let fy = (y as f32 - cy) / radius;
            let form = (fx * lx + fy * ly) * s.form;
            let v = edge + form - s.shadow_amount;
            let mut t: i8 = if v < -0.62 && s.tones >= 4 {
                -2
            } else if v < -0.12 {
                -1
            } else if v > 0.55 && s.tones >= 3 && height(i) < 0.95 {
                1
            } else {
                0
            };
            // Cast shadow from layers above (hair over the face, head over the neck...).
            if cast > 0 && !occluders.is_empty() && !occluders[i] {
                for step in 1..=cast as i32 {
                    let (ox, oy) = ((x as f32 + lx * step as f32).round() as i32, (y as f32 + ly * step as f32).round() as i32);
                    if ox >= 0 && oy >= 0 && ox < w && oy < h && occluders[(oy * w + ox) as usize] {
                        t = t.min(-1);
                        break;
                    }
                }
            }
            tone_map[i] = t;
        }
        // Remove lonely tone pixels (keeps clusters clean).
        for &i in members {
            let (x, y) = (i as i32 % w, i as i32 / w);
            let mut counts: HashMap<i8, u32> = HashMap::new();
            let mut alone = true;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = (ny * w + nx) as usize;
                if region[j] as usize == rid {
                    *counts.entry(tone_map[j]).or_default() += 1;
                    if tone_map[j] == tone_map[i] {
                        alone = false;
                    }
                }
            }
            if alone && let Some((&t, _)) = counts.iter().max_by_key(|(_, c)| **c) {
                tone_map[i] = t;
            }
        }
        for &i in members {
            let mut c = tones[(tone_map[i] + 2) as usize];
            if let Some((rim, k)) = s.rim {
                let (x, y) = (i as i32 % w, i as i32 / w);
                let open = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                    let (nx, ny) = (x + dx, y + dy);
                    let back = (*dx as f32 * lx + *dy as f32 * ly) < -0.3;
                    back && (nx < 0 || ny < 0 || nx >= w || ny >= h || src[((ny * w + nx) * 4 + 3) as usize] == 0)
                });
                if open {
                    c = mix(c, rim, k);
                }
            }
            c.a = base.a;
            let o = i * 4;
            if img.pixels[o..o + 4] != [c.r, c.g, c.b, c.a] {
                img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
                changed += 1;
            }
        }
    }
    changed
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgba::new(f(a.r, b.r), f(a.g, b.g), f(a.b, b.b), a.a)
}

/// Outline color for sel-out: darker hue of the neighbour; `darkness` 0-1 (0.58 = default).
pub fn selout_color(c: Rgba, darkness: f32) -> Rgba {
    let [l, ch, h] = lch(c);
    let keep = (1.0 - darkness).clamp(0.05, 0.95);
    from_lch((l * keep).max(0.1), (ch * 1.15).min(0.22), hue_toward(h, 330.0, 20.0 * darkness), 255)
}

/// Exterior (or interior with `inside`) outline. `selout` lightens the outline on the lit side
/// so the silhouette reads soft where the light hits — the pastel anime look.
pub fn outline_ex(
    img: &mut Image,
    color: Option<Rgba>,
    corners: bool,
    selout: bool,
    darkness: f32,
    light: (f32, f32),
    inside: bool,
) -> usize {
    let (w, h) = (img.width as i32, img.height as i32);
    let src = img.pixels.clone();
    let alpha = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && src[((y * w + x) * 4 + 3) as usize] > 0;
    let col = |x: i32, y: i32| {
        let o = ((y * w + x) * 4) as usize;
        Rgba::new(src[o], src[o + 1], src[o + 2], 255)
    };
    let ll = (light.0 * light.0 + light.1 * light.1).sqrt().max(1e-3);
    let (lx, ly) = (light.0 / ll, light.1 / ll);
    let n4 = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let n8 = [(1, 1), (-1, 1), (1, -1), (-1, -1)];
    let mut changed = 0;
    for y in 0..h {
        for x in 0..w {
            let target = if inside { alpha(x, y) && n4.iter().any(|(dx, dy)| !alpha(x + dx, y + dy)) } else { !alpha(x, y) };
            if !target {
                continue;
            }
            // Pick the reference interior color and the outward direction.
            let mut best: Option<(Rgba, f32, f32)> = None;
            if inside {
                let mut ox = 0.0;
                let mut oy = 0.0;
                for (dx, dy) in n4 {
                    if !alpha(x + dx, y + dy) {
                        ox += dx as f32;
                        oy += dy as f32;
                    }
                }
                best = Some((col(x, y), ox, oy));
            } else {
                for (dx, dy) in n4.iter().chain(if corners { n8.iter() } else { [].iter() }) {
                    if alpha(x + dx, y + dy) {
                        let c = col(x + dx, y + dy);
                        if best.is_none_or(|(b, _, _)| lch(c)[0] < lch(b)[0]) {
                            best = Some((c, -(*dx as f32), -(*dy as f32)));
                        }
                    }
                }
            }
            let Some((b, ox, oy)) = best else { continue };
            let mut c = color.unwrap_or_else(|| if selout || darkness != 0.58 { selout_color(b, darkness) } else { outline_color(b) });
            if selout {
                let facing = (ox * lx + oy * ly) / (ox.hypot(oy)).max(1e-3);
                if facing > 0.3 {
                    c = mix(c, b, 0.45);
                }
            }
            let o = ((y * w + x) * 4) as usize;
            img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
            changed += 1;
        }
    }
    changed
}

/// Gradient map: luminance → color along `stops`, mixed in by `strength` (0-1).
pub fn gradient_map(img: &mut Image, stops: &[Rgba], strength: f32) -> usize {
    if stops.len() < 2 {
        return 0;
    }
    let mut changed = 0;
    for p in img.pixels.as_chunks_mut::<4>().0 {
        if p[3] == 0 {
            continue;
        }
        let c = Rgba::new(p[0], p[1], p[2], p[3]);
        let l = super::oklab(c)[0].clamp(0.0, 1.0);
        let t = l * (stops.len() - 1) as f32;
        let i = (t.floor() as usize).min(stops.len() - 2);
        let g = mix(stops[i], stops[i + 1], t - i as f32);
        let out = mix(c, g, strength.clamp(0.0, 1.0));
        if [out.r, out.g, out.b] != [p[0], p[1], p[2]] {
            changed += 1;
        }
        p[0] = out.r;
        p[1] = out.g;
        p[2] = out.b;
    }
    changed
}

/// 4-point sparkle / star stamp pixels relative to its center: (dx, dy, is_core).
pub fn sparkle_shape(size: u32, style: &str) -> Vec<(i32, i32, bool)> {
    let s = size.clamp(1, 8) as i32;
    let mut out = Vec::new();
    match style {
        "dot" => out.push((0, 0, true)),
        "cross" => {
            for d in -s..=s {
                out.push((d, d, d == 0));
                if d != 0 {
                    out.push((d, -d, false));
                }
            }
        }
        "diamond" => {
            for dy in -s..=s {
                for dx in -s..=s {
                    if dx.abs() + dy.abs() <= s {
                        out.push((dx, dy, dx.abs() + dy.abs() < s / 2 + 1));
                    }
                }
            }
        }
        _ => {
            // star4: thin arms with a fat diamond core.
            let core = (s / 3).max(if s >= 3 { 1 } else { 0 });
            for dy in -s..=s {
                for dx in -s..=s {
                    let arm = (dx == 0 || dy == 0) && dx.abs() + dy.abs() <= s;
                    let body = dx.abs() + dy.abs() <= core;
                    if arm || body {
                        out.push((dx, dy, dx.abs() + dy.abs() <= core.max(0) && s >= 2 || (dx == 0 && dy == 0)));
                    }
                }
            }
        }
    }
    out
}

/// "Pixel perfect" cleanup: removes L-shaped corner doubles from 1-px lines of each color.
pub fn pixel_cleanup(img: &mut Image, only: Option<Rgba>) -> usize {
    let (w, h) = (img.width as i32, img.height as i32);
    let mut changed = 0;
    let get = |p: &[u8], x: i32, y: i32| -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x >= w || y >= h {
            return None;
        }
        let o = ((y * w + x) * 4) as usize;
        Some([p[o], p[o + 1], p[o + 2], p[o + 3]])
    };
    for y in 0..h {
        for x in 0..w {
            let Some(c) = get(&img.pixels, x, y) else { continue };
            if c[3] == 0 || only.is_some_and(|o| [o.r, o.g, o.b] != [c[0], c[1], c[2]]) {
                continue;
            }
            let same = |dx: i32, dy: i32| get(&img.pixels, x + dx, y + dy) == Some(c);
            let hn: Vec<i32> = [1, -1].into_iter().filter(|&dx| same(dx, 0)).collect();
            let vn: Vec<i32> = [1, -1].into_iter().filter(|&dy| same(0, dy)).collect();
            // An L corner of a 1-px line: exactly one horizontal and one vertical neighbour,
            // and the pixel between them diagonally is not part of the line (not a blob).
            if hn.len() == 1 && vn.len() == 1 && !same(hn[0], vn[0]) {
                // Replace with the most common other neighbour color (or clear on the edge).
                let mut counts: HashMap<[u8; 4], u32> = HashMap::new();
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    if let Some(v) = get(&img.pixels, x + dx, y + dy).filter(|v| *v != c) {
                        *counts.entry(v).or_default() += 1;
                    }
                }
                let fill = counts.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v).unwrap_or([0; 4]);
                let o = ((y * w + x) * 4) as usize;
                img.pixels[o..o + 4].copy_from_slice(&fill);
                changed += 1;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc(size: u32, c: Rgba) -> Image {
        let mut img = Image::new(size, size);
        let r = size as f32 / 2.0 - 1.0;
        for y in 0..size {
            for x in 0..size {
                if (x as f32 + 0.5 - size as f32 / 2.0).hypot(y as f32 + 0.5 - size as f32 / 2.0) <= r {
                    let o = ((y * size + x) * 4) as usize;
                    img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
                }
            }
        }
        img
    }

    #[test]
    fn auto_shade_lights_top_left_and_shadows_bottom_right() {
        let base = Rgba::new(240, 150, 190, 255);
        let mut img = disc(24, base);
        let n = auto_shade(&mut img, &ShadeSettings::default(), &[], 0);
        assert!(n > 20);
        let lum = |c: Rgba| c.r as u32 + c.g as u32 + c.b as u32;
        let tl = img.get(6, 6);
        let br = img.get(17, 17);
        assert!(lum(tl) > lum(br), "lit side brighter: {tl:?} vs {br:?}");
        assert_eq!(img.get(12, 12), base, "interior keeps the base color");
        let distinct: std::collections::HashSet<[u8; 4]> = img.pixels.as_chunks::<4>().0.iter().copied().collect();
        assert!(distinct.len() <= 5, "only ramp tones (+ transparent)");
    }

    #[test]
    fn occluders_cast_shadows_and_small_regions_are_kept() {
        let base = Rgba::new(250, 220, 200, 255);
        let mut img = Image::new(16, 16);
        for p in img.pixels.as_chunks_mut::<4>().0 {
            *p = [base.r, base.g, base.b, 255];
        }
        // Tiny 2-px detail stays untouched.
        img.pixels[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 4].copy_from_slice(&[10, 10, 10, 255]);
        img.pixels[(8 * 16 + 9) * 4..(8 * 16 + 9) * 4 + 4].copy_from_slice(&[10, 10, 10, 255]);
        let mut occ = vec![false; 256];
        for x in 0..16 {
            occ[2 * 16 + x] = true; // a "bang" row above
        }
        let s = ShadeSettings { form: 0.0, ..Default::default() };
        auto_shade(&mut img, &s, &occ, 2);
        let lum = |c: Rgba| c.r as u32 + c.g as u32 + c.b as u32;
        assert!(lum(img.get(8, 3)) < lum(img.get(8, 12)), "shadow just below the occluder");
        assert_eq!(img.get(8, 8), Rgba::new(10, 10, 10, 255));
    }

    #[test]
    fn selout_gradient_map_sparkle_and_cleanup() {
        let mut img = disc(12, Rgba::new(255, 170, 200, 255));
        outline_ex(&mut img, None, false, true, 0.5, (-1.0, -1.0), false);
        let top = img.get(6, 0);
        let bottom = img.get(6, 11);
        assert!(top.a == 255 && bottom.a == 255);
        assert!(top.r as u32 + top.g as u32 > bottom.r as u32 + bottom.g as u32, "lit outline is lighter");
        let mut g = disc(8, Rgba::new(200, 200, 200, 255));
        gradient_map(&mut g, &[Rgba::new(0, 0, 80, 255), Rgba::new(120, 200, 255, 255)], 1.0);
        let c = g.get(4, 4);
        assert!(c.b > c.r, "graded toward blue");
        assert!(sparkle_shape(4, "star4").len() > 12);
        let mut line = Image::new(5, 5);
        for (x, y) in [(0, 0), (1, 0), (1, 1), (2, 1)] {
            let o = (y * 5 + x) * 4;
            line.pixels[o..o + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
        assert_eq!(pixel_cleanup(&mut line, None), 1, "one corner double removed");
    }
}
