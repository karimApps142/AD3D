//! Plate layouts (design §2.1, §6.2): square cells with LuBan snapping, and hex
//! cells (pointy- or flat-top) with the §6.2 lattice phase and size snap, so a
//! clipped hex is never a sliver. Also the partition builder (§6, §14.1 cut lines)
//! and per-facet geometry (pivot = full-cell centre, full-cell corners).

use glam::DVec2;
use serde::Serialize;

use super::SquamaParams;
use crate::error::{Error, Result};
use crate::image::{Crop, LinearLuma, cell_brightness, centre_crop};
use crate::relief::partition::{HexLattice, hex_grid};
use crate::relief::{Partition, Tol, square_grid};

/// Hex orientation (§6.2). `Pointy`: rows run along `u` (the default). `Flat`: the
/// same construction with `u` and `v` swapped, so rows run along `v`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum HexOrientation {
    #[default]
    Pointy,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum LayoutKind {
    #[default]
    Square,
    Hex { orientation: HexOrientation },
}

/// Snapped plate geometry (§2.1, §6.2).
///
/// - Square: `nx × ny` cells of side `cell_mm`; facet `j·nx + i`.
/// - Hex: `nx = m` (plate extent across the flats in half-cells, `≥ 2`) and
///   `ny = n` (extent along the rows in row pitches, `≥ 1`); see
///   [`HexLattice`]. For pointy-top `W = m·cell/2`, `H = n·p`; for flat-top
///   `W = n·p`, `H = m·cell/2` (`p = (√3/2)·cell`). Facets are ordered row by row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Layout {
    pub kind: LayoutKind,
    pub w_mm: f64,
    pub h_mm: f64,
    pub cell_mm: f64,
    pub nx: u32,
    pub ny: u32,
    /// Image crop used to fill the plate (normalised, y down), when cropping.
    #[serde(skip)]
    pub crop: Option<Crop>,
}

impl Layout {
    /// The hex lattice of a hex layout.
    pub fn hex_lattice(&self) -> Option<HexLattice> {
        match self.kind {
            LayoutKind::Square => None,
            LayoutKind::Hex { orientation } => Some(HexLattice {
                m: self.nx,
                n: self.ny,
                cell: self.cell_mm,
                flat: orientation == HexOrientation::Flat,
            }),
        }
    }

    /// Number of facets (= cells with positive area inside the plate).
    pub fn facet_count(&self) -> u32 {
        match self.hex_lattice() {
            None => self.nx * self.ny,
            Some(h) => h.facet_count(),
        }
    }

    /// Full-cell centre of facet `f`: its pivot (§4). For a hex clipped by the
    /// plate edge this is the full hexagon's centre, never the piece centroid.
    pub fn facet_centre(&self, f: u32) -> DVec2 {
        match self.hex_lattice() {
            None => {
                let (i, j) = (f % self.nx, f / self.nx);
                DVec2::new((i as f64 + 0.5) * self.cell_mm, (j as f64 + 0.5) * self.cell_mm)
            }
            Some(h) => h.centre(f),
        }
    }

