//! `.pxs` — Pixel Studio's binary project format.
//!
//! ```text
//! "PXS\0" | version u16 | reserved u16
//! chunk*  = tag[4] | length u32 | payload
//!   META  name, size, active layer/frame, id counters
//!   PALT  palette (RGBA)
//!   LAYR  layers bottom→top
//!   FRAM  frames in order with durations
//!   TAGS  animation tags
//!   TILE  de-duplicated 16×16 tiles, RLE-encoded
//!   CELS  (layer id, frame id) → tile references
//!   HIST  operation log (who changed what)
//! "END\0" | 4 | CRC-32 of every byte before this chunk
//! ```
//! All integers are little-endian. Unknown chunks are skipped for forward compatibility.
//! Identical tiles are stored once, so duplicated/held animation frames cost almost nothing.

use std::collections::HashMap;
use std::sync::Arc;

use crate::animation::{Direction, Frame, Tag, validate_duration};
use crate::export::deflate::crc32;
use crate::history::LogEntry;
use crate::layers::Layer;
use crate::pixel::{Palette, PixelGrid, Rgba, TILE_AREA, TILE_SIZE, Tile};
use crate::project::{Document, MAX_FRAMES, MAX_LAYERS, validate_size};

pub const MAGIC: &[u8; 4] = b"PXS\0";
pub const VERSION: u16 = 1;

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
}

fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: W) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(body.0.len() as u32).to_le_bytes());
    out.extend_from_slice(&body.0);
}

fn rle_tile(tile: &Tile, w: &mut W) {
    let mut runs: Vec<(u16, u16)> = Vec::new();
    for &v in tile.iter() {
        match runs.last_mut() {
            Some((n, last)) if *last == v => *n += 1,
            _ => runs.push((1, v)),
        }
    }
    w.u16(runs.len() as u16);
    for (n, v) in runs {
        w.u16(n);
        w.u16(v);
    }
}

pub fn save(doc: &Document, log: &[LogEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    let mut m = W(Vec::new());
    m.str(&doc.name);
    m.u32(doc.width);
    m.u32(doc.height);
    m.u32(doc.active_layer);
    m.u32(doc.active_frame);
    m.u32(doc.next_layer_id);
    m.u32(doc.next_frame_id);
    chunk(&mut out, b"META", m);

    let mut p = W(Vec::new());
    p.u32(doc.palette.len() as u32);
    doc.palette.colors().iter().for_each(|c| p.u32(c.to_u32()));
    chunk(&mut out, b"PALT", p);

    let mut l = W(Vec::new());
    l.u32(doc.layers.len() as u32);
    for layer in &doc.layers {
        l.u32(layer.id);
        l.str(&layer.name);
        l.u8(layer.visible as u8);
        l.u8(layer.locked as u8);
        l.u8(layer.opacity);
    }
    chunk(&mut out, b"LAYR", l);

    let mut f = W(Vec::new());
    f.u32(doc.frames.len() as u32);
    for frame in &doc.frames {
        f.u32(frame.id);
        f.u32(frame.duration_ms);
    }
    chunk(&mut out, b"FRAM", f);

    let mut t = W(Vec::new());
    t.u32(doc.tags.len() as u32);
    for tag in &doc.tags {
        t.str(&tag.name);
        t.u32(tag.from_id);
        t.u32(tag.to_id);
        t.u8(tag.direction.code());
        t.u32(tag.repeat);
    }
    chunk(&mut out, b"TAGS", t);

    // De-duplicate tiles by content. Deterministic order: layer order, then frame order.
    let mut tile_ids: HashMap<&Tile, u32> = HashMap::new();
    let mut tiles: Vec<&Tile> = Vec::new();
    let mut cel_body = W(Vec::new());
    let mut cel_count = 0u32;
    let mut cel_entries = W(Vec::new());
    for layer in &doc.layers {
        for frame in &doc.frames {
            let Some(grid) = doc.cel(layer.id, frame.id) else { continue };
            if grid.is_empty() {
                continue;
            }
            cel_count += 1;
            cel_entries.u32(layer.id);
            cel_entries.u32(frame.id);
            for slot in grid.tiles() {
                let reference = match slot {
                    Some(tile) if tile.iter().any(|&v| v != 0) => {
                        let key: &Tile = tile.as_ref();
                        *tile_ids.entry(key).or_insert_with(|| {
                            tiles.push(key);
                            tiles.len() as u32
                        })
                    }
                    _ => 0,
                };
                cel_entries.u32(reference);
            }
        }
    }
    let mut tb = W(Vec::new());
    tb.u32(tiles.len() as u32);
    tiles.iter().for_each(|t| rle_tile(t, &mut tb));
    chunk(&mut out, b"TILE", tb);
    cel_body.u32(cel_count);
    cel_body.0.extend_from_slice(&cel_entries.0);
    chunk(&mut out, b"CELS", cel_body);

    let mut h = W(Vec::new());
    h.u32(log.len() as u32);
    for e in log {
        h.u64(e.revision);
        h.str(&e.actor);
        h.str(&e.op);
        h.str(e.operation_id.as_deref().unwrap_or(""));
        h.u64(e.changed_pixels);
        h.u64(e.timestamp_ms);
    }
    chunk(&mut out, b"HIST", h);

    let crc = crc32(&out);
    out.extend_from_slice(b"END\0");
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

struct R<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.d.len()).ok_or("unexpected end of data")?;
        let s = &self.d[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn str(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "invalid UTF-8 string".into())
    }
    /// Count prefix that must be plausible given the bytes left (each item ≥ `min_item` bytes).
    fn count(&mut self, min_item: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min_item) > self.d.len() - self.pos {
            return Err("corrupt count".into());
        }
        Ok(n)
    }
}

