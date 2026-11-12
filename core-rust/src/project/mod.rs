//! The project document: canvas size, palette, layers, frames, cels and tags.
//!
//! A `Document` is cheap to clone: cels are `Arc<PixelGrid>` whose tiles are themselves
//! shared, so undo snapshots and transactions copy pointers, not pixels.

use std::collections::HashMap;
use std::sync::Arc;

use crate::animation::{DEFAULT_FRAME_MS, Frame, Tag};
use crate::layers::Layer;
use crate::pixel::{Palette, PixelGrid, Rgba};
use crate::voxel::{Material, Texture, VoxelModel};

pub const MAX_CANVAS: u32 = 4096;
pub const MAX_LAYERS: usize = 1024;
/// Safety valve against runaway requests, not a creative limit.
pub const MAX_FRAMES: usize = 100_000;

pub type CelKey = (u32, u32); // (layer id, frame id)

#[derive(Debug, Clone)]
pub struct Document {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub palette: Palette,
    /// Bottom-to-top order.
    pub layers: Vec<Layer>,
    pub frames: Vec<Frame>,
    pub cels: HashMap<CelKey, Arc<PixelGrid>>,
    pub tags: Vec<Tag>,
    pub active_layer: u32,
    pub active_frame: u32,
    pub next_layer_id: u32,
    pub next_frame_id: u32,
    /// 3D pixel (voxel) models living in this project.
    pub models: Vec<VoxelModel>,
    /// PBR materials shared by all models. Index 0 is the default material.
    pub materials: Vec<Material>,
    /// Texture maps (imported PNGs or procedural sets).
    pub textures: Vec<Arc<Texture>>,
}

/// Small neutral starter palette so humans have something to pick from immediately.
pub fn starter_palette() -> Palette {
    let hex = [
        "#000000", "#FFFFFF", "#3A3A46", "#7A7A86", "#BEBEC8", "#5B2C1E", "#A0522D", "#E3A06B", "#F6D7B0", "#C0283C", "#F2545B", "#F5A623",
        "#F7E26B", "#2E8B57", "#7BD389", "#1E3A8A", "#3B82F6", "#7DD3FC", "#6D28D9", "#C084FC",
    ];
    let mut colors = vec![Rgba::TRANSPARENT];
    colors.extend(hex.iter().map(|h| Rgba::parse_hex(h).expect("valid starter color")));
    Palette::from_colors(colors).expect("starter palette fits")
}

pub fn validate_size(width: i64, height: i64) -> Result<(u32, u32), String> {
    if width < 1 || height < 1 || width > MAX_CANVAS as i64 || height > MAX_CANVAS as i64 {
        return Err(format!("canvas size must be between 1x1 and {MAX_CANVAS}x{MAX_CANVAS}"));
    }
    Ok((width as u32, height as u32))
}

impl Document {
    pub fn new(width: u32, height: u32) -> Document {
        let mut doc = Document {
            name: "Untitled".into(),
            width,
            height,
            palette: starter_palette(),
            layers: Vec::new(),
            frames: Vec::new(),
            cels: HashMap::new(),
            tags: Vec::new(),
            active_layer: 0,
            active_frame: 0,
            next_layer_id: 1,
            next_frame_id: 1,
            models: Vec::new(),
            materials: vec![Material::new("default")],
            textures: Vec::new(),
        };
        let l = doc.insert_layer("Layer 1", 0);
        let f = doc.insert_frame(0, DEFAULT_FRAME_MS);
        doc.active_layer = l;
        doc.active_frame = f;
        doc
    }

    pub fn layer_index(&self, id: u32) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn frame_index(&self, id: u32) -> Option<usize> {
        self.frames.iter().position(|f| f.id == id)
    }

    pub fn cel(&self, layer_id: u32, frame_id: u32) -> Option<&PixelGrid> {
        self.cels.get(&(layer_id, frame_id)).map(|c| c.as_ref())
    }

    /// Mutable cel, created on first write. Copy-on-write if the cel is shared.
    pub fn cel_mut(&mut self, layer_id: u32, frame_id: u32) -> &mut PixelGrid {
        let (w, h) = (self.width, self.height);
        let arc = self.cels.entry((layer_id, frame_id)).or_insert_with(|| Arc::new(PixelGrid::new(w, h)));
        Arc::make_mut(arc)
    }

