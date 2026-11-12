//! # pixcore — the Pixel Studio core engine
//!
//! Platform-independent, deterministic and dependency-free (Rust `std` only).
//! Android and Windows hosts load it through JNI (`ffi`); every pixel change from a human
//! or an AI agent runs through the same [`engine::Engine::execute`].

pub mod animation;
pub mod commands;
pub mod engine;
pub mod export;
pub mod ffi;
pub mod history;
pub mod layers;
pub mod pixel;
pub mod project;
pub mod raster;
pub mod renderer;
pub mod storage;
pub mod voxel;

pub use engine::{Engine, Response};
