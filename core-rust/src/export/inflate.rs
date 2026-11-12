//! DEFLATE / zlib decoder (RFC 1950/1951) with strict bounds checks — used for PNG import,
//! texture import and compressed project data. Malformed input returns an error; it never panics.

const LEN_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385,
    24577,
];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    acc: u64,
    n: u32,
}

impl Bits<'_> {
    fn need(&mut self, k: u32) -> Result<(), String> {
        while self.n < k {
            let b = *self.d.get(self.pos).ok_or("unexpected end of compressed data")?;
            self.pos += 1;
            self.acc |= (b as u64) << self.n;
            self.n += 8;
        }
        Ok(())
    }

    fn get(&mut self, k: u32) -> Result<u32, String> {
        if k == 0 {
            return Ok(0);
        }
        self.need(k)?;
        let v = (self.acc & ((1u64 << k) - 1)) as u32;
        self.acc >>= k;
        self.n -= k;
        Ok(v)
    }

    fn align(&mut self) {
        let drop = self.n % 8;
        self.acc >>= drop;
        self.n -= drop;
    }
}

struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman, String> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l > 15 {
                return Err("invalid code length".into());
            }
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut left: i32 = 1;
        for &c in counts.iter().skip(1) {
            left = (left << 1) - c as i32;
            if left < 0 {
                return Err("over-subscribed Huffman code".into());
            }
        }
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huffman { counts, symbols })
    }

    fn decode(&self, b: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.get(1)? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return self.symbols.get((index + code - first) as usize).copied().ok_or_else(|| "bad Huffman symbol".to_string());
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("invalid Huffman code".into())
    }
}

/// Inflates raw DEFLATE data, refusing to produce more than `limit` bytes.
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut b = Bits { d: data, pos: 0, acc: 0, n: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = b.get(1)?;
        match b.get(2)? {
            0 => {
                b.align();
                let len = b.get(16)? as usize;
                let nlen = b.get(16)? as usize;
                if len != (!nlen & 0xFFFF) {
                    return Err("stored block length mismatch".into());
                }
                if out.len() + len > limit {
                    return Err("decompressed data too large".into());
                }
                for _ in 0..len {
                    out.push(b.get(8)? as u8);
                }
            }
            t @ (1 | 2) => {
                let (lit, dist) = if t == 1 {
                    let mut l = [0u8; 288];
                    l[..144].fill(8);
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    l[280..].fill(8);
                    (Huffman::new(&l)?, Huffman::new(&[5u8; 30])?)
                } else {
                    let hlit = b.get(5)? as usize + 257;
                    let hdist = b.get(5)? as usize + 1;
                    let hclen = b.get(4)? as usize + 4;
                    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                    let mut cl = [0u8; 19];
                    for &o in ORDER.iter().take(hclen) {
                        cl[o] = b.get(3)? as u8;
                    }
                    let ch = Huffman::new(&cl)?;
                    let mut lens: Vec<u8> = Vec::with_capacity(hlit + hdist);
                    while lens.len() < hlit + hdist {
                        let (value, repeat) = match ch.decode(&mut b)? {
                            s @ 0..=15 => (s as u8, 1),
                            16 => (*lens.last().ok_or("repeat without previous length")?, 3 + b.get(2)? as usize),
                            17 => (0, 3 + b.get(3)? as usize),
                            _ => (0, 11 + b.get(7)? as usize),
                        };
                        if lens.len() + repeat > hlit + hdist {
                            return Err("code lengths overflow".into());
                        }
                        lens.extend(std::iter::repeat_n(value, repeat));
                    }
                    (Huffman::new(&lens[..hlit])?, Huffman::new(&lens[hlit..])?)
                };
                loop {
                    let sym = lit.decode(&mut b)? as usize;
                    if sym < 256 {
                        if out.len() >= limit {
                            return Err("decompressed data too large".into());
                        }
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let li = sym - 257;
                        if li >= 29 {
                            return Err("invalid length symbol".into());
                        }
                        let len = LEN_BASE[li] as usize + b.get(LEN_EXTRA[li] as u32)? as usize;
                        let di = dist.decode(&mut b)? as usize;
                        if di >= 30 {
                            return Err("invalid distance symbol".into());
                        }
                        let d = DIST_BASE[di] as usize + b.get(DIST_EXTRA[di] as u32)? as usize;
                        if d > out.len() {
                            return Err("distance too far back".into());
                        }
                        if out.len() + len > limit {
                            return Err("decompressed data too large".into());
                        }
                        let start = out.len() - d;
                        for k in 0..len {
                            out.push(out[start + k]);
                        }
                    }
                }
            }
            _ => return Err("invalid block type".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

pub fn zlib_decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    if data.len() < 6 || (data[0] & 0x0F) != 8 || !((data[0] as u16) << 8 | data[1] as u16).is_multiple_of(31) {
        return Err("not a zlib stream".into());
    }
    if data[1] & 0x20 != 0 {
        return Err("zlib preset dictionaries are not supported".into());
    }
    let out = inflate(&data[2..], limit)?;
    let expected = u32::from_be_bytes(data[data.len() - 4..].try_into().unwrap());
    if super::deflate::adler32(&out) != expected {
        return Err("zlib checksum mismatch".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::deflate::zlib_compress;

    #[test]
    fn roundtrip_and_rejects_garbage() {
        let mut data = Vec::new();
        for i in 0..50_000u32 {
            data.push((i * 7 % 13) as u8);
        }
        assert_eq!(zlib_decompress(&zlib_compress(&data), 1 << 20).unwrap(), data);
        assert!(zlib_decompress(&zlib_compress(&data), 100).is_err(), "limit enforced");
        let mut bad = zlib_compress(&data);
        let mid = bad.len() / 2;
        bad[mid] ^= 0xFF;
        assert!(zlib_decompress(&bad, 1 << 20).is_err());
        assert!(zlib_decompress(b"\x78\x9c\xff\xff\xff", 1000).is_err());
        assert!(zlib_decompress(b"", 1000).is_err());
    }

    #[test]
    fn decodes_dynamic_huffman_from_reference_encoder() {
        // Produced by the reference zlib (level 9, BTYPE=2 dynamic Huffman block).
        let z = [
            0x78, 0xda, 0xd5, 0xcc, 0xc1, 0x0d, 0x80, 0x20, 0x10, 0x44, 0xd1, 0x56, 0xc6, 0x36, 0xac, 0x82, 0xc4, 0x0a, 0x10, 0x36, 0xb0,
            0xc9, 0xc2, 0x24, 0x2c, 0x18, 0xcb, 0x57, 0xcb, 0xf0, 0xf8, 0x0e, 0xff, 0x07, 0xbd, 0xc5, 0x70, 0xcc, 0x95, 0x95, 0xb8, 0xf8,
            0xa1, 0xca, 0xe0, 0x8e, 0x68, 0xa7, 0x64, 0xa2, 0x73, 0xb4, 0x68, 0x18, 0x5c, 0xa5, 0x76, 0x71, 0x47, 0x93, 0x19, 0xcd, 0x34,
            0x81, 0x29, 0xd9, 0x72, 0x65, 0x7f, 0x03, 0x2d, 0x75, 0x42, 0x9a, 0xfa, 0xe7, 0x0d, 0xe1, 0x37, 0xd3, 0x07, 0xcd, 0x99, 0x5f,
            0x8c,
        ];
        let text = "Pixel Studio voxel hero: albedo normal roughness metallic occlusion height emission! ".repeat(3);
        assert_eq!(zlib_decompress(&z, 1000).unwrap(), text.as_bytes());
    }
}
