//! Image → target pipeline (design §16): property tests against independent oracles
//! and golden values.

use std::f64::consts::PI;
use std::io::Cursor;

use ad3d_core::image::sat::{Sat, clip_to_rect};
use ad3d_core::image::tone::{ToneLut, tone_v};
use ad3d_core::image::{
    ImageOpts, LinearLuma, TargetSampler, WorkingImage, build_working, decode_bytes, linear_to_srgb_f64,
    srgb_to_linear, srgb_to_linear_f64, synthetic_target,
};
use glam::DVec2;
use proptest::prelude::*;

// ---------------------------------------------------------------- helpers

fn shoelace(p: &[DVec2]) -> f64 {
    (0..p.len()).map(|k| p[k].perp_dot(p[(k + 1) % p.len()])).sum::<f64>() * 0.5
}

/// Independent exact oracle: clip the polygon to every pixel square it touches.
fn brute_polygon(w: u32, h: u32, px: &[f32], poly: &[DVec2]) -> (f64, f64) {
    let (mut s, mut a) = (0.0, 0.0);
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in poly {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let x0 = lo.x.floor().max(0.0) as u32;
    let y0 = lo.y.floor().max(0.0) as u32;
    let x1 = (hi.x.ceil().max(0.0) as u32).min(w);
    let y1 = (hi.y.ceil().max(0.0) as u32).min(h);
    for j in y0..y1 {
        for i in x0..x1 {
            let c = clip_to_rect(poly, i as f64, j as f64, i as f64 + 1.0, j as f64 + 1.0);
            if c.len() >= 3 {
                let ar = shoelace(&c).abs();
                s += ar * px[(j * w + i) as usize] as f64;
                a += ar;
            }
        }
    }
    (s, a)
}

/// Pointy-top hex cells of width `cell` across flats on the §6.2 phase, clipped to the
/// snapped plate `[0, m·cell/2] × [0, n·p]`. Flat-top swaps u and v.
fn hex_pieces(cell: f64, m: u32, n: u32, flat: bool) -> ((f64, f64), Vec<Vec<DVec2>>) {
    let r = cell / 3f64.sqrt();
    let pitch = 1.5 * r;
    let (w, h) = (m as f64 * cell / 2.0, n as f64 * pitch);
    let mut out = Vec::new();
    for j in -1..=(n as i32 + 1) {
        for i in -1..=((m / 2) as i32 + 2) {
            let c = DVec2::new((i as f64 + 0.5 * (j.rem_euclid(2)) as f64) * cell, j as f64 * pitch);
            let hex: Vec<DVec2> =
                (0..6).map(|k| c + r * DVec2::from_angle(PI / 6.0 + k as f64 * PI / 3.0)).collect();
            let cl = clip_to_rect(&hex, 0.0, 0.0, w, h);
            if cl.len() >= 3 && shoelace(&cl).abs() > 1e-9 {
                out.push(if flat { cl.iter().rev().map(|p| DVec2::new(p.y, p.x)).collect() } else { cl });
            }
        }
    }
    (if flat { (h, w) } else { (w, h) }, out)
}

fn rand_img(w: u32, h: u32, seed: u64) -> LinearLuma {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let px = (0..w * h)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 40) as f32) / (1u64 << 24) as f32
        })
        .collect();
    LinearLuma { w, h, px }
}

fn sampler_identity(img: &LinearLuma, opts: &ImageOpts) -> TargetSampler {
    let wk = build_working(img, opts, 4096).unwrap();
    TargetSampler::new(&wk, &ToneLut::new(opts, Some(&wk.hist)))
}

