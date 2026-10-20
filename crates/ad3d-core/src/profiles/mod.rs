//! Machine profiles (FEATURES APP-MACHINES): printers now, laser and CNC stubs
//! for M4. Profiles are plain data (serde camelCase) so a project file can embed
//! the one it was made for.
//!
//! The default printer is the owner's Creality CR-6 SE: 235 × 235 × 250 mm, a
//! bed-slinger whose bed moves in Y, so tall thin parts printed standing should
//! have their face parallel to Y (START_HERE).

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Build plate for tiling and bed layout (design §14.1 `Bed`).
///
/// The tiling module's `squama::tiles::Bed` adds the moving-bed axis; convert
/// with [`PrinterProfile::tile_bed`] and `Bed::from(tiles_bed)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bed {
    pub x_mm: f64,
    pub y_mm: f64,
    pub z_mm: f64,
    /// Clearance kept free around parts in X and Y (brim, skirt, clips).
    pub margin_mm: f64,
}

impl Bed {
    /// Usable X and Y after the margin on both sides.
    pub fn usable_xy(&self) -> [f64; 2] {
        [(self.x_mm - 2.0 * self.margin_mm).max(0.0), (self.y_mm - 2.0 * self.margin_mm).max(0.0)]
    }
    /// Whether an axis-aligned box fits, optionally after a 90° turn about Z.
    pub fn fits(&self, size: [f64; 3], allow_z_turn: bool) -> bool {
        let [ux, uy] = self.usable_xy();
        let eps = 1e-9;
        if size[2] > self.z_mm + eps {
            return false;
        }
        (size[0] <= ux + eps && size[1] <= uy + eps) || (allow_z_turn && size[1] <= ux + eps && size[0] <= uy + eps)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum BedShape {
    Rect,
    Circle { diameter_mm: f64 },
}

/// FFF printer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterProfile {
    /// Stable short id (`cr6se`).
    pub id: String,
    pub name: String,
    /// Build volume X × Y × Z.
    pub build_mm: [f64; 3],
    pub bed_shape: BedShape,
    /// The axis the bed moves along (Y on a bed-slinger, Z on CoreXY), if any.
    pub moving_bed_axis: Option<Axis>,
    pub nozzle_mm: f64,
    pub layer_mm: f64,
    /// Default clearance from the build-volume limits.
    pub margin_mm: f64,
}

impl PrinterProfile {
    pub fn bed(&self) -> Bed {
        self.bed_with_margin(self.margin_mm)
    }

    pub fn bed_with_margin(&self, margin_mm: f64) -> Bed {
        let [x, y, z] = self.build_mm;
        match self.bed_shape {
            BedShape::Rect => Bed { x_mm: x, y_mm: y, z_mm: z, margin_mm },
            // Largest inscribed square of a round bed.
            BedShape::Circle { diameter_mm } => {
                let s = diameter_mm / std::f64::consts::SQRT_2;
                Bed { x_mm: s, y_mm: s, z_mm: z, margin_mm }
            }
        }
    }

    /// Which bed axis the face of a tall plate printed standing should be parallel
    /// to: the bed's motion axis on a bed-slinger (so the plate is shaken edge-on,
    /// not face-on), otherwise X.
    ///
    /// The export (§9, `orient::standing_xform`) follows this through
    /// [`tile_bed`](Self::tile_bed)'s motion axis.
    pub fn standing_face_axis(&self) -> Axis {
        match self.moving_bed_axis {
            Some(Axis::Y) => Axis::Y,
            _ => Axis::X,
        }
    }

    /// The §14.1 tiling bed for `bed` on this printer: the face of a standing
    /// plate goes along the moving-bed axis.
    pub fn tile_bed(&self, bed: &Bed) -> crate::squama::tiles::Bed {
        use crate::orient::BedMotion;
        crate::squama::tiles::Bed {
            x_mm: bed.x_mm,
            y_mm: bed.y_mm,
            z_mm: bed.z_mm,
            margin_mm: bed.margin_mm,
            motion: match self.moving_bed_axis {
                Some(Axis::Y) => BedMotion::Y,
                Some(Axis::X) => BedMotion::X,
                _ => BedMotion::None,
            },
        }
    }

    /// Largest plate (width, height) that can stand on this printer with its face
    /// along [`standing_face_axis`](Self::standing_face_axis).
    pub fn standing_envelope(&self, margin_mm: f64) -> [f64; 2] {
        let b = self.bed_with_margin(margin_mm);
        let [ux, uy] = b.usable_xy();
        let w = if self.standing_face_axis() == Axis::Y { uy } else { ux };
        [w, b.z_mm]
    }
}

impl From<crate::squama::tiles::Bed> for Bed {
    fn from(b: crate::squama::tiles::Bed) -> Bed {
        Bed { x_mm: b.x_mm, y_mm: b.y_mm, z_mm: b.z_mm, margin_mm: b.margin_mm }
    }
}

/// Laser cutter (stub until M4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaserProfile {
    pub id: String,
    pub name: String,
    pub bed_mm: [f64; 2],
    /// Beam width; cut paths are offset by kerf/2.
    pub kerf_mm: f64,
    /// `fiber`, `co2` or `None` while unknown (START_HERE open question 1).
    pub source: Option<String>,
}

/// CNC router (stub until M4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CncProfile {
    pub id: String,
    pub name: String,
    pub bed_mm: [f64; 2],
    pub z_travel_mm: f64,
    pub tool_diameter_mm: f64,
}

