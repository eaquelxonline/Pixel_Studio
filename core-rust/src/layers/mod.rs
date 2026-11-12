//! Layer metadata. Pixel content lives in cels (see `project::Document`).

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Stable identifier; survives reordering.
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// 0 = invisible, 255 = fully opaque.
    pub opacity: u8,
}

impl Layer {
    pub fn new(id: u32, name: impl Into<String>) -> Layer {
        Layer { id, name: name.into(), visible: true, locked: false, opacity: 255 }
    }
}

pub const MAX_LAYER_NAME: usize = 128;

pub fn validate_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("layer name must not be empty".into());
    }
    if n.chars().count() > MAX_LAYER_NAME {
        return Err(format!("layer name longer than {MAX_LAYER_NAME} characters"));
    }
    if n.chars().any(|c| c.is_control()) {
        return Err("layer name contains control characters".into());
    }
    Ok(n.to_string())
}
