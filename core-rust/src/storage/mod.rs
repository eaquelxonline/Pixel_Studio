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
use crate::export::deflate::{crc32, zlib_compress};
use crate::export::inflate::zlib_decompress;
use crate::history::LogEntry;
use crate::layers::Layer;
use crate::pixel::{Palette, PixelGrid, Rgba, TILE_AREA, TILE_SIZE, Tile};
use crate::project::{Document, MAX_FRAMES, MAX_LAYERS, validate_size};
use crate::voxel::model::{CHUNK, CHUNK_VOLUME, MAX_BONES, MAX_MODEL_SIZE, Voxel, VoxelModel};
use crate::voxel::rig::{Animation, Bone, Key};
use crate::voxel::{Material, Texture, Vec3};

pub const MAGIC: &[u8; 4] = b"PXS\0";
pub const VERSION: u16 = 2;

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
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    fn blob(&mut self, data: &[u8]) {
        let z = zlib_compress(data);
        self.u32(z.len() as u32);
        self.0.extend_from_slice(&z);
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
    let mut runs: Vec<(u16, u32)> = Vec::new();
    for &v in tile.iter() {
        match runs.last_mut() {
            Some((n, last)) if *last == v => *n += 1,
            _ => runs.push((1, v)),
        }
    }
    w.u16(runs.len() as u16);
    for (n, v) in runs {
        w.u16(n);
        w.u32(v);
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

    save_voxels(doc, &mut out);

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
    fn f32(&mut self) -> Result<f32, String> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        if v.is_finite() { Ok(v) } else { Err("invalid number".into()) }
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn vec3(&mut self) -> Result<Vec3, String> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn blob(&mut self, limit: usize) -> Result<Vec<u8>, String> {
        let n = self.u32()? as usize;
        zlib_decompress(self.take(n)?, limit)
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
    if version != VERSION {
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
    let n = tb.count(8)?;
    let mut tiles: Vec<Arc<Tile>> = Vec::with_capacity(n);
    for _ in 0..n {
        let runs = tb.u16()? as usize;
        let mut tile = [0u32; TILE_AREA];
        let mut i = 0;
        for _ in 0..runs {
            let (len, v) = (tb.u16()? as usize, tb.u32()?);
            if i + len > TILE_AREA {
                return Err("tile RLE overflow".into());
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
        models: Vec::new(),
        materials: vec![Material::new("default")],
        textures: Vec::new(),
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

    load_voxels(&mut doc, &chunks)?;

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

const VOXEL_BYTES: usize = 6;

fn save_voxels(doc: &Document, out: &mut Vec<u8>) {
    let mut m = W(Vec::new());
    m.u32(doc.materials.len() as u32);
    for mat in &doc.materials {
        m.str(&mat.name);
        for v in [mat.roughness, mat.metallic, mat.emission, mat.uv_scale, mat.normal_strength] {
            m.f32(v);
        }
        for slot in mat.maps {
            m.i32(slot.map(|i| i as i32).unwrap_or(-1));
        }
    }
    chunk(out, b"MATL", m);

    let mut t = W(Vec::new());
    t.u32(doc.textures.len() as u32);
    for tex in &doc.textures {
        t.str(&tex.name);
        t.u32(tex.width);
        t.u32(tex.height);
        t.blob(&tex.data);
    }
    chunk(out, b"TXTR", t);

    let mut v = W(Vec::new());
    v.u32(doc.models.len() as u32);
    for model in &doc.models {
        v.str(&model.name);
        for d in model.size {
            v.u32(d);
        }
        v.vec3(model.origin);
        v.u32(model.bones.len() as u32);
        for b in &model.bones {
            v.str(&b.name);
            v.i32(b.parent.map(|p| p as i32).unwrap_or(-1));
            v.vec3(b.head);
            v.vec3(b.tail);
        }
        v.u32(model.animations.len() as u32);
        for a in &model.animations {
            v.str(&a.name);
            v.u32(a.frames);
            v.u32(a.frame_ms);
            v.u8(a.looped as u8);
            v.u32(a.keys.len() as u32);
            for k in &a.keys {
                v.u32(k.bone as u32);
                v.f32(k.frame);
                v.vec3(k.rot);
                v.vec3(k.pos);
            }
        }
        let mut keys: Vec<_> = model.chunks.keys().copied().collect();
        keys.sort();
        let keys: Vec<_> = keys.into_iter().filter(|k| model.chunks[k].iter().any(|x| !x.is_empty())).collect();
        v.u32(keys.len() as u32);
        for key in keys {
            v.i32(key.0);
            v.i32(key.1);
            v.i32(key.2);
            let mut raw = Vec::with_capacity(CHUNK_VOLUME * VOXEL_BYTES);
            for vox in model.chunks[&key].iter() {
                raw.extend_from_slice(&vox.rgba.to_le_bytes());
                raw.push(vox.material);
                raw.push(vox.bone);
            }
            v.blob(&raw);
        }
    }
    chunk(out, b"VOXM", v);
}

fn load_voxels(doc: &mut Document, chunks: &HashMap<[u8; 4], &[u8]>) -> Result<(), String> {
    if let Some(data) = chunks.get(b"TXTR") {
        let mut r = R { d: data, pos: 0 };
        let n = r.count(16)?;
        for _ in 0..n {
            let name = r.str()?;
            let (w, h) = (r.u32()?, r.u32()?);
            if w == 0 || h == 0 || w > 4096 || h > 4096 {
                return Err("invalid texture size".into());
            }
            let data = r.blob((w * h * 4) as usize)?;
            if data.len() != (w * h * 4) as usize {
                return Err("texture data has the wrong size".into());
            }
            doc.textures.push(Arc::new(Texture { name, width: w, height: h, data }));
        }
    }
    if let Some(data) = chunks.get(b"MATL") {
        let mut r = R { d: data, pos: 0 };
        let n = r.count(52)?;
        if n == 0 || n > 256 {
            return Err("invalid material count".into());
        }
        doc.materials.clear();
        for _ in 0..n {
            let mut m = Material::new(&r.str()?);
            m.roughness = r.f32()?.clamp(0.0, 1.0);
            m.metallic = r.f32()?.clamp(0.0, 1.0);
            m.emission = r.f32()?.clamp(0.0, 100.0);
            m.uv_scale = r.f32()?.clamp(0.01, 64.0);
            m.normal_strength = r.f32()?.clamp(0.0, 4.0);
            for slot in m.maps.iter_mut() {
                let i = r.i32()?;
                *slot = if i < 0 {
                    None
                } else {
                    Some(i as usize).filter(|&i| i < doc.textures.len()).ok_or("material references a missing texture")?.into()
                };
            }
            doc.materials.push(m);
        }
    }
    if let Some(data) = chunks.get(b"VOXM") {
        let mut r = R { d: data, pos: 0 };
        let n = r.count(32)?;
        for _ in 0..n {
            let name = r.str()?;
            let size = [r.u32()?, r.u32()?, r.u32()?];
            if size.iter().any(|&d| d == 0 || d > MAX_MODEL_SIZE) {
                return Err("invalid model size".into());
            }
            let mut model = VoxelModel::new(&name, size);
            model.origin = r.vec3()?;
            let nb = r.count(32)?;
            if nb > MAX_BONES {
                return Err("too many bones".into());
            }
            for i in 0..nb {
                let name = r.str()?;
                let parent = r.i32()?;
                let parent = if parent < 0 {
                    None
                } else if (parent as usize) < i {
                    Some(parent as usize)
                } else {
                    return Err("bone parent must precede the bone".into());
                };
                model.bones.push(Bone { name, parent, head: r.vec3()?, tail: r.vec3()? });
            }
            let na = r.count(17)?;
            for _ in 0..na {
                let mut a = Animation::new(&r.str()?, 1, 100);
                a.frames = r.u32()?.clamp(1, 100_000);
                a.frame_ms = r.u32()?.clamp(1, 60_000);
                a.looped = r.u8()? != 0;
                let nk = r.count(32)?;
                for _ in 0..nk {
                    let bone = r.u32()? as usize;
                    if bone >= nb {
                        return Err("animation key references a missing bone".into());
                    }
                    a.keys.push(Key { bone, frame: r.f32()?, rot: r.vec3()?, pos: r.vec3()? });
                }
                model.animations.push(a);
            }
            let nc = r.count(16)?;
            let max_chunks = size.iter().map(|&d| (d as i32 + CHUNK - 1) / CHUNK).product::<i32>();
            if nc as i32 > max_chunks {
                return Err("too many voxel chunks".into());
            }
            for _ in 0..nc {
                let key = (r.i32()?, r.i32()?, r.i32()?);
                let raw = r.blob(CHUNK_VOLUME * VOXEL_BYTES)?;
                if raw.len() != CHUNK_VOLUME * VOXEL_BYTES {
                    return Err("voxel chunk has the wrong size".into());
                }
                let mut c = [Voxel::EMPTY; CHUNK_VOLUME];
                for (i, v) in raw.as_chunks::<VOXEL_BYTES>().0.iter().enumerate() {
                    let vox = Voxel { rgba: u32::from_le_bytes([v[0], v[1], v[2], v[3]]), material: v[4], bone: v[5] };
                    if (vox.bone as usize) >= nb.max(1) || (vox.material as usize) >= doc.materials.len() {
                        return Err("voxel references a missing bone or material".into());
                    }
                    c[i] = if vox.is_empty() { Voxel::EMPTY } else { vox };
                }
                model.chunks.insert(key, Arc::new(c));
            }
            doc.models.push(model);
        }
    }
    Ok(())
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
        let red = Rgba::new(217, 75, 66, 255).to_pixel();
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