    /// The facet whose cell contains plate point `p` (mm), or `None` outside the
    /// plate. Hex cells are the Voronoi cells of their centres.
    pub fn facet_at(&self, p: DVec2) -> Option<u32> {
        if !(p.x >= 0.0 && p.y >= 0.0 && p.x <= self.w_mm && p.y <= self.h_mm) {
            return None;
        }
        match self.hex_lattice() {
            None => {
                let i = ((p.x / self.cell_mm) as u32).min(self.nx - 1);
                let j = ((p.y / self.cell_mm) as u32).min(self.ny - 1);
                Some(j * self.nx + i)
            }
            Some(h) => {
                // Lattice coordinates: s across the flats, t along the rows.
                let (s, t) = if h.flat { (p.y, p.x) } else { (p.x, p.y) };
                let (su, pitch) = (h.s_unit(), h.pitch());
                let j0 = (t / pitch).floor() as i64;
                let mut best: Option<(f64, u32)> = None;
                for j in j0..=j0 + 1 {
                    if j < 0 || j > h.n as i64 {
                        continue;
                    }
                    let par = j.rem_euclid(2);
                    let k = (((s / su) - par as f64) / 2.0).round() as i64;
                    for uc in [2 * k - 2 + par, 2 * k + par, 2 * k + 2 + par] {
                        if uc < 0 || uc > h.m as i64 {
                            continue;
                        }
                        let (cs, ct) = (uc as f64 * su, j as f64 * pitch);
                        let d = (cs - s).powi(2) + (ct - t).powi(2);
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, h.facet_of(j as u32, uc as u32)));
                        }
                    }
                }
                best.map(|b| b.1)
            }
        }
    }

    /// Full-cell corner offsets from the centre (CCW), the same for every facet.
    pub fn corner_offsets(&self) -> Vec<DVec2> {
        match self.hex_lattice() {
            None => {
                let c = 0.5 * self.cell_mm;
                vec![DVec2::new(-c, -c), DVec2::new(c, -c), DVec2::new(c, c), DVec2::new(-c, c)]
            }
            Some(h) => h.corner_offsets().to_vec(),
        }
    }

    /// Largest `|a · (corner − centre)|` over a full cell for the in-plane unit
    /// direction `a` (§4 `e_max` for one facet).
    pub fn e_max_dir(&self, a: DVec2) -> f64 {
        match self.kind {
            // Same expression as [`e_max`] so square plates stay bit-identical.
            LayoutKind::Square => 0.5 * self.cell_mm * (a.x.abs() + a.y.abs()),
            LayoutKind::Hex { .. } => self.corner_offsets().iter().map(|o| a.dot(*o).abs()).fold(0.0, f64::max),
        }
    }
}

pub(crate) fn to_um(x: f64) -> i64 {
    (x * 1000.0).round() as i64
}

/// Half-up integer snap `max(1, (len + cell/2) / cell)`.
pub(crate) fn snap(len_um: i64, cell_um: i64) -> u32 {
    ((len_um + cell_um / 2) / cell_um).max(1) as u32
}

/// Half-up rounding of a positive real, at least `min`.
fn round_half_up(x: f64, min: u32) -> u32 {
    let r = (x + 0.5).floor();
    if r.is_finite() && r > min as f64 { r.min(u32::MAX as f64) as u32 } else { min }
}

/// `e_max` for square cells: the largest `|a · (corner − centre)|`.
pub fn e_max(cell: f64, azimuth: f64) -> f64 {
    0.5 * cell * (azimuth.cos().abs() + azimuth.sin().abs())
}

/// `e_max` for any cell kind (full-cell corners, in-plane light azimuth).
pub fn cell_e_max(kind: LayoutKind, cell: f64, azimuth: f64) -> f64 {
    let a = DVec2::new(azimuth.cos(), azimuth.sin());
    match kind {
        LayoutKind::Square => e_max(cell, azimuth),
        LayoutKind::Hex { orientation } => {
            let lat = HexLattice { m: 2, n: 1, cell, flat: orientation == HexOrientation::Flat };
            lat.corner_offsets().iter().map(|o| a.dot(*o).abs()).fold(0.0, f64::max)
        }
    }
}

/// Smallest cell size the layout supports: `10ε` for squares (§6.1 split); for
/// hexes the shortest partition edge `R/2 = cell/(2√3)` must be at least `4δ`.
pub fn min_cell_mm(kind: LayoutKind, tol: &Tol) -> f64 {
    match kind {
        LayoutKind::Square => 10.0 * tol.eps,
        LayoutKind::Hex { .. } => 8.0 * 3f64.sqrt() * tol.delta,
    }
}

/// §2.1: snapped square layout from params and the image size (or crop size) in pixels.
pub fn layout(p: &SquamaParams, img_px: (u32, u32)) -> Result<Layout> {
    layout_for(p, LayoutKind::Square, img_px)
}

