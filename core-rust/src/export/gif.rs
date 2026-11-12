//! Animated GIF89a encoder with an in-house LZW compressor.

use std::collections::HashMap;

use crate::renderer::Image;

pub struct GifFrame<'a> {
    pub image: &'a Image,
    pub duration_ms: u32,
}

/// Alpha below this threshold becomes the GIF transparent index.
const ALPHA_CUTOFF: u8 = 128;

struct Indexed {
    table: Vec<[u8; 3]>, // index 0 is transparent
    pixels: Vec<u8>,
}

fn index_frame(img: &Image) -> Indexed {
    let mut freq: HashMap<[u8; 3], u32> = HashMap::new();
    for p in img.pixels.as_chunks::<4>().0 {
        if p[3] >= ALPHA_CUTOFF {
            *freq.entry([p[0], p[1], p[2]]).or_default() += 1;
        }
    }
    let mut colors: Vec<([u8; 3], u32)> = freq.into_iter().collect();
    // Deterministic order: most frequent first, ties by value.
    colors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    colors.truncate(255);
    let mut table = vec![[0u8; 3]];
    table.extend(colors.iter().map(|c| c.0));
    let mut lookup: HashMap<[u8; 3], u8> = table.iter().enumerate().skip(1).map(|(i, c)| (*c, i as u8)).collect();
    let mut pixels = Vec::with_capacity(img.pixels.len() / 4);
    for p in img.pixels.as_chunks::<4>().0 {
        if p[3] < ALPHA_CUTOFF {
            pixels.push(0);
            continue;
        }
        let key = [p[0], p[1], p[2]];
        let idx = *lookup.entry(key).or_insert_with(|| {
            // More than 255 colors: map to the nearest kept color.
            let mut best = (u32::MAX, 1u8);
            for (i, c) in table.iter().enumerate().skip(1) {
                let d = (0..3).map(|k| (c[k] as i32 - key[k] as i32).pow(2) as u32).sum::<u32>();
                if d < best.0 {
                    best = (d, i as u8);
                }
            }
            best.1
        });
        pixels.push(idx);
    }
    Indexed { table, pixels }
}

struct BitPacker {
    bytes: Vec<u8>,
    acc: u32,
    n: u32,
}

