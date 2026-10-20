//! `imageOpts` of the §12.1 DTO, their validation and the cache keys of §12/§16.
//!
//! Order of operations (fixed, independent of field order):
//! geometry: rotate (clockwise quarter turns) → mirror (left↔right) → crop (normalised
//! rectangle in the rotated-and-mirrored picture) → working-resolution downsample →
//! unsharp mask; tone: invert → auto-levels → brightness → contrast → gamma.

use serde::{Deserialize, Serialize};

use super::LinearLuma;
use crate::error::{Error, Result};

/// Unsharp mask on the perceptual working image: `v += amount·(v − gauss_σ(v))`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unsharp {
    /// 0–5; 0.5–1.5 is typical.
    pub amount: f64,
    /// Gaussian σ in working-image pixels, 0.3–50.
    pub radius_px: f64,
}

/// Image preparation options. Every field is optional in JSON (camelCase); missing
/// fields take the identity defaults, so `{}` changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageOpts {
    /// Normalised `x0, y0, x1, y1` (y down) in the rotated and mirrored picture. The
    /// caller snaps it to whole cells (§2.1, §6.2). `None` = the whole picture.
    pub crop: Option<[f64; 4]>,
    /// Clockwise rotation in degrees; a multiple of 90 (any sign). The app's DTO
    /// calls it `rotate`.
    #[serde(alias = "rotate")]
    pub rotate_deg: i32,
    /// Mirror left↔right (after rotating).
    pub mirror: bool,
    pub invert: bool,
    /// 0.2–5, default 1; `v ← v^(1/γ)`.
    pub gamma: f64,
    /// 0–3, default 1, pivot 0.5.
    pub contrast: f64,
    /// −1…1, default 0, added to `v`.
    pub brightness: f64,
    /// Percentile stretch `(lo, hi)` of `v`, e.g. `(0.01, 0.99)`; `None` = off.
    pub auto_levels: Option<[f64; 2]>,
    pub sharpen: Option<Unsharp>,
}

impl Default for ImageOpts {
    fn default() -> Self {
        ImageOpts {
            crop: None,
            rotate_deg: 0,
            mirror: false,
            invert: false,
            gamma: 1.0,
            contrast: 1.0,
            brightness: 0.0,
            auto_levels: None,
            sharpen: None,
        }
    }
}

/// The auto-levels percentiles §16 suggests when the user turns it on.
pub const DEFAULT_AUTO_LEVELS: [f64; 2] = [0.01, 0.99];

fn bad(m: impl Into<String>) -> Error {
    Error::InvalidParams(m.into())
}

