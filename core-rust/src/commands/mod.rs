//! The command protocol: the single entry point used by the UI, MCP, REST and tests.
//!
//! Request (UTF-8 JSON):
//! ```json
//! {"op":"set_pixel","actor":"claude-agent","operationId":"op_1042","expectedRevision":125,
//!  "args":{"x":12,"y":18,"color":"#D94B42","frame":4}}
//! ```
//! Arguments may also be given flat next to `op`. Response:
//! `{"ok":true,"op":"set_pixel","revision":126,"result":{...}}` or
//! `{"ok":false,"op":"set_pixel","revision":125,"error":{"code":"conflict","message":"..."}}`.

pub mod json;

mod args;
mod draw;
pub mod pixel2d;
mod query;
mod structure;
pub mod voxel;

use json::Json;

use crate::project::Document;

/// Machine-readable failure.
#[derive(Debug, Clone, PartialEq)]
pub struct CmdError {
    pub code: &'static str,
    pub message: String,
}

impl CmdError {
    pub fn new(code: &'static str, message: impl Into<String>) -> CmdError {
        CmdError { code, message: message.into() }
    }
    pub fn invalid(message: impl Into<String>) -> CmdError {
        CmdError::new("invalid_argument", message)
    }
    pub fn not_found(message: impl Into<String>) -> CmdError {
        CmdError::new("not_found", message)
    }
}