// ---------------------------------------------------------------- properties

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// Exact polygon coverage equals the per-pixel clipping oracle for random convex
    /// polygons (random hexagons, any rotation, partly outside the image).
    #[test]
    fn polygon_coverage_matches_oracle(
        w in 3u32..40, h in 3u32..40, seed in any::<u64>(),
        cx in -2.0f64..42.0, cy in -2.0f64..42.0, r in 0.2f64..15.0, rot in 0.0f64..PI, squash in 0.3f64..1.0,
    ) {
        let img = rand_img(w, h, seed);
        let sat = Sat::build(w, h, &img.px, |v| v);
        let poly: Vec<DVec2> = (0..6)
            .map(|k| {
                let a = rot + k as f64 * PI / 3.0;
                DVec2::new(cx + r * a.cos(), cy + squash * r * a.sin())
            })
            .collect();
        let (i, a) = sat.polygon_integral(&poly);
        let (bi, ba) = brute_polygon(w, h, &img.px, &poly);
        prop_assert!((a.abs() - ba).abs() < 1e-9 * (1.0 + ba), "area {a} vs {ba}");
        prop_assert!((i.abs() - bi).abs() < 1e-9 * (1.0 + bi), "integral {i} vs {bi}");
    }

    /// Square cells preserve the window mean, and every cell lies in the image's range.
    #[test]
    fn square_cells_preserve_mean(
        w in 2u32..60, h in 2u32..60, seed in any::<u64>(), nx in 1u32..30, ny in 1u32..30,
        c0 in 0.0f64..0.4, c1 in 0.0f64..0.4, c2 in 0.6f64..1.0, c3 in 0.6f64..1.0,
    ) {
        let img = rand_img(w, h, seed);
        let s = TargetSampler::from_linear(&img, Some([c0, c1, c2, c3])).unwrap();
        let cb = s.square_cells(nx, ny).unwrap();
        let m: f64 = cb.g.iter().map(|&v| v as f64).sum::<f64>() / cb.g.len() as f64;
        prop_assert!((m - s.mean()).abs() < 1e-5, "{m} vs {}", s.mean());
        let (lo, hi) = img.px.iter().fold((1f32, 0f32), |(a, b), &v| (a.min(v), b.max(v)));
        prop_assert!(cb.g.iter().all(|&v| v >= lo - 1e-5 && v <= hi + 1e-5));
    }

    /// Clipped hex cells tile the plate: their area-weighted mean is the plate mean, a
    /// constant image gives that constant everywhere, and both orientations work.
    #[test]
    fn hex_cells_tile_the_plate(
        w in 8u32..80, h in 8u32..80, seed in any::<u64>(), cell in 1.0f64..12.0,
        m in 2u32..24, n in 1u32..16, flat in any::<bool>(),
    ) {
        let (plate, pieces) = hex_pieces(cell, m, n, flat);
        let img = rand_img(w, h, seed);
        let s = TargetSampler::from_linear(&img, None).unwrap();
        let g = s.polygon_means(plate, pieces.iter().enumerate().map(|(k, p)| (k as u32, p.as_slice())), pieces.len()).unwrap();
        let areas: Vec<f64> = pieces.iter().map(|p| shoelace(p).abs()).collect();
        let total: f64 = areas.iter().sum();
        prop_assert!((total - plate.0 * plate.1).abs() < 1e-6 * total, "pieces cover {total} of {}", plate.0 * plate.1);
        let wm: f64 = g.iter().zip(&areas).map(|(&v, &a)| v as f64 * a).sum::<f64>() / total;
        prop_assert!((wm - s.mean()).abs() < 1e-5, "{wm} vs {}", s.mean());
        let flat_img = LinearLuma { w, h, px: vec![0.37; (w * h) as usize] };
        let sf = TargetSampler::from_linear(&flat_img, None).unwrap();
        let gf = sf.polygon_means(plate, pieces.iter().enumerate().map(|(k, p)| (k as u32, p.as_slice())), pieces.len()).unwrap();
        prop_assert!(gf.iter().all(|&v| (v - 0.37).abs() < 1e-5));
    }

    /// Rotating the picture permutes the cells: rotated cell (i', j') is source cell
    /// (nx − 1 − j', i'); mirroring maps (i, j) to (nx − 1 − i, j).
    #[test]
    fn rotation_and_mirror_permute_cells(fx in 1u32..6, fy in 1u32..6, nx in 1u32..8, ny in 1u32..8, seed in any::<u64>()) {
        let (w, h) = (nx * fx, ny * fy);
        let img = rand_img(w, h, seed);
        let src = sampler_identity(&img, &ImageOpts::default()).square_cells(nx, ny).unwrap();
        let rot = sampler_identity(&img, &ImageOpts { rotate_deg: 90, ..Default::default() }).square_cells(ny, nx).unwrap();
        for jr in 0..nx {
            for ir in 0..ny {
                let a = rot.at(ir, jr);
                let b = src.at(nx - 1 - jr, ir);
                prop_assert!((a - b).abs() < 1e-5, "({ir},{jr}) {a} vs {b}");
            }
        }
        let mir = sampler_identity(&img, &ImageOpts { mirror: true, ..Default::default() }).square_cells(nx, ny).unwrap();
        for j in 0..ny {
            for i in 0..nx {
                prop_assert!((mir.at(i, j) - src.at(nx - 1 - i, j)).abs() < 1e-5);
            }
        }
        let r4 = sampler_identity(&img, &ImageOpts { rotate_deg: 360, ..Default::default() }).square_cells(nx, ny).unwrap();
        prop_assert_eq!(&r4.g, &src.g);
    }

    /// A rotated crop selects the same pixels as cropping the pre-rotated image.
    #[test]
    fn crop_applies_in_rotated_frame(seed in any::<u64>(), turns in 0i32..4, mirror in any::<bool>(),
                                     x0 in 0.0f64..0.45, y0 in 0.0f64..0.45, x1 in 0.55f64..1.0, y1 in 0.55f64..1.0) {
        let img = rand_img(17, 11, seed);
        let geo = ImageOpts { rotate_deg: 90 * turns, mirror, ..Default::default() };
        // Materialise the rotated/mirrored picture, then crop it without any rotation.
        let wk = build_working(&img, &geo, 4096).unwrap();
        let lin = LinearLuma { w: wk.w, h: wk.h, px: wk.v.iter().map(|&v| ToneLut::identity().apply(v)).collect() };
        let crop = [x0, y0, x1, y1];
        let a = sampler_identity(&img, &ImageOpts { crop: Some(crop), ..geo }).square_cells(5, 4).unwrap();
        let b = TargetSampler::from_linear(&lin, Some(crop)).unwrap().square_cells(5, 4).unwrap();
        for (p, q) in a.g.iter().zip(&b.g) {
            prop_assert!((p - q).abs() < 1e-5, "{p} vs {q}");
        }
    }

    /// The tone curve is monotone (non-decreasing, or non-increasing when inverted) and
    /// stays in [0, 1] for every valid option set.
    #[test]
    fn tone_is_monotone(invert in any::<bool>(), gamma in 0.2f64..5.0, contrast in 0.0f64..3.0,
                        brightness in -1.0f64..1.0, lv in proptest::option::of((0.0f64..0.4, 0.6f64..1.0))) {
        let o = ImageOpts { invert, gamma, contrast, brightness, ..Default::default() };
        let mut prev = if invert { f64::INFINITY } else { f64::NEG_INFINITY };
        for k in 0..=1000 {
            let v = tone_v(k as f64 / 1000.0, &o, lv);
            prop_assert!((0.0..=1.0).contains(&v));
            if invert { prop_assert!(v <= prev + 1e-12) } else { prop_assert!(v >= prev - 1e-12) }
            prev = v;
        }
        let lut = ToneLut::new(&o, None);
        let monotone = lut.table().windows(2).all(|p| if invert { p[1] <= p[0] + 1e-7 } else { p[1] >= p[0] - 1e-7 });
        prop_assert!(monotone, "LUT not monotone");
    }

    /// The working-resolution downsample keeps the crop mean (in linear light).
    #[test]
    fn downsample_preserves_mean(w in 40u32..300, h in 40u32..300, max_side in 8u32..64, seed in any::<u64>()) {
        let img = rand_img(w, h, seed);
        let full = TargetSampler::from_linear(&img, None).unwrap().mean();
        let wk = build_working(&img, &ImageOpts::default(), max_side).unwrap();
        prop_assert!(wk.w.max(wk.h) <= max_side);
        let small = TargetSampler::new(&wk, &ToneLut::identity()).mean();
        prop_assert!((full - small).abs() < 2e-4, "{full} vs {small}");
    }
}

