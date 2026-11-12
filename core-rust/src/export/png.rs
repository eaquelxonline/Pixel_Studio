//! PNG encoder (RGBA8, non-interlaced) built on the in-house zlib encoder.

use super::deflate::{crc32_update, zlib_compress};
use crate::renderer::Image;

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32_update(crc32_update(0, kind), data);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Encodes with a per-row filter chosen by the minimum-sum-of-absolute-differences heuristic.
pub fn encode(img: &Image) -> Vec<u8> {
    let stride = img.width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * img.height as usize);
    let zero = vec![0u8; stride];
    let mut candidates: [Vec<u8>; 5] = Default::default();
    for y in 0..img.height as usize {
        let row = &img.pixels[y * stride..(y + 1) * stride];
        let up = if y == 0 { &zero[..] } else { &img.pixels[(y - 1) * stride..y * stride] };
        for (f, cand) in candidates.iter_mut().enumerate() {
            cand.clear();
            for i in 0..stride {
                let a = if i >= 4 { row[i - 4] } else { 0 };
                let b = up[i];
                let c = if i >= 4 { up[i - 4] } else { 0 };
                let pred = match f {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((a as u16 + b as u16) / 2) as u8,
                    _ => paeth(a, b, c),
                };
                cand.push(row[i].wrapping_sub(pred));
            }
        }
        let score = |v: &Vec<u8>| v.iter().map(|&x| (x as i8).unsigned_abs() as u32).sum::<u32>();
        let best = (0..5).min_by_key(|&f| score(&candidates[f])).unwrap();
        raw.push(best as u8);
        raw.extend_from_slice(&candidates[best]);
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, adaptive filter, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"tEXt", b"Software\0Pixel Studio");
    chunk(&mut out, b"IDAT", &zlib_compress(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::deflate::inflate::zlib_decompress;

    /// Decodes our own output back to RGBA to prove the filters are reversible.
    fn decode(png: &[u8]) -> Image {
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let mut pos = 8;
        let (mut w, mut h, mut idat) = (0, 0, Vec::new());
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let kind = &png[pos + 4..pos + 8];
            let data = &png[pos + 8..pos + 8 + len];
            let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32_update(crc32_update(0, kind), data));
            match kind {
                b"IHDR" => {
                    w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                    h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                }
                b"IDAT" => idat.extend_from_slice(data),
                _ => {}
            }
            pos += 12 + len;
        }
        let raw = zlib_decompress(&idat);
        let stride = w as usize * 4;
        let mut img = Image::new(w, h);
        for y in 0..h as usize {
            let f = raw[y * (stride + 1)];
            for i in 0..stride {
                let x = raw[y * (stride + 1) + 1 + i];
                let a = if i >= 4 { img.pixels[y * stride + i - 4] } else { 0 };
                let b = if y > 0 { img.pixels[(y - 1) * stride + i] } else { 0 };
                let c = if y > 0 && i >= 4 { img.pixels[(y - 1) * stride + i - 4] } else { 0 };
                let pred = match f {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((a as u16 + b as u16) / 2) as u8,
                    _ => paeth(a, b, c),
                };
                img.pixels[y * stride + i] = x.wrapping_add(pred);
            }
        }
        img
    }

    #[test]
    fn png_roundtrip() {
        let mut img = Image::new(37, 23);
        for (i, p) in img.pixels.iter_mut().enumerate() {
            *p = ((i * 7) ^ (i / 13)) as u8;
        }
        let png = encode(&img);
        assert_eq!(decode(&png), img);
    }
}
