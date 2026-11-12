//! 3D pixel art: sparse voxel models with true color, PBR materials and textures, skeletal
//! rigs, keyframe animation and a deterministic software renderer that turns models into
//! pixel-perfect sprites (any angle, any frame, every G-buffer pass).

pub mod material;
pub mod math;
pub mod model;
pub mod render;
pub mod rig;

pub use material::{Material, Texture};
pub use math::{Affine, Vec3};
pub use model::{Shape, Voxel, VoxelModel};