// ---------------------------------------------------------------- fixed and golden cases

#[test]
fn auto_levels_stretch_to_full_range() {
    // v spans [0.3, 0.6]; (0, 1) percentiles stretch it to [0, 1].
    let v: Vec<f32> = (0..1000).map(|k| 0.3 + 0.3 * k as f32 / 999.0).collect();
    let wk = WorkingImage::from_v(1000, 1, v).unwrap();
    let o = ImageOpts { auto_levels: Some([0.0, 1.0]), ..Default::default() };
    let lut = ToneLut::new(&o, Some(&wk.hist));
    assert!(lut.apply(0.3) < 1e-3, "{}", lut.apply(0.3));
    assert!(lut.apply(0.6) > 0.995, "{}", lut.apply(0.6));
    let inv = ImageOpts { invert: true, ..o };
    let lut = ToneLut::new(&inv, Some(&wk.hist));
    assert!(lut.apply(0.6) < 1e-3 && lut.apply(0.3) > 0.995);
}

/// Golden: hand-computed tone composite at one point.
#[test]
fn golden_tone_composite() {
    // v = 0.5 → +0.1 = 0.6 → contrast 1.5: 0.65 → gamma 2.2: 0.65^(1/2.2) → sRGB decode.
    let o = ImageOpts { brightness: 0.1, contrast: 1.5, gamma: 2.2, ..Default::default() };
    let v = tone_v(0.5, &o, None);
    assert!((v - 0.822_168_089).abs() < 1e-8, "{v}");
    let g = srgb_to_linear_f64(v);
    assert!((g - 0.642_085_518).abs() < 1e-8, "{g}");
    let lut = ToneLut::new(&o, None);
    assert!((lut.apply(0.5) as f64 - g).abs() < 1e-5);
}