impl BitPacker {
    fn put(&mut self, code: u32, size: u32) {
        self.acc |= code << self.n;
        self.n += size;
        while self.n >= 8 {
            self.bytes.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
}

/// LZW as specified by GIF89a (variable code size, max 12 bits, clear on full table).
pub fn lzw_encode(min_code_size: u32, pixels: &[u8]) -> Vec<u8> {
    let clear = 1u32 << min_code_size;
    let eoi = clear + 1;
    let mut size = min_code_size + 1;
    let mut next = eoi + 1;
    let mut dict: HashMap<u32, u32> = HashMap::new();
    let mut w = BitPacker { bytes: Vec::new(), acc: 0, n: 0 };
    let emit = |w: &mut BitPacker, code: u32, size: &mut u32, next: u32| {
        w.put(code, *size);
        if next >= (1 << *size) && *size < 12 {
            *size += 1;
        }
    };
    emit(&mut w, clear, &mut size, next);
    let Some((&first, rest)) = pixels.split_first() else {
        emit(&mut w, eoi, &mut size, next);
        if w.n > 0 {
            w.bytes.push(w.acc as u8);
        }
        return w.bytes;
    };
    let mut prefix = first as u32;
    for &k in rest {
        let key = (prefix << 8) | k as u32;
        if let Some(&code) = dict.get(&key) {
            prefix = code;
            continue;
        }
        emit(&mut w, prefix, &mut size, next);
        if next >= 4095 {
            emit(&mut w, clear, &mut size, next);
            dict.clear();
            next = eoi + 1;
            size = min_code_size + 1;
        } else {
            dict.insert(key, next);
            next += 1;
        }
        prefix = k as u32;
    }
    emit(&mut w, prefix, &mut size, next);
    emit(&mut w, eoi, &mut size, next);
    if w.n > 0 {
        w.bytes.push(w.acc as u8);
    }
    w.bytes
}

/// `loop_count` 0 = loop forever.
pub fn encode(width: u32, height: u32, frames: &[GifFrame], loop_count: u16) -> Vec<u8> {
    let mut out = b"GIF89a".to_vec();
    out.extend_from_slice(&(width.min(65535) as u16).to_le_bytes());
    out.extend_from_slice(&(height.min(65535) as u16).to_le_bytes());
    out.extend_from_slice(&[0x00, 0x00, 0x00]); // no global color table
    // NETSCAPE2.0 application extension: looping.
    out.extend_from_slice(&[0x21, 0xFF, 0x0B]);
    out.extend_from_slice(b"NETSCAPE2.0");
    out.extend_from_slice(&[0x03, 0x01]);
    out.extend_from_slice(&loop_count.to_le_bytes());
    out.push(0x00);
    for frame in frames {
        let indexed = index_frame(frame.image);
        let bits = (1..=8).find(|b| (1usize << b) >= indexed.table.len()).unwrap_or(8);
        let delay = ((frame.duration_ms + 5) / 10).clamp(2, 65535) as u16;
        // Graphic control extension: disposal 2 (restore background), transparent index 0.
        out.extend_from_slice(&[0x21, 0xF9, 0x04, (2 << 2) | 0x01]);
        out.extend_from_slice(&delay.to_le_bytes());
        out.extend_from_slice(&[0x00, 0x00]);
        // Image descriptor with a local color table.
        out.push(0x2C);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(frame.image.width.min(65535) as u16).to_le_bytes());
        out.extend_from_slice(&(frame.image.height.min(65535) as u16).to_le_bytes());
        out.push(0x80 | (bits as u8 - 1));
        for i in 0..(1usize << bits) {
            out.extend_from_slice(indexed.table.get(i).unwrap_or(&[0, 0, 0]));
        }
        let min_code = bits.max(2) as u32;
        out.push(min_code as u8);
        for block in lzw_encode(min_code, &indexed.pixels).chunks(255) {
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0x00);
    }
    out.push(0x3B);
    out
}

#[cfg(test)]
pub(crate) fn lzw_decode(min_code_size: u32, data: &[u8]) -> Vec<u8> {
    let clear = 1u32 << min_code_size;
    let eoi = clear + 1;
    let (mut pos, mut acc, mut n) = (0usize, 0u32, 0u32);
    let mut size = min_code_size + 1;
    let mut table: Vec<Vec<u8>> = Vec::new();
    let reset = |t: &mut Vec<Vec<u8>>| {
        t.clear();
        for i in 0..clear {
            t.push(vec![i as u8]);
        }
        t.push(vec![]);
        t.push(vec![]);
    };
    reset(&mut table);
    let mut out = Vec::new();
    let mut prev: Option<Vec<u8>> = None;
    loop {
        while n < size {
            acc |= (data[pos] as u32) << n;
            pos += 1;
            n += 8;
        }
        let code = acc & ((1 << size) - 1);
        acc >>= size;
        n -= size;
        if code == clear {
            reset(&mut table);
            size = min_code_size + 1;
            prev = None;
            continue;
        }
        if code == eoi {
            return out;
        }
        let entry = if (code as usize) < table.len() {
            table[code as usize].clone()
        } else {
            let p = prev.clone().unwrap();
            let mut e = p.clone();
            e.push(p[0]);
            e
        };
        out.extend_from_slice(&entry);
        if let Some(p) = prev {
            let mut e = p;
            e.push(entry[0]);
            table.push(e);
            if table.len() >= (1 << size) && size < 12 {
                size += 1;
            }
        }
        prev = Some(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lzw_roundtrip_including_table_reset() {
        let mut rng = 99u32;
        let mut data = Vec::new();
        for _ in 0..200_000 {
            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.push((rng >> 24) as u8 & 0x0F);
        }
        for min in [2u32, 4, 8] {
            let mask = ((1u32 << min) - 1) as u8;
            let d: Vec<u8> = data.iter().map(|v| v & mask).collect();
            assert_eq!(lzw_decode(min, &lzw_encode(min, &d)), d, "min code size {min}");
        }
        assert_eq!(lzw_decode(2, &lzw_encode(2, &[1])), vec![1]);
    }

    #[test]
    fn gif_structure() {
        let mut img = Image::new(4, 4);
        img.pixels[0..4].copy_from_slice(&[255, 0, 0, 255]);
        let gif = encode(4, 4, &[GifFrame { image: &img, duration_ms: 120 }, GifFrame { image: &img, duration_ms: 80 }], 0);
        assert_eq!(&gif[..6], b"GIF89a");
        assert_eq!(*gif.last().unwrap(), 0x3B);
    }
}
