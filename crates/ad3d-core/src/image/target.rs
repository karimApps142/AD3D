//! Per-facet targets (design §16 step 7): exact area means of linear `g` over plate
//! cells. Square cells cost four SAT lookups; any other piece (hex cells clipped by the
//! outline, §6.2) uses exact polygon coverage. A facet made of several pieces gets the
//! area-weighted mean over their union.
//!
//! Plate frame: millimetres, `u` right, `v` up, the plate is `[0, W] × [0, H]` and maps
//! onto the crop window of the working image (plate `v = 0` is the bottom of the crop).

use std::sync::Arc;

use glam::DVec2;

use super::opts::{ImageHash, ImageOpts, TargetKey, WorkingKey};
use rayon::prelude::*;

use super::sat::Sat;
use super::tone::ToneLut;
use super::working::{DEFAULT_MAX_SIDE, WorkingImage, build_working};
use super::{CellBrightness, Crop, LinearLuma};
use crate::error::{Error, Result};
use crate::relief::Partition;

/// One linear brightness `g ∈ [0, 1]` per facet, indexed by `Piece::facet`.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub g: Vec<f32>,
}

/// A toned summed-area table and the crop window it samples. Immutable; share it with
/// `Arc` across jobs.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetSampler {
    sat: Arc<Sat>,
    /// Crop rectangle `x0, y0, x1, y1` in SAT pixels (y down).
    window: [f64; 4],
}

fn check_plate(plate_mm: (f64, f64)) -> Result<()> {
    if !(plate_mm.0.is_finite() && plate_mm.1.is_finite() && plate_mm.0 > 0.0 && plate_mm.1 > 0.0) {
        return Err(Error::InvalidParams(format!("plate size {plate_mm:?} must be positive")));
    }
    Ok(())
}

/// Below this many pieces `polygon_means` stays on the calling thread.
const PAR_MIN_PIECES: usize = 512;

#[inline]
fn finish(g: f64) -> f32 {
    if g.is_finite() { g.clamp(0.0, 1.0) as f32 } else { 0.0 }
}

impl TargetSampler {
    /// Tones the working image through `lut` and builds its SAT (§16 step 6).
    pub fn new(working: &WorkingImage, lut: &ToneLut) -> Self {
        TargetSampler { sat: Arc::new(Sat::build(working.w, working.h, &working.v, |v| lut.apply(v))), window: working.window }
    }

