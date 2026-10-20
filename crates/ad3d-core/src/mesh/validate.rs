//! `validate()` (design §10): three stages, each running the full check list.
//!
//! 1. **raw**: the index buffer as generated, no weld (what the viewport and 3MF see).
//!    Positions are the stored `f32` values widened to `f64`.
//! 2. **written**: the transformed `f32` values written to STL/3MF, bit-identical
//!    positions welded (`-0.0 == 0.0`). Must merge 0 vertices for a generated mesh.
//! 3. **slicer**: CuraEngine-style load (integer µm, merge within 30 µm of an earlier
//!    vertex in the same hash bucket, drop faces with repeated indices). Must merge 0.
//!
//! Checks per stage: degenerate triangles (repeated index or zero `f64` area), every
//! undirected edge used by exactly two triangles in opposite directions, one fan per
//! vertex (no pinch), signed volume > 0 for every shell (no inside-out shell),
//! Euler characteristic and component count.
//! Global checks: every coordinate finite and within ±1e9 mm (checked first; nothing
//! else runs otherwise), no unreferenced vertices (non-soup), minimum distance
//! between distinct vertices ≥ δ, minimum triangle altitude, facet vertices at
//! `w ≥ T − q/2`, and the back face (single 2D orientation, areas add up).
//!
//! What "ok" means: without `check_self_intersection` the mesh is a closed,
//! consistently wound, unpinched 2-manifold with positive shells, i.e. topologically
//! valid. It is not proven free of self-intersections unless that stage ran
//! (`ValidateOpts::check_self_intersection`, see [`super::intersect`]).

use std::collections::HashMap;

use super::{FaceKind, Mesh, weld_key};
use crate::orient::Xform;

/// What to check beyond the topology.
#[derive(Debug, Clone)]
pub struct ValidateOpts {
    /// Plate space → written space (stages 2 and 3).
    pub transform: Xform,
    /// δ gate: minimum distance between distinct written vertices (mm).
    pub min_distance: Option<f64>,
    /// Slicer merge radius in µm (CuraEngine: 30). `None` skips stage 3.
    pub slicer_merge_um: Option<i64>,
    /// Facet vertices (plate space, `FaceKind::Facet`) must have `w >= this`.
    pub min_facet_w: Option<f64>,
    /// Expected back-face area (plate space, `FaceKind::Back`, mm²).
    pub back_area: Option<f64>,
    /// Number of closed genus-0 shells expected (Euler = 2 per shell).
    pub expect_components: usize,
    /// Expected Euler characteristic (default `2 × expect_components`).
    pub expect_euler: Option<i64>,
    /// The input is a triangle soup (e.g. read from STL): skip the raw stage and
    /// allow the written-stage weld to merge vertices.
    pub soup: bool,
    /// How many offending elements to describe per check.
    pub max_examples: usize,
    /// Smallest allowed triangle altitude in the written stage (mm). Slivers below
    /// this fail. `None` only reports `min_altitude`.
    pub min_altitude: Option<f64>,
    /// Run the triangle-triangle self-intersection stage on the written mesh.
    /// Off by default because it costs several times the rest of `validate()`.
    pub check_self_intersection: bool,
}

/// Default sliver gate: δ/10 = 5 µm. Generated plates are ≥ ~30 µm on walls and
/// facets and ≥ ~cell/√2 on the back face (audit A5/L2).
pub const DEFAULT_MIN_ALTITUDE_MM: f64 = 0.005;

/// Coordinates beyond this (mm) are treated as corrupt input (audit A4).
pub const MAX_ABS_COORD_MM: f32 = 1.0e9;

impl Default for ValidateOpts {
    fn default() -> Self {
        ValidateOpts {
            transform: Xform::IDENTITY,
            min_distance: Some(crate::relief::Tol::default().delta),
            slicer_merge_um: Some(30),
            min_facet_w: None,
            back_area: None,
            expect_components: 1,
            expect_euler: None,
            soup: false,
            max_examples: 5,
            min_altitude: Some(DEFAULT_MIN_ALTITUDE_MM),
            check_self_intersection: false,
        }
    }
}

