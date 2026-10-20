//! Tone operations (design §16 step 5) on the perceptual value `v = srgb_encode(Y)`:
//! invert → auto-levels → brightness → contrast → gamma, then `g = srgb_decode(v)`.
//! Every step clamps to `[0, 1]`. The composite is tabulated in a 4096-entry LUT over
//! `v` and read with linear interpolation.

use super::opts::ImageOpts;
use super::working::HIST_BINS;
use super::{linear_to_srgb_f64, srgb_to_linear_f64};

pub const LUT_SIZE: usize = 4096;

/// `v → g` (perceptual in, linear out).
#[derive(Debug, Clone, PartialEq)]
pub struct ToneLut {
    lut: Vec<f32>,
}

/// The `p`-quantile of a histogram over `[0, 1]`, interpolated inside the bin.
pub fn quantile(hist: &[u32], p: f64) -> f64 {
    let total: u64 = hist.iter().map(|&c| c as u64).sum();
    if total == 0 {
        return p.clamp(0.0, 1.0);
    }
    let want = p.clamp(0.0, 1.0) * total as f64;
    let n = hist.len() as f64;
    let mut cum = 0f64;
    for (b, &c) in hist.iter().enumerate() {
        let next = cum + c as f64;
        if c > 0 && next >= want {
            let f = ((want - cum) / c as f64).clamp(0.0, 1.0);
            return (b as f64 + f) / n;
        }
        cum = next;
    }
    1.0
}

/// Auto-levels black and white points `(lo, hi)` in the (possibly inverted) `v`
/// domain, from the histogram of the un-inverted `v`. `None` when the picture is too
/// flat to stretch (`hi − lo` within about one histogram bin).
pub fn levels(hist: &[u32], invert: bool, pct: [f64; 2]) -> Option<(f64, f64)> {
    let (lo, hi) = if invert {
        // u = 1 − v: the p-quantile of u is 1 − the (1 − p)-quantile of v.
        (1.0 - quantile(hist, 1.0 - pct[0]), 1.0 - quantile(hist, 1.0 - pct[1]))
    } else {
        (quantile(hist, pct[0]), quantile(hist, pct[1]))
    };
    (hi - lo > 1.5 / HIST_BINS as f64).then_some((lo, hi))
}

/// The tone curve on one perceptual value (exact, no LUT). `lv` comes from [`levels`].
pub fn tone_v(v: f64, o: &ImageOpts, lv: Option<(f64, f64)>) -> f64 {
    let mut v = v.clamp(0.0, 1.0);
    if o.invert {
        v = 1.0 - v;
    }
    if let Some((lo, hi)) = lv {
        v = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    }
    v = (v + o.brightness).clamp(0.0, 1.0);
    v = ((v - 0.5) * o.contrast + 0.5).clamp(0.0, 1.0);
    if o.gamma != 1.0 {
        v = v.powf(1.0 / o.gamma);
    }
    v.clamp(0.0, 1.0)
}

impl ToneLut {
    /// Builds the LUT for `opts`; `hist` (the working image's) is needed only when
    /// auto-levels is on.
    pub fn new(opts: &ImageOpts, hist: Option<&[u32]>) -> Self {
        let lv = match (opts.auto_levels, hist) {
            (Some(p), Some(h)) => levels(h, opts.invert, p),
            _ => None,
        };
        let lut = (0..LUT_SIZE)
            .map(|i| srgb_to_linear_f64(tone_v(i as f64 / (LUT_SIZE - 1) as f64, opts, lv)) as f32)
            .collect();
        ToneLut { lut }
    }

    /// Identity tone: `g = srgb_decode(v)`.
    pub fn identity() -> Self {
        Self::new(&ImageOpts::default(), None)
    }

    /// `g` for a perceptual `v` (clamped), interpolated.
    #[inline]
    pub fn apply(&self, v: f32) -> f32 {
        let x = v.clamp(0.0, 1.0) * (LUT_SIZE - 1) as f32;
        let i = (x as usize).min(LUT_SIZE - 2);
        let f = x - i as f32;
        self.lut[i] + f * (self.lut[i + 1] - self.lut[i])
    }

    pub fn table(&self) -> &[f32] {
        &self.lut
    }
}

/// `srgb_encode` exposed for callers that tone a linear value directly.
pub fn tone_linear(y: f64, o: &ImageOpts, lv: Option<(f64, f64)>) -> f64 {
    srgb_to_linear_f64(tone_v(linear_to_srgb_f64(y), o, lv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_lut_decodes_srgb() {
        let l = ToneLut::identity();
        for k in 0..=100 {
            let v = k as f32 / 100.0;
            let want = srgb_to_linear_f64(v as f64) as f32;
            assert!((l.apply(v) - want).abs() < 2e-6, "{v}: {} vs {want}", l.apply(v));
        }
        assert_eq!(l.apply(0.0), 0.0);
        assert_eq!(l.apply(1.0), 1.0);
    }

    #[test]
    fn invert_and_contrast() {
        let o = ImageOpts { invert: true, ..Default::default() };
        assert!((tone_v(0.2, &o, None) - 0.8).abs() < 1e-12);
        let o = ImageOpts { contrast: 2.0, ..Default::default() };
        assert_eq!(tone_v(0.5, &o, None), 0.5);
        assert!((tone_v(0.6, &o, None) - 0.7).abs() < 1e-12);
        assert_eq!(tone_v(0.9, &o, None), 1.0);
        let o = ImageOpts { contrast: 0.0, ..Default::default() };
        assert_eq!(tone_v(0.1, &o, None), 0.5);
    }

    #[test]
    fn brightness_and_gamma() {
        let o = ImageOpts { brightness: 0.25, ..Default::default() };
        assert_eq!(tone_v(0.5, &o, None), 0.75);
        let o = ImageOpts { gamma: 2.0, ..Default::default() };
        assert!((tone_v(0.25, &o, None) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn quantiles_and_levels() {
        let mut h = vec![0u32; HIST_BINS];
        h[1024] = 50; // v ≈ 0.25
        h[3072] = 50; // v ≈ 0.75
        let (lo, hi) = levels(&h, false, [0.0, 1.0]).unwrap();
        assert!((lo - 0.25).abs() < 1e-3 && (hi - 0.75).abs() < 1e-3, "{lo} {hi}");
        let (lo, hi) = levels(&h, true, [0.0, 1.0]).unwrap();
        assert!((lo - 0.25).abs() < 1e-3 && (hi - 0.75).abs() < 1e-3, "{lo} {hi}");
        let mut flat = vec![0u32; HIST_BINS];
        flat[2000] = 1;
        assert!(levels(&flat, false, [0.0, 1.0]).is_none());
    }
}