/// Golden: §16's motivating number. A mid-grey code value of 128 is linear 0.2159, not
/// 0.502 (the prototypes' `pixel/255`).
#[test]
fn golden_mid_grey_is_linear() {
    let l = srgb_to_linear(128.0 / 255.0);
    assert!((l - 0.215_860_5).abs() < 1e-6);
    assert!((linear_to_srgb_f64(l as f64) - 128.0 / 255.0).abs() < 1e-6);
}

/// Golden: the synthetic target at the LuBan defaults (50 × 63 cells). Cells wholly in
/// the disc read code value 230, wholly in the dark square 40, wholly in the empty band
/// 0; the whole-plate mean is pinned.
#[test]
fn golden_synthetic_cells() {
    let img = synthetic_target(500, 630);
    let s = sampler_identity(&img, &ImageOpts::default());
    let cb = s.square_cells(50, 63).unwrap();
    let disc = srgb_to_linear(230.0 / 255.0);
    let dark = srgb_to_linear(40.0 / 255.0);
    // Cell (i, j): pixels x 10i..10i+10, y from the top 630 − 10(j+1)..630 − 10j.
    assert!((cb.at(25, 63 - 1 - 30) - disc).abs() < 2e-5, "disc {}", cb.at(25, 32));
    assert!((cb.at(24, 63 - 1 - 40) - dark).abs() < 2e-5, "dark {}", cb.at(24, 22));
    assert!(cb.at(2, 63 - 1 - 30).abs() < 1e-6, "empty {}", cb.at(2, 32));
    // Top band: brightness rises left to right.
    assert!((0..49).all(|i| cb.at(i + 1, 62) > cb.at(i, 62)));
    let mean = s.mean();
    assert!((mean - GOLDEN_SYNTH_MEAN).abs() < 1e-5, "mean {mean}");
}
/// Independently computed in Python from the target's definition (sRGB LUT, 500 × 630).
const GOLDEN_SYNTH_MEAN: f64 = 0.246_122_45;

