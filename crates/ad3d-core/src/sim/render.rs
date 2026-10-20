//! Renders the simulated front view to a grayscale image (previews, tests, the
//! "predicted appearance" PNG of PROJ-SQUAMA).
//!
//! Every pixel is the mean of `supersample²` point samples. A sample takes its
//! facet's direct term when it lies in the lit part of its light line's segment
//! (exact along the line; the nearest line is used across lines, whose spacing
//! matches the sample spacing), and the shadowed (ambient) term otherwise.

use glam::DVec2;
use rayon::prelude::*;

use super::{FacetLight, LineSet, Light, Response, ResponseCurve, base_estimate, facet_lights, line_set, prepare_response, shade_lines};
use crate::error::{Error, Result};
use crate::image::LinearLuma;
use crate::relief::{Facet, Partition};

/// Render settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOpts {
    pub px_per_mm: f64,
    /// Samples per pixel along each axis.
    pub supersample: u32,
    /// Brightness shown as white. `None`: the brightest unshadowed facet.
    pub white: Option<f64>,
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts { px_per_mm: 2.0, supersample: 2, white: None }
    }
}

/// Upper bound on rendered pixels (64 MP).
pub const MAX_RENDER_PX: u64 = 64 << 20;

/// Renders the appearance of `facets` on `p` under `light` as linear luminance in
/// `[0, 1]` (`brightness / white`, clamped), row 0 at the top of the plate
/// (`v` max), column 0 at `u` min. Pixels outside the outline are 0.
pub fn render(p: &Partition, facets: &[Facet], light: &Light, resp: &Response, o: &RenderOpts) -> Result<LinearLuma> {
    let curve = prepare_response(p, light, resp, base_estimate(facets))?;
    render_curve(p, facets, light, &curve, o)
}

/// [`render`] with a prepared response (e.g. the one the solver used).
pub fn render_curve(p: &Partition, facets: &[Facet], light: &Light, resp: &ResponseCurve, o: &RenderOpts) -> Result<LinearLuma> {
    if !(o.px_per_mm.is_finite() && o.px_per_mm > 0.0) || o.supersample == 0 {
        return Err(Error::InvalidParams("render needs px_per_mm > 0 and supersample ≥ 1".into()));
    }
    let outline: Vec<DVec2> = p.outline.iter().map(|&v| p.verts[v as usize]).collect();
    let (lo, hi) = outline.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(a, b), q| {
        (a.min(*q), b.max(*q))
    });
    let w = ((hi.x - lo.x) * o.px_per_mm).ceil().max(1.0) as u64;
    let h = ((hi.y - lo.y) * o.px_per_mm).ceil().max(1.0) as u64;
    if w * h > MAX_RENDER_PX {
        return Err(Error::InvalidParams(format!("render of {w}×{h} px is too large")));
    }
    let ss = o.supersample as f64;
    let step = 1.0 / (o.px_per_mm * ss);
    // Lines at the sample spacing.
    let cell = super::lines::mean_cell(p);
    let lpc = (cell / step).ceil().max(1.0) as u32;
    let ls = line_set(p, facets, light, lpc)?;
    let centres: Vec<(DVec2, f64)> = facets.iter().map(|f| (f.pivot, f.h)).collect();
    let lights = facet_lights(p, &centres, light)?;
    let sh = shade_lines(&ls, facets, &lights, resp);
    let direct: Vec<f64> = (0..ls.n_facets()).map(|f| direct_of(&lights[f], &facets[f], resp)).collect();
    let shadowed = resp.shadowed();
    let white = o.white.unwrap_or_else(|| shadowed + direct.iter().copied().fold(0.0, f64::max));
    let inv_white = if white > 0.0 { 1.0 / white } else { 0.0 };
    let (w, h) = (w as usize, h as usize);
    let px: Vec<f32> = (0..h)
        .into_par_iter()
        .flat_map_iter(|row| {
            let ls = &ls;
            let lit_start = &sh.lit_start;
            let direct = &direct;
            (0..w).map(move |col| {
                let (mut sum, mut cnt) = (0.0, 0u32);
                for sy in 0..o.supersample {
                    for sx in 0..o.supersample {
                        let q = DVec2::new(
                            lo.x + (col as f64 + (sx as f64 + 0.5) / ss) / o.px_per_mm,
                            hi.y - (row as f64 + (sy as f64 + 0.5) / ss) / o.px_per_mm,
                        );
                        if let Some(v) = sample(ls, lit_start, direct, shadowed, q) {
                            sum += v;
                        }
                        cnt += 1;
                    }
                }
                ((sum / cnt as f64) * inv_white).clamp(0.0, 1.0) as f32
            })
        })
        .collect();
    Ok(LinearLuma { w: w as u32, h: h as u32, px })
}

fn direct_of(fl: &FacetLight, f: &Facet, resp: &ResponseCurve) -> f64 {
    resp.direct(fl.cos_facet(f), fl.irr)
}

fn sample(ls: &LineSet, lit_start: &[f64], direct: &[f64], shadowed: f64, q: DVec2) -> Option<f64> {
    let k = ls.line_at(q)?;
    let line = &ls.lines[k];
    let segs = &ls.segs[line.segs.start as usize..line.segs.end as usize];
    let s = ls.param_on(k, q);
    let i = segs.partition_point(|g| g.s0 <= s);
    if i == 0 {
        return None;
    }
    let g = &segs[i - 1];
    if s > g.s1 {
        return None;
    }
    let gi = line.segs.start as usize + i - 1;
    let lit = s >= lit_start[gi];
    Some(if lit { shadowed + direct[g.facet as usize] } else { shadowed })
}

/// Encodes linear luminance to 8-bit sRGB gray (for PNG previews).
pub fn to_gray8_srgb(img: &LinearLuma) -> image::GrayImage {
    let data = img
        .px
        .iter()
        .map(|&v| {
            let v = v.clamp(0.0, 1.0);
            let e = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (e * 255.0 + 0.5) as u8
        })
        .collect();
    image::GrayImage::from_raw(img.w, img.h, data).expect("buffer size matches")
}
