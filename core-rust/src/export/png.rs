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

/// Decodes a PNG (8/16-bit, grayscale/RGB/palette/alpha, non-interlaced) into RGBA8.
pub fn decode(png: &[u8]) -> Result<Image, String> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if png.len() < 8 || png[..8] != SIG {
        return Err("not a PNG file".into());
    }
    let mut pos = 8;
    let (mut w, mut h, mut depth, mut ctype, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut plte: Vec<[u8; 3]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat = Vec::new();
    while pos + 12 <= png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = &png[pos + 4..pos + 8];
        let end = pos.checked_add(12 + len).filter(|&e| e <= png.len()).ok_or("truncated PNG chunk")?;
        let data = &png[pos + 8..pos + 8 + len];
        let crc = u32::from_be_bytes(png[end - 4..end].try_into().unwrap());
        if crc != crc32_update(crc32_update(0, kind), data) {
            return Err("PNG chunk checksum mismatch".into());
        }
        match kind {
            b"IHDR" if len == 13 => {
                w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                depth = data[8];
                ctype = data[9];
                interlace = data[12];
            }
            b"PLTE" => plte = data.as_chunks::<3>().0.iter().map(|c| [c[0], c[1], c[2]]).collect(),
            b"tRNS" => trns = data.to_vec(),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        pos = end;
    }
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err("unsupported PNG size (max 8192x8192)".into());
    }
    if interlace != 0 {
        return Err("interlaced PNGs are not supported; re-save without interlacing".into());
    }
    let channels = match ctype {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err("unsupported PNG color type".into()),
    };
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) || (depth < 8 && !matches!(ctype, 0 | 3)) || (depth == 16 && ctype == 3) {
        return Err("unsupported PNG bit depth".into());
    }
    let bits_pp = channels * depth as usize;
    let stride = (w as usize * bits_pp).div_ceil(8);
    let bpp = bits_pp.div_ceil(8).max(1);
    let raw = crate::export::inflate::zlib_decompress(&idat, (stride + 1) * h as usize)?;
    if raw.len() != (stride + 1) * h as usize {
        return Err("PNG image data has the wrong size".into());
    }
    let mut cur = vec![0u8; stride];
    let mut prev = vec![0u8; stride];
    let mut img = Image::new(w, h);
    for y in 0..h as usize {
        let f = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for i in 0..stride {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let pred = match f {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                4 => paeth(a, b, c),
                _ => return Err("invalid PNG filter".into()),
            };
            cur[i] = line[i].wrapping_add(pred);
        }
        for x in 0..w as usize {
            let sample = |ch: usize| -> u8 {
                match depth {
                    8 => cur[x * channels + ch],
                    16 => cur[(x * channels + ch) * 2],
                    d => {
                        let bit = x * d as usize;
                        let v = (cur[bit / 8] >> (8 - d as usize - bit % 8)) & ((1u8 << d) - 1);
                        if ctype == 3 { v } else { (v as u32 * 255 / ((1u32 << d) - 1)) as u8 }
                    }
                }
            };
            let px = match ctype {
                0 => {
                    let g = sample(0);
                    let transparent = depth == 8 && trns.len() >= 2 && trns[1] == g;
                    [g, g, g, if transparent { 0 } else { 255 }]
                }
                2 => [sample(0), sample(1), sample(2), 255],
                3 => {
                    let i = sample(0) as usize;
                    let c = plte.get(i).copied().ok_or("PNG palette index out of range")?;
                    [c[0], c[1], c[2], trns.get(i).copied().unwrap_or(255)]
                }
                4 => {
                    let g = sample(0);
                    [g, g, g, sample(1)]
                }
                _ => [sample(0), sample(1), sample(2), sample(3)],
            };
            let o = (y * w as usize + x) * 4;
            img.pixels[o..o + 4].copy_from_slice(&px);
        }
        std::mem::swap(&mut cur, &mut prev);
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_roundtrip() {
        let mut img = Image::new(37, 23);
        for (i, p) in img.pixels.iter_mut().enumerate() {
            *p = ((i * 7) ^ (i / 13)) as u8;
        }
        let png = encode(&img);
        assert_eq!(decode(&png).unwrap(), img);
        let mut bad = png.clone();
        bad[40] ^= 1;
        assert!(decode(&bad).is_err());
        assert!(decode(b"nope").is_err());
    }
}
