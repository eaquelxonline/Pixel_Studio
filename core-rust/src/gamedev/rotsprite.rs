//! RotSprite: pixel-art-friendly rotation. The sprite is upscaled 8× with Scale2x (which keeps
//! diagonal edges clean), rotated with nearest sampling at high resolution, then sampled back
//! down — no blur, no new colors, far fewer broken lines than a plain nearest rotation.

/// Scale2x (EPX) on a true-color grid (0 = transparent).
fn scale2x(src: &[u32], w: usize, h: usize) -> Vec<u32> {
    let at = |x: isize, y: isize| -> u32 {
        let (x, y) = (x.clamp(0, w as isize - 1) as usize, y.clamp(0, h as isize - 1) as usize);
        src[y * w + x]
    };
    let mut out = vec![0u32; w * h * 4];
    for y in 0..h as isize {
        for x in 0..w as isize {
            let p = at(x, y);
            let (a, b, c, d) = (at(x, y - 1), at(x + 1, y), at(x - 1, y), at(x, y + 1));
            let (mut e0, mut e1, mut e2, mut e3) = (p, p, p, p);
            if c == a && c != d && a != b {
                e0 = a;
            }
            if a == b && a != c && b != d {
                e1 = b;
            }
            if d == c && d != b && c != a {
                e2 = c;
            }
            if b == d && b != a && d != c {
                e3 = d;
            }
            let (ox, oy) = (x as usize * 2, y as usize * 2);
            out[oy * w * 2 + ox] = e0;
            out[oy * w * 2 + ox + 1] = e1;
            out[(oy + 1) * w * 2 + ox] = e2;
            out[(oy + 1) * w * 2 + ox + 1] = e3;
        }
    }
    out
}

/// Rotates `src` (w×h) by `degrees` clockwise around (`cx`, `cy`); output has the same size.
pub fn rotate(src: &[u32], w: usize, h: usize, degrees: f32, cx: f32, cy: f32) -> Vec<u32> {
    let mut big = src.to_vec();
    let (mut bw, mut bh) = (w, h);
    for _ in 0..3 {
        big = scale2x(&big, bw, bh);
        bw *= 2;
        bh *= 2;
    }
    let (s, c) = (-degrees.to_radians()).sin_cos();
    let mut out = vec![0u32; w * h];
    for y in 0..h {
        for x in 0..w {
            // Sample the center of the destination pixel, rotated back into the source.
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let (sx, sy) = (c * dx - s * dy + cx, s * dx + c * dy + cy);
            let (bx, by) = ((sx * 8.0).floor() as isize, (sy * 8.0).floor() as isize);
            if bx < 0 || by < 0 || bx >= bw as isize || by >= bh as isize {
                continue;
            }
            out[y * w + x] = big[by as usize * bw + bx as usize];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turn_and_no_new_colors() {
        let (w, h) = (9, 9);
        let mut src = vec![0u32; w * h];
        for x in 4..9 {
            src[4 * w + x] = 0xFF0000FF; // ray pointing right from the center
        }
        let r = rotate(&src, w, h, 90.0, 4.5, 4.5);
        assert_eq!(r[8 * w + 4], 0xFF0000FF, "clockwise 90° turns right into down");
        assert_eq!(r[4 * w + 8], 0);
        let r30 = rotate(&src, w, h, 30.0, 4.5, 4.5);
        assert!(r30.iter().all(|&v| v == 0 || v == 0xFF0000FF));
        assert!(r30.iter().filter(|&&v| v != 0).count() >= 4);
    }
}
