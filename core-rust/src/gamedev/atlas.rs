//! Texture atlas packing: trim transparent borders, de-duplicate identical sprites and pack
//! with MaxRects (best short side fit), padding and edge extrusion against texture bleeding.

use crate::renderer::Image;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Where a source sprite ended up.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// Rectangle in the atlas.
    pub frame: Rect,
    /// Offset of the trimmed rectangle inside the original sprite.
    pub offset_x: u32,
    pub offset_y: u32,
    pub source_w: u32,
    pub source_h: u32,
    pub trimmed: bool,
    /// Index of the sprite whose pixels this entry reuses (duplicates).
    pub same_as: Option<usize>,
}

pub struct Atlas {
    pub image: Image,
    pub placements: Vec<Placement>,
}

/// Opaque bounds of an image, or None when fully transparent.
pub fn trim_bounds(img: &Image) -> Option<Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..img.height {
        for x in 0..img.width {
            if img.pixels[((y * img.width + x) * 4 + 3) as usize] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x0 != u32::MAX).then(|| Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 })
}

fn crop(img: &Image, r: Rect) -> Image {
    let mut out = Image::new(r.w, r.h);
    for y in 0..r.h {
        let s = (((r.y + y) * img.width + r.x) * 4) as usize;
        let d = (y * r.w * 4) as usize;
        out.pixels[d..d + (r.w * 4) as usize].copy_from_slice(&img.pixels[s..s + (r.w * 4) as usize]);
    }
    out
}

struct MaxRects {
    free: Vec<Rect>,
}

impl MaxRects {
    fn new(w: u32, h: u32) -> MaxRects {
        MaxRects { free: vec![Rect { x: 0, y: 0, w, h }] }
    }

    fn insert(&mut self, w: u32, h: u32) -> Option<Rect> {
        let mut best: Option<(u32, u32, Rect)> = None;
        for f in &self.free {
            if w <= f.w && h <= f.h {
                let short = (f.w - w).min(f.h - h);
                let long = (f.w - w).max(f.h - h);
                if best.is_none_or(|(s, l, _)| (short, long) < (s, l)) {
                    best = Some((short, long, Rect { x: f.x, y: f.y, w, h }));
                }
            }
        }
        let (_, _, r) = best?;
        let mut next = Vec::with_capacity(self.free.len() + 4);
        for f in self.free.drain(..) {
            let overlaps = r.x < f.x + f.w && r.x + r.w > f.x && r.y < f.y + f.h && r.y + r.h > f.y;
            if !overlaps {
                next.push(f);
                continue;
            }
            if r.x > f.x {
                next.push(Rect { x: f.x, y: f.y, w: r.x - f.x, h: f.h });
            }
            if r.x + r.w < f.x + f.w {
                next.push(Rect { x: r.x + r.w, y: f.y, w: f.x + f.w - r.x - r.w, h: f.h });
            }
            if r.y > f.y {
                next.push(Rect { x: f.x, y: f.y, w: f.w, h: r.y - f.y });
            }
            if r.y + r.h < f.y + f.h {
                next.push(Rect { x: f.x, y: r.y + r.h, w: f.w, h: f.y + f.h - r.y - r.h });
            }
        }
        // Drop free rectangles contained in others.
        let mut pruned: Vec<Rect> = Vec::with_capacity(next.len());
        for (i, a) in next.iter().enumerate() {
            let contained = next
                .iter()
                .enumerate()
                .any(|(j, b)| i != j && a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h && (a != b || j < i));
            if !contained {
                pruned.push(*a);
            }
        }
        self.free = pruned;
        Some(r)
    }
}

