use std::sync::Arc;

use super::TRANSPARENT;

pub const TILE_SIZE: u32 = 16;
pub const TILE_AREA: usize = (TILE_SIZE * TILE_SIZE) as usize;

/// One 16×16 block of true-color pixels (`Rgba::to_pixel`, 0 = transparent).
pub type Tile = [u32; TILE_AREA];

/// Sparse pixel storage for one cel (one layer in one frame).
///
/// Tiles are reference counted and copy-on-write: cloning a grid (for undo snapshots or
/// frame duplication) only copies pointers, and unchanged tiles stay shared between frames.
/// A `None` tile is fully transparent and costs no pixel memory.
#[derive(Debug, Clone, PartialEq)]
pub struct PixelGrid {
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles_y: u32,
    tiles: Vec<Option<Arc<Tile>>>,
}

impl PixelGrid {
    pub fn new(width: u32, height: u32) -> PixelGrid {
        let tiles_x = width.div_ceil(TILE_SIZE);
        let tiles_y = height.div_ceil(TILE_SIZE);
        PixelGrid { width, height, tiles_x, tiles_y, tiles: vec![None; (tiles_x * tiles_y) as usize] }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn tiles_x(&self) -> u32 {
        self.tiles_x
    }

    pub fn tiles_y(&self) -> u32 {
        self.tiles_y
    }

    pub fn in_bounds(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < self.width as i64 && y < self.height as i64
    }

    #[inline]
    fn locate(&self, x: u32, y: u32) -> (usize, usize) {
        let t = ((y / TILE_SIZE) * self.tiles_x + (x / TILE_SIZE)) as usize;
        let i = ((y % TILE_SIZE) * TILE_SIZE + (x % TILE_SIZE)) as usize;
        (t, i)
    }

    /// Out-of-bounds reads return transparent.
    #[inline]
    pub fn get(&self, x: i64, y: i64) -> u32 {
        if !self.in_bounds(x, y) {
            return TRANSPARENT;
        }
        let (t, i) = self.locate(x as u32, y as u32);
        match &self.tiles[t] {
            Some(tile) => tile[i],
            None => TRANSPARENT,
        }
    }

    /// Writes a pixel; out-of-bounds writes are clipped. Returns true when the value changed.
    #[inline]
    pub fn set(&mut self, x: i64, y: i64, value: u32) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        let (t, i) = self.locate(x as u32, y as u32);
        match &mut self.tiles[t] {
            None if value == TRANSPARENT => false,
            None => {
                let mut tile = [TRANSPARENT; TILE_AREA];
                tile[i] = value;
                self.tiles[t] = Some(Arc::new(tile));
                true
            }
            Some(tile) => {
                if tile[i] == value {
                    return false;
                }
                Arc::make_mut(tile)[i] = value;
                true
            }
        }
    }

    pub fn clear(&mut self) {
        self.tiles.iter_mut().for_each(|t| *t = None);
    }

    /// Drops tiles that became fully transparent.
    pub fn compact(&mut self) {
        for t in self.tiles.iter_mut() {
            if t.as_ref().is_some_and(|tile| tile.iter().all(|&v| v == TRANSPARENT)) {
                *t = None;
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.iter().all(|t| t.as_ref().is_none_or(|tile| tile.iter().all(|&v| v == TRANSPARENT)))
    }

    pub fn tile(&self, tx: u32, ty: u32) -> Option<&Arc<Tile>> {
        self.tiles.get((ty * self.tiles_x + tx) as usize).and_then(|t| t.as_ref())
    }

    pub fn tiles(&self) -> &[Option<Arc<Tile>>] {
        &self.tiles
    }

    pub fn set_tile(&mut self, tx: u32, ty: u32, tile: Option<Arc<Tile>>) {
        let idx = (ty * self.tiles_x + tx) as usize;
        if idx < self.tiles.len() {
            self.tiles[idx] = tile;
        }
    }

    /// Number of allocated tiles (memory diagnostics).
    pub fn allocated_tiles(&self) -> usize {
        self.tiles.iter().filter(|t| t.is_some()).count()
    }

    /// Calls `f(x, y, index)` for every non-transparent pixel, tile by tile.
    pub fn for_each_opaque(&self, mut f: impl FnMut(u32, u32, u32)) {
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                let Some(tile) = self.tile(tx, ty) else { continue };
                for (i, &v) in tile.iter().enumerate() {
                    if v == TRANSPARENT {
                        continue;
                    }
                    let x = tx * TILE_SIZE + (i as u32 % TILE_SIZE);
                    let y = ty * TILE_SIZE + (i as u32 / TILE_SIZE);
                    if x < self.width && y < self.height {
                        f(x, y, v);
                    }
                }
            }
        }
    }

    /// Bounding box `(x, y, w, h)` of non-transparent pixels.
    pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        let mut any = false;
        self.for_each_opaque(|x, y, _| {
            any = true;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        });
        any.then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    }

    /// Returns a copy of this grid resized to `width × height`, keeping pixels at an offset.
    pub fn resized(&self, width: u32, height: u32, offset_x: i64, offset_y: i64) -> PixelGrid {
        let mut out = PixelGrid::new(width, height);
        self.for_each_opaque(|x, y, v| {
            out.set(x as i64 + offset_x, y as i64 + offset_y, v);
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_set_get() {
        let mut g = PixelGrid::new(40, 20);
        assert_eq!(g.tiles().len(), 3 * 2);
        assert!(g.set(39, 19, 7));
        assert!(!g.set(39, 19, 7));
        assert_eq!(g.get(39, 19), 7);
        assert_eq!(g.get(40, 19), 0);
        assert!(!g.set(-1, 0, 3));
        assert_eq!(g.allocated_tiles(), 1);
        assert_eq!(g.bounds(), Some((39, 19, 1, 1)));
        g.set(39, 19, 0);
        g.compact();
        assert_eq!(g.allocated_tiles(), 0);
        assert!(g.is_empty());
    }

    #[test]
    fn copy_on_write_sharing() {
        let mut a = PixelGrid::new(32, 32);
        a.set(1, 1, 5);
        a.set(20, 20, 6);
        let mut b = a.clone();
        b.set(1, 1, 9);
        assert_eq!(a.get(1, 1), 5);
        assert_eq!(b.get(1, 1), 9);
        // The untouched tile is still physically shared.
        assert!(Arc::ptr_eq(a.tile(1, 1).unwrap(), b.tile(1, 1).unwrap()));
        assert!(!Arc::ptr_eq(a.tile(0, 0).unwrap(), b.tile(0, 0).unwrap()));
    }
}