/// §2.1 / §6.2: snapped layout of any kind. Square snapping reproduces LuBan
/// (200 @ 6 → 198, 202 @ 6 → 204). Hex snapping keeps a hex centre at the origin
/// and puts the outline on centre lines (`W = m·cell/2`, `H = n·p`, axes swapped
/// for flat-top; an explicit hex height snaps down). With `height_mm` the image is centre-cropped to fill; otherwise
/// the height follows the image aspect from the **snapped** width.
pub fn layout_for(p: &SquamaParams, kind: LayoutKind, img_px: (u32, u32)) -> Result<Layout> {
    let cell_um = to_um(p.cell_mm);
    if cell_um <= 0 || !p.width_mm.is_finite() || p.height_mm.is_some_and(|h| !h.is_finite()) {
        return Err(Error::InvalidParams("cell must be positive and sizes finite".into()));
    }
    let cell = cell_um as f64 / 1000.0;
    let (cw, ch) = (img_px.0.max(1), img_px.1.max(1));
    let l = match kind {
        LayoutKind::Square => {
            let nx = snap(to_um(p.width_mm), cell_um);
            let (ny, crop) = match p.height_mm {
                Some(h) => {
                    let ny = snap(to_um(h), cell_um);
                    (ny, Some(centre_crop(cw, ch, nx as f64, ny as f64)))
                }
                None => {
                    let (cw, ch) = (cw as u64, ch as u64);
                    (((2 * nx as u64 * ch + cw) / (2 * cw)).max(1) as u32, None)
                }
            };
            Layout { kind, w_mm: nx as f64 * cell, h_mm: ny as f64 * cell, cell_mm: cell, nx, ny, crop }
        }
        LayoutKind::Hex { orientation } => {
            let flat = orientation == HexOrientation::Flat;
            let pitch = HexLattice { m: 2, n: 1, cell, flat }.pitch();
            // Across-flats extent in half-cells: integer µm, half-up (§2.1).
            let half_cells = |len_um: i64| ((2 * len_um + cell_um / 2) / cell_um).max(2) as u32;
            let rows = |len_mm: f64| round_half_up(len_mm / pitch, 1);
            // An explicit height is a limit (e.g. a bed or a frame): snap it down, so a
            // plate asked for at 378 mm is never 379.3 mm.
            let half_cells_down = |len_um: i64| ((2 * len_um) / cell_um).max(2) as u32;
            let rows_down = |len_mm: f64| ((len_mm / pitch + 1e-9).floor() as u32).max(1);
            let (m, n) = if !flat {
                let m = half_cells(to_um(p.width_mm));
                let w = m as f64 * cell / 2.0;
                let n = match p.height_mm {
                    Some(h) => rows_down(h),
                    None => rows(w * ch as f64 / cw as f64),
                };
                (m, n)
            } else {
                let n = rows(p.width_mm);
                let w = n as f64 * pitch;
                let m = match p.height_mm {
                    Some(h) => half_cells_down(to_um(h)),
                    None => half_cells(to_um(w * ch as f64 / cw as f64)),
                };
                (m, n)
            };
            let lat = HexLattice { m, n, cell, flat };
            let (w, h) = lat.size_mm();
            let crop = p.height_mm.map(|_| centre_crop(cw, ch, w, h));
            Layout { kind, w_mm: w, h_mm: h, cell_mm: cell, nx: m, ny: n, crop }
        }
    };
    // Like M0, the 2048 mm limit is enforced by the generators, not here, so the UI
    // can still show the snapped size it would refuse.
    Ok(l)
}

/// A straight tile cut across the whole plate (§14.1), in plate millimetres.
/// Square: on a cell line. Hex: on a §6.2 centre line (`k·cell/2` across the
/// flats, `j·p` along the rows).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum TileCut {
    /// Vertical line `u = x`.
    U(f64),
    /// Horizontal line `v = y`.
    V(f64),
}

/// Lattice index of `x` in units of `unit`, if it lies within 1 µm of a line.
fn on_line(x: f64, unit: f64) -> Option<i64> {
    let k = (x / unit).round();
    if k.is_finite() && (k * unit - x).abs() <= 1e-3 { Some(k as i64) } else { None }
}

