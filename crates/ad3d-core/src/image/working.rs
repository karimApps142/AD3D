//! The working copy (design §16 steps 3–4): geometry, area downsample in linear light,
//! optional unsharp mask, perceptual encoding and histogram. Cached by
//! [`WorkingKey`](super::WorkingKey).
//!
//! The crop is not cut on pixel boundaries. The working image covers the whole source
//! pixels the crop touches, and [`WorkingImage::window`] holds the exact (fractional)
//! crop rectangle in working pixels, so area means over plate cells stay exact.
//! Rotation and mirroring are pixel permutations, applied after the downsample (which
//! commutes with them), so a 12 MP photo is never copied at full size.

use super::opts::ImageOpts;
use super::{LinearLuma, linear_to_srgb};
use crate::error::{Error, Result};

/// Long-side limit of the working copy (§16 step 4).
pub const DEFAULT_MAX_SIDE: u32 = 2048;
/// Long-side limit while a tone slider drags (§16 step 6).
pub const DRAG_MAX_SIDE: u32 = 1024;
/// Histogram bins over `v ∈ [0, 1]`.
pub const HIST_BINS: usize = 4096;

/// Perceptual working image.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkingImage {
    pub w: u32,
    pub h: u32,
    /// `v = srgb_encode(Y) ∈ [0, 1]`, row-major, row 0 at the top.
    pub v: Vec<f32>,
    /// The crop rectangle `x0, y0, x1, y1` in working pixels (y down).
    pub window: [f64; 4],
    /// Histogram of `v` over the pixels whose centres lie in the window.
    pub hist: Vec<u32>,
}

impl WorkingImage {
    /// Wraps a perceptual image with the whole image as window (tests, synthetic input).
    pub fn from_v(w: u32, h: u32, v: Vec<f32>) -> Result<Self> {
        if w == 0 || h == 0 || v.len() != w as usize * h as usize {
            return Err(Error::InvalidParams("empty or malformed working image".into()));
        }
        let window = [0.0, 0.0, w as f64, h as f64];
        let hist = histogram(w, h, &v, &window);
        Ok(WorkingImage { w, h, v, window, hist })
    }
}

/// Inverse of the picture transform (rotate `k` quarter turns clockwise, then mirror)
/// applied to a normalised point of the transformed picture.
fn inverse_point(k: u8, mirror: bool, x: f64, y: f64) -> (f64, f64) {
    let (mut x, mut y) = if mirror { (1.0 - x, y) } else { (x, y) };
    for _ in 0..k {
        // Inverse of clockwise (x, y) → (1 − y, x) is (x, y) → (y, 1 − x).
        (x, y) = (y, 1.0 - x);
    }
    (x, y)
}

/// Rectangle of the transformed picture → rectangle of the source (normalised).
fn crop_in_source(k: u8, mirror: bool, c: [f64; 4]) -> [f64; 4] {
    let pts = [(c[0], c[1]), (c[2], c[1]), (c[0], c[3]), (c[2], c[3])].map(|(x, y)| inverse_point(k, mirror, x, y));
    let xs = pts.map(|p| p.0);
    let ys = pts.map(|p| p.1);
    let mn = |a: [f64; 4]| a.iter().copied().fold(f64::INFINITY, f64::min).clamp(0.0, 1.0);
    let mx = |a: [f64; 4]| a.iter().copied().fold(f64::NEG_INFINITY, f64::max).clamp(0.0, 1.0);
    [mn(xs), mn(ys), mx(xs), mx(ys)]
}

/// Fractional coverage of `n` equal bins over `[a, a + len)` source pixels (`a`, `len`
/// integers). Returns per bin the first pixel and the weight range in `weights`.
fn bins(n: usize, a: usize, len: usize) -> (Vec<(usize, usize, usize)>, Vec<f32>) {
    let step = len as f64 / n as f64;
    let mut spans = Vec::with_capacity(n);
    let mut weights = Vec::new();
    for b in 0..n {
        let lo = b as f64 * step;
        let hi = if b + 1 == n { len as f64 } else { (b + 1) as f64 * step };
        let p0 = (lo.floor() as usize).min(len - 1);
        let p1 = (hi.ceil() as usize).clamp(p0 + 1, len);
        let off = weights.len();
        for p in p0..p1 {
            weights.push(((hi.min(p as f64 + 1.0) - lo.max(p as f64)).max(0.0) / step) as f32);
        }
        spans.push((a + p0, off, weights.len()));
    }
    (spans, weights)
}