impl From<String> for CmdError {
    fn from(message: String) -> Self {
        CmdError::invalid(message)
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

/// Result of a successful mutation.
pub struct Outcome {
    pub result: Json,
    pub changed: u64,
}

impl Outcome {
    pub fn new(result: Json, changed: u64) -> Outcome {
        Outcome { result, changed }
    }
}

/// Result of a read-only query (optionally with binary data such as PNG bytes).
pub struct Query {
    pub result: Json,
    pub binary: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Read,
    Mutate,
    /// Mutations with engine-level semantics (history, transactions, whole-document swaps).
    Special,
}

/// Every operation the engine understands, with its kind. Kept in one place so the
/// gateway's tool catalogue can be checked against it.
pub const OPERATIONS: &[(&str, Kind)] = &[
    // queries
    ("status", Kind::Read),
    ("canvas_inspect", Kind::Read),
    ("project_inspect", Kind::Read),
    ("get_pixel", Kind::Read),
    ("read_region", Kind::Read),
    ("frame_get", Kind::Read),
    ("palette_get", Kind::Read),
    ("animation_info", Kind::Read),
    ("history_list", Kind::Read),
    ("render_frame", Kind::Read),
    ("canvas_snapshot", Kind::Read),
    ("export_png", Kind::Read),
    ("export_gif", Kind::Read),
    ("export_spritesheet", Kind::Read),
    ("project_save", Kind::Read),
    ("palette_list", Kind::Read),
    ("export_normal_map", Kind::Read),
    ("voxel_inspect", Kind::Read),
    ("voxel_get", Kind::Read),
    ("voxel_render", Kind::Read),
    ("voxel_render_sheet", Kind::Read),
    ("voxel_pick", Kind::Read),
    // drawing
    ("set_pixel", Kind::Mutate),
    ("set_pixels", Kind::Mutate),
    ("erase_pixel", Kind::Mutate),
    ("draw_line", Kind::Mutate),
    ("draw_rectangle", Kind::Mutate),
    ("draw_ellipse", Kind::Mutate),
    ("fill_region", Kind::Mutate),
    ("replace_color", Kind::Mutate),
    ("flip_horizontal", Kind::Mutate),
    ("flip_vertical", Kind::Mutate),
    ("rotate", Kind::Mutate),
    ("move_region", Kind::Mutate),
    ("copy_region", Kind::Mutate),
    ("clear", Kind::Mutate),
    // structure
    ("layer_create", Kind::Mutate),
    ("layer_delete", Kind::Mutate),
    ("layer_rename", Kind::Mutate),
    ("layer_visibility", Kind::Mutate),
    ("layer_lock", Kind::Mutate),
    ("layer_opacity", Kind::Mutate),
    ("layer_move", Kind::Mutate),
    ("layer_select", Kind::Mutate),
    ("frame_add", Kind::Mutate),
    ("frame_duplicate", Kind::Mutate),
    ("frame_delete", Kind::Mutate),
    ("frame_move", Kind::Mutate),
    ("frame_set_duration", Kind::Mutate),
    ("frame_select", Kind::Mutate),
    ("animation_tag_create", Kind::Mutate),
    ("animation_tag_delete", Kind::Mutate),
    ("animation_set_loop", Kind::Mutate),
    ("palette_set", Kind::Mutate),
    ("palette_add", Kind::Mutate),
    ("canvas_resize", Kind::Mutate),
    ("project_rename", Kind::Mutate),
    ("image_import", Kind::Mutate),
    ("draw_ascii", Kind::Mutate),
    ("outline", Kind::Mutate),
    ("color_ramp", Kind::Mutate),
    ("palette_load", Kind::Mutate),
    ("quantize", Kind::Mutate),
    ("dither_fill", Kind::Mutate),
    // 3D pixel art
    ("voxel_model_create", Kind::Mutate),
    ("voxel_model_delete", Kind::Mutate),
    ("voxel_model_clone", Kind::Mutate),
    ("voxel_model_set", Kind::Mutate),
    ("voxel_set", Kind::Mutate),
    ("voxel_set_many", Kind::Mutate),
    ("voxel_shape", Kind::Mutate),
    ("voxel_mirror", Kind::Mutate),
    ("material_set", Kind::Mutate),
    ("texture_import", Kind::Mutate),
    ("texture_generate", Kind::Mutate),
    ("rig_bone_add", Kind::Mutate),
    ("rig_clear", Kind::Mutate),
    ("rig_bind", Kind::Mutate),
    ("anim3d_create", Kind::Mutate),
    ("anim3d_key", Kind::Mutate),
    ("anim3d_generate", Kind::Mutate),
    ("voxel_render_to_frames", Kind::Mutate),
    // engine-level
    ("project_new", Kind::Special),
    ("project_load", Kind::Special),
    ("history_undo", Kind::Special),
    ("history_redo", Kind::Special),
    ("transaction", Kind::Special),
];

pub fn kind_of(op: &str) -> Option<Kind> {
    OPERATIONS.iter().find(|(name, _)| *name == op).map(|(_, k)| *k)
}

/// Applies one plain mutation to a document.
pub fn apply_mutation(doc: &mut Document, op: &str, a: &Json, payload: &[u8], cache: &mut voxel::RenderCache) -> CmdResult<Outcome> {
    match op {
        "image_import" => voxel::image_import(doc, a, payload),
        "draw_ascii" => pixel2d::draw_ascii(doc, a),
        "outline" => pixel2d::outline(doc, a),
        "color_ramp" => pixel2d::color_ramp(doc, a),
        "palette_load" => pixel2d::palette_load(doc, a),
        "quantize" => pixel2d::quantize(doc, a),
        "dither_fill" => pixel2d::dither_fill(doc, a),
        "voxel_model_create" => voxel::model_create(doc, a),
        "voxel_model_delete" => voxel::model_delete(doc, a),
        "voxel_model_clone" => voxel::model_clone(doc, a),
        "voxel_model_set" => voxel::model_set(doc, a),
        "voxel_set" => voxel::voxel_set(doc, a),
        "voxel_set_many" => voxel::voxel_set_many(doc, a),
        "voxel_shape" => voxel::voxel_shape(doc, a),
        "voxel_mirror" => voxel::voxel_mirror(doc, a),
        "material_set" => voxel::material_set(doc, a),
        "texture_import" => voxel::texture_import(doc, a, payload),
        "texture_generate" => voxel::texture_generate(doc, a),
        "rig_bone_add" => voxel::bone_add(doc, a),
        "rig_clear" => voxel::rig_clear(doc, a),
        "rig_bind" => voxel::rig_bind(doc, a),
        "anim3d_create" => voxel::anim_create(doc, a),
        "anim3d_key" => voxel::anim_key(doc, a),
        "anim3d_generate" => voxel::anim_generate(doc, a),
        "voxel_render_to_frames" => voxel::voxel_render_to_frames(doc, cache, a),
        "set_pixel" => draw::set_pixel(doc, a),
        "set_pixels" => draw::set_pixels(doc, a),
        "erase_pixel" => draw::erase_pixel(doc, a),
        "draw_line" => draw::draw_line(doc, a),
        "draw_rectangle" => draw::draw_rectangle(doc, a),
        "draw_ellipse" => draw::draw_ellipse(doc, a),
        "fill_region" => draw::fill_region(doc, a),
        "replace_color" => draw::replace_color(doc, a),
        "flip_horizontal" => draw::flip(doc, a, true),
        "flip_vertical" => draw::flip(doc, a, false),
        "rotate" => draw::rotate(doc, a),
        "move_region" => draw::move_region(doc, a),
        "copy_region" => draw::copy_region(doc, a),
        "clear" => draw::clear(doc, a),
        "layer_create" => structure::layer_create(doc, a),
        "layer_delete" => structure::layer_delete(doc, a),
        "layer_rename" => structure::layer_rename(doc, a),
        "layer_visibility" => structure::layer_visibility(doc, a),
        "layer_lock" => structure::layer_lock(doc, a),
        "layer_opacity" => structure::layer_opacity(doc, a),
        "layer_move" => structure::layer_move(doc, a),
        "layer_select" => structure::layer_select(doc, a),
        "frame_add" => structure::frame_add(doc, a),
        "frame_duplicate" => structure::frame_duplicate(doc, a),
        "frame_delete" => structure::frame_delete(doc, a),
        "frame_move" => structure::frame_move(doc, a),
        "frame_set_duration" => structure::frame_set_duration(doc, a),
        "frame_select" => structure::frame_select(doc, a),
        "animation_tag_create" => structure::tag_create(doc, a),
        "animation_tag_delete" => structure::tag_delete(doc, a),
        "animation_set_loop" => structure::set_loop(doc, a),
        "palette_set" => structure::palette_set(doc, a),
        "palette_add" => structure::palette_add(doc, a),
        "canvas_resize" => structure::canvas_resize(doc, a),
        "project_rename" => structure::project_rename(doc, a),
        _ => Err(CmdError::new("unknown_op", format!("unknown operation '{op}'"))),
    }
}

pub use query::run_query;