/// §6 / §14.1: the plate partition, with the cut lines built in (so §5.1
/// clustering runs once over the whole plate). Square plates need no extra
/// vertices (cuts lie on cell lines); hex plates split every hex a cut crosses
/// into pieces that share its facet.
pub fn build_partition(l: &Layout, cuts: &[TileCut]) -> Result<Partition> {
    let tol = Tol::default();
    if l.cell_mm < min_cell_mm(l.kind, &tol) - 1e-12 {
        return Err(Error::InvalidParams(format!(
            "cell {} mm below {:.4} mm for this layout",
            l.cell_mm,
            min_cell_mm(l.kind, &tol)
        )));
    }
    match l.hex_lattice() {
        None => {
            for c in cuts {
                let (x, len, n) = match *c {
                    TileCut::U(x) => (x, l.w_mm, l.nx),
                    TileCut::V(y) => (y, l.h_mm, l.ny),
                };
                match on_line(x, l.cell_mm) {
                    Some(k) if k > 0 && k < n as i64 => {}
                    _ => return Err(Error::InvalidParams(format!("cut at {x} mm is not an interior cell line of {len} mm"))),
                }
            }
            Ok(square_grid(l.nx, l.ny, l.cell_mm, DVec2::ZERO))
        }
        Some(lat) => {
            let (mut cs, mut ct) = (Vec::new(), Vec::new());
            for c in cuts {
                // Plate axis → lattice axis (s across the flats, t along the rows).
                let (x, along_s) = match (*c, lat.flat) {
                    (TileCut::U(x), false) | (TileCut::V(x), true) => (x, true),
                    (TileCut::V(y), false) | (TileCut::U(y), true) => (y, false),
                };
                let k = if along_s { on_line(x, lat.s_unit()) } else { on_line(x, lat.pitch()).map(|j| 3 * j) };
                match k {
                    Some(k) if along_s => cs.push(k),
                    Some(k) => ct.push(k),
                    None => return Err(Error::InvalidParams(format!("cut at {x} mm is not on a hex centre line"))),
                }
            }
            hex_grid(&lat, &cs, &ct)
        }
    }
}