    pub fn insert_layer(&mut self, name: &str, index: usize) -> u32 {
        let id = self.next_layer_id;
        self.next_layer_id += 1;
        let index = index.min(self.layers.len());
        self.layers.insert(index, Layer::new(id, name));
        id
    }

    pub fn remove_layer(&mut self, index: usize) -> Layer {
        let layer = self.layers.remove(index);
        self.cels.retain(|(l, _), _| *l != layer.id);
        if self.active_layer == layer.id {
            let i = index.min(self.layers.len().saturating_sub(1));
            self.active_layer = self.layers.get(i).map(|l| l.id).unwrap_or(0);
        }
        layer
    }

    pub fn insert_frame(&mut self, index: usize, duration_ms: u32) -> u32 {
        let id = self.next_frame_id;
        self.next_frame_id += 1;
        let index = index.min(self.frames.len());
        self.frames.insert(index, Frame { id, duration_ms });
        id
    }

    /// Copies a frame; its cels are shared until one side is edited.
    pub fn duplicate_frame(&mut self, index: usize, to: usize) -> u32 {
        let src = self.frames[index].clone();
        let id = self.insert_frame(to, src.duration_ms);
        let copies: Vec<(CelKey, Arc<PixelGrid>)> =
            self.cels.iter().filter(|((_, f), _)| *f == src.id).map(|((l, _), grid)| ((*l, id), Arc::clone(grid))).collect();
        self.cels.extend(copies);
        id
    }

    pub fn remove_frame(&mut self, index: usize) -> Frame {
        let frame = self.frames.remove(index);
        self.cels.retain(|(_, f), _| *f != frame.id);
        if self.active_frame == frame.id {
            let i = index.min(self.frames.len().saturating_sub(1));
            self.active_frame = self.frames.get(i).map(|f| f.id).unwrap_or(0);
        }
        // Repair tags whose end points vanished; drop tags that became empty.
        let remaining: Vec<u32> = self.frames.iter().map(|f| f.id).collect();
        let neighbour = remaining.get(index.min(remaining.len().saturating_sub(1))).copied();
        let prev = if index > 0 { remaining.get(index - 1).copied() } else { None };
        self.tags.retain_mut(|t| {
            if t.from_id == frame.id && t.to_id == frame.id {
                return false;
            }
            if t.from_id == frame.id {
                match neighbour {
                    Some(n) => t.from_id = n,
                    None => return false,
                }
            }
            if t.to_id == frame.id {
                match prev {
                    Some(p) => t.to_id = p,
                    None => return false,
                }
            }
            true
        });
        frame
    }

    /// Opaque pixel count of a cel, used by inspection tools.
    pub fn cel_stats(&self, layer_id: u32, frame_id: u32) -> (usize, Option<(u32, u32, u32, u32)>) {
        match self.cel(layer_id, frame_id) {
            None => (0, None),
            Some(g) => {
                let mut n = 0;
                g.for_each_opaque(|_, _, _| n += 1);
                (n, g.bounds())
            }
        }
    }

    /// Crops/extends the canvas. Pixels keep their position relative to the top-left
    /// corner shifted by the given offset.
    pub fn resize(&mut self, width: u32, height: u32, offset_x: i64, offset_y: i64) {
        for cel in self.cels.values_mut() {
            *cel = Arc::new(cel.resized(width, height, offset_x, offset_y));
        }
        self.width = width;
        self.height = height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_shares_until_edit() {
        let mut d = Document::new(16, 16);
        let l = d.active_layer;
        let f0 = d.active_frame;
        d.cel_mut(l, f0).set(3, 3, 1);
        let f1 = d.duplicate_frame(0, 1);
        assert!(Arc::ptr_eq(&d.cels[&(l, f0)], &d.cels[&(l, f1)]));
        d.cel_mut(l, f1).set(3, 3, 2);
        assert_eq!(d.cel(l, f0).unwrap().get(3, 3), 1);
        assert_eq!(d.cel(l, f1).unwrap().get(3, 3), 2);
    }

    #[test]
    fn removing_frames_repairs_tags() {
        let mut d = Document::new(8, 8);
        let ids: Vec<u32> = (0..4).map(|i| d.insert_frame(i + 1, 100)).collect();
        d.tags.push(Tag {
            name: "walk".into(),
            from_id: ids[0],
            to_id: ids[2],
            direction: crate::animation::Direction::Forward,
            repeat: 0,
        });
        let idx = d.frame_index(ids[2]).unwrap();
        d.remove_frame(idx);
        assert_eq!(d.tags[0].to_id, ids[1]);
    }
}