/// Topology and geometry counts for one stage.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct StageReport {
    pub stage: &'static str,
    /// Vertices fed into this stage.
    pub vertices_in: usize,
    /// Distinct vertices after this stage's weld (raw stage: same as `vertices_in`).
    pub vertices_welded: usize,
    /// `vertices_in - vertices_welded`.
    pub merged: usize,
    /// Vertices referenced by at least one non-degenerate triangle.
    pub vertices_used: usize,
    pub triangles: usize,
    pub repeated_index_tris: usize,
    pub zero_area_tris: usize,
    pub edges: usize,
    pub boundary_edges: usize,
    pub nonmanifold_edges: usize,
    pub inconsistent_winding_edges: usize,
    pub pinched_vertices: usize,
    pub components: usize,
    /// Shells (connected components) whose own signed volume is ≤ 0.
    pub negative_shells: usize,
    pub euler: i64,
    pub signed_volume: f64,
    pub min_altitude: f64,
    /// Index (into the input triangles) of the triangle with `min_altitude`.
    pub min_altitude_triangle: Option<usize>,
    pub examples: Vec<String>,
}

impl StageReport {
    pub fn degenerate_tris(&self) -> usize {
        self.repeated_index_tris + self.zero_area_tris
    }

    /// Closed, consistently wound, unpinched, non-degenerate, positive volume, with
    /// the expected shell count and Euler characteristic.
    pub fn topology_ok(&self, components: usize, euler: i64) -> bool {
        self.degenerate_tris() == 0
            && self.boundary_edges == 0
            && self.nonmanifold_edges == 0
            && self.inconsistent_winding_edges == 0
            && self.pinched_vertices == 0
            && self.negative_shells == 0
            && self.signed_volume > 0.0
            && self.components == components
            && self.euler == euler
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Report {
    pub raw: Option<StageReport>,
    pub written: StageReport,
    pub slicer: Option<StageReport>,
    /// Minimum distance between distinct written vertices. Exact when below the
    /// search radius (2δ); otherwise the smallest distance found, or `None`.
    pub min_distinct_distance: Option<f64>,
    pub min_distance_gate: Option<f64>,
    pub facet_vertices_below_min: usize,
    pub min_facet_w: Option<f64>,
    /// Back triangles whose 2D orientation differs from the first one.
    pub back_orientation_mismatch: usize,
    /// Σ|area| of back triangles (plate space).
    pub back_area_sum: Option<f64>,
    /// Tolerance used for the back-area comparison (mm²).
    pub back_area_tolerance: Option<f64>,
    /// Coordinates that are NaN, infinite or beyond ±1e9 mm.
    pub non_finite_values: usize,
    /// Vertices no triangle references (non-soup meshes only).
    pub unreferenced_vertices: usize,
    /// Intersecting triangle pairs, when the self-intersection stage ran.
    pub self_intersecting_pairs: Option<usize>,
    pub self_intersection_examples: Vec<String>,
    pub failures: Vec<String>,
    pub ok: bool,
}

impl Report {
    /// Multi-line, human-readable summary.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        let stage = |s: &mut String, r: &StageReport| {
            s.push_str(&format!(
                "  [{:<7}] verts in {} welded {} (merged {}) used {} | tris {} | degenerate {} (repeated {}, zero-area {}) | \
                 edges {} boundary {} non-manifold {} inconsistent {} | pinched {} | components {} euler {} | volume {:.3} mm3 | min altitude {:.4} mm\n",
                r.stage,
                r.vertices_in,
                r.vertices_welded,
                r.merged,
                r.vertices_used,
                r.triangles,
                r.degenerate_tris(),
                r.repeated_index_tris,
                r.zero_area_tris,
                r.edges,
                r.boundary_edges,
                r.nonmanifold_edges,
                r.inconsistent_winding_edges,
                r.pinched_vertices,
                r.components,
                r.euler,
                r.signed_volume,
                r.min_altitude
            ));
            for e in &r.examples {
                s.push_str(&format!("      {e}\n"));
            }
        };
        if let Some(r) = &self.raw {
            stage(&mut s, r);
        }
        stage(&mut s, &self.written);
        if let Some(r) = &self.slicer {
            stage(&mut s, r);
        }
        match self.min_distinct_distance {
            Some(d) => s.push_str(&format!("  min distinct-vertex distance {:.1} um", d * 1000.0)),
            None => s.push_str("  min distinct-vertex distance: none within search radius"),
        }
        if let Some(g) = self.min_distance_gate {
            s.push_str(&format!(" (gate {:.1} um)", g * 1000.0));
        }
        s.push('\n');
        if let Some(t) = self.min_facet_w {
            s.push_str(&format!("  facet vertices below w = {t:.4}: {}\n", self.facet_vertices_below_min));
        }
        if let Some(a) = self.back_area_sum {
            s.push_str(&format!("  back face: area {a:.3} mm2, orientation mismatches {}\n", self.back_orientation_mismatch));
        }
        if self.unreferenced_vertices > 0 {
            s.push_str(&format!("  unreferenced vertices: {}\n", self.unreferenced_vertices));
        }
        match self.self_intersecting_pairs {
            Some(n) => {
                s.push_str(&format!("  self-intersecting triangle pairs: {n}\n"));
                for e in &self.self_intersection_examples {
                    s.push_str(&format!("      {e}\n"));
                }
            }
            None => s.push_str("  self-intersection stage: not run (ok = closed 2-manifold, not a proven solid)\n"),
        }
        if self.ok {
            s.push_str("  validate: OK\n");
        } else {
            s.push_str("  validate: FAILED\n");
            for f in &self.failures {
                s.push_str(&format!("    - {f}\n"));
            }
        }
        s
    }
}

