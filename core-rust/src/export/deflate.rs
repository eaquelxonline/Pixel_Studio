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
pub(crate) mod inflate {
    //! Tiny reference inflater (fixed + stored + dynamic blocks) used only by tests to
    //! prove the encoder output is valid DEFLATE.
    struct Bits<'a> {
        d: &'a [u8],
        pos: usize,
        bit: u32,
    }
    impl Bits<'_> {
        fn get(&mut self, n: u32) -> u32 {
            let mut v = 0;
            for i in 0..n {
                let b = (self.d[self.pos] >> self.bit) & 1;
                v |= (b as u32) << i;
                self.bit += 1;
                if self.bit == 8 {
                    self.bit = 0;
                    self.pos += 1;
                }
            }
            v
        }
    }
    struct Huff {
        counts: [u16; 16],
        symbols: Vec<u16>,
    }
    fn build(lengths: &[u8]) -> Huff {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
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
        Huff { counts, symbols }
    }
    fn decode(b: &mut Bits, h: &Huff) -> u16 {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.get(1) as i32;
            let count = h.counts[len] as i32;
            if code - count < first {
                return h.symbols[(index + (code - first)) as usize];
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        panic!("bad code");
    }
    const LB: [u16; 29] = super::LEN_BASE;
    const LE: [u8; 29] = super::LEN_EXTRA;
    const DB: [u16; 30] = super::DIST_BASE;
    const DE: [u8; 30] = super::DIST_EXTRA;

    pub fn zlib_decompress(z: &[u8]) -> Vec<u8> {
        let mut b = Bits { d: &z[2..], pos: 0, bit: 0 };
        let mut out = Vec::new();
        loop {
            let last = b.get(1);
            let ty = b.get(2);
            match ty {
                0 => {
                    if b.bit != 0 {
                        b.bit = 0;
                        b.pos += 1;
                    }
                    let len = u16::from_le_bytes([b.d[b.pos], b.d[b.pos + 1]]) as usize;
                    b.pos += 4;
                    out.extend_from_slice(&b.d[b.pos..b.pos + len]);
                    b.pos += len;
                }
                1 | 2 => {
                    let (lit, dist) = if ty == 1 {
                        let mut l = [0u8; 288];
                        l[..144].fill(8);
                        l[144..256].fill(9);
                        l[256..280].fill(7);
                        l[280..].fill(8);
                        (build(&l), build(&[5u8; 30]))
                    } else {
                        let hlit = b.get(5) as usize + 257;
                        let hdist = b.get(5) as usize + 1;
                        let hclen = b.get(4) as usize + 4;
                        const ORD: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                        let mut cl = [0u8; 19];
                        for &o in ORD.iter().take(hclen) {
                            cl[o] = b.get(3) as u8;
                        }
                        let ch = build(&cl);
                        let mut lens = Vec::new();
                        while lens.len() < hlit + hdist {
                            let s = decode(&mut b, &ch);
                            match s {
                                0..=15 => lens.push(s as u8),
                                16 => {
                                    let p = *lens.last().unwrap();
                                    for _ in 0..3 + b.get(2) {
                                        lens.push(p)
                                    }
                                }
                                17 => {
                                    for _ in 0..3 + b.get(3) {
                                        lens.push(0)
                                    }
                                }
                                _ => {
                                    for _ in 0..11 + b.get(7) {
                                        lens.push(0)
                                    }
                                }
                            }
                        }
                        (build(&lens[..hlit]), build(&lens[hlit..]))
                    };
                    loop {
                        let s = decode(&mut b, &lit) as usize;
                        if s < 256 {
                            out.push(s as u8);
                        } else if s == 256 {
                            break;
                        } else {
                            let li = s - 257;
                            let len = LB[li] as usize + b.get(LE[li] as u32) as usize;
                            let di = decode(&mut b, &dist) as usize;
                            let d = DB[di] as usize + b.get(DE[di] as u32) as usize;
                            let start = out.len() - d;
                            for k in 0..len {
                                out.push(out[start + k]);
                            }
                        }
                    }
                }
                _ => panic!("bad block type"),
            }
            if last == 1 {
                break;
            }
        }
        out
    }
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
            assert_eq!(inflate::zlib_decompress(&z), s);
        }
        assert!(zlib_compress(&[7u8; 100_000]).len() < 2_000);
    }
}
