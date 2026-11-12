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
    pub blend: Blend,
}

impl Layer {
    pub fn new(id: u32, name: impl Into<String>) -> Layer {
        Layer { id, name: name.into(), visible: true, locked: false, opacity: 255, blend: Blend::Normal }
    }
}

/// Layer blend modes (lighting, shadow and glow layers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Normal,
    Multiply,
    Screen,
    Add,
    Overlay,
    Darken,
    Lighten,
    SoftLight,
}

impl Blend {
    pub const ALL: [(&'static str, Blend); 8] = [
        ("normal", Blend::Normal),
        ("multiply", Blend::Multiply),
        ("screen", Blend::Screen),
        ("add", Blend::Add),
        ("overlay", Blend::Overlay),
        ("darken", Blend::Darken),
        ("lighten", Blend::Lighten),
        ("soft_light", Blend::SoftLight),
    ];

    pub fn parse(s: &str) -> Option<Blend> {
        Self::ALL.iter().find(|(n, _)| *n == s.to_ascii_lowercase().replace(['-', ' '], "_")).map(|(_, b)| *b)
    }

    pub fn as_str(self) -> &'static str {
        Self::ALL.iter().find(|(_, b)| *b == self).map(|(n, _)| *n).unwrap_or("normal")
    }

    pub fn code(self) -> u8 {
        Self::ALL.iter().position(|(_, b)| *b == self).unwrap_or(0) as u8
    }

    pub fn from_code(c: u8) -> Blend {
        Self::ALL.get(c as usize).map(|(_, b)| *b).unwrap_or(Blend::Normal)
    }

    /// Blends one channel (0..255 values, straight).
    pub fn channel(self, d: u32, s: u32) -> u32 {
        match self {
            Blend::Normal => s,
            Blend::Multiply => d * s / 255,
            Blend::Screen => 255 - (255 - d) * (255 - s) / 255,
            Blend::Add => (d + s).min(255),
            Blend::Overlay => {
                if d < 128 {
                    2 * d * s / 255
                } else {
                    255 - 2 * (255 - d) * (255 - s) / 255
                }
            }
            Blend::Darken => d.min(s),
            Blend::Lighten => d.max(s),
            Blend::SoftLight => {
                let (df, sf) = (d as f32 / 255.0, s as f32 / 255.0);
                let r = if sf < 0.5 { df - (1.0 - 2.0 * sf) * df * (1.0 - df) } else { df + (2.0 * sf - 1.0) * (df.sqrt() - df) };
                (r * 255.0).round().clamp(0.0, 255.0) as u32
            }
        }
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