/// Id of the default printer (the owner's).
pub const DEFAULT_PRINTER: &str = "cr6se";

fn printer(id: &str, name: &str, build: [f64; 3], moving: Option<Axis>) -> PrinterProfile {
    PrinterProfile {
        id: id.into(),
        name: name.into(),
        build_mm: build,
        bed_shape: BedShape::Rect,
        moving_bed_axis: moving,
        nozzle_mm: 0.4,
        layer_mm: 0.2,
        // Room for a 6–8 mm brim around tall standing parts.
        margin_mm: 8.0,
    }
}

/// Built-in printer presets; the first is the default.
pub fn printers() -> Vec<PrinterProfile> {
    vec![
        printer(DEFAULT_PRINTER, "Creality CR-6 SE", [235.0, 235.0, 250.0], Some(Axis::Y)),
        printer("ender3v2", "Creality Ender-3 V2", [220.0, 220.0, 250.0], Some(Axis::Y)),
        printer("mk4", "Prusa MK4", [250.0, 210.0, 220.0], Some(Axis::Y)),
        printer("mini", "Prusa MINI+", [180.0, 180.0, 180.0], Some(Axis::Y)),
        printer("p1s", "Bambu Lab P1S", [256.0, 256.0, 256.0], Some(Axis::Z)),
    ]
}

pub fn default_printer() -> PrinterProfile {
    printers().into_iter().next().expect("presets are not empty")
}

/// Looks up a preset by id (case-insensitive).
pub fn printer_by_id(id: &str) -> Option<PrinterProfile> {
    printers().into_iter().find(|p| p.id.eq_ignore_ascii_case(id))
}

/// A custom rectangular printer from `"X×Y×Z"` / `"XxYxZ"` (mm), bed motion unknown.
pub fn custom_printer(spec: &str) -> Result<PrinterProfile> {
    let b = parse_dims::<3>(spec)?;
    Ok(PrinterProfile { id: "custom".into(), name: format!("Custom {}×{}×{} mm", b[0], b[1], b[2]), ..printer("custom", "", b, None) })
}

/// The owner's laser ("3015" class) and router ("1325" class); details unknown.
pub fn lasers() -> Vec<LaserProfile> {
    vec![LaserProfile { id: "laser3015".into(), name: "3015 laser (3000 × 1500)".into(), bed_mm: [3000.0, 1500.0], kerf_mm: 0.15, source: None }]
}

pub fn cncs() -> Vec<CncProfile> {
    vec![CncProfile {
        id: "cnc1325".into(),
        name: "1325 router (1300 × 2500)".into(),
        bed_mm: [1300.0, 2500.0],
        z_travel_mm: 200.0,
        tool_diameter_mm: 3.175,
    }]
}

/// Parses `N` positive millimetre values separated by `x`, `×`, `,` or `*`.
pub fn parse_dims<const N: usize>(s: &str) -> Result<[f64; N]> {
    let parts: Vec<&str> = s.split(['x', 'X', '×', ',', '*']).map(str::trim).collect();
    if parts.len() != N {
        return Err(Error::InvalidParams(format!("expected {N} sizes like 235x235x250, got {s:?}")));
    }
    let mut out = [0.0; N];
    for (o, p) in out.iter_mut().zip(&parts) {
        *o = p.parse::<f64>().map_err(|_| Error::InvalidParams(format!("bad size {p:?} in {s:?}")))?;
        if !(o.is_finite() && *o > 0.0 && *o <= 10_000.0) {
            return Err(Error::InvalidParams(format!("size {p} mm out of range in {s:?}")));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_owners_cr6se() {
        let p = default_printer();
        assert_eq!(p.id, "cr6se");
        assert_eq!(p.build_mm, [235.0, 235.0, 250.0]);
        assert_eq!(p.moving_bed_axis, Some(Axis::Y));
        assert_eq!(p.standing_face_axis(), Axis::Y);
        assert_eq!(p.standing_envelope(5.0), [225.0, 250.0]);
        let b = p.bed();
        assert!(b.fits([219.0, 10.0, 250.0], false));
        assert!(!b.fits([220.0, 10.0, 250.0], false));
        assert!(!b.fits([100.0, 100.0, 251.0], true));
        assert!(b.fits([10.0, 219.0, 1.0], true));
    }

    #[test]
    fn lookup_and_custom() {
        assert_eq!(printer_by_id("MK4").unwrap().build_mm, [250.0, 210.0, 220.0]);
        assert!(printer_by_id("nope").is_none());
        let c = custom_printer("300×300×400").unwrap();
        assert_eq!(c.build_mm, [300.0, 300.0, 400.0]);
        assert!(custom_printer("300x300").is_err());
        assert!(custom_printer("0x1x1").is_err());
        let ids: std::collections::HashSet<_> = printers().into_iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), printers().len());
    }

    #[test]
    fn serde_round_trip() {
        let p = default_printer();
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("\"movingBedAxis\":\"y\"") && s.contains("\"bedShape\":{\"kind\":\"rect\"}"), "{s}");
        assert_eq!(serde_json::from_str::<PrinterProfile>(&s).unwrap(), p);
        let round = PrinterProfile { bed_shape: BedShape::Circle { diameter_mm: 200.0 }, ..p };
        assert!((round.bed().x_mm - 141.421).abs() < 1e-3);
    }
}
