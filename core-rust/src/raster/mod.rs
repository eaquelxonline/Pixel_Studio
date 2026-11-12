//! Deterministic raster algorithms operating on palette indices.
//!
//! Every function returns the number of pixels whose value actually changed.

use crate::pixel::{PixelGrid, TRANSPARENT};

/// Axis-aligned rectangle in canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl Region {
    pub fn full(grid: &PixelGrid) -> Region {
        Region { x: 0, y: 0, w: grid.width() as i64, h: grid.height() as i64 }
    }

    /// Intersects with the grid; `None` when nothing remains.
    pub fn clip(self, grid: &PixelGrid) -> Option<Region> {
        let x0 = self.x.max(0);
        let y0 = self.y.max(0);
        let x1 = (self.x + self.w).min(grid.width() as i64);
        let y1 = (self.y + self.h).min(grid.height() as i64);
        (x1 > x0 && y1 > y0).then_some(Region { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }
}

/// How copied pixels are combined with the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteMode {
    /// Transparent source pixels leave the destination untouched.
    Over,
    /// Destination is overwritten exactly, including transparent pixels.
    Replace,
}

/// Bresenham line, inclusive of both end points.
pub fn line(grid: &mut PixelGrid, x0: i64, y0: i64, x1: i64, y1: i64, value: u32) -> usize {
    let mut changed = 0;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    // Guard against absurd coordinates: never step more than the canvas diagonal plus the span.
    let max_steps = dx.max(-dy) + 1;
    for _ in 0..max_steps {
        if grid.set(x, y, value) {
            changed += 1;
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    changed
}

pub fn rectangle(grid: &mut PixelGrid, r: Region, value: u32, filled: bool) -> usize {
    if r.w <= 0 || r.h <= 0 {
        return 0;
    }
    let mut changed = 0;
    if filled {
        let Some(c) = r.clip(grid) else { return 0 };
        for y in c.y..c.y + c.h {
            for x in c.x..c.x + c.w {
                changed += grid.set(x, y, value) as usize;
            }
        }
    } else {
        let (x1, y1) = (r.x + r.w - 1, r.y + r.h - 1);
        changed += line(grid, r.x, r.y, x1, r.y, value);
        changed += line(grid, r.x, y1, x1, y1, value);
        if r.h > 2 {
            changed += line(grid, r.x, r.y + 1, r.x, y1 - 1, value);
            changed += line(grid, x1, r.y + 1, x1, y1 - 1, value);
        }
    }
    changed
}

/// Ellipse inscribed in the bounding box, symmetric and gap-free.
pub fn ellipse(grid: &mut PixelGrid, r: Region, value: u32, filled: bool) -> usize {
    if r.w <= 0 || r.h <= 0 {
        return 0;
    }
    let cx = r.x as f64 + r.w as f64 / 2.0;
    let cy = r.y as f64 + r.h as f64 / 2.0;
    let rx = r.w as f64 / 2.0;
    let ry = r.h as f64 / 2.0;
    // Span of each row: pixels whose centers lie inside the ellipse.
    let span = |y: i64| -> Option<(i64, i64)> {
        let dy = (y as f64 + 0.5 - cy) / ry;
        let t = 1.0 - dy * dy;
        if t < 0.0 {
            return None;
        }
        let half = rx * t.sqrt();
        let x0 = (cx - half - 0.5).ceil() as i64;
        let x1 = (cx + half - 0.5).floor() as i64;
        let (x0, x1) = (x0.max(r.x), x1.min(r.x + r.w - 1));
        (x1 >= x0).then_some((x0, x1))
    };
    let rows: Vec<Option<(i64, i64)>> = (r.y..r.y + r.h).map(span).collect();
    let mut changed = 0;
    for (i, row) in rows.iter().enumerate() {
        let y = r.y + i as i64;
        let Some((x0, x1)) = *row else { continue };
        if filled {
            for x in x0..=x1 {
                changed += grid.set(x, y, value) as usize;
            }
            continue;
        }
        // Outline: a pixel is on the edge if a 4-neighbour lies outside the shape.
        let inside = |xx: i64, yy: i64| -> bool {
            if yy < r.y || yy >= r.y + r.h {
                return false;
            }
            matches!(rows[(yy - r.y) as usize], Some((a, b)) if xx >= a && xx <= b)
        };
        for x in x0..=x1 {
            if !inside(x - 1, y) || !inside(x + 1, y) || !inside(x, y - 1) || !inside(x, y + 1) {
                changed += grid.set(x, y, value) as usize;
            }
        }
    }
    changed
}

/// Fill starting at (x, y). `contiguous = false` replaces every pixel of the seed's value.
pub fn fill(grid: &mut PixelGrid, x: i64, y: i64, value: u32, contiguous: bool) -> usize {
    if !grid.in_bounds(x, y) {
        return 0;
    }
    let target = grid.get(x, y);
    if target == value {
        return 0;
    }
    if !contiguous {
        return replace(grid, Region::full(grid), target, value);
    }
    let (w, h) = (grid.width() as i64, grid.height() as i64);
    let mut changed = 0;
    let mut stack = vec![(x, y)];
    while let Some((px, py)) = stack.pop() {
        // Scanline fill: expand left/right, then queue the rows above and below.
        if grid.get(px, py) != target {
            continue;
        }
        let mut left = px;
        while left > 0 && grid.get(left - 1, py) == target {
            left -= 1;
        }
        let mut right = px;
        while right + 1 < w && grid.get(right + 1, py) == target {
            right += 1;
        }
        for xx in left..=right {
            changed += grid.set(xx, py, value) as usize;
        }
        for ny in [py - 1, py + 1] {
            if ny < 0 || ny >= h {
                continue;
            }
            let mut xx = left;
            while xx <= right {
                if grid.get(xx, ny) == target {
                    stack.push((xx, ny));
                    while xx <= right && grid.get(xx, ny) == target {
                        xx += 1;
                    }
                } else {
                    xx += 1;
                }
            }
        }
    }
    changed
}

pub fn replace(grid: &mut PixelGrid, region: Region, from: u32, to: u32) -> usize {
    if from == to {
        return 0;
    }
    let Some(c) = region.clip(grid) else { return 0 };
    let mut changed = 0;
    for y in c.y..c.y + c.h {
        for x in c.x..c.x + c.w {
            if grid.get(x, y) == from {
                changed += grid.set(x, y, to) as usize;
            }
        }
    }
    changed
}

/// Copies the pixels of a region row-major. Out-of-bounds pixels read as transparent.
pub fn extract(grid: &PixelGrid, r: Region) -> Vec<u32> {
    let mut out = Vec::with_capacity((r.w.max(0) * r.h.max(0)) as usize);
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            out.push(grid.get(x, y));
        }
    }
    out
}

