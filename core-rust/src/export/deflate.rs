//! zlib (RFC 1950) wrapper around a DEFLATE (RFC 1951) encoder using LZ77 with hash
//! chains and the fixed Huffman code. Pixel art is extremely repetitive, so this gets
//! most of the benefit of a full encoder at a fraction of the code.

const WINDOW: usize = 32 * 1024;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const HASH_BITS: usize = 15;
const MAX_CHAIN: usize = 64;

const LEN_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385,
    24577,
];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    nbits: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, bits: u32) {
        self.acc |= (value as u64) << self.nbits;
        self.nbits += bits;
        while self.nbits >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.nbits -= 8;
        }
    }

    /// Huffman codes are defined MSB-first; the stream is LSB-first.
    fn put_code(&mut self, code: u32, bits: u32) {
        let mut rev = 0;
        for i in 0..bits {
            rev |= ((code >> i) & 1) << (bits - 1 - i);
        }
        self.put(rev, bits);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

fn put_literal(w: &mut BitWriter, sym: u32) {
    match sym {
        0..=143 => w.put_code(0x30 + sym, 8),
        144..=255 => w.put_code(0x190 + (sym - 144), 9),
        256..=279 => w.put_code(sym - 256, 7),
        _ => w.put_code(0xC0 + (sym - 280), 8),
    }
}

fn put_match(w: &mut BitWriter, len: usize, dist: usize) {
    let li = LEN_BASE.iter().rposition(|&b| b as usize <= len).unwrap();
    put_literal(w, 257 + li as u32);
    w.put((len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
    let di = DIST_BASE.iter().rposition(|&b| b as usize <= dist).unwrap();
    w.put_code(di as u32, 5);
    w.put((dist - DIST_BASE[di] as usize) as u32, DIST_EXTRA[di] as u32);
}

#[inline]
fn hash3(d: &[u8], i: usize) -> usize {
    let v = (d[i] as u32) << 16 | (d[i + 1] as u32) << 8 | d[i + 2] as u32;
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut w = BitWriter { out: Vec::with_capacity(data.len() / 4 + 16), acc: 0, nbits: 0 };
    w.put(1, 1); // BFINAL
    w.put(1, 2); // BTYPE = 01 fixed Huffman
    let mut head = vec![usize::MAX; 1 << HASH_BITS];
    let mut prev = vec![usize::MAX; WINDOW];
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, i: usize| {
        if i + MIN_MATCH <= data.len() {
            let h = hash3(data, i);
            prev[i % WINDOW] = head[h];
            head[h] = i;
        }
    };
    let mut i = 0;
    while i < data.len() {
        let mut best_len = 0;
        let mut best_dist = 0;
        if i + MIN_MATCH <= data.len() {
            let mut cand = head[hash3(data, i)];
            let max_len = MAX_MATCH.min(data.len() - i);
            let mut chain = 0;
            while cand != usize::MAX && i - cand <= WINDOW && chain < MAX_CHAIN {
                if data[cand + best_len.min(max_len - 1)] == data[i + best_len.min(max_len - 1)] {
                    let mut l = 0;
                    while l < max_len && data[cand + l] == data[i + l] {
                        l += 1;
                    }
                    if l > best_len {
                        best_len = l;
                        best_dist = i - cand;
                        if l == max_len {
                            break;
                        }
                    }
                }
                let next = prev[cand % WINDOW];
                if next == usize::MAX || next >= cand {
                    break;
                }
                cand = next;
                chain += 1;
            }
        }
        if best_len >= MIN_MATCH {
            put_match(&mut w, best_len, best_dist);
            for k in i..i + best_len {
                insert(&mut head, &mut prev, k);
            }
            i += best_len;
        } else {
            put_literal(&mut w, data[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    put_literal(&mut w, 256);
    w.finish()
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.extend(deflate(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

static CRC_TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();

pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

pub fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let table = CRC_TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut c = !crc;
    for &b in data {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_checksums() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn deflate_roundtrip() {
        let mut samples: Vec<Vec<u8>> = vec![vec![], b"a".to_vec(), b"abcabcabcabcabcabc".to_vec(), vec![7u8; 100_000]];
        let mut rng = 12345u32;
        let mut noise = Vec::new();
        for _ in 0..70_000 {
            rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12345);
            noise.push((rng >> 16) as u8 % 7);
        }
        samples.push(noise);
        for s in samples {
            let z = zlib_compress(&s);
            assert_eq!(crate::export::inflate::zlib_decompress(&z, 1 << 24).unwrap(), s);
        }
        assert!(zlib_compress(&[7u8; 100_000]).len() < 2_000);
    }
}