impl ImageOpts {
    /// Range checks (`Error::InvalidParams`).
    pub fn check(&self) -> Result<()> {
        if let Some(c) = self.crop {
            if !c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) || c[2] <= c[0] || c[3] <= c[1] {
                return Err(bad(format!("crop {c:?} must be 0 ≤ x0 < x1 ≤ 1, 0 ≤ y0 < y1 ≤ 1")));
            }
        }
        if self.rotate_deg % 90 != 0 {
            return Err(bad(format!("rotation {}° is not a multiple of 90°", self.rotate_deg)));
        }
        if !(self.gamma.is_finite() && (0.2..=5.0).contains(&self.gamma)) {
            return Err(bad(format!("gamma {} outside 0.2–5", self.gamma)));
        }
        if !(self.contrast.is_finite() && (0.0..=3.0).contains(&self.contrast)) {
            return Err(bad(format!("contrast {} outside 0–3", self.contrast)));
        }
        if !(self.brightness.is_finite() && (-1.0..=1.0).contains(&self.brightness)) {
            return Err(bad(format!("brightness {} outside −1…1", self.brightness)));
        }
        if let Some([lo, hi]) = self.auto_levels {
            if !(lo.is_finite() && hi.is_finite() && 0.0 <= lo && lo < hi && hi <= 1.0) {
                return Err(bad(format!("auto-levels ({lo}, {hi}) must be 0 ≤ lo < hi ≤ 1")));
            }
        }
        if let Some(s) = self.sharpen {
            if !(s.amount.is_finite() && (0.0..=5.0).contains(&s.amount)) {
                return Err(bad(format!("sharpen amount {} outside 0–5", s.amount)));
            }
            if !(s.radius_px.is_finite() && (0.3..=50.0).contains(&s.radius_px)) {
                return Err(bad(format!("sharpen radius {} px outside 0.3–50", s.radius_px)));
            }
        }
        Ok(())
    }

    /// `true` when every option is the identity default.
    pub fn is_default(&self) -> bool {
        *self == ImageOpts::default()
    }

    /// Clockwise quarter turns, 0–3.
    pub fn quarter_turns(&self) -> u8 {
        (self.rotate_deg / 90).rem_euclid(4) as u8
    }

    /// The crop, or the whole picture.
    pub fn crop_or_full(&self) -> [f64; 4] {
        self.crop.unwrap_or([0.0, 0.0, 1.0, 1.0])
    }

    /// Aspect `(w, h)` of the cropped, rotated picture of a `w × h` source, in pixels
    /// (fractional), for the layout's height snap (§2.1).
    pub fn cropped_size_px(&self, src_w: u32, src_h: u32) -> (f64, f64) {
        let (w, h) = if self.quarter_turns() % 2 == 1 { (src_h, src_w) } else { (src_w, src_h) };
        let c = self.crop_or_full();
        ((c[2] - c[0]) * w as f64, (c[3] - c[1]) * h as f64)
    }

    /// Cache key of the working copy (geometry + sharpen + resolution).
    pub fn working_key(&self, image: ImageHash, max_side: u32) -> WorkingKey {
        WorkingKey {
            image,
            crop: self.crop.map(|c| c.map(f64::to_bits)),
            quarter_turns: self.quarter_turns(),
            mirror: self.mirror,
            sharpen: self.sharpen.map(|s| [s.amount.to_bits(), s.radius_px.to_bits()]),
            max_side,
        }
    }

    /// Cache key of the tone stage.
    pub fn tone_key(&self) -> ToneKey {
        ToneKey {
            invert: self.invert,
            gamma: self.gamma.to_bits(),
            contrast: self.contrast.to_bits(),
            brightness: self.brightness.to_bits(),
            auto_levels: self.auto_levels.map(|a| a.map(f64::to_bits)),
        }
    }

    /// Cache key of the summed-area table (§12: image hash + `ImageOpts`).
    pub fn target_key(&self, image: ImageHash, max_side: u32) -> TargetKey {
        TargetKey { working: self.working_key(image, max_side), tone: self.tone_key() }
    }
}

/// Content hash of a source image, stable across runs and platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ImageHash(pub u64);

const K0: u64 = 0x9E37_79B9_7F4A_7C15;
const K1: u64 = 0xBF58_476D_1CE4_E5B9;
const K2: u64 = 0x94D0_49BB_1331_11EB;

#[inline]
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(K1);
    z = (z ^ (z >> 27)).wrapping_mul(K2);
    z ^ (z >> 31)
}

fn hash_bytes(seed: u64, bytes: &[u8]) -> u64 {
    // Four independent lanes keep the loop off a single multiply chain.
    let mut lanes = [seed ^ K0, seed.rotate_left(17) ^ K1, seed.rotate_left(31) ^ K2, !seed];
    let mut chunks = bytes.chunks_exact(32);
    for c in &mut chunks {
        for (l, w) in lanes.iter_mut().zip(c.chunks_exact(8)) {
            let v = u64::from_le_bytes(w.try_into().unwrap());
            *l = (*l ^ mix(v)).rotate_left(29).wrapping_mul(K0);
        }
    }
    let mut h = mix(lanes[0]) ^ mix(lanes[1]).rotate_left(16) ^ mix(lanes[2]).rotate_left(32) ^ mix(lanes[3]).rotate_left(48);
    for (k, &b) in chunks.remainder().iter().enumerate() {
        h = mix(h ^ ((b as u64) << ((k % 8) * 8)) ^ k as u64);
    }
    mix(h ^ bytes.len() as u64)
}

impl ImageHash {
    /// Hash of encoded file bytes (cheapest: the app has them before decoding).
    pub fn of_bytes(bytes: &[u8]) -> Self {
        ImageHash(hash_bytes(0x41_44_33_44, bytes))
    }
    /// Hash of decoded pixels and size.
    pub fn of_luma(img: &LinearLuma) -> Self {
        let seed = mix(((img.w as u64) << 32) | img.h as u64);
        ImageHash(hash_bytes(seed, bytemuck::cast_slice(&img.px)))
    }
}

