use super::Rgba;

/// Palette index 0 is permanently reserved for "no pixel" (fully transparent).
pub const TRANSPARENT: u16 = 0;
/// Pixels are stored as u16 palette indices.
pub const MAX_PALETTE: usize = u16::MAX as usize + 1;

/// Indexed palette. Changing an entry recolors every pixel that uses it.
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
            return Err(format!("palette exceeds {MAX_PALETTE} entries"));
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

    pub fn get(&self, index: u16) -> Option<Rgba> {
        self.colors.get(index as usize).copied()
    }

    /// Exact-match lookup. Any color with alpha 0 maps to index 0.
    pub fn find(&self, color: Rgba) -> Option<u16> {
        if color.a == 0 {
            return Some(TRANSPARENT);
        }
        self.colors.iter().skip(1).position(|c| *c == color).map(|i| (i + 1) as u16)
    }

    /// Returns the index of `color`, appending it when it is new.
    pub fn intern(&mut self, color: Rgba) -> Result<u16, String> {
        if let Some(i) = self.find(color) {
            return Ok(i);
        }
        if self.colors.len() >= MAX_PALETTE {
            return Err("palette is full".into());
        }
        self.colors.push(color);
        Ok((self.colors.len() - 1) as u16)
    }

    pub fn set(&mut self, index: u16, color: Rgba) -> Result<(), String> {
        if index == TRANSPARENT {
            return Err("palette index 0 is reserved for transparency".into());
        }
        match self.colors.get_mut(index as usize) {
            Some(slot) => {
                *slot = color;
                Ok(())
            }
            None => Err(format!("palette index {index} does not exist")),
        }
    }
}