/// Runs all stages and checks. See the module docs.
pub fn validate(m: &Mesh, opts: &ValidateOpts) -> Report {
    let mut rep = Report { min_distance_gate: opts.min_distance, ..Default::default() };
    // Corrupt input first: NaN/Inf or absurd coordinates would poison every later
    // stage (and overflowed the slicer stage's integer conversion, audit A4).
    rep.non_finite_values =
        m.positions.iter().filter(|x| !x.is_finite() || x.abs() > MAX_ABS_COORD_MM).count();
    if rep.non_finite_values > 0 || m.positions.len() % 3 != 0 || m.indices.len() % 3 != 0 {
        if rep.non_finite_values > 0 {
            rep.failures.push(format!(
                "{} non-finite or out-of-range (|x| > {MAX_ABS_COORD_MM:e} mm) vertex coordinates",
                rep.non_finite_values
            ));
        } else {
            rep.failures.push("position or index buffer length is not a multiple of 3".into());
        }
        rep.ok = false;
        return rep;
    }
    let ntri = m.triangle_count();
    let tris: Vec<[u32; 3]> = (0..ntri).map(|t| m.triangle(t)).collect();
    let euler = opts.expect_euler.unwrap_or(2 * opts.expect_components as i64);
    let comps = opts.expect_components;

    // Stage 1: raw.
    if !opts.soup {
        let pos: Vec<[f64; 3]> =
            m.positions.chunks_exact(3).map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        let r = check_topology("raw", &pos, &tris, opts.max_examples);
        if !r.topology_ok(comps, euler) {
            rep.failures.push(format!("raw stage: {}", describe(&r, comps, euler)));
        }
        // An unreferenced vertex still moves `Mesh::bounds()` and so the payload
        // bbox (audit L4).
        rep.unreferenced_vertices = r.vertices_in - r.vertices_used;
        if rep.unreferenced_vertices > 0 && r.degenerate_tris() == 0 {
            rep.failures.push(format!("{} unreferenced vertices", rep.unreferenced_vertices));
        }
        rep.raw = Some(r);
    }

    // Stage 2: as written (transformed f32, exact weld).
    let written: Vec<[f32; 3]> =
        m.positions.chunks_exact(3).map(|p| opts.transform.apply_f32([p[0], p[1], p[2]])).collect();
    let (wpos, remap) = weld_bits(&written);
    let wtris: Vec<[u32; 3]> =
        tris.iter().map(|t| [remap[t[0] as usize], remap[t[1] as usize], remap[t[2] as usize]]).collect();
    let wpos64: Vec<[f64; 3]> = wpos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    let mut w = check_topology("written", &wpos64, &wtris, opts.max_examples);
    w.vertices_in = written.len();
    w.merged = written.len() - wpos.len();
    if !w.topology_ok(comps, euler) {
        rep.failures.push(format!("written stage: {}", describe(&w, comps, euler)));
    }
    if let Some(min_alt) = opts.min_altitude {
        if w.triangles > w.degenerate_tris() && w.min_altitude < min_alt {
            let what = w
                .min_altitude_triangle
                .map(|t| {
                    let p = wtris[t].map(|v| wpos64[v as usize].map(|x| (x * 1e4).round() / 1e4));
                    format!(" (triangle {t}, kind {:?}, at {p:?})", m.face_kind.get(t))
                })
                .unwrap_or_default();
            rep.failures.push(format!(
                "written stage: sliver triangle, min altitude {:.3} um < gate {:.3} um{what}",
                w.min_altitude * 1e3,
                min_alt * 1e3
            ));
        }
    }
    if opts.check_self_intersection {
        let (n, ex) = super::intersect::self_intersections_indexed(&wpos64, &wtris, &m.face_kind, opts.max_examples);
        rep.self_intersecting_pairs = Some(n);
        rep.self_intersection_examples = ex;
        if n > 0 {
            rep.failures.push(format!("written stage: {n} self-intersecting triangle pairs"));
        }
    }
    if !opts.soup {
        if w.merged != 0 {
            rep.failures.push(format!("written stage: exact weld merged {} vertices (must be 0)", w.merged));
        }
        if let Some(r) = &rep.raw {
            if r.vertices_used != w.vertices_used || r.edges != w.edges {
                rep.failures.push(format!(
                    "written stage counts differ from raw (used verts {} vs {}, edges {} vs {})",
                    w.vertices_used, r.vertices_used, w.edges, r.edges
                ));
            }
        }
    }
    rep.written = w;

    // Stage 3: slicer merge.
    if let Some(meld) = opts.slicer_merge_um {
        let (spos, sremap, merged) = cura_meld(&wpos, meld);
        let stris: Vec<[u32; 3]> =
            wtris.iter().map(|t| [sremap[t[0] as usize], sremap[t[1] as usize], sremap[t[2] as usize]]).collect();
        let mut s = check_topology("slicer", &spos, &stris, opts.max_examples);
        s.vertices_in = wpos.len();
        s.merged = merged;
        if merged != 0 {
            rep.failures.push(format!("slicer stage: {} vertices merged within {} um (must be 0)", merged, meld));
        }
        if !s.topology_ok(comps, euler) {
            rep.failures.push(format!("slicer stage: {}", describe(&s, comps, euler)));
        }
        rep.slicer = Some(s);
    }

    // δ gate on the written vertex set.
    let radius = opts.min_distance.map(|d| 2.0 * d).unwrap_or(0.1);
    rep.min_distinct_distance = min_distinct_distance(&wpos64, radius);
    if let (Some(gate), Some(d)) = (opts.min_distance, rep.min_distinct_distance) {
        if d < gate {
            rep.failures.push(format!("min distinct-vertex distance {:.2} um < gate {:.2} um", d * 1e3, gate * 1e3));
        }
    }

    // Facet heights and the back face (plate space, needs face kinds).
    let have_kinds = m.face_kind.len() == ntri;
    if let Some(min_w) = opts.min_facet_w {
        rep.min_facet_w = Some(min_w);
        if have_kinds {
            let mut seen = vec![false; m.vertex_count()];
            for (t, tri) in tris.iter().enumerate() {
                if m.face_kind[t] != FaceKind::Facet {
                    continue;
                }
                for &v in tri {
                    if !seen[v as usize] {
                        seen[v as usize] = true;
                        if (m.vertex(v)[2] as f64) < min_w {
                            rep.facet_vertices_below_min += 1;
                        }
                    }
                }
            }
            if rep.facet_vertices_below_min > 0 {
                rep.failures.push(format!("{} facet vertices below w = {min_w}", rep.facet_vertices_below_min));
            }
        } else {
            rep.failures.push("min_facet_w requested but the mesh has no face kinds".into());
        }
    }
    if have_kinds && m.face_kind.contains(&FaceKind::Back) {
        let mut sign0 = 0.0f64;
        let mut sum = 0.0;
        // Boundary of the back region (edges used once by back triangles) and the
        // largest coordinate, for an f32-aware area tolerance (audit A3).
        let mut back_edges: HashMap<(u32, u32), u32> = HashMap::new();
        let mut max_coord = 0.0f32;
        for (t, tri) in tris.iter().enumerate() {
            if m.face_kind[t] != FaceKind::Back {
                continue;
            }
            for k in 0..3 {
                let (x, y) = (tri[k], tri[(k + 1) % 3]);
                *back_edges.entry((x.min(y), x.max(y))).or_default() += 1;
                let p = m.vertex(x);
                max_coord = max_coord.max(p[0].abs()).max(p[1].abs());
            }
            let [a, b, c] = tri.map(|v| m.vertex(v));
            let cross = (b[0] as f64 - a[0] as f64) * (c[1] as f64 - a[1] as f64)
                - (b[1] as f64 - a[1] as f64) * (c[0] as f64 - a[0] as f64);
            let s = cross.signum();
            if sign0 == 0.0 {
                sign0 = s;
            }
            if s != sign0 || cross == 0.0 {
                rep.back_orientation_mismatch += 1;
            }
            sum += cross.abs() * 0.5;
        }
        rep.back_area_sum = Some(sum);
        if rep.back_orientation_mismatch > 0 {
            rep.failures.push(format!("back face: {} triangles fold over", rep.back_orientation_mismatch));
        }
        if let Some(a) = opts.back_area {
            // Each stored vertex is within half an f32 ulp of its f64 position, so the
            // area may differ by up to about perimeter × ulp; allow twice that plus a
            // 1e-6 relative term. Small parts far from the origin need the first term.
            let perimeter: f64 = back_edges
                .iter()
                .filter(|&(_, &n)| n == 1)
                .map(|(&(x, y), _)| {
                    let (p, q) = (m.vertex(x), m.vertex(y));
                    ((p[0] as f64 - q[0] as f64).powi(2) + (p[1] as f64 - q[1] as f64).powi(2)).sqrt()
                })
                .sum();
            let ulp = (max_coord.next_up() - max_coord) as f64;
            let tol = 1e-6 * a.abs().max(1.0) + 2.0 * ulp * perimeter;
            rep.back_area_tolerance = Some(tol);
            if (sum - a).abs() > tol {
                rep.failures.push(format!("back face: area {sum} != outline area {a} (tolerance {tol:.3e} mm2)"));
            }
        }
    } else if opts.back_area.is_some() {
        rep.failures.push("back_area requested but the mesh has no back triangles".into());
    }

    rep.ok = rep.failures.is_empty();
    rep
}

