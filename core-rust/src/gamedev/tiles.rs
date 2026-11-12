//! Tilesets and tilemaps with 47-tile "blob" autotiling (the layout Godot terrains, Tiled
//! Wang sets, LDtk and GameMaker expect), plus a generator that synthesizes all 47 pixel-art
//! terrain tiles from one seamless fill tile.

use crate::pixel::Rgba;
use crate::renderer::Image;

// Neighbour bits: N, NE, E, SE, S, SW, W, NW.
pub const N: u8 = 1;
pub const NE: u8 = 2;
pub const E: u8 = 4;
pub const SE: u8 = 8;
pub const S: u8 = 16;
pub const SW: u8 = 32;
pub const W: u8 = 64;
pub const NW: u8 = 128;

/// Corners only count when both adjacent edges are present → 47 distinct masks.
pub fn reduce(mask: u8) -> u8 {
    let mut m = mask & (N | E | S | W);
    if mask & NE != 0 && mask & N != 0 && mask & E != 0 {
        m |= NE;
    }
    if mask & SE != 0 && mask & S != 0 && mask & E != 0 {
        m |= SE;
    }
    if mask & SW != 0 && mask & S != 0 && mask & W != 0 {
        m |= SW;
    }
    if mask & NW != 0 && mask & N != 0 && mask & W != 0 {
        m |= NW;
    }
    m
}

