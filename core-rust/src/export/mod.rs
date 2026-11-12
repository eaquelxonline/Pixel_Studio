//! Export engine: PNG, animated GIF and sprite sheets (+ JSON metadata for game engines).

pub mod deflate;
pub mod gif;
pub mod png;

use crate::commands::json::Json;
use crate::renderer::Image;

pub struct SheetFrame<'a> {
    pub index: usize,
    pub image: &'a Image,
    pub duration_ms: u32,
}

pub struct SpriteSheet {
    pub image: Image,
    /// Engine-neutral metadata (frame rectangles, durations) consumable by Godot/Unity importers.
    pub metadata: Json,
}

/// Packs equally sized frames into a grid. `columns = 0` picks a near-square layout.
pub fn sprite_sheet(frames: &[SheetFrame], columns: usize, padding: u32) -> Result<SpriteSheet, String> {
    let Some(first) = frames.first() else { return Err("no frames to pack".into()) };
    let (fw, fh) = (first.image.width, first.image.height);
    let n = frames.len();
    let cols = if columns == 0 { (n as f64).sqrt().ceil() as usize } else { columns.min(n) }.max(1);
    let rows = n.div_ceil(cols);
    let w = cols as u64 * (fw + padding) as u64 - padding as u64;
    let h = rows as u64 * (fh + padding) as u64 - padding as u64;
    if w > 32_768 || h > 32_768 {
        return Err(format!("sprite sheet would be {w}x{h}; use more/less columns or a smaller scale"));
    }
    let mut sheet = Image::new(w as u32, h as u32);
    let mut rects = Vec::with_capacity(n);
    for (i, f) in frames.iter().enumerate() {
        let x = (i % cols) as u32 * (fw + padding);
        let y = (i / cols) as u32 * (fh + padding);
        sheet.blit(f.image, x, y);
        rects
            .push(Json::obj().with("frame", f.index).with("x", x).with("y", y).with("w", fw).with("h", fh).with("duration", f.duration_ms));
    }
    let metadata = Json::obj()
        .with("image", Json::obj().with("width", w).with("height", h))
        .with("frameSize", Json::obj().with("w", fw).with("h", fh))
        .with("columns", cols)
        .with("rows", rows)
        .with("padding", padding)
        .with("frames", Json::Arr(rects));
    Ok(SpriteSheet { image: sheet, metadata })
}
