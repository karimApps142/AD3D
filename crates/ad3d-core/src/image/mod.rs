//! Image → per-cell brightness (design §16, spike subset).
//!
//! [`LinearLuma`] holds linear relative luminance in `[0, 1]`, row 0 at the top of
//! the picture. [`cell_brightness`] averages it exactly over each cell's area
//! (piecewise-constant pixels, fractional coverage at cell borders). Cell row 0 is
//! the bottom of the plate (`v = 0`), i.e. the bottom of the picture.

use crate::error::{Error, Result};

/// Linear relative luminance, row-major, row 0 = top of the picture.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearLuma {
    pub w: u32,
    pub h: u32,
    pub px: Vec<f32>,
}

/// One linear brightness `g ∈ [0, 1]` per cell, row-major, row 0 at `v = 0`.
#[derive(Debug, Clone, PartialEq)]
pub struct CellBrightness {
    pub nx: u32,
    pub ny: u32,
    pub g: Vec<f32>,
}

impl CellBrightness {
    pub fn uniform(nx: u32, ny: u32, g: f32) -> Self {
        CellBrightness { nx, ny, g: vec![g; (nx * ny) as usize] }
    }
    #[inline]
    pub fn at(&self, i: u32, j: u32) -> f32 {
        self.g[(j * self.nx + i) as usize]
    }
}

/// sRGB transfer function, decode (code value in [0,1] → linear).
#[inline]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

impl LinearLuma {
    /// sRGB → linear per channel, then `Y = 0.2126 R + 0.7152 G + 0.0722 B`.
    pub fn from_dynamic(img: &image::DynamicImage) -> Self {
        let mut lut = [0f32; 256];
        for (k, v) in lut.iter_mut().enumerate() {
            *v = srgb_to_linear(k as f32 / 255.0);
        }
        let (w, h) = (img.width(), img.height());
        let px = match img {
            image::DynamicImage::ImageLuma8(g) => g.as_raw().iter().map(|&c| lut[c as usize]).collect(),
            _ => {
                let rgb = img.to_rgb8();
                rgb.as_raw()
                    .chunks_exact(3)
                    .map(|c| 0.2126 * lut[c[0] as usize] + 0.7152 * lut[c[1] as usize] + 0.0722 * lut[c[2] as usize])
                    .collect()
            }
        };
        LinearLuma { w, h, px }
    }

    /// Decodes a file, applies its EXIF orientation and linearises it. This is an IO
    /// helper for the CLI and app; generators never call it.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        use image::ImageDecoder;
        let reader = image::ImageReader::open(path)
            .map_err(|e| Error::Io(format!("{}: {e}", path.display())))?
            .with_guessed_format()
            .map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        let mut dec = reader.into_decoder().map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        let orient = dec.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
        let mut img =
            image::DynamicImage::from_decoder(dec).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        img.apply_orientation(orient);
        Ok(Self::from_dynamic(&img))
    }
}

/// Synthetic test target, modelled on the prototype's `target_image()` (500×630 at
/// scale 1): a left-to-right gradient over the top third, a bright disc and a dark
/// square inside it. Code values are treated as sRGB and linearised.
pub fn synthetic_target(w: u32, h: u32) -> LinearLuma {
    let sx = w as f64 / 500.0;
    let sy = h as f64 / 630.0;
    let mut px = Vec::with_capacity((w * h) as usize);
    let mut lut = [0f32; 256];
    for (k, v) in lut.iter_mut().enumerate() {
        *v = srgb_to_linear(k as f32 / 255.0);
    }
    let (l230, l40) = (lut[230], lut[40]);
    for y in 0..h {
        let fy = (y as f64 + 0.5) / sy; // in 500×630 units
        for x in 0..w {
            let fx = (x as f64 + 0.5) / sx;
            let mut v = 0.0f32;
            if fy <= 210.0 {
                v = lut[((255.0 * (fx / 500.0)).floor() as usize).min(255)];
            }
            let (dx, dy) = ((fx - 250.0) / 150.0, (fy - 410.0) / 150.0);
            if dx * dx + dy * dy <= 1.0 {
                v = l230;
            }
            if (180.0..=320.0).contains(&fx) && (360.0..=460.0).contains(&fy) {
                v = l40;
            }
            px.push(v);
        }
    }
    LinearLuma { w, h, px }
}

/// Normalised crop rectangle `x0, y0, x1, y1` (image space, y down).
pub type Crop = [f64; 4];

/// The centred crop of a `w × h` image with aspect `aw : ah` (filling, no bars).
pub fn centre_crop(w: u32, h: u32, aw: f64, ah: f64) -> Crop {
    let img_aspect = w as f64 / h as f64;
    let want = aw / ah;
    if img_aspect > want {
        let f = want / img_aspect;
        [0.5 - f / 2.0, 0.0, 0.5 + f / 2.0, 1.0]
    } else {
        let f = img_aspect / want;
        [0.0, 0.5 - f / 2.0, 1.0, 0.5 + f / 2.0]
    }
}