/// Packs sprites into one atlas. `padding` = transparent gap, `extrude` = border pixels copied
/// outward around each sprite (prevents bleeding with filtering or sub-pixel cameras).
pub fn pack(sprites: &[Image], trim: bool, padding: u32, extrude: u32, power_of_two: bool) -> Result<Atlas, String> {
    let mut items: Vec<(usize, Image, Rect)> = Vec::new();
    let mut placements: Vec<Option<Placement>> = vec![None; sprites.len()];
    let mut seen: Vec<(usize, Rect, Image)> = Vec::new();
    for (i, img) in sprites.iter().enumerate() {
        let r = if trim {
            trim_bounds(img).unwrap_or(Rect { x: 0, y: 0, w: 1, h: 1 })
        } else {
            Rect { x: 0, y: 0, w: img.width, h: img.height }
        };
        let c = crop(img, r);
        if let Some((j, _, _)) = seen.iter().find(|(_, rr, ci)| *rr == r && *ci == c) {
            placements[i] = Some(Placement {
                frame: Rect { x: 0, y: 0, w: 0, h: 0 },
                offset_x: r.x,
                offset_y: r.y,
                source_w: img.width,
                source_h: img.height,
                trimmed: trim,
                same_as: Some(*j),
            });
            continue;
        }
        seen.push((i, r, c.clone()));
        items.push((i, c, r));
    }
    let gap = padding + extrude * 2;
    let area: u64 = items.iter().map(|(_, c, _)| (c.width + gap) as u64 * (c.height + gap) as u64).sum();
    let widest = items.iter().map(|(_, c, _)| c.width + gap).max().unwrap_or(1);
    let mut side = ((area as f64 * 1.1).sqrt().ceil() as u32).max(widest).max(1);
    if power_of_two {
        side = side.next_power_of_two();
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&k| std::cmp::Reverse((items[k].1.height, items[k].1.width)));
    loop {
        if side > 16384 {
            return Err("sprites do not fit in a 16384×16384 atlas".into());
        }
        let mut mr = MaxRects::new(side, side);
        let mut spots = vec![Rect { x: 0, y: 0, w: 0, h: 0 }; items.len()];
        let mut ok = true;
        for &k in &order {
            let (_, c, _) = &items[k];
            match mr.insert(c.width + gap, c.height + gap) {
                Some(r) => spots[k] = r,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            side = if power_of_two { side * 2 } else { side + side / 4 + 1 };
            continue;
        }
        // Shrink to the used area.
        let used_w = spots.iter().map(|r| r.x + r.w).max().unwrap_or(1);
        let used_h = spots.iter().map(|r| r.y + r.h).max().unwrap_or(1);
        let (aw, ah) = if power_of_two { (used_w.next_power_of_two(), used_h.next_power_of_two()) } else { (used_w, used_h) };
        let mut atlas = Image::new(aw, ah);
        for (k, (i, c, r)) in items.iter().enumerate() {
            let s = spots[k];
            let (dx, dy) = (s.x + extrude, s.y + extrude);
            atlas.blit(c, dx, dy);
            if extrude > 0 {
                for e in 1..=extrude {
                    for y in 0..c.height {
                        copy_px(&mut atlas, c, 0, y, dx - e, dy + y);
                        copy_px(&mut atlas, c, c.width - 1, y, dx + c.width - 1 + e, dy + y);
                    }
                    for x in 0..c.width {
                        copy_px(&mut atlas, c, x, 0, dx + x, dy - e);
                        copy_px(&mut atlas, c, x, c.height - 1, dx + x, dy + c.height - 1 + e);
                    }
                }
            }
            placements[*i] = Some(Placement {
                frame: Rect { x: dx, y: dy, w: c.width, h: c.height },
                offset_x: r.x,
                offset_y: r.y,
                source_w: sprites[*i].width,
                source_h: sprites[*i].height,
                trimmed: trim,
                same_as: None,
            });
        }
        let mut out: Vec<Placement> = placements.into_iter().map(|p| p.expect("placed")).collect();
        for i in 0..out.len() {
            if let Some(j) = out[i].same_as {
                out[i].frame = out[j].frame;
            }
        }
        return Ok(Atlas { image: atlas, placements: out });
    }
}

fn copy_px(dst: &mut Image, src: &Image, sx: u32, sy: u32, dx: u32, dy: u32) {
    if dx >= dst.width || dy >= dst.height {
        return;
    }
    let s = ((sy * src.width + sx) * 4) as usize;
    let d = ((dy * dst.width + dx) * 4) as usize;
    dst.pixels[d..d + 4].copy_from_slice(&src.pixels[s..s + 4]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(w: u32, h: u32, x: u32, y: u32, c: u8) -> Image {
        let mut i = Image::new(w, h);
        let o = ((y * w + x) * 4) as usize;
        i.pixels[o..o + 4].copy_from_slice(&[c, 0, 0, 255]);
        i
    }

    #[test]
    fn trims_dedupes_and_never_overlaps() {
        let mut sprites = vec![dot(32, 32, 5, 7, 10), dot(32, 32, 5, 7, 10), dot(32, 32, 20, 3, 99)];
        for k in 0..20 {
            let mut img = Image::new(24, 24);
            for y in 0..(4 + k % 9) {
                for x in 0..(3 + k % 7) {
                    let o = ((y * 24 + x) * 4) as usize;
                    img.pixels[o..o + 4].copy_from_slice(&[k as u8, 1, 2, 255]);
                }
            }
            sprites.push(img);
        }
        let a = pack(&sprites, true, 1, 1, false).unwrap();
        assert_eq!(a.placements[0].frame, a.placements[1].frame, "duplicates share pixels");
        assert_eq!(a.placements[1].same_as, Some(0));
        assert_eq!((a.placements[2].offset_x, a.placements[2].offset_y, a.placements[2].frame.w), (20, 3, 1));
        let rects: Vec<Rect> = a.placements.iter().filter(|p| p.same_as.is_none()).map(|p| p.frame).collect();
        for (i, r) in rects.iter().enumerate() {
            assert!(r.x + r.w <= a.image.width && r.y + r.h <= a.image.height);
            for s in &rects[i + 1..] {
                let overlap = r.x < s.x + s.w && r.x + r.w > s.x && r.y < s.y + s.h && r.y + r.h > s.y;
                assert!(!overlap, "{r:?} overlaps {s:?}");
            }
        }
        // Pixel content preserved.
        let p = &a.placements[2];
        assert_eq!(a.image.get(p.frame.x, p.frame.y).r, 99);
        let pot = pack(&sprites, true, 0, 0, true).unwrap();
        assert!(pot.image.width.is_power_of_two() && pot.image.height.is_power_of_two());
    }
}
