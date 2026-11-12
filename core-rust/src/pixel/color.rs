/// Straight (non-premultiplied) 8-bit RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }

    pub fn to_u32(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }

    pub fn from_u32(v: u32) -> Self {
        let [r, g, b, a] = v.to_be_bytes();
        Rgba { r, g, b, a }
    }

    /// Parses `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA` (the `#` is optional) and `transparent`.
    pub fn parse_hex(text: &str) -> Option<Rgba> {
        let t = text.trim();
        if t.eq_ignore_ascii_case("transparent") {
            return Some(Rgba::TRANSPARENT);
        }
        let h = t.strip_prefix('#').unwrap_or(t);
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let nib = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        match h.len() {
            3 => Some(Rgba::new(nib(0)?, nib(1)?, nib(2)?, 255)),
            4 => Some(Rgba::new(nib(0)?, nib(1)?, nib(2)?, nib(3)?)),
            6 => Some(Rgba::new(byte(0)?, byte(2)?, byte(4)?, 255)),
            8 => Some(Rgba::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// `#RRGGBB` when opaque, `#RRGGBBAA` otherwise.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(Rgba::parse_hex("#D94B42"), Some(Rgba::new(0xD9, 0x4B, 0x42, 255)));
        assert_eq!(Rgba::parse_hex("fff"), Some(Rgba::new(255, 255, 255, 255)));
        assert_eq!(Rgba::parse_hex("#11223380").unwrap().to_hex(), "#11223380");
        assert_eq!(Rgba::parse_hex("transparent"), Some(Rgba::TRANSPARENT));
        assert_eq!(Rgba::parse_hex("#12345"), None);
        assert_eq!(Rgba::parse_hex("#GG0000"), None);
        assert_eq!(Rgba::parse_hex("#ééé"), None);
        let c = Rgba::new(1, 2, 3, 4);
        assert_eq!(Rgba::from_u32(c.to_u32()), c);
    }
}