/// The 47 reduced masks in ascending order (tile i of a blob tileset = BLOB_MASKS[i]).
pub fn blob_masks() -> Vec<u8> {
    let mut v: Vec<u8> = (0..=255u8).map(reduce).collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[derive(Debug, Clone, PartialEq)]
pub enum TilesetKind {
    Plain,
    /// Tile i is the autotile for `blob_masks()[i]`.
    Blob,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tileset {
    pub name: String,
    pub tile_w: u32,
    pub tile_h: u32,
    pub kind: TilesetKind,
    /// Tiles as true-color pixels (`Rgba::to_pixel`, 0 = transparent).
    pub tiles: Vec<Vec<u32>>,
}

impl Tileset {
    /// Atlas image with `columns` tiles per row.
    pub fn atlas(&self, columns: u32) -> Image {
        let n = self.tiles.len().max(1) as u32;
        let cols = columns.clamp(1, n);
        let rows = n.div_ceil(cols);
        let mut img = Image::new(cols * self.tile_w, rows * self.tile_h);
        for (i, t) in self.tiles.iter().enumerate() {
            let (ox, oy) = ((i as u32 % cols) * self.tile_w, (i as u32 / cols) * self.tile_h);
            for y in 0..self.tile_h {
                for x in 0..self.tile_w {
                    let c = Rgba::from_u32(t[(y * self.tile_w + x) as usize]);
                    let o = (((oy + y) * img.width + ox + x) * 4) as usize;
                    img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
                }
            }
        }
        img
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tilemap {
    pub name: String,
    pub tileset: usize,
    pub cols: u32,
    pub rows: u32,
    /// 0 = empty, otherwise tile index + 1.
    pub cells: Vec<u32>,
    /// Terrain occupancy used by blob autotiling.
    pub terrain: Vec<bool>,
}

impl Tilemap {
    pub fn new(name: &str, tileset: usize, cols: u32, rows: u32) -> Tilemap {
        let n = (cols * rows) as usize;
        Tilemap { name: name.into(), tileset, cols, rows, cells: vec![0; n], terrain: vec![false; n] }
    }

    pub fn in_bounds(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < self.cols as i64 && y < self.rows as i64
    }

    fn terrain_at(&self, x: i64, y: i64) -> bool {
        self.in_bounds(x, y) && self.terrain[(y as u32 * self.cols + x as u32) as usize]
    }

    /// Recomputes blob tiles from the terrain map (edges of the map count as terrain so
    /// fills reach the border cleanly).
    pub fn autotile(&mut self) {
        let masks = blob_masks();
        for y in 0..self.rows as i64 {
            for x in 0..self.cols as i64 {
                let i = (y as u32 * self.cols + x as u32) as usize;
                if !self.terrain[i] {
                    self.cells[i] = 0;
                    continue;
                }
                let t = |dx: i64, dy: i64| {
                    let (nx, ny) = (x + dx, y + dy);
                    !self.in_bounds(nx, ny) || self.terrain_at(nx, ny)
                };
                let mut m = 0u8;
                for (bit, dx, dy) in [(N, 0, -1), (NE, 1, -1), (E, 1, 0), (SE, 1, 1), (S, 0, 1), (SW, -1, 1), (W, -1, 0), (NW, -1, -1)] {
                    if t(dx, dy) {
                        m |= bit;
                    }
                }
                let r = reduce(m);
                self.cells[i] = masks.iter().position(|&k| k == r).unwrap() as u32 + 1;
            }
        }
    }

    /// Renders the map into an image.
    pub fn render(&self, ts: &Tileset) -> Image {
        let mut img = Image::new(self.cols * ts.tile_w, self.rows * ts.tile_h);
        for y in 0..self.rows {
            for x in 0..self.cols {
                let c = self.cells[(y * self.cols + x) as usize];
                let Some(tile) = c.checked_sub(1).and_then(|t| ts.tiles.get(t as usize)) else { continue };
                for ty in 0..ts.tile_h {
                    for tx in 0..ts.tile_w {
                        let v = tile[(ty * ts.tile_w + tx) as usize];
                        if v == 0 {
                            continue;
                        }
                        let col = Rgba::from_u32(v);
                        let o = (((y * ts.tile_h + ty) * img.width + x * ts.tile_w + tx) * 4) as usize;
                        crate::renderer::blend_over(&mut img.pixels[o..o + 4], col, 255);
                    }
                }
            }
        }
        img
    }
}

/// Parameters for synthesizing a blob terrain tileset.
pub struct BlobStyle {
    /// Transparent margin (px) on sides that border empty space.
    pub inset: u32,
    /// Width of the decorated edge band inside the terrain.
    pub edge: u32,
    pub light: Rgba,
    pub dark: Rgba,
    pub outline: Rgba,
    /// Extra "cliff lip" rows below the terrain on open south sides (depth illusion).
    pub lip: u32,
    pub lip_color: Rgba,
    pub rounded: bool,
}

fn shade(c: Rgba, k: f32) -> Rgba {
    let f = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
    // Darken with a slight shift toward blue, like hue-shifted pixel-art shadows.
    Rgba::new(f(c.r), f(c.g), (c.b as f32 * (k + 0.08).min(1.0)).round() as u8, c.a)
}

/// Builds the 47 blob tiles from one seamless fill tile (w×h true-color pixels).
pub fn synthesize_blob(fill: &[u32], w: u32, h: u32, style: &BlobStyle) -> Vec<Vec<u32>> {
    let masks = blob_masks();
    masks
        .iter()
        .map(|&m| {
            let open = |bit: u8| m & bit == 0;
            let inset = style.inset as i32;
            let lip = style.lip as i32;
            let (wi, hi) = (w as i32, h as i32);
            // Signed distance (px) of a pixel to the terrain boundary; negative = outside.
            let inside_dist = |x: i32, y: i32| -> i32 {
                let mut d = i32::MAX;
                if open(N) {
                    d = d.min(y - inset);
                }
                if open(S) {
                    d = d.min(hi - 1 - inset - lip - y);
                }
                if open(W) {
                    d = d.min(x - inset);
                }
                if open(E) {
                    d = d.min(wi - 1 - inset - x);
                }
                // Inner corners: neighbour edges present but the diagonal is missing.
                let notch = |cx: bool, cy: bool| {
                    let dx = if cx { wi - 1 - x } else { x };
                    let dy = if cy { hi - 1 - y } else { y };
                    let ext = inset + if cy { lip } else { 0 };
                    if dx < inset && dy < ext { (dx - inset).max(dy - ext) } else { i32::MAX }
                };
                if !open(N) && !open(E) && open(NE) {
                    d = d.min(notch(true, false));
                }
                if !open(S) && !open(E) && open(SE) {
                    d = d.min(notch(true, true));
                }
                if !open(S) && !open(W) && open(SW) {
                    d = d.min(notch(false, true));
                }
                if !open(N) && !open(W) && open(NW) {
                    d = d.min(notch(false, false));
                }
                // Rounded outer corners.
                if style.rounded {
                    for (cx, cy, a, b) in [(false, false, N, W), (true, false, N, E), (false, true, S, W), (true, true, S, E)] {
                        if open(a) && open(b) {
                            let px = if cx { wi - 1 - inset - x } else { x - inset };
                            let py = if cy { hi - 1 - inset - lip - y } else { y - inset };
                            if px < 2 && py < 2 && px + py < 2 {
                                d = d.min(px + py - 2);
                            }
                        }
                    }
                }
                d
            };
            let mut tile = vec![0u32; (w * h) as usize];
            for y in 0..hi {
                for x in 0..wi {
                    let d = inside_dist(x, y);
                    let i = (y * wi + x) as usize;
                    if d < 0 {
                        // Cliff lip under an open south edge.
                        if open(S) && lip > 0 {
                            let below = y - (hi - 1 - inset - lip);
                            let horiz_ok = (!open(W) || x >= inset) && (!open(E) || x <= wi - 1 - inset);
                            if below > 0 && below <= lip && horiz_ok {
                                tile[i] = if below == lip {
                                    style.outline.to_pixel()
                                } else if below == 1 {
                                    // Shadow under the overhanging top surface.
                                    shade(style.lip_color, 0.62).to_pixel()
                                } else if (x * 5 + below * 3 + (x / 3) * 7) % 7 == 0 {
                                    // Vertical strata / cracks on the cliff face.
                                    shade(style.lip_color, 0.8).to_pixel()
                                } else {
                                    style.lip_color.to_pixel()
                                };
                            }
                        }
                        continue;
                    }
                    tile[i] = if d == 0 {
                        style.outline.to_pixel()
                    } else if (d as u32) <= style.edge {
                        // Top edges catch the light, bottom/side edges are shaded.
                        let near_top = open(N) && y - inset <= style.edge as i32;
                        if near_top { style.light.to_pixel() } else { style.dark.to_pixel() }
                    } else {
                        fill[(y as u32 % h * w + x as u32 % w) as usize]
                    };
                }
            }
            tile
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_47_blob_masks() {
        let m = blob_masks();
        assert_eq!(m.len(), 47);
        assert_eq!(m[0], 0);
        assert_eq!(*m.last().unwrap(), 255);
        assert_eq!(reduce(NE), 0, "lonely corner is ignored");
    }

    #[test]
    fn autotile_picks_edges_and_center() {
        let fill = vec![Rgba::new(80, 160, 60, 255).to_pixel(); 64];
        let style = BlobStyle {
            inset: 1,
            edge: 1,
            light: Rgba::new(150, 220, 100, 255),
            dark: Rgba::new(40, 100, 40, 255),
            outline: Rgba::new(20, 40, 30, 255),
            lip: 1,
            lip_color: Rgba::new(100, 60, 40, 255),
            rounded: true,
        };
        let tiles = synthesize_blob(&fill, 8, 8, &style);
        assert_eq!(tiles.len(), 47);
        let ts = Tileset { name: "g".into(), tile_w: 8, tile_h: 8, kind: TilesetKind::Blob, tiles };
        let mut map = Tilemap::new("m", 0, 5, 5);
        for y in 1..4 {
            for x in 1..4 {
                map.terrain[y * 5 + x] = true;
            }
        }
        map.autotile();
        let masks = blob_masks();
        let center = map.cells[2 * 5 + 2] - 1;
        assert_eq!(masks[center as usize], 255);
        let top = map.cells[5 + 2] - 1;
        assert_eq!(masks[top as usize], reduce(E | SE | S | SW | W));
        assert_eq!(map.cells[0], 0);
        let img = map.render(&ts);
        assert_eq!(img.width, 40);
        // Top edge is lit, interior keeps the fill.
        assert_eq!(img.get(20, 10), Rgba::new(150, 220, 100, 255));
        assert_eq!(img.get(20, 20), Rgba::new(80, 160, 60, 255));
        assert_eq!(img.get(20, 8).a, 0, "inset above the top edge stays open");
    }
}
