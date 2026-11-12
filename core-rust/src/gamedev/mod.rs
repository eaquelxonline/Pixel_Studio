//! Game-production features: slices (hitboxes, pivots), texture atlas packing, tilesets and
//! tilemaps with 47-tile blob autotiling, normal-mapped 2D lighting and RotSprite rotation.

pub mod atlas;
pub mod light2d;
pub mod rotsprite;
pub mod tiles;

/// A named rectangle per frame (Aseprite-compatible): hitbox, hurtbox, pivot, attach point…
/// A key applies from its frame until the next key.
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub name: String,
    pub color: u32,
    pub keys: Vec<SliceKey>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliceKey {
    pub frame_id: u32,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub pivot: Option<(i32, i32)>,
}