/// Cache key of a [`WorkingImage`](super::WorkingImage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkingKey {
    pub image: ImageHash,
    pub crop: Option<[u64; 4]>,
    pub quarter_turns: u8,
    pub mirror: bool,
    pub sharpen: Option<[u64; 2]>,
    pub max_side: u32,
}

/// Cache key of the tone stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToneKey {
    pub invert: bool,
    pub gamma: u64,
    pub contrast: u64,
    pub brightness: u64,
    pub auto_levels: Option<[u64; 2]>,
}

/// Cache key of a [`TargetSampler`](super::TargetSampler) (its summed-area table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetKey {
    pub working: WorkingKey,
    pub tone: ToneKey,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_json_is_identity() {
        let o: ImageOpts = serde_json::from_str("{}").unwrap();
        assert_eq!(o, ImageOpts::default());
        o.check().unwrap();
    }

    #[test]
    fn dto_fields_parse() {
        let o: ImageOpts = serde_json::from_str(
            r#"{"crop":[0.1,0.2,0.9,0.8],"gamma":2.2,"contrast":1.5,"brightness":-0.1,"invert":true,
                "autoLevels":[0.01,0.99],"rotateDeg":-90,"mirror":true,"sharpen":{"amount":1,"radiusPx":2}}"#,
        )
        .unwrap();
        o.check().unwrap();
        assert_eq!(o.quarter_turns(), 3);
        assert_eq!(o.auto_levels, Some([0.01, 0.99]));
        let n: ImageOpts = serde_json::from_str(r#"{"autoLevels":null}"#).unwrap();
        assert_eq!(n.auto_levels, None);
    }

    #[test]
    fn checks_reject_out_of_range() {
        let bad = [
            ImageOpts { crop: Some([0.5, 0.0, 0.5, 1.0]), ..Default::default() },
            ImageOpts { crop: Some([0.0, 0.0, 1.1, 1.0]), ..Default::default() },
            ImageOpts { rotate_deg: 45, ..Default::default() },
            ImageOpts { gamma: 0.1, ..Default::default() },
            ImageOpts { contrast: f64::NAN, ..Default::default() },
            ImageOpts { brightness: 1.5, ..Default::default() },
            ImageOpts { auto_levels: Some([0.6, 0.4]), ..Default::default() },
            ImageOpts { sharpen: Some(Unsharp { amount: 1.0, radius_px: 0.0 }), ..Default::default() },
        ];
        for o in bad {
            assert!(matches!(o.check(), Err(Error::InvalidParams(_))), "{o:?}");
        }
    }

    #[test]
    fn keys_separate_geometry_from_tone() {
        let h = ImageHash(7);
        let a = ImageOpts::default();
        let b = ImageOpts { gamma: 2.0, ..a };
        assert_eq!(a.working_key(h, 2048), b.working_key(h, 2048));
        assert_ne!(a.target_key(h, 2048), b.target_key(h, 2048));
        let c = ImageOpts { rotate_deg: 360, ..a };
        assert_eq!(a.working_key(h, 2048), c.working_key(h, 2048));
        assert_ne!(a.working_key(h, 2048), a.working_key(h, 1024));
    }

    #[test]
    fn hash_is_content_sensitive_and_stable() {
        let a = LinearLuma { w: 3, h: 1, px: vec![0.1, 0.2, 0.3] };
        let mut b = a.clone();
        assert_eq!(ImageHash::of_luma(&a), ImageHash::of_luma(&b));
        b.px[2] = 0.30001;
        assert_ne!(ImageHash::of_luma(&a), ImageHash::of_luma(&b));
        let c = LinearLuma { w: 1, h: 3, px: a.px.clone() };
        assert_ne!(ImageHash::of_luma(&a), ImageHash::of_luma(&c));
        assert_ne!(ImageHash::of_bytes(b"abc"), ImageHash::of_bytes(b"abd"));
        assert_ne!(ImageHash::of_bytes(&[0u8; 40]), ImageHash::of_bytes(&[0u8; 41]));
    }

    #[test]
    fn cropped_size_follows_rotation() {
        let o = ImageOpts { rotate_deg: 90, crop: Some([0.0, 0.0, 0.5, 1.0]), ..Default::default() };
        assert_eq!(o.cropped_size_px(400, 300), (150.0, 400.0));
    }
}