pub struct Loaded {
    pub doc: Document,
    pub log: Vec<LogEntry>,
}

pub fn load(data: &[u8]) -> Result<Loaded, String> {
    if data.len() < 8 + 12 || &data[..4] != MAGIC {
        return Err("not a Pixel Studio (.pxs) file".into());
    }
    let version = u16::from_le_bytes([data[4], data[5]]);
    if version == 0 || version > VERSION {
        return Err(format!("unsupported .pxs version {version}"));
    }
    let end = data.len() - 12;
    if &data[end..end + 4] != b"END\0" || u32::from_le_bytes(data[end + 4..end + 8].try_into().unwrap()) != 4 {
        return Err("missing END chunk (file truncated?)".into());
    }
    let stored = u32::from_le_bytes(data[end + 8..end + 12].try_into().unwrap());
    if crc32(&data[..end]) != stored {
        return Err("checksum mismatch: file is corrupted".into());
    }

    let mut chunks: HashMap<[u8; 4], &[u8]> = HashMap::new();
    let mut r = R { d: &data[..end], pos: 8 };
    while r.pos < end {
        let tag: [u8; 4] = r.take(4)?.try_into().unwrap();
        let len = r.u32()? as usize;
        chunks.insert(tag, r.take(len)?);
    }
    let get = |t: &[u8; 4]| chunks.get(t).copied().ok_or_else(|| format!("missing {} chunk", String::from_utf8_lossy(t)));

    let mut m = R { d: get(b"META")?, pos: 0 };
    let name = m.str()?;
    let (width, height) = validate_size(m.u32()? as i64, m.u32()? as i64)?;
    let (active_layer, active_frame, next_layer_id, next_frame_id) = (m.u32()?, m.u32()?, m.u32()?, m.u32()?);

    let mut p = R { d: get(b"PALT")?, pos: 0 };
    let n = p.count(4)?;
    let colors = (0..n).map(|_| p.u32().map(Rgba::from_u32)).collect::<Result<Vec<_>, _>>()?;
    let palette = Palette::from_colors(colors)?;

    let mut l = R { d: get(b"LAYR")?, pos: 0 };
    let n = l.count(11)?;
    if n == 0 || n > MAX_LAYERS {
        return Err("invalid layer count".into());
    }
    let mut layers = Vec::with_capacity(n);
    for _ in 0..n {
        let id = l.u32()?;
        let name = l.str()?;
        let visible = l.u8()? != 0;
        let locked = l.u8()? != 0;
        let opacity = l.u8()?;
        if id == 0 || id >= next_layer_id || layers.iter().any(|x: &Layer| x.id == id) {
            return Err("invalid layer id".into());
        }
        layers.push(Layer { id, name, visible, locked, opacity });
    }

    let mut f = R { d: get(b"FRAM")?, pos: 0 };
    let n = f.count(8)?;
    if n == 0 || n > MAX_FRAMES {
        return Err("invalid frame count".into());
    }
    let mut frames = Vec::with_capacity(n);
    for _ in 0..n {
        let id = f.u32()?;
        let duration_ms = validate_duration(f.u32()? as i64)?;
        if id == 0 || id >= next_frame_id || frames.iter().any(|x: &Frame| x.id == id) {
            return Err("invalid frame id".into());
        }
        frames.push(Frame { id, duration_ms });
    }

    let mut t = R { d: get(b"TAGS")?, pos: 0 };
    let n = t.count(17)?;
    let mut tags = Vec::with_capacity(n);
    for _ in 0..n {
        let name = t.str()?;
        let (from_id, to_id) = (t.u32()?, t.u32()?);
        let direction = Direction::from_code(t.u8()?).ok_or("invalid tag direction")?;
        let repeat = t.u32()?;
        if !frames.iter().any(|x| x.id == from_id) || !frames.iter().any(|x| x.id == to_id) {
            return Err("tag references unknown frame".into());
        }
        tags.push(Tag { name, from_id, to_id, direction, repeat });
    }

    let mut tb = R { d: get(b"TILE")?, pos: 0 };
    let n = tb.count(6)?;
    let mut tiles: Vec<Arc<Tile>> = Vec::with_capacity(n);
    for _ in 0..n {
        let runs = tb.u16()? as usize;
        let mut tile = [0u16; TILE_AREA];
        let mut i = 0;
        for _ in 0..runs {
            let (len, v) = (tb.u16()? as usize, tb.u16()?);
            if i + len > TILE_AREA {
                return Err("tile RLE overflow".into());
            }
            if v as usize >= palette.len() {
                return Err("pixel references a missing palette entry".into());
            }
            tile[i..i + len].fill(v);
            i += len;
        }
        if i != TILE_AREA {
            return Err("tile RLE underflow".into());
        }
        tiles.push(Arc::new(tile));
    }

    let mut doc = Document {
        name,
        width,
        height,
        palette,
        layers,
        frames,
        cels: HashMap::new(),
        tags,
        active_layer,
        active_frame,
        next_layer_id,
        next_frame_id,
    };
    let tiles_x = width.div_ceil(TILE_SIZE);
    let tiles_y = height.div_ceil(TILE_SIZE);
    let mut c = R { d: get(b"CELS")?, pos: 0 };
    let n = c.count(8)?;
    for _ in 0..n {
        let (layer_id, frame_id) = (c.u32()?, c.u32()?);
        if doc.layer_index(layer_id).is_none() || doc.frame_index(frame_id).is_none() {
            return Err("cel references unknown layer/frame".into());
        }
        let mut grid = PixelGrid::new(width, height);
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let reference = c.u32()? as usize;
                if reference > tiles.len() {
                    return Err("cel references missing tile".into());
                }
                if reference > 0 {
                    grid.set_tile(tx, ty, Some(Arc::clone(&tiles[reference - 1])));
                }
            }
        }
        doc.cels.insert((layer_id, frame_id), Arc::new(grid));
    }
    if doc.layer_index(doc.active_layer).is_none() {
        doc.active_layer = doc.layers[0].id;
    }
    if doc.frame_index(doc.active_frame).is_none() {
        doc.active_frame = doc.frames[0].id;
    }

    let mut log = Vec::new();
    if let Some(hist) = chunks.get(b"HIST") {
        let mut h = R { d: hist, pos: 0 };
        let n = h.count(36)?;
        for _ in 0..n {
            let revision = h.u64()?;
            let actor = h.str()?;
            let op = h.str()?;
            let opid = h.str()?;
            let changed_pixels = h.u64()?;
            let timestamp_ms = h.u64()?;
            log.push(LogEntry { revision, actor, op, operation_id: (!opid.is_empty()).then_some(opid), changed_pixels, timestamp_ms });
        }
    }
    Ok(Loaded { doc, log })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{RenderOptions, render};

    fn sample() -> Document {
        let mut d = Document::new(40, 24);
        let l0 = d.active_layer;
        let l1 = d.insert_layer("Weapon", 1);
        d.layers[1].opacity = 200;
        let f0 = d.active_frame;
        let red = d.palette.intern(Rgba::new(217, 75, 66, 255)).unwrap();
        for i in 0..30 {
            d.cel_mut(l0, f0).set(i, i % 24, red);
        }
        d.cel_mut(l1, f0).set(39, 23, 3);
        let f1 = d.duplicate_frame(0, 1);
        d.cel_mut(l1, f1).set(0, 0, 5);
        d.frames[1].duration_ms = 80;
        d.tags.push(Tag { name: "Idle".into(), from_id: f0, to_id: f1, direction: Direction::PingPong, repeat: 0 });
        d.name = "Knight".into();
        d
    }

    #[test]
    fn roundtrip_preserves_everything() {
        let d = sample();
        let log = vec![LogEntry {
            revision: 7,
            actor: "claude".into(),
            op: "set_pixel".into(),
            operation_id: Some("op_1".into()),
            changed_pixels: 1,
            timestamp_ms: 42,
        }];
        let bytes = save(&d, &log);
        let loaded = load(&bytes).unwrap();
        let e = &loaded.doc;
        assert_eq!(e.name, "Knight");
        assert_eq!((e.width, e.height), (40, 24));
        assert_eq!(e.layers, d.layers);
        assert_eq!(e.frames, d.frames);
        assert_eq!(e.tags, d.tags);
        assert_eq!(e.palette, d.palette);
        assert_eq!(loaded.log, log);
        for i in 0..d.frames.len() {
            assert_eq!(render(e, i, &RenderOptions::default()), render(&d, i, &RenderOptions::default()));
        }
        // Saving the loaded document is byte-identical (deterministic format).
        assert_eq!(save(e, &loaded.log), bytes);
    }

    #[test]
    fn rejects_corruption() {
        let bytes = save(&sample(), &[]);
        for i in [0usize, 9, bytes.len() / 2, bytes.len() - 1] {
            let mut bad = bytes.clone();
            bad[i] ^= 0x55;
            assert!(load(&bad).is_err(), "flip at {i} not detected");
        }
        assert!(load(&bytes[..bytes.len() - 3]).is_err());
        assert!(load(b"hello").is_err());
    }

    #[test]
    fn duplicate_frames_are_nearly_free() {
        let mut d = sample();
        let one = save(&d, &[]).len();
        for _ in 0..50 {
            d.duplicate_frame(0, 1);
        }
        let many = save(&d, &[]).len();
        // Each extra frame only stores tile references, never pixel data again.
        assert!(many - one <= 50 * 72, "grew by {}", many - one);
    }
}