fn describe(r: &StageReport, comps: usize, euler: i64) -> String {
    let mut v = Vec::new();
    if r.repeated_index_tris > 0 {
        v.push(format!("{} repeated-index triangles", r.repeated_index_tris));
    }
    if r.zero_area_tris > 0 {
        v.push(format!("{} zero-area triangles", r.zero_area_tris));
    }
    if r.boundary_edges > 0 {
        v.push(format!("{} boundary edges", r.boundary_edges));
    }
    if r.nonmanifold_edges > 0 {
        v.push(format!("{} non-manifold edges", r.nonmanifold_edges));
    }
    if r.inconsistent_winding_edges > 0 {
        v.push(format!("{} inconsistent-winding edges", r.inconsistent_winding_edges));
    }
    if r.pinched_vertices > 0 {
        v.push(format!("{} pinched vertices", r.pinched_vertices));
    }
    if r.signed_volume <= 0.0 {
        v.push(format!("signed volume {} <= 0", r.signed_volume));
    }
    if r.negative_shells > 0 {
        v.push(format!("{} inside-out shells (own signed volume <= 0)", r.negative_shells));
    }
    if r.components != comps {
        v.push(format!("{} components (expected {comps})", r.components));
    }
    if r.euler != euler {
        v.push(format!("euler {} (expected {euler})", r.euler));
    }
    v.join(", ")
}

