//! M1 verify round 2, physical printability review (CR-6 SE, 0.4 mm nozzle).
//! Uncommitted; for the fixer.

use ad3d_core::orient::{Orientation, QuarterTurns};
use ad3d_core::squama::parts::{KeySpec, STAND_KEY_CLEARANCE_MM, StandSpec, key_layout};
use ad3d_core::squama::tiles::{Bed, MAX_STANDING_RATIO, TileOpts, plan_tiles};
use ad3d_core::squama::{Layout, LayoutKind};
use glam::DVec2;

/// Splints lie flat on the back of the assembled plate, so no two may overlap.
/// Where a vertical and a horizontal seam cross, the end keys (10 mm from the
/// segment ends) put an 18 × 6 mm splint of one seam over the splint of the other.
#[test]
fn splints_do_not_overlap_at_seam_crossings() {
    let k = KeySpec::default();
    let thickness = 3.79;
    let l = Layout { kind: LayoutKind::Square, w_mm: 300.0, h_mm: 378.0, cell_mm: 6.0, nx: 50, ny: 63, crop: None };
    let opts = TileOpts { thickness_mm: thickness, min_tile_mm: k.min_tile_mm(), grid: None, max_standing_ratio: Some(MAX_STANDING_RATIO) };
    let plan = plan_tiles(&l, &Bed::CR6_SE, Orientation::Standing(QuarterTurns::Auto), std::f64::consts::PI, &opts).unwrap();
    assert!(plan.cols > 1 && plan.rows > 1);
    let s = StandSpec::for_plate(300.0, 378.0, thickness, k.splint_t_mm, &Bed::CR6_SE);
    let ks = KeySpec { bottom_clear_mm: s.slot_depth_mm + 0.5 * k.splint_w_mm + STAND_KEY_CLEARANCE_MM, ..k };
    let keys = key_layout(&plan, &ks);
    let len = 2.0 * ks.offset_mm + ks.peg_d_mm + 3.0;
    // Axis-aligned footprint of each splint on the back (plate mm).
    let rects: Vec<(DVec2, DVec2)> = keys
        .pairs
        .iter()
        .map(|(a, b)| {
            let c = 0.5 * (*a + *b);
            let along_x = (a.x - b.x).abs() > (a.y - b.y).abs();
            let half = if along_x { DVec2::new(0.5 * len, 0.5 * ks.splint_w_mm) } else { DVec2::new(0.5 * ks.splint_w_mm, 0.5 * len) };
            (c - half, c + half)
        })
        .collect();
    let mut hits = Vec::new();
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            let (a, b) = (rects[i], rects[j]);
            if a.0.x < b.1.x && b.0.x < a.1.x && a.0.y < b.1.y && b.0.y < a.1.y {
                hits.push((keys.pairs[i], keys.pairs[j]));
            }
        }
    }
    assert!(hits.is_empty(), "{} overlapping splint pairs, e.g. {:?}", hits.len(), hits.first());
}
