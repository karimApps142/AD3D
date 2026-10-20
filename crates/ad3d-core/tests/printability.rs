//! Physical printability checks for the owner's Creality CR-6 SE (0.4 mm nozzle,
//! FDM, bed-slinger moving in Y), from the M1 verify round 1 printability review.
//!
//! Each test encodes a defect found in that review; all are fixed (round 1).

use ad3d_core::squama::SquamaParams;
use ad3d_core::squama::calibration::CalibrationSpec;
use ad3d_core::squama::parts::{KeySpec, PART_GAP_MM, STAND_CLEARANCE_MM, StandSpec};
use ad3d_core::squama::tiles::Bed;

/// FDM fit clearance (diametral) for a pin in a hole printed with its axis
/// horizontal, as the key holes are in a standing tile: the top of the hole
/// droops, so 0.2 mm (3.0 in 3.2) binds. 0.4 mm is the usual minimum.
#[test]
fn key_peg_hole_clearance_fits_fdm() {
    let k = KeySpec::default();
    // Peg is an inscribed polygon, hole circumscribed, so the real gap is at least this.
    assert!(k.hole_d_mm - k.peg_d_mm >= 0.4 - 1e-9, "hole {} − peg {} < 0.4 mm", k.hole_d_mm, k.peg_d_mm);
}

/// Stand slot vs plate: both are XY features (slot walls, and the plate's
/// thickness when it was printed standing), and elephant's foot widens the edge
/// that goes into the slot. 0.3 mm total binds; 0.5 mm is a safe FDM default.
#[test]
fn stand_slot_clearance_fits_fdm() {
    assert!(STAND_CLEARANCE_MM >= 0.5 - 1e-9, "stand clearance {STAND_CLEARANCE_MM} mm");
}

/// With keys, the slot is widened by the splint thickness everywhere, but splints
/// only sit at seams: elsewhere a 4 mm plate rattles in a ~5.9 mm slot (≈ 6° of
/// lean, ~40 mm at the top of a 378 mm plate). Keep splints out of the slot (keys
/// at least slot depth + a few mm above the bottom edge) and size the slot to the plate.
#[test]
fn stand_slot_does_not_rattle_with_keys() {
    use ad3d_core::orient::{Orientation, QuarterTurns};
    use ad3d_core::squama::parts::{STAND_KEY_CLEARANCE_MM, key_layout};
    use ad3d_core::squama::tiles::{MAX_STANDING_RATIO, TileOpts, plan_tiles};
    use ad3d_core::squama::{Layout, LayoutKind};
    let k = KeySpec::default();
    let thickness = 4.04;
    let s = StandSpec::for_plate(300.0, 378.0, thickness, k.splint_t_mm, &Bed::CR6_SE);
    let play = s.slot_mm - thickness;
    assert!(play <= 0.7, "slot {} mm for a {thickness} mm plate: {play:.2} mm of play", s.slot_mm);
    // The export keeps keys on vertical seams above the slot (as `export_inner` sets it).
    let l = Layout { kind: LayoutKind::Square, w_mm: 300.0, h_mm: 378.0, cell_mm: 6.0, nx: 50, ny: 63, crop: None };
    let opts = TileOpts { thickness_mm: thickness, min_tile_mm: k.min_tile_mm(), grid: None, max_standing_ratio: Some(MAX_STANDING_RATIO) };
    let plan = plan_tiles(&l, &Bed::CR6_SE, Orientation::Standing(QuarterTurns::Auto), std::f64::consts::PI, &opts).unwrap();
    assert!(plan.cols > 1);
    let ks = KeySpec { bottom_clear_mm: s.slot_depth_mm + 0.5 * k.splint_w_mm + STAND_KEY_CLEARANCE_MM, ..k };
    let keys = key_layout(&plan, &ks);
    let lowest = keys.pairs.iter().filter(|(a, b)| (a.y - b.y).abs() < 1e-9).map(|(a, _)| a.y).fold(f64::INFINITY, f64::min);
    // The splint (half its width either side of the key centre) clears the slot.
    assert!(lowest - 0.5 * k.splint_w_mm >= s.slot_depth_mm + 3.0 - 1e-9, "lowest key at {lowest} mm, slot {} mm deep", s.slot_depth_mm);
}

/// Standing tiles stand on a ~4 mm footprint and need a brim; a 5 mm gap makes
/// neighbouring brims merge and leaves no room to remove them cleanly.
#[test]
fn part_gap_leaves_room_for_brims() {
    assert!(PART_GAP_MM >= 10.0, "part gap {PART_GAP_MM} mm");
    assert!(Bed::CR6_SE.margin_mm >= 8.0, "bed margin {} mm (brim 5–8 mm)", Bed::CR6_SE.margin_mm);
}

/// The calibration ramp should only spend patches on tilts that can be lit, and
/// must not create downward ledges above the 0.6 mm sag limit (§9) when it stands
/// in the plate's orientation. At α 5°, S 10°, cell 6 mm the default ramp starts at
/// −10°: three of twelve patches are predicted pitch black and the tile has
/// 6·tan 10° = 1.06 mm downward ledges (measured on the exported STL).
#[test]
fn calibration_ramp_is_lit_and_printable() {
    let p = SquamaParams::default();
    let spec = CalibrationSpec::from_params(&p);
    let (lo, _hi) = spec.tilt_range();
    let ledge = spec.cell_mm * lo.abs().tan();
    assert!(ledge <= 0.6 + 1e-9, "lowest tilt {:.1}° gives {ledge:.2} mm downward ledges", lo.to_degrees());
    let dead = (0..spec.steps)
        .map(|i| lo + (spec.tilt_range().1 - lo) * i as f64 / (spec.steps - 1) as f64)
        .filter(|t| (spec.light_elevation + t).sin() <= 0.0)
        .count();
    assert!(dead <= 1, "{dead} ramp patches are fully self-shadowed (one black fiducial is enough)");
}