#[inline]
fn clean(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

/// Area-resamples the source region `[x0, x0+rw) × [y0, y0+rh)` to `ow × oh` (linear).
fn resample(img: &LinearLuma, x0: usize, y0: usize, rw: usize, rh: usize, ow: usize, oh: usize) -> Vec<f32> {
    let w = img.w as usize;
    if ow == rw && oh == rh {
        let mut out = Vec::with_capacity(ow * oh);
        for y in y0..y0 + rh {
            out.extend(img.px[y * w + x0..y * w + x0 + rw].iter().map(|&v| clean(v)));
        }
        return out;
    }
    let (xs, xw) = bins(ow, x0, rw);
    let (ys, yw) = bins(oh, y0, rh);
    // Horizontal pass over every source row of the region.
    let mut rows = vec![0f32; rh * ow];
    for (ri, y) in (y0..y0 + rh).enumerate() {
        let line = &img.px[y * w..(y + 1) * w];
        let out = &mut rows[ri * ow..(ri + 1) * ow];
        for (o, &(p0, a, b)) in out.iter_mut().zip(&xs) {
            let mut s = 0f32;
            for (k, &wt) in xw[a..b].iter().enumerate() {
                s += wt * clean(line[p0 + k]);
            }
            *o = s;
        }
    }
    // Vertical pass.
    let mut out = vec![0f32; ow * oh];
    for (oy, &(p0, a, b)) in ys.iter().enumerate() {
        let dst = &mut out[oy * ow..(oy + 1) * ow];
        for (k, &wt) in yw[a..b].iter().enumerate() {
            let src = &rows[(p0 - y0 + k) * ow..(p0 - y0 + k + 1) * ow];
            for (d, &s) in dst.iter_mut().zip(src) {
                *d += wt * s;
            }
        }
        for d in dst.iter_mut() {
            *d = d.clamp(0.0, 1.0);
        }
    }
    out
}

/// Rotates a `w × h` buffer one quarter turn clockwise (result `h × w`).
fn rot_cw(w: usize, h: usize, src: &[f32]) -> Vec<f32> {
    let mut out = vec![0f32; w * h];
    // New width is h: new (x', y') = (h − 1 − y, x).
    for y in 0..h {
        for x in 0..w {
            out[x * h + (h - 1 - y)] = src[y * w + x];
        }
    }
    out
}

fn mirror_rows(w: usize, buf: &mut [f32]) {
    for row in buf.chunks_exact_mut(w) {
        row.reverse();
    }
}

fn gaussian_blur(w: usize, h: usize, src: &[f32], sigma: f64) -> Vec<f32> {
    let r = (3.0 * sigma).ceil().max(1.0) as isize;
    let mut k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f64 / (2.0 * sigma * sigma)).exp() as f32).collect();
    let s: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= s);
    let clampi = |i: isize, n: usize| i.clamp(0, n as isize - 1) as usize;
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        for x in 0..w {
            let mut acc = 0f32;
            for (t, &kv) in k.iter().enumerate() {
                acc += kv * row[clampi(x as isize + t as isize - r, w)];
            }
            tmp[y * w + x] = acc;
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        for (t, &kv) in k.iter().enumerate() {
            let sy = clampi(y as isize + t as isize - r, h);
            let srow = &tmp[sy * w..(sy + 1) * w];
            let drow = &mut out[y * w..(y + 1) * w];
            for (d, &s) in drow.iter_mut().zip(srow) {
                *d += kv * s;
            }
        }
    }
    out
}