/// Exact weld by bit pattern. Returns unique positions and the old → new map.
pub fn weld_bits(pos: &[[f32; 3]]) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut map: HashMap<[u32; 3], u32> = HashMap::with_capacity(pos.len());
    let mut out = Vec::with_capacity(pos.len());
    let mut remap = Vec::with_capacity(pos.len());
    for p in pos {
        let id = *map.entry(weld_key(*p)).or_insert_with(|| {
            out.push([p[0] + 0.0, p[1] + 0.0, p[2] + 0.0]);
            (out.len() - 1) as u32
        });
        remap.push(id);
    }
    (out, remap)
}

/// CuraEngine-style meld (src/mesh.cpp): integer µm, bucket `(c + r/2) / r` with
/// truncating division, merge into the first earlier vertex in the same bucket
/// within `r` µm. Returns µm-scaled positions in mm, the map and the merge count.
pub fn cura_meld(pos: &[[f32; 3]], r: i64) -> (Vec<[f64; 3]>, Vec<u32>, usize) {
    // `validate()` rejects non-finite and huge coordinates before this runs; the
    // clamp and saturating adds keep a direct caller from overflowing (audit A4).
    let mm2int = |x: f32| -> i64 {
        let x = (x as f64).clamp(-1.0e12, 1.0e12);
        (x * 1000.0 + if x > 0.0 { 0.5 } else if x < 0.0 { -0.5 } else { 0.0 }) as i64
    };
    let mut buckets: HashMap<[i64; 3], Vec<u32>> = HashMap::with_capacity(pos.len());
    let mut verts: Vec<[i64; 3]> = Vec::with_capacity(pos.len());
    let mut remap = Vec::with_capacity(pos.len());
    for p in pos {
        let q = [mm2int(p[0]), mm2int(p[1]), mm2int(p[2])];
        let b = q.map(|c| c.saturating_add(r / 2) / r);
        let list = buckets.entry(b).or_default();
        let mut found = None;
        for &i in list.iter() {
            let o = verts[i as usize];
            let (dx, dy, dz) = (o[0] - q[0], o[1] - q[1], o[2] - q[2]);
            if dx.abs() <= r && dy.abs() <= r && dz.abs() <= r && dx * dx + dy * dy + dz * dz <= r * r {
                found = Some(i);
                break;
            }
        }
        let id = match found {
            Some(i) => i,
            None => {
                let i = verts.len() as u32;
                verts.push(q);
                list.push(i);
                i
            }
        };
        remap.push(id);
    }
    let merged = pos.len() - verts.len();
    let mm = verts.iter().map(|q| [q[0] as f64 / 1000.0, q[1] as f64 / 1000.0, q[2] as f64 / 1000.0]).collect();
    (mm, remap, merged)
}