pub fn paste(grid: &mut PixelGrid, x: i64, y: i64, w: i64, h: i64, data: &[u32], mode: PasteMode) -> usize {
    let mut changed = 0;
    for row in 0..h {
        for col in 0..w {
            let v = data[(row * w + col) as usize];
            if v == TRANSPARENT && mode == PasteMode::Over {
                continue;
            }
            changed += grid.set(x + col, y + row, v) as usize;
        }
    }
    changed
}

pub fn clear_region(grid: &mut PixelGrid, r: Region) -> usize {
    rectangle(grid, r, TRANSPARENT, true)
}

pub fn flip_horizontal(grid: &mut PixelGrid, r: Region) -> usize {
    let Some(c) = r.clip(grid) else { return 0 };
    let data = extract(grid, c);
    let mut flipped = vec![TRANSPARENT; data.len()];
    for row in 0..c.h {
        for col in 0..c.w {
            flipped[(row * c.w + col) as usize] = data[(row * c.w + (c.w - 1 - col)) as usize];
        }
    }
    paste(grid, c.x, c.y, c.w, c.h, &flipped, PasteMode::Replace)
}

pub fn flip_vertical(grid: &mut PixelGrid, r: Region) -> usize {
    let Some(c) = r.clip(grid) else { return 0 };
    let data = extract(grid, c);
    let mut flipped = vec![TRANSPARENT; data.len()];
    for row in 0..c.h {
        for col in 0..c.w {
            flipped[(row * c.w + col) as usize] = data[((c.h - 1 - row) * c.w + col) as usize];
        }
    }
    paste(grid, c.x, c.y, c.w, c.h, &flipped, PasteMode::Replace)
}