fn histogram(w: u32, h: u32, v: &[f32], win: &[f64; 4]) -> Vec<u32> {
    let mut hist = vec![0u32; HIST_BINS];
    let (w, h) = (w as usize, h as usize);
    // Pixel x has centre x + 0.5; include it when x0 ≤ x + 0.5 < x1. Always at least one.
    let xa = ((win[0] - 0.5).ceil().max(0.0) as usize).min(w - 1);
    let xb = ((win[2] - 0.5).ceil().max(0.0) as usize).clamp(xa + 1, w);
    let ya = ((win[1] - 0.5).ceil().max(0.0) as usize).min(h - 1);
    let yb = ((win[3] - 0.5).ceil().max(0.0) as usize).clamp(ya + 1, h);
    for y in ya..yb {
        for &p in &v[y * w + xa..y * w + xb] {
            hist[((p as f64 * HIST_BINS as f64) as usize).min(HIST_BINS - 1)] += 1;
        }
    }
    hist
}

/// Builds the working copy of `img` for `opts` with the long side limited to
/// `max_side` (use [`DEFAULT_MAX_SIDE`], or [`DRAG_MAX_SIDE`] while dragging). Images
/// smaller than that keep their native pixels. Tone options are ignored here.
pub fn build_working(img: &LinearLuma, opts: &ImageOpts, max_side: u32) -> Result<WorkingImage> {
    img.check()?;
    opts.check()?;
    if max_side == 0 {
        return Err(Error::InvalidParams("working resolution must be positive".into()));
    }
    let (k, mirror) = (opts.quarter_turns(), opts.mirror);
    let c = crop_in_source(k, mirror, opts.crop_or_full());
    let (sw, sh) = (img.w as f64, img.h as f64);
    let (cx0, cy0, cx1, cy1) = (c[0] * sw, c[1] * sh, c[2] * sw, c[3] * sh);
    let px0 = (cx0.floor().max(0.0) as usize).min(img.w as usize - 1);
    let py0 = (cy0.floor().max(0.0) as usize).min(img.h as usize - 1);
    let px1 = (cx1.ceil() as usize).clamp(px0 + 1, img.w as usize);
    let py1 = (cy1.ceil() as usize).clamp(py0 + 1, img.h as usize);
    let (rw, rh) = (px1 - px0, py1 - py0);
    let long = rw.max(rh);
    let (ow, oh) = if long > max_side as usize {
        let s = max_side as f64 / long as f64;
        (((rw as f64 * s).ceil() as usize).clamp(1, max_side as usize), ((rh as f64 * s).ceil() as usize).clamp(1, max_side as usize))
    } else {
        (rw, rh)
    };
    let mut y = resample(img, px0, py0, rw, rh, ow, oh);
    let (fx, fy) = (ow as f64 / rw as f64, oh as f64 / rh as f64);
    let mut win = [(cx0 - px0 as f64) * fx, (cy0 - py0 as f64) * fy, (cx1 - px0 as f64) * fx, (cy1 - py0 as f64) * fy];
    let (mut w, mut h) = (ow, oh);
    for _ in 0..k {
        y = rot_cw(w, h, &y);
        // (x, y) → (h − y, x) on continuous coordinates.
        win = [h as f64 - win[3], win[0], h as f64 - win[1], win[2]];
        (w, h) = (h, w);
    }
    if mirror {
        mirror_rows(w, &mut y);
        win = [w as f64 - win[2], win[1], w as f64 - win[0], win[3]];
    }
    let mut v: Vec<f32> = y.iter().map(|&l| linear_to_srgb(l).clamp(0.0, 1.0)).collect();
    if let Some(s) = opts.sharpen {
        if s.amount > 0.0 {
            let blur = gaussian_blur(w, h, &v, s.radius_px);
            let a = s.amount as f32;
            for (p, b) in v.iter_mut().zip(blur) {
                *p = (*p + a * (*p - b)).clamp(0.0, 1.0);
            }
        }
    }
    let win = win.map(|c| c.max(0.0));
    let win = [win[0], win[1], win[2].min(w as f64), win[3].min(h as f64)];
    let hist = histogram(w as u32, h as u32, &v, &win);
    Ok(WorkingImage { w: w as u32, h: h as u32, v, window: win, hist })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::srgb_to_linear;

    fn ramp(w: u32, h: u32) -> LinearLuma {
        LinearLuma { w, h, px: (0..w * h).map(|k| k as f32 / (w * h) as f32).collect() }
    }

    #[test]
    fn identity_keeps_pixels() {
        let img = ramp(5, 3);
        let wk = build_working(&img, &ImageOpts::default(), 2048).unwrap();
        assert_eq!((wk.w, wk.h), (5, 3));
        assert_eq!(wk.window, [0.0, 0.0, 5.0, 3.0]);
        for (v, &l) in wk.v.iter().zip(&img.px) {
            assert!((srgb_to_linear(*v) - l).abs() < 1e-5);
        }
        assert_eq!(wk.hist.iter().sum::<u32>(), 15);
    }

    #[test]
    fn rotation_permutes_pixels() {
        // 2×1: [a, b] rotated clockwise is 1×2: [a; b].
        let img = LinearLuma { w: 2, h: 1, px: vec![0.0, 1.0] };
        let wk = build_working(&img, &ImageOpts { rotate_deg: 90, ..Default::default() }, 2048).unwrap();
        assert_eq!((wk.w, wk.h), (1, 2));
        assert_eq!(wk.v, vec![0.0, 1.0]);
        let wk = build_working(&img, &ImageOpts { rotate_deg: -90, ..Default::default() }, 2048).unwrap();
        assert_eq!(wk.v, vec![1.0, 0.0]);
        let wk = build_working(&img, &ImageOpts { mirror: true, ..Default::default() }, 2048).unwrap();
        assert_eq!(wk.v, vec![1.0, 0.0]);
    }

    #[test]
    fn crop_window_is_fractional_and_rotated() {
        let img = ramp(10, 4);
        let o = ImageOpts { crop: Some([0.25, 0.0, 0.5, 0.5]), ..Default::default() };
        let wk = build_working(&img, &o, 2048).unwrap();
        // Source crop x 2.5..5, y 0..2 → pixels 2..5, 0..2.
        assert_eq!((wk.w, wk.h), (3, 2));
        assert_eq!(wk.window, [0.5, 0.0, 3.0, 2.0]);
        // The crop is in the rotated picture: rotate 90° (10×4 → 4×10), take the top half.
        let o = ImageOpts { crop: Some([0.0, 0.0, 1.0, 0.5]), rotate_deg: 90, ..Default::default() };
        let wk = build_working(&img, &o, 2048).unwrap();
        assert_eq!((wk.w, wk.h), (4, 5));
        assert_eq!(wk.window, [0.0, 0.0, 4.0, 5.0]);
    }

    #[test]
    fn downsample_limits_long_side_and_keeps_mean() {
        let img = ramp(300, 120);
        let wk = build_working(&img, &ImageOpts::default(), 64).unwrap();
        assert_eq!((wk.w, wk.h), (64, 26));
        let m0: f64 = img.px.iter().map(|&v| v as f64).sum::<f64>() / img.px.len() as f64;
        let m1: f64 = wk.v.iter().map(|&v| srgb_to_linear(v) as f64).sum::<f64>() / wk.v.len() as f64;
        assert!((m0 - m1).abs() < 1e-4, "{m0} {m1}");
    }

    #[test]
    fn sharpen_leaves_flat_image_and_raises_edges() {
        let img = LinearLuma { w: 8, h: 1, px: vec![0.2; 8] };
        let o = ImageOpts { sharpen: Some(crate::image::Unsharp { amount: 1.0, radius_px: 1.0 }), ..Default::default() };
        let wk = build_working(&img, &o, 2048).unwrap();
        let v0 = linear_to_srgb(0.2);
        assert!(wk.v.iter().all(|&v| (v - v0).abs() < 1e-5));
        let img = LinearLuma { w: 8, h: 1, px: vec![0.1, 0.1, 0.1, 0.1, 0.6, 0.6, 0.6, 0.6] };
        let wk = build_working(&img, &o, 2048).unwrap();
        assert!(wk.v[4] > linear_to_srgb(0.6) && wk.v[3] < linear_to_srgb(0.1));
    }

    #[test]
    fn bad_image_rejected() {
        let img = LinearLuma { w: 2, h: 2, px: vec![0.0; 3] };
        assert!(build_working(&img, &ImageOpts::default(), 2048).is_err());
    }
}