/// The per-stage check list on an indexed mesh (no weld).
pub fn check_topology(stage: &'static str, pos: &[[f64; 3]], tris: &[[u32; 3]], max_examples: usize) -> StageReport {
    let nv = pos.len();
    let mut r = StageReport {
        stage,
        vertices_in: nv,
        vertices_welded: nv,
        triangles: tris.len(),
        min_altitude: f64::INFINITY,
        ..Default::default()
    };
    let fmt_p = |p: [f64; 3]| format!("({:.4}, {:.4}, {:.4})", p[0], p[1], p[2]);

    // Degenerate triangles.
    let mut good: Vec<[u32; 3]> = Vec::with_capacity(tris.len());
    for (ti, t) in tris.iter().enumerate() {
        if t.iter().any(|&i| i as usize >= nv) {
            r.repeated_index_tris += 1;
            if r.examples.len() < max_examples {
                r.examples.push(format!("index out of range {t:?}"));
            }
            continue;
        }
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            r.repeated_index_tris += 1;
            if r.examples.len() < max_examples {
                r.examples.push(format!("repeated index {t:?}"));
            }
            continue;
        }
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let e1 = sub(b, a);
        let e2 = sub(c, a);
        let e3 = sub(c, b);
        let cr = cross(e1, e2);
        let a2 = norm(cr);
        if a2 == 0.0 {
            r.zero_area_tris += 1;
            if r.examples.len() < max_examples {
                r.examples.push(format!("zero-area triangle at {}", fmt_p(a)));
            }
            continue;
        }
        let lmax = norm(e1).max(norm(e2)).max(norm(e3));
        if a2 / lmax < r.min_altitude {
            r.min_altitude = a2 / lmax;
            r.min_altitude_triangle = Some(ti);
        }
        good.push(*t);
    }

    // Edges: key = (min, max), with the direction bit.
    let mut keys: Vec<u64> = Vec::with_capacity(good.len() * 3);
    for t in &good {
        for (x, y) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (lo, hi, fwd) = if x < y { (x, y, 1u64) } else { (y, x, 0u64) };
            keys.push(((lo as u64) << 33) | ((hi as u64) << 1) | fwd);
        }
    }
    keys.sort_unstable();
    let mut i = 0;
    while i < keys.len() {
        let k = keys[i] >> 1;
        let mut j = i + 1;
        while j < keys.len() && keys[j] >> 1 == k {
            j += 1;
        }
        r.edges += 1;
        let n = j - i;
        let lo = (k >> 32) as usize;
        let hi = (k & 0xFFFF_FFFF) as usize;
        let bad = match n {
            1 => {
                r.boundary_edges += 1;
                Some("boundary")
            }
            2 => {
                if (keys[i] & 1) == (keys[i + 1] & 1) {
                    r.inconsistent_winding_edges += 1;
                    Some("same-direction")
                } else {
                    None
                }
            }
            _ => {
                r.nonmanifold_edges += 1;
                Some("non-manifold")
            }
        };
        if let Some(what) = bad {
            if r.examples.len() < max_examples {
                r.examples.push(format!("{what} edge ({n} uses) {} - {}", fmt_p(pos[lo]), fmt_p(pos[hi])));
            }
        }
        i = j;
    }

    // Vertex fans: CSR of opposite edges per vertex; the link must be connected.
    let mut deg = vec![0u32; nv + 1];
    for t in &good {
        for &v in t {
            deg[v as usize + 1] += 1;
        }
    }
    for k in 0..nv {
        deg[k + 1] += deg[k];
    }
    let start = deg.clone();
    let mut fill = deg;
    let mut link = vec![(0u32, 0u32); good.len() * 3];
    for t in &good {
        for (v, x, y) in [(t[0], t[1], t[2]), (t[1], t[2], t[0]), (t[2], t[0], t[1])] {
            let s = &mut fill[v as usize];
            link[*s as usize] = (x, y);
            *s += 1;
        }
    }
    let mut local: Vec<u32> = Vec::new();
    let mut parent: Vec<usize> = Vec::new();
    for v in 0..nv {
        let edges = &link[start[v] as usize..start[v + 1] as usize];
        if edges.is_empty() {
            continue;
        }
        r.vertices_used += 1;
        local.clear();
        parent.clear();
        let id = |x: u32, local: &mut Vec<u32>, parent: &mut Vec<usize>| -> usize {
            if let Some(p) = local.iter().position(|&l| l == x) {
                p
            } else {
                local.push(x);
                parent.push(parent.len());
                parent.len() - 1
            }
        };
        for &(x, y) in edges {
            let a = id(x, &mut local, &mut parent);
            let b = id(y, &mut local, &mut parent);
            let ra = find(&mut parent, a);
            let rb = find(&mut parent, b);
            parent[ra] = rb;
        }
        let roots = (0..parent.len()).filter(|&i| find(&mut parent, i) == i).count();
        if roots > 1 {
            r.pinched_vertices += 1;
            if r.examples.len() < max_examples {
                r.examples.push(format!("pinched vertex ({roots} fans) at {}", fmt_p(pos[v])));
            }
        }
    }

    // Components (union-find over vertices of good triangles).
    let mut uf: Vec<u32> = (0..nv as u32).collect();
    fn root(uf: &mut [u32], mut x: u32) -> u32 {
        while uf[x as usize] != x {
            uf[x as usize] = uf[uf[x as usize] as usize];
            x = uf[x as usize];
        }
        x
    }
    for t in &good {
        let a = root(&mut uf, t[0]);
        let b = root(&mut uf, t[1]);
        let c = root(&mut uf, t[2]);
        uf[b as usize] = a;
        uf[c as usize] = a;
    }
    let mut used = vec![false; nv];
    for t in &good {
        for &v in t {
            used[v as usize] = true;
        }
    }
    r.components = (0..nv as u32).filter(|&v| used[v as usize] && root(&mut uf, v) == v).count();
    r.euler = r.vertices_used as i64 - r.edges as i64 + good.len() as i64;

    // Signed volume about the bounding-box centre of used vertices.
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (v, p) in pos.iter().enumerate() {
        if used[v] {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    // Total and per-shell signed volume (audit L3: one inside-out shell must not be
    // hidden by a larger positive one). Cavities (inner shells wound inward) are not
    // supported yet and fail here too.
    let mut vol = 0.0;
    let mut shell_vol: HashMap<u32, f64> = HashMap::new();
    for t in &good {
        let v = super::triple(sub(pos[t[0] as usize], c), sub(pos[t[1] as usize], c), sub(pos[t[2] as usize], c));
        vol += v;
        *shell_vol.entry(root(&mut uf, t[0])).or_default() += v;
    }
    r.signed_volume = vol / 6.0;
    r.negative_shells = shell_vol.values().filter(|&&v| v <= 0.0).count();
    if !r.min_altitude.is_finite() {
        r.min_altitude = 0.0;
    }
    r
}

fn find(p: &mut [usize], mut x: usize) -> usize {
    while p[x] != x {
        p[x] = p[p[x]];
        x = p[x];
    }
    x
}

/// Minimum distance between distinct points, searching pairs closer than `radius`
/// with a uniform grid of cell `radius`. Exact when the true minimum is below
/// `radius`; returns `None` when no pair is within `radius`.
pub fn min_distinct_distance(pos: &[[f64; 3]], radius: f64) -> Option<f64> {
    let inv = 1.0 / radius;
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::with_capacity(pos.len());
    let mut best = f64::INFINITY;
    for (i, p) in pos.iter().enumerate() {
        let c = [(p[0] * inv).floor() as i64, (p[1] * inv).floor() as i64, (p[2] * inv).floor() as i64];
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) {
                        for &j in list {
                            let q = pos[j as usize];
                            let d = norm(sub(*p, q));
                            if d > 0.0 && d < best {
                                best = d;
                            }
                        }
                    }
                }
            }
        }
        grid.entry(c).or_default().push(i as u32);
    }
    if best.is_finite() { Some(best) } else { None }
}

#[inline]
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit cube, outward CCW.
    pub(crate) fn cube() -> Mesh {
        let p: [[f32; 3]; 8] = [
            [0., 0., 0.],
            [1., 0., 0.],
            [1., 1., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [1., 0., 1.],
            [1., 1., 1.],
            [0., 1., 1.],
        ];
        let t: [[u32; 3]; 12] = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        let mut m = Mesh::default();
        for q in p {
            m.push_vertex(q);
        }
        for q in t {
            m.push_triangle(q, FaceKind::Other);
        }
        m
    }

    #[test]
    fn cube_is_valid() {
        let r = validate(&cube(), &ValidateOpts::default());
        assert!(r.ok, "{}", r.summary());
        assert!((r.written.signed_volume - 1.0).abs() < 1e-12);
        assert_eq!(r.written.euler, 2);
    }
}