/// Fractional coverage weights of `n` equal bins over the pixel span `[x0, x1)`.
/// Returns, per bin, `(first pixel, weights...)` packed as (start index, weights range).
fn bin_weights(n: u32, x0: f64, x1: f64, npx: u32) -> (Vec<(u32, u32)>, Vec<f64>) {
    let mut spans = Vec::with_capacity(n as usize);
    let mut weights = Vec::new();
    let step = (x1 - x0) / n as f64;
    for b in 0..n {
        let a = x0 + b as f64 * step;
        let e = if b + 1 == n { x1 } else { x0 + (b + 1) as f64 * step };
        let p0 = (a.floor().max(0.0) as u32).min(npx - 1);
        let p1 = ((e.ceil() as i64).clamp(1, npx as i64)) as u32; // exclusive
        let off = weights.len() as u32;
        for p in p0..p1.max(p0 + 1) {
            let lo = a.max(p as f64);
            let hi = e.min(p as f64 + 1.0);
            weights.push((hi - lo).max(0.0));
        }
        spans.push((p0, off));
    }
    // Store the end offset implicitly via the next span's offset.
    spans.push((0, weights.len() as u32));
    (spans, weights)
}

/// Exact area mean of `img` over each of `nx × ny` cells covering `crop`.
pub fn cell_brightness(img: &LinearLuma, nx: u32, ny: u32, crop: Option<Crop>) -> Result<CellBrightness> {
    if img.w == 0 || img.h == 0 || img.px.len() != (img.w * img.h) as usize {
        return Err(Error::InvalidParams("empty or malformed image".into()));
    }
    if nx == 0 || ny == 0 {
        return Err(Error::InvalidParams("grid has no cells".into()));
    }
    let c = crop.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let (x0, x1) = (c[0] * img.w as f64, c[2] * img.w as f64);
    let (y0, y1) = (c[1] * img.h as f64, c[3] * img.h as f64);
    if !(x1 > x0 && y1 > y0) {
        return Err(Error::InvalidParams("empty crop".into()));
    }
    let (xs, xw) = bin_weights(nx, x0, x1, img.w);
    // Image rows top→bottom map to plate rows bottom→top: bin b counts from the top.
    let (ys, yw) = bin_weights(ny, y0, y1, img.h);
    let w = img.w as usize;
    // Horizontal pass: per image row in the crop, per cell column integral.
    let r0 = ys[0].0 as usize;
    let r1 = ((y1.ceil() as usize).min(img.h as usize)).max(r0 + 1);
    let mut rows = vec![0f64; (r1 - r0) * nx as usize];
    for (ri, y) in (r0..r1).enumerate() {
        let line = &img.px[y * w..(y + 1) * w];
        let out = &mut rows[ri * nx as usize..(ri + 1) * nx as usize];
        for b in 0..nx as usize {
            let (p0, off) = xs[b];
            let end = xs[b + 1].1;
            let mut s = 0.0f64;
            for (k, &wt) in xw[off as usize..end as usize].iter().enumerate() {
                let v = line[p0 as usize + k];
                let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
                s += wt * v as f64;
            }
            out[b] = s;
        }
    }
    // Vertical pass.
    let area_x = (x1 - x0) / nx as f64;
    let area_y = (y1 - y0) / ny as f64;
    let inv_area = 1.0 / (area_x * area_y);
    let mut g = vec![0f32; (nx * ny) as usize];
    for b in 0..ny as usize {
        let (p0, off) = ys[b];
        let end = ys[b + 1].1;
        let plate_row = ny as usize - 1 - b;
        for cx in 0..nx as usize {
            let mut s = 0.0f64;
            for (k, &wt) in yw[off as usize..end as usize].iter().enumerate() {
                let ri = p0 as usize + k - r0;
                s += wt * rows[ri * nx as usize + cx];
            }
            g[plate_row * nx as usize + cx] = (s * inv_area).clamp(0.0, 1.0) as f32;
        }
    }
    Ok(CellBrightness { nx, ny, g })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_image_gives_uniform_cells() {
        let img = LinearLuma { w: 7, h: 5, px: vec![0.25; 35] };
        let cb = cell_brightness(&img, 3, 4, None).unwrap();
        for g in cb.g {
            assert!((g - 0.25).abs() < 1e-6);
        }
    }

    #[test]
    fn exact_area_average_with_fractional_cells() {
        // 3 px wide, values 0, 1, 0; two cells of 1.5 px: each sees 0·1 + 1·0.5 → 1/3.
        let img = LinearLuma { w: 3, h: 1, px: vec![0.0, 1.0, 0.0] };
        let cb = cell_brightness(&img, 2, 1, None).unwrap();
        assert!((cb.g[0] - 1.0 / 3.0).abs() < 1e-6 && (cb.g[1] - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn row_zero_is_bottom_of_picture() {
        // Top row bright, bottom row dark.
        let img = LinearLuma { w: 1, h: 2, px: vec![1.0, 0.0] };
        let cb = cell_brightness(&img, 1, 2, None).unwrap();
        assert_eq!(cb.g, vec![0.0, 1.0]);
    }

    #[test]
    fn mean_is_preserved() {
        let img = synthetic_target(123, 157);
        let mean: f64 = img.px.iter().map(|&v| v as f64).sum::<f64>() / img.px.len() as f64;
        let cb = cell_brightness(&img, 10, 13, None).unwrap();
        let cm: f64 = cb.g.iter().map(|&v| v as f64).sum::<f64>() / cb.g.len() as f64;
        assert!((mean - cm).abs() < 1e-5, "{mean} vs {cm}");
    }
}