    /// Samples a linear image directly at full resolution, with no tone or downsample
    /// (exact reference; tests and callers that already prepared `g`).
    pub fn from_linear(img: &LinearLuma, crop: Option<Crop>) -> Result<Self> {
        img.check()?;
        let c = crop.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let (w, h) = (img.w as f64, img.h as f64);
        let window = [c[0] * w, c[1] * h, c[2] * w, c[3] * h];
        if !(window[2] > window[0] && window[3] > window[1]) {
            return Err(Error::InvalidParams("empty crop".into()));
        }
        let sat = Sat::build(img.w, img.h, &img.px, |v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 });
        Ok(TargetSampler { sat: Arc::new(sat), window })
    }

    /// The same image restricted to a sub-rectangle of the current window
    /// (normalised `x0, y0, x1, y1`, y down), e.g. the layout's centre crop for a
    /// fixed plate height (§2.1). Shares the SAT, so it is cheap.
    pub fn cropped(&self, crop: Option<Crop>) -> Result<TargetSampler> {
        let Some(c) = crop else { return Ok(self.clone()) };
        if !c.iter().all(|v| v.is_finite()) || c[2] <= c[0] || c[3] <= c[1] {
            return Err(Error::InvalidParams(format!("empty crop {c:?}")));
        }
        let [x0, y0, x1, y1] = self.window;
        let (w, h) = (x1 - x0, y1 - y0);
        let window = [x0 + c[0] * w, y0 + c[1] * h, x0 + c[2] * w, y0 + c[3] * h];
        Ok(TargetSampler { sat: self.sat.clone(), window })
    }

    pub fn sat(&self) -> &Sat {
        &self.sat
    }
    pub fn window(&self) -> [f64; 4] {
        self.window
    }

    /// Plate point (mm) → SAT pixel coordinates.
    #[inline]
    pub fn plate_to_px(&self, p: DVec2, plate_mm: (f64, f64)) -> DVec2 {
        let [x0, y0, x1, y1] = self.window;
        DVec2::new(x0 + p.x / plate_mm.0 * (x1 - x0), y1 - p.y / plate_mm.1 * (y1 - y0))
    }

    /// Mean of `g` over the window (the whole plate).
    pub fn mean(&self) -> f64 {
        let [x0, y0, x1, y1] = self.window;
        self.sat.rect_mean(x0, y0, x1, y1)
    }

    /// `nx × ny` square cells over the whole window; row 0 at the plate bottom (§16 step 7,
    /// O(1) per cell).
    pub fn square_cells(&self, nx: u32, ny: u32) -> Result<CellBrightness> {
        if nx == 0 || ny == 0 {
            return Err(Error::InvalidParams("grid has no cells".into()));
        }
        let [x0, y0, x1, y1] = self.window;
        let (sx, sy) = ((x1 - x0) / nx as f64, (y1 - y0) / ny as f64);
        let inv = 1.0 / (sx * sy);
        // Integral-image values at the cell corners, one row of corners at a time.
        let xs: Vec<f64> = (0..=nx).map(|i| if i == nx { x1 } else { x0 + i as f64 * sx }).collect();
        let mut g = vec![0f32; nx as usize * ny as usize];
        let corner_row = |y: f64| xs.iter().map(|&x| self.sat.integral_to(x, y)).collect::<Vec<f64>>();
        let mut below = corner_row(y1); // plate v = 0
        for j in 0..ny as usize {
            let yt = if j + 1 == ny as usize { y0 } else { y1 - (j + 1) as f64 * sy };
            let above = corner_row(yt);
            let row = &mut g[j * nx as usize..(j + 1) * nx as usize];
            for (i, out) in row.iter_mut().enumerate() {
                let s = below[i + 1] - below[i] - above[i + 1] + above[i];
                *out = finish(s * inv);
            }
            below = above;
        }
        Ok(CellBrightness { nx, ny, g })
    }

    /// `(∬ g, area)` in pixel units over a plate polygon (mm, any vertex order).
    fn poly_px(&self, poly: &[DVec2], plate_mm: (f64, f64), buf: &mut Vec<DVec2>) -> (f64, f64) {
        // Axis-aligned rectangles (square cells, rectangular tile pieces) take the O(1) path.
        if poly.len() == 4 {
            let (a, b, c, d) = (poly[0], poly[1], poly[2], poly[3]);
            let rect = (a.y == b.y && b.x == c.x && c.y == d.y && d.x == a.x) || (a.x == b.x && b.y == c.y && c.x == d.x && d.y == a.y);
            if rect {
                let p = self.plate_to_px(a, plate_mm);
                let q = self.plate_to_px(c, plate_mm);
                let (x0, x1) = (p.x.min(q.x), p.x.max(q.x));
                let (y0, y1) = (p.y.min(q.y), p.y.max(q.y));
                let (w, h) = (self.sat.width() as f64, self.sat.height() as f64);
                let area = (x1.clamp(0.0, w) - x0.clamp(0.0, w)) * (y1.clamp(0.0, h) - y0.clamp(0.0, h));
                return (self.sat.rect_integral(x0, y0, x1, y1), area);
            }
        }
        buf.clear();
        buf.extend(poly.iter().map(|&p| self.plate_to_px(p, plate_mm)));
        let (i, a) = self.sat.polygon_integral(buf);
        (i.abs(), a.abs())
    }

    /// Area mean of `g` over one plate polygon (mm).
    pub fn polygon_mean(&self, poly: &[DVec2], plate_mm: (f64, f64)) -> f64 {
        let (i, a) = self.poly_px(poly, plate_mm, &mut Vec::new());
        i / a
    }

    /// Area-weighted means per facet over `(facet, polygon in plate mm)` pieces.
    /// `Err(InvalidParams)` if a facet in `0..n_facets` has no area on the plate.
    pub fn polygon_means<'a, I>(&self, plate_mm: (f64, f64), pieces: I, n_facets: usize) -> Result<Vec<f32>>
    where
        I: IntoIterator<Item = (u32, &'a [DVec2])>,
    {
        check_plate(plate_mm)?;
        let pieces: Vec<(u32, &[DVec2])> = pieces.into_iter().collect();
        if let Some(&(f, _)) = pieces.iter().find(|&&(f, _)| f as usize >= n_facets) {
            return Err(Error::InvalidParams(format!("piece facet {f} ≥ facet count {n_facets}")));
        }
        // Pieces are independent: integrate them in parallel, then accumulate in input
        // order so the result is bit-identical for any thread count.
        let per_piece: Vec<(f64, f64)> = if pieces.len() >= PAR_MIN_PIECES {
            pieces
                .par_iter()
                .with_min_len(256)
                .map_init(|| Vec::with_capacity(16), |buf, &(_, poly)| self.poly_px(poly, plate_mm, buf))
                .collect()
        } else {
            let mut buf = Vec::with_capacity(16);
            pieces.iter().map(|&(_, poly)| self.poly_px(poly, plate_mm, &mut buf)).collect()
        };
        let mut sum = vec![0f64; n_facets];
        let mut area = vec![0f64; n_facets];
        for (&(f, _), &(i, a)) in pieces.iter().zip(&per_piece) {
            sum[f as usize] += i;
            area[f as usize] += a;
        }
        let min_area = 1e-12 * (self.window[2] - self.window[0]) * (self.window[3] - self.window[1]);
        sum.iter()
            .zip(&area)
            .enumerate()
            .map(|(k, (&s, &a))| {
                if a > min_area { Ok(finish(s / a)) } else { Err(Error::InvalidParams(format!("facet {k} has no area on the plate"))) }
            })
            .collect()
    }

    /// Targets for every facet of a partition whose plate is `[0, W] × [0, H]`.
    pub fn facet_targets(&self, p: &Partition, plate_mm: (f64, f64)) -> Result<Target> {
        let n = p.pieces.iter().map(|pc| pc.facet as usize + 1).max().unwrap_or(0);
        let mut polys: Vec<Vec<DVec2>> = Vec::with_capacity(p.pieces.len());
        for pc in &p.pieces {
            let ring = p
                .rings
                .get(pc.ring.start as usize..pc.ring.end as usize)
                .ok_or_else(|| Error::Internal("piece ring out of range".into()))?;
            let poly = ring
                .iter()
                .map(|&v| p.verts.get(v as usize).copied().ok_or_else(|| Error::Internal("ring vertex out of range".into())))
                .collect::<Result<Vec<_>>>()?;
            polys.push(poly);
        }
        let g = self.polygon_means(plate_mm, p.pieces.iter().zip(&polys).map(|(pc, poly)| (pc.facet, poly.as_slice())), n)?;
        Ok(Target { g })
    }
}

