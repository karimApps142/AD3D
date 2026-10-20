#![allow(dead_code)]

use ad3d_core::CancelToken;
use ad3d_core::mesh::Mesh;
use ad3d_core::mesh::validate::{Report, ValidateOpts, validate};
use ad3d_core::relief::{
    Heights, MeshStats, Partition, PieceSet, Tol, cluster_heights, mesh_relief_stats, square_grid,
};
use glam::DVec2;

/// Grid with explicit integer corner heights (µm) per cell, corner order
/// `[(i,j), (i+1,j), (i+1,j+1), (i,j+1)]`, row-major from the bottom row.
/// Clusters (§5.1), sets each apex to the mean corner height, meshes and validates.
pub struct Built {
    pub part: Partition,
    pub mesh: Mesh,
    pub stats: MeshStats,
    pub report: Report,
}

pub fn build_grid(nx: u32, ny: u32, cell: f64, origin: DVec2, corners: &[[i64; 4]], base_um: i64) -> Built {
    assert_eq!(corners.len(), (nx * ny) as usize);
    let tol = Tol::default();
    let part = square_grid(nx, ny, cell, origin);
    let mut corner: Vec<i64> = corners.iter().flat_map(|c| c.iter().copied()).collect();
    cluster_heights(&part, &mut corner, tol.tau_q);
    let apex_w = corners.iter().map(|c| c.iter().sum::<i64>() as f64 / 4.0 * tol.q).collect();
    let h = Heights { corner, apex_w };
    let (mesh, stats) = mesh_relief_stats(&part, PieceSet::All, &h, &[], &tol, &CancelToken::new())
        .unwrap_or_else(|e| panic!("mesh_relief failed: {e}"));
    let opts = ValidateOpts {
        min_facet_w: Some(base_um as f64 * tol.q - tol.q / 2.0),
        back_area: Some(part.outline_area()),
        ..Default::default()
    };
    let report = validate(&mesh, &opts);
    Built { part, mesh, stats, report }
}

pub fn assert_ok(b: &Built) {
    assert!(b.report.ok, "validate failed:\n{}", b.report.summary());
}

/// Every triangle's vertices as f32 bit patterns, for determinism checks.
pub fn fingerprint(m: &Mesh) -> (Vec<u32>, Vec<u32>) {
    (m.positions.iter().map(|v| v.to_bits()).collect(), m.indices.clone())
}
