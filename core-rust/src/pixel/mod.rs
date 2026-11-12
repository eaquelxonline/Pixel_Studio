//! Pixel data model: colors, palette and the 16×16 sparse tile grid.

mod color;
mod grid;
mod palette;

pub use color::Rgba;
pub use grid::{PixelGrid, TILE_AREA, TILE_SIZE, Tile};
pub use palette::{MAX_PALETTE, Palette, TRANSPARENT};