/// Uncached §16 pipeline for one call: working copy at [`DEFAULT_MAX_SIDE`], tone,
/// SAT, then [`TargetSampler::facet_targets`].
/// `squama::prepare_target` (§12) wraps this with the layout; the app caches the
/// stages with [`TargetPipeline`].
pub fn prepare_target_for(img: &LinearLuma, opts: &ImageOpts, plate_mm: (f64, f64), p: &Partition) -> Result<Target> {
    check_plate(plate_mm)?;
    let wk = build_working(img, opts, DEFAULT_MAX_SIDE)?;
    let lut = ToneLut::new(opts, Some(&wk.hist));
    TargetSampler::new(&wk, &lut).facet_targets(p, plate_mm)
}

const CACHE_SLOTS: usize = 2;

/// Small most-recent-first cache.
#[derive(Debug)]
struct Lru<K, V>(Vec<(K, Arc<V>)>);

impl<K: PartialEq + Copy, V> Lru<K, V> {
    fn get(&mut self, k: &K) -> Option<Arc<V>> {
        let pos = self.0.iter().position(|(kk, _)| kk == k)?;
        let e = self.0.remove(pos);
        let v = e.1.clone();
        self.0.insert(0, e);
        Some(v)
    }
    fn put(&mut self, k: K, v: Arc<V>) {
        self.0.retain(|(kk, _)| kk != &k);
        self.0.insert(0, (k, v));
        self.0.truncate(CACHE_SLOTS);
    }
}

/// Caches the expensive stages for one source image: working copies by
/// [`WorkingKey`] and toned SATs by [`TargetKey`] (two slots each, so the drag level
/// and the full level both stay warm). Not thread-safe; wrap it in a `Mutex` in the app.
#[derive(Debug)]
pub struct TargetPipeline {
    hash: ImageHash,
    src: Arc<LinearLuma>,
    working: Lru<WorkingKey, WorkingImage>,
    samplers: Lru<TargetKey, TargetSampler>,
}

impl TargetPipeline {
    /// Hashes the decoded pixels.
    pub fn new(src: Arc<LinearLuma>) -> Self {
        let hash = ImageHash::of_luma(&src);
        Self::with_hash(src, hash)
    }

    /// Uses a hash the caller already has (for example [`ImageHash::of_bytes`] of the file).
    pub fn with_hash(src: Arc<LinearLuma>, hash: ImageHash) -> Self {
        TargetPipeline { hash, src, working: Lru(Vec::new()), samplers: Lru(Vec::new()) }
    }

    pub fn image_hash(&self) -> ImageHash {
        self.hash
    }
    pub fn source(&self) -> &Arc<LinearLuma> {
        &self.src
    }

