//! Small PNG thumbnails for project files and 3MF packages.
//!
//! [`render_relief_png`] draws a relief mesh straight down its `+w` axis, lit by
//! the design light (Lambert, no cast shadows), so the thumbnail looks like the
//! plate on the wall. [`image_png`] is a plain downscale of the input picture.

use crate::error::{Error, Result};
use crate::image::LinearLuma;
use crate::mesh::Mesh;

fn srgb_encode(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let e = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (e * 255.0).round() as u8
}

fn encode_png(img: &image::DynamicImage) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| Error::Io(format!("png: {e}")))?;
    Ok(out.into_inner())
}

fn fit(w: f64, h: f64, max_px: u32) -> (u32, u32, f64) {
    let s = max_px as f64 / w.max(h).max(1e-9);
    (((w * s).round() as u32).max(1), ((h * s).round() as u32).max(1), s)
}

/// Renders `m` (plate space: `u` right, `v` up, `w` out of the face) seen from
/// the front, lit from azimuth/elevation (radians), into a grey + alpha PNG
/// whose long side is `max_px`.
pub fn render_relief_png(m: &Mesh, light_azimuth: f64, light_elevation: f64, max_px: u32) -> Result<Vec<u8>> {
    let b = m.bounds().ok_or_else(|| Error::InvalidParams("thumbnail of an empty mesh".into()))?;
    let max_px = max_px.clamp(8, 2048);
    let (w, h) = ((b.max[0] - b.min[0]) as f64, (b.max[1] - b.min[1]) as f64);
    let (pw, ph, s) = fit(w, h, max_px);
    let l = [light_elevation.cos() * light_azimuth.cos(), light_elevation.cos() * light_azimuth.sin(), light_elevation.sin()];
    let n = (pw * ph) as usize;
    let mut zbuf = vec![f64::NEG_INFINITY; n];
    let mut shade = vec![0f64; n];
    let to_px = |p: [f32; 3]| [(p[0] - b.min[0]) as f64 * s, (b.max[1] - p[1]) as f64 * s, p[2] as f64];
    for t in 0..m.triangle_count() {
        let [a, bb, c] = m.triangle(t).map(|i| m.vertex(i));
        let e1 = [(bb[0] - a[0]) as f64, (bb[1] - a[1]) as f64, (bb[2] - a[2]) as f64];
        let e2 = [(c[0] - a[0]) as f64, (c[1] - a[1]) as f64, (c[2] - a[2]) as f64];
        let nn = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        let len = (nn[0] * nn[0] + nn[1] * nn[1] + nn[2] * nn[2]).sqrt();
        if len == 0.0 || nn[2] <= 0.0 {
            continue; // back face, wall seen edge-on, or degenerate
        }
        let lum = ((nn[0] * l[0] + nn[1] * l[1] + nn[2] * l[2]) / len).max(0.0);
        let (p0, p1, p2) = (to_px(a), to_px(bb), to_px(c));
        let x0 = p0[0].min(p1[0]).min(p2[0]).floor().max(0.0) as u32;
        let x1 = (p0[0].max(p1[0]).max(p2[0]).ceil() as u32).min(pw);
        let y0 = p0[1].min(p1[1]).min(p2[1]).floor().max(0.0) as u32;
        let y1 = (p0[1].max(p1[1]).max(p2[1]).ceil() as u32).min(ph);
        let area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p2[0] - p0[0]) * (p1[1] - p0[1]);
        if area.abs() < 1e-12 {
            continue;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let (qx, qy) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((p1[0] - qx) * (p2[1] - qy) - (p2[0] - qx) * (p1[1] - qy)) / area;
                let w1 = ((p2[0] - qx) * (p0[1] - qy) - (p0[0] - qx) * (p2[1] - qy)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * p0[2] + w1 * p1[2] + w2 * p2[2];
                let k = (y * pw + x) as usize;
                if z > zbuf[k] {
                    zbuf[k] = z;
                    shade[k] = lum;
                }
            }
        }
    }
    let peak = shade.iter().cloned().fold(0.0, f64::max).max(1e-9);
    let mut img = image::GrayAlphaImage::new(pw, ph);
    for (k, px) in img.pixels_mut().enumerate() {
        if zbuf[k].is_finite() {
            *px = image::LumaA([srgb_encode(shade[k] / peak), 255]);
        } else {
            *px = image::LumaA([0, 0]);
        }
    }
    encode_png(&image::DynamicImage::ImageLumaA8(img))
}

/// Box-filtered downscale of a linear-luminance picture to a grey PNG.
pub fn image_png(img: &LinearLuma, max_px: u32) -> Result<Vec<u8>> {
    if img.w == 0 || img.h == 0 || img.px.len() != (img.w * img.h) as usize {
        return Err(Error::InvalidParams("thumbnail of an empty image".into()));
    }
    let (pw, ph, _) = fit(img.w as f64, img.h as f64, max_px.clamp(8, 2048).min(img.w.max(img.h)));
    let mut out = image::GrayImage::new(pw, ph);
    for y in 0..ph {
        let sy0 = (y as u64 * img.h as u64 / ph as u64) as u32;
        let sy1 = (((y + 1) as u64 * img.h as u64).div_ceil(ph as u64) as u32).max(sy0 + 1).min(img.h);
        for x in 0..pw {
            let sx0 = (x as u64 * img.w as u64 / pw as u64) as u32;
            let sx1 = (((x + 1) as u64 * img.w as u64).div_ceil(pw as u64) as u32).max(sx0 + 1).min(img.w);
            let mut s = 0.0f64;
            for yy in sy0..sy1 {
                for xx in sx0..sx1 {
                    let v = img.px[(yy * img.w + xx) as usize];
                    s += if v.is_finite() { v as f64 } else { 0.0 };
                }
            }
            let n = ((sy1 - sy0) * (sx1 - sx0)) as f64;
            out.put_pixel(x, y, image::Luma([srgb_encode(s / n)]));
        }
    }
    encode_png(&image::DynamicImage::ImageLuma8(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_thumbnail_is_a_png_of_the_right_size() {
        let img = crate::image::synthetic_target(500, 630);
        let png = image_png(&img, 128).unwrap();
        let d = image::load_from_memory(&png).unwrap();
        assert_eq!((d.width(), d.height()), (102, 128));
    }

    #[test]
    fn relief_thumbnail_renders() {
        let m = crate::synth::torus(2000);
        let png = render_relief_png(&m, std::f64::consts::PI, 0.3, 64).unwrap();
        let d = image::load_from_memory(&png).unwrap();
        assert!(d.width() == 64 || d.height() == 64);
        assert!(render_relief_png(&Mesh::default(), 0.0, 0.3, 64).is_err());
    }
}