/// Golden: hex targets on the synthetic image agree with the same regions sampled by the
/// rectangle path where a hex lies wholly inside a uniform region.
#[test]
fn golden_hex_on_synthetic() {
    let img = synthetic_target(500, 630);
    let s = TargetSampler::from_linear(&img, None).unwrap();
    let (plate, pieces) = hex_pieces(6.0, 100, 70, false);
    let g = s.polygon_means(plate, pieces.iter().enumerate().map(|(k, p)| (k as u32, p.as_slice())), pieces.len()).unwrap();
    let hex_at = |px: f64, py: f64| {
        // Picture pixel (px, py) → plate point; the hex whose centre is within R of it.
        let at = DVec2::new(px / 500.0 * plate.0, (1.0 - py / 630.0) * plate.1);
        let k = pieces
            .iter()
            .position(|p| p.len() == 6 && (p.iter().copied().sum::<DVec2>() / 6.0 - at).length() < 3.5)
            .unwrap();
        g[k]
    };
    // Inside the dark square (centre of the disc), inside the disc above the square,
    // and in the empty band left of the disc.
    let dark = srgb_to_linear(40.0 / 255.0);
    let disc = srgb_to_linear(230.0 / 255.0);
    assert!((hex_at(250.0, 410.0) - dark).abs() < 2e-5, "{}", hex_at(250.0, 410.0));
    assert!((hex_at(250.0, 300.0) - disc).abs() < 2e-5, "{}", hex_at(250.0, 300.0));
    assert!(hex_at(40.0, 400.0).abs() < 1e-6, "{}", hex_at(40.0, 400.0));
}

// ---------------------------------------------------------------- decode with EXIF

/// Inserts an APP1 EXIF segment with the given orientation right after the JPEG SOI.
fn with_exif_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes()); // one IFD entry
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

#[test]
fn exif_orientation_is_applied() {
    // 32 × 16: left half black, right half white.
    let img = image::GrayImage::from_fn(32, 16, |x, _| image::Luma([if x < 16 { 0 } else { 255 }]));
    let mut jpeg = Vec::new();
    image::DynamicImage::ImageLuma8(img).write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg).unwrap();
    let plain = decode_bytes(&jpeg).unwrap();
    assert_eq!((plain.w, plain.h), (32, 16));
    // Orientation 6 = rotate 90° clockwise to display: the left (black) half goes to the top.
    let rot = decode_bytes(&with_exif_orientation(&jpeg, 6)).unwrap();
    assert_eq!((rot.w, rot.h), (16, 32));
    let top: f32 = rot.px[..16 * 8].iter().sum::<f32>() / 128.0;
    let bottom: f32 = rot.px[16 * 24..].iter().sum::<f32>() / 128.0;
    assert!(top < 0.05 && bottom > 0.9, "top {top} bottom {bottom}");
    // Orientation 2 = mirror: black moves to the right.
    let mir = decode_bytes(&with_exif_orientation(&jpeg, 2)).unwrap();
    assert_eq!((mir.w, mir.h), (32, 16));
    assert!(mir.px[0] > 0.9 && mir.px[31] < 0.05);
}

#[test]
fn all_enabled_formats_decode() {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(9, 7, |x, y| image::Rgb([(x * 28) as u8, (y * 36) as u8, 77])));
    for fmt in [image::ImageFormat::Png, image::ImageFormat::Tiff, image::ImageFormat::Bmp, image::ImageFormat::WebP] {
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), fmt).unwrap();
        let l = decode_bytes(&buf).unwrap_or_else(|e| panic!("{fmt:?}: {e}"));
        assert_eq!((l.w, l.h), (9, 7), "{fmt:?}");
        let want = 0.2126 * srgb_to_linear(28.0 * 3.0 / 255.0)
            + 0.7152 * srgb_to_linear(36.0 * 2.0 / 255.0)
            + 0.0722 * srgb_to_linear(77.0 / 255.0);
        assert!((l.px[2 * 9 + 3] - want).abs() < 1e-5, "{fmt:?}: {} vs {want}", l.px[2 * 9 + 3]);
    }
}