/// Per-facet linear brightness from an image, area-weighted over each facet's
/// region inside the plate (§6.2, §16 step 7). Square layouts use the exact
/// square-cell average; hex layouts average the pixels whose centres fall inside
/// the full hex ∩ plate (the nearest pixel when none do).
///
/// The app and exports use `pipeline::prepare_target` (§16: tone, SAT, exact
/// polygon coverage); this simpler sampler stays for tests and benches.
pub fn facet_brightness(img: &LinearLuma, l: &Layout) -> Result<Vec<f32>> {
    let Some(lat) = l.hex_lattice() else {
        return cell_brightness(img, l.nx, l.ny, l.crop).map(|cb| cb.g);
    };
    if img.w == 0 || img.h == 0 || img.px.len() != (img.w * img.h) as usize {
        return Err(Error::InvalidParams("empty or malformed image".into()));
    }
    let c = l.crop.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let (x0, x1) = (c[0] * img.w as f64, c[2] * img.w as f64);
    let (y0, y1) = (c[1] * img.h as f64, c[3] * img.h as f64);
    if !(x1 > x0 && y1 > y0) {
        return Err(Error::InvalidParams("empty crop".into()));
    }
    let (sx, sy) = ((x1 - x0) / l.w_mm, (y1 - y0) / l.h_mm); // px per mm
    let offs = lat.corner_offsets();
    let pix = |ix: i64, iy: i64| -> f64 {
        let ix = ix.clamp(0, img.w as i64 - 1) as usize;
        let iy = iy.clamp(0, img.h as i64 - 1) as usize;
        let v = img.px[iy * img.w as usize + ix];
        if v.is_finite() { v.clamp(0.0, 1.0) as f64 } else { 0.0 }
    };
    let mut out = Vec::with_capacity(lat.facet_count() as usize);
    for f in 0..lat.facet_count() {
        let ctr = lat.centre(f);
        let (mut lo, mut hi) = (ctr, ctr);
        for o in &offs {
            lo = lo.min(ctr + *o);
            hi = hi.max(ctr + *o);
        }
        lo = lo.max(DVec2::ZERO);
        hi = hi.min(DVec2::new(l.w_mm, l.h_mm));
        // Pixel index ranges whose centres may fall in [lo, hi].
        let ix0 = ((x0 + lo.x * sx) - 0.5).ceil() as i64;
        let ix1 = ((x0 + hi.x * sx) - 0.5).floor() as i64;
        let iy0 = ((y1 - hi.y * sy) - 0.5).ceil() as i64;
        let iy1 = ((y1 - lo.y * sy) - 0.5).floor() as i64;
        let (mut s, mut cnt) = (0.0f64, 0u32);
        for iy in iy0.max(0)..=iy1.min(img.h as i64 - 1) {
            let v = (y1 - (iy as f64 + 0.5)) / sy;
            for ix in ix0.max(0)..=ix1.min(img.w as i64 - 1) {
                let u = ((ix as f64 + 0.5) - x0) / sx;
                let q = DVec2::new(u, v);
                if u < 0.0 || v < 0.0 || u > l.w_mm || v > l.h_mm {
                    continue;
                }
                let inside = (0..6).all(|k| {
                    let (a, b) = (ctr + offs[k], ctr + offs[(k + 1) % 6]);
                    (b - a).perp_dot(q - a) >= 0.0
                });
                if inside {
                    s += pix(ix, iy);
                    cnt += 1;
                }
            }
        }
        let g = if cnt > 0 {
            s / cnt as f64
        } else {
            let q = ctr.clamp(DVec2::ZERO, DVec2::new(l.w_mm, l.h_mm));
            pix((x0 + q.x * sx).floor() as i64, (y1 - q.y * sy).floor() as i64)
        };
        out.push(g.clamp(0.0, 1.0) as f32);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(o: HexOrientation) -> LayoutKind {
        LayoutKind::Hex { orientation: o }
    }

    #[test]
    fn square_snapping_unchanged() {
        let p = SquamaParams { width_mm: 200.0, ..Default::default() };
        assert_eq!(layout(&p, (1, 1)).unwrap().w_mm, 198.0);
        let p = SquamaParams { width_mm: 100.8, cell_mm: 3.2, ..Default::default() };
        assert_eq!(layout(&p, (1, 1)).unwrap().nx, 32);
    }

    #[test]
    fn hex_snap_pointy() {
        // 100 mm at cell 6: 2W/cell = 33.33 → m = 33, W = 99.
        let p = SquamaParams { width_mm: 100.0, cell_mm: 6.0, height_mm: Some(50.0), ..Default::default() };
        let l = layout_for(&p, hex(HexOrientation::Pointy), (10, 10)).unwrap();
        assert_eq!(l.nx, 33);
        assert!((l.w_mm - 99.0).abs() < 1e-12);
        let pitch = 3f64.sqrt() / 2.0 * 6.0; // 5.196
        // An explicit height snaps down (never past the request): 50 / 5.196 = 9.6 → 9.
        assert_eq!(l.ny, (50.0 / pitch).floor() as u32); // 9
        assert!((l.h_mm - l.ny as f64 * pitch).abs() < 1e-9 && l.h_mm <= 50.0);
        // Decimal tie in µm, half-up: 2·100.8/3.2 = 63 exactly; 2·100.8/6.4 = 31.5 → 32.
        let p = SquamaParams { width_mm: 100.8, cell_mm: 6.4, ..Default::default() };
        assert_eq!(layout_for(&p, hex(HexOrientation::Pointy), (1, 1)).unwrap().nx, 32);
        // Tiny plates clamp to m = 2, n = 1.
        let p = SquamaParams { width_mm: 1.0, cell_mm: 6.0, height_mm: Some(1.0), ..Default::default() };
        let l = layout_for(&p, hex(HexOrientation::Pointy), (1, 1)).unwrap();
        assert_eq!((l.nx, l.ny), (2, 1));
    }

    #[test]
    fn hex_snap_from_aspect_and_flat() {
        let p = SquamaParams { width_mm: 300.0, cell_mm: 6.0, ..Default::default() };
        let l = layout_for(&p, hex(HexOrientation::Pointy), (500, 630)).unwrap();
        assert_eq!(l.nx, 100);
        let pitch = 3f64.sqrt() / 2.0 * 6.0;
        assert_eq!(l.ny, (300.0 * 630.0 / 500.0 / pitch + 0.5).floor() as u32);
        let f = layout_for(&p, hex(HexOrientation::Flat), (500, 630)).unwrap();
        assert_eq!(f.ny, (300.0 / pitch + 0.5).floor() as u32);
        assert!((f.w_mm - f.ny as f64 * pitch).abs() < 1e-9);
        assert!((f.h_mm - f.nx as f64 * 3.0).abs() < 1e-9);
        assert!((f.h_mm / f.w_mm - 630.0 / 500.0).abs() < 0.03);
    }

    #[test]
    fn facet_geometry() {
        let p = SquamaParams { width_mm: 30.0, cell_mm: 6.0, height_mm: Some(20.0), ..Default::default() };
        for o in [HexOrientation::Pointy, HexOrientation::Flat] {
            let l = layout_for(&p, hex(o), (1, 1)).unwrap();
            let part = build_partition(&l, &[]).unwrap();
            let used: std::collections::BTreeSet<u32> = part.pieces.iter().map(|pc| pc.facet).collect();
            assert_eq!(used.len() as u32, l.facet_count());
            assert_eq!(l.facet_centre(0), DVec2::ZERO);
            // e_max of a hex: R for a light along a corner direction, cell/2 across flats.
            let r = 6.0 / 3f64.sqrt();
            let along_flats = if o == HexOrientation::Pointy { DVec2::X } else { DVec2::Y };
            assert!((l.e_max_dir(along_flats) - 3.0).abs() < 1e-12);
            assert!((l.e_max_dir(along_flats.perp()) - r).abs() < 1e-12);
        }
    }

    #[test]
    fn cut_validation() {
        let p = SquamaParams { width_mm: 60.0, cell_mm: 6.0, height_mm: Some(60.0), ..Default::default() };
        let l = layout(&p, (1, 1)).unwrap();
        assert!(build_partition(&l, &[TileCut::U(30.0), TileCut::V(24.0)]).is_ok());
        assert!(build_partition(&l, &[TileCut::U(31.0)]).is_err());
        assert!(build_partition(&l, &[TileCut::U(60.0)]).is_err());
        let h = layout_for(&p, hex(HexOrientation::Pointy), (1, 1)).unwrap();
        let lat = h.hex_lattice().unwrap();
        assert!(build_partition(&h, &[TileCut::U(9.0), TileCut::V(2.0 * lat.pitch())]).is_ok());
        assert!(build_partition(&h, &[TileCut::U(10.0)]).is_err());
        assert!(build_partition(&h, &[TileCut::V(lat.pitch() * 0.5)]).is_err());
    }

    #[test]
    fn hex_min_cell() {
        let tol = Tol::default();
        let p = SquamaParams { width_mm: 10.0, cell_mm: 0.6, height_mm: Some(10.0), ..Default::default() };
        let l = layout_for(&p, hex(HexOrientation::Pointy), (1, 1)).unwrap();
        assert!(build_partition(&l, &[]).is_err());
        let p = SquamaParams { cell_mm: 0.7, ..p };
        let l = layout_for(&p, hex(HexOrientation::Pointy), (1, 1)).unwrap();
        build_partition(&l, &[]).unwrap().check(&tol).unwrap();
    }

    #[test]
    fn hex_brightness_uniform_and_gradient() {
        let p = SquamaParams { width_mm: 60.0, cell_mm: 6.0, height_mm: Some(40.0), ..Default::default() };
        let l = layout_for(&p, hex(HexOrientation::Pointy), (300, 200)).unwrap();
        let img = LinearLuma { w: 300, h: 200, px: vec![0.3; 300 * 200] };
        let g = facet_brightness(&img, &l).unwrap();
        assert_eq!(g.len() as u32, l.facet_count());
        assert!(g.iter().all(|&x| (x - 0.3).abs() < 1e-6));
        // Left-to-right ramp: facet brightness follows the facet centre.
        let px: Vec<f32> = (0..200 * 300).map(|k| (k % 300) as f32 / 299.0).collect();
        let img = LinearLuma { w: 300, h: 200, px };
        let g = facet_brightness(&img, &l).unwrap();
        let (a, b) = (1, 2); // row 0, centres at u = 6 and u = 12
        assert!(g[b] > g[a], "ramp not increasing: {} {}", g[a], g[b]);
    }
}