/// Rotates a region clockwise by 90, 180 or 270 degrees. 90/270 require a square region.
pub fn rotate(grid: &mut PixelGrid, r: Region, degrees: i64) -> Result<usize, String> {
    let Some(c) = r.clip(grid) else { return Ok(0) };
    let deg = degrees.rem_euclid(360);
    if deg == 0 {
        return Ok(0);
    }
    if deg % 90 != 0 {
        return Err("rotation must be a multiple of 90 degrees".into());
    }
    if deg != 180 && c.w != c.h {
        return Err(format!("90/270 degree rotation needs a square region (got {}x{})", c.w, c.h));
    }
    let n = c.w;
    let data = extract(grid, c);
    let mut out = vec![TRANSPARENT; data.len()];
    for row in 0..c.h {
        for col in 0..c.w {
            let src = data[(row * c.w + col) as usize];
            let (nr, nc) = match deg {
                90 => (col, n - 1 - row),
                180 => (c.h - 1 - row, c.w - 1 - col),
                _ => (n - 1 - col, row),
            };
            out[(nr * c.w + nc) as usize] = src;
        }
    }
    Ok(paste(grid, c.x, c.y, c.w, c.h, &out, PasteMode::Replace))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(g: &PixelGrid) -> String {
        let mut s = String::new();
        for y in 0..g.height() as i64 {
            for x in 0..g.width() as i64 {
                s.push(if g.get(x, y) == 0 { '.' } else { char::from_digit(g.get(x, y), 10).unwrap() });
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn line_is_inclusive_and_symmetric() {
        let mut a = PixelGrid::new(8, 8);
        let mut b = PixelGrid::new(8, 8);
        assert_eq!(line(&mut a, 0, 0, 7, 3, 1), 8);
        line(&mut b, 7, 3, 0, 0, 1);
        assert_eq!(a.get(0, 0), 1);
        assert_eq!(a.get(7, 3), 1);
        assert_eq!(b.get(0, 0), 1);
    }

    #[test]
    fn rect_outline_and_fill() {
        let mut g = PixelGrid::new(5, 4);
        assert_eq!(rectangle(&mut g, Region { x: 0, y: 0, w: 5, h: 4 }, 1, false), 14);
        assert_eq!(dump(&g), "11111\n1...1\n1...1\n11111\n");
        assert_eq!(fill(&mut g, 2, 2, 2, true), 6);
        assert_eq!(dump(&g), "11111\n12221\n12221\n11111\n");
    }

    #[test]
    fn flood_fill_respects_walls() {
        let mut g = PixelGrid::new(6, 3);
        line(&mut g, 2, 0, 2, 2, 1);
        assert_eq!(fill(&mut g, 0, 0, 3, true), 6);
        assert_eq!(g.get(4, 1), 0);
        assert_eq!(fill(&mut g, 4, 1, 3, false), 9);
    }

    #[test]
    fn ellipse_is_symmetric() {
        let mut g = PixelGrid::new(9, 7);
        ellipse(&mut g, Region { x: 0, y: 0, w: 9, h: 7 }, 1, false);
        let s = dump(&g);
        let rows: Vec<&str> = s.lines().collect();
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(*row, rows[rows.len() - 1 - i]);
            assert_eq!(row.chars().rev().collect::<String>(), *row);
        }
        assert_eq!(g.get(4, 3), 0, "outline must be hollow");
    }

    #[test]
    fn flips_and_rotations() {
        let mut g = PixelGrid::new(3, 3);
        g.set(0, 0, 1);
        g.set(1, 0, 2);
        let full = Region::full(&g);
        flip_horizontal(&mut g, full);
        assert_eq!(dump(&g), ".21\n...\n...\n");
        flip_vertical(&mut g, full);
        assert_eq!(dump(&g), "...\n...\n.21\n");
        rotate(&mut g, full, 90).unwrap();
        assert_eq!(dump(&g), "...\n2..\n1..\n");
        rotate(&mut g, full, 270).unwrap();
        assert_eq!(dump(&g), "...\n...\n.21\n");
        let mut wide = PixelGrid::new(4, 2);
        assert!(rotate(&mut wide, Region::full(&PixelGrid::new(4, 2)), 90).is_err());
        assert!(rotate(&mut wide, Region::full(&PixelGrid::new(4, 2)), 180).is_ok());
    }
}