    /// The working copy for `opts` at `max_side`, built on a miss.
    pub fn working(&mut self, opts: &ImageOpts, max_side: u32) -> Result<Arc<WorkingImage>> {
        opts.check()?;
        let key = opts.working_key(self.hash, max_side);
        if let Some(w) = self.working.get(&key) {
            return Ok(w);
        }
        let w = Arc::new(build_working(&self.src, opts, max_side)?);
        self.working.put(key, w.clone());
        Ok(w)
    }

    /// The toned SAT for `opts` at `max_side`, built on a miss (a tone change reuses the
    /// working copy; a crop or rotation rebuilds both).
    pub fn sampler(&mut self, opts: &ImageOpts, max_side: u32) -> Result<Arc<TargetSampler>> {
        opts.check()?;
        let key = opts.target_key(self.hash, max_side);
        if let Some(s) = self.samplers.get(&key) {
            return Ok(s);
        }
        let wk = self.working(opts, max_side)?;
        let lut = ToneLut::new(opts, Some(&wk.hist));
        let s = Arc::new(TargetSampler::new(&wk, &lut));
        self.samplers.put(key, s.clone());
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relief::square_grid;

    fn checker(w: u32, h: u32) -> LinearLuma {
        LinearLuma { w, h, px: (0..w * h).map(|k| if (k % w + k / w) % 2 == 0 { 0.9 } else { 0.1 }).collect() }
    }

    #[test]
    fn square_cells_match_legacy_cell_brightness() {
        let img = crate::image::synthetic_target(123, 157);
        let s = TargetSampler::from_linear(&img, Some([0.1, 0.05, 0.93, 0.9])).unwrap();
        let a = s.square_cells(13, 17).unwrap();
        let b = crate::image::cell_brightness(&img, 13, 17, Some([0.1, 0.05, 0.93, 0.9])).unwrap();
        for (x, y) in a.g.iter().zip(&b.g) {
            assert!((x - y).abs() < 2e-5, "{x} {y}");
        }
    }

    #[test]
    fn partition_targets_match_square_cells() {
        let img = checker(37, 23);
        let s = TargetSampler::from_linear(&img, None).unwrap();
        let p = square_grid(5, 4, 2.5, DVec2::ZERO);
        let t = s.facet_targets(&p, (12.5, 10.0)).unwrap();
        let c = s.square_cells(5, 4).unwrap();
        assert_eq!(t.g.len(), c.g.len());
        for (x, y) in t.g.iter().zip(&c.g) {
            assert!((x - y).abs() < 1e-6, "{x} {y}");
        }
    }

    #[test]
    fn rect_and_polygon_paths_agree() {
        let img = checker(37, 23);
        let s = TargetSampler::from_linear(&img, None).unwrap();
        let r = [DVec2::new(1.0, 1.0), DVec2::new(4.0, 1.0), DVec2::new(4.0, 3.5), DVec2::new(1.0, 3.5)];
        let tri_a = [r[0], r[1], r[2]];
        let tri_b = [r[0], r[2], r[3]];
        let plate = (10.0, 7.0);
        let rect = s.polygon_means(plate, [(0u32, &r[..])], 1).unwrap()[0];
        let tris = s.polygon_means(plate, [(0u32, &tri_a[..]), (0, &tri_b[..])], 1).unwrap()[0];
        assert!((rect - tris).abs() < 1e-6, "{rect} {tris}");
    }

    #[test]
    fn missing_facet_is_an_error() {
        let img = checker(4, 4);
        let s = TargetSampler::from_linear(&img, None).unwrap();
        let r = [DVec2::new(0.0, 0.0), DVec2::new(1.0, 0.0), DVec2::new(1.0, 1.0)];
        assert!(s.polygon_means((2.0, 2.0), [(0u32, &r[..])], 2).is_err());
    }

    #[test]
    fn pipeline_reuses_stages() {
        let mut pl = TargetPipeline::new(Arc::new(checker(64, 48)));
        let o = ImageOpts::default();
        let a = pl.sampler(&o, 2048).unwrap();
        let b = pl.sampler(&o, 2048).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let w0 = pl.working(&o, 2048).unwrap();
        let toned = ImageOpts { gamma: 2.0, ..o };
        let c = pl.sampler(&toned, 2048).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert!(Arc::ptr_eq(&w0, &pl.working(&toned, 2048).unwrap()));
        assert!(c.mean() > a.mean());
        // Both slots stay warm.
        assert!(Arc::ptr_eq(&a, &pl.sampler(&o, 2048).unwrap()));
    }
}
