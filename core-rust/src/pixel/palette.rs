use super::Rgba;

/// Pixel value of "no pixel". Pixels are true-color `0xRRGGBBAA`; any alpha-0 color is stored as 0.
pub const TRANSPARENT: u32 = 0;
/// Swatch list limit (a UI convenience, not a color limit: pixels can use any of 2^32 RGBA values).
pub const MAX_PALETTE: usize = 4096;

/// The project's color swatches. Pixels do not depend on it: the canvas is true color.
/// Index 0 is reserved for "transparent" so swatch indices stay stable for agents.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    colors: Vec<Rgba>,
}

impl Default for Palette {
    fn default() -> Self {
        Palette { colors: vec![Rgba::TRANSPARENT] }
    }
}

impl Palette {
    pub fn from_colors(mut colors: Vec<Rgba>) -> Result<Palette, String> {
        if colors.is_empty() {
            colors.push(Rgba::TRANSPARENT);
        }
        if colors.len() > MAX_PALETTE {
            return Err(format!("palette exceeds {MAX_PALETTE} swatches"));
        }
        colors[0] = Rgba::TRANSPARENT;
        Ok(Palette { colors })
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.len() <= 1
    }

    pub fn colors(&self) -> &[Rgba] {
        &self.colors
    }

    pub fn get(&self, index: usize) -> Option<Rgba> {
        self.colors.get(index).copied()
    }

    pub fn find(&self, color: Rgba) -> Option<usize> {
        if color.a == 0 {
            return Some(0);
        }
        self.colors.iter().skip(1).position(|c| *c == color).map(|i| i + 1)
    }

    /// Returns the swatch index of `color`, appending it when new.
    pub fn intern(&mut self, color: Rgba) -> Result<usize, String> {
        if let Some(i) = self.find(color) {
            return Ok(i);
        }
        if self.colors.len() >= MAX_PALETTE {
            return Err("swatch list is full".into());
        }
        self.colors.push(color);
        Ok(self.colors.len() - 1)
    }

    /// Replaces a swatch and returns the previous color.
    pub fn set(&mut self, index: usize, color: Rgba) -> Result<Rgba, String> {
        if index == 0 {
            return Err("swatch 0 is reserved for transparency".into());
        }
        match self.colors.get_mut(index) {
            Some(slot) => Ok(std::mem::replace(slot, color)),
            None => Err(format!("swatch {index} does not exist")),
        }
    }
}
