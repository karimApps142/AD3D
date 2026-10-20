//! Design lights (§15.4): a distant directional light, or a point light (a bare
//! bulb at a known position) that gives every facet its own light direction,
//! elevation and `1/d²` irradiance.

use glam::{DVec2, DVec3};

use super::SquamaParams;
use crate::error::{Error, Result, Warning};

/// Lowest per-facet elevation the solver accepts (raised to this with a warning).
pub const MIN_ELEVATION_DEG: f64 = 1.0;
/// Highest directional elevation (§2).
pub const MAX_DIRECTIONAL_ELEVATION_DEG: f64 = 45.0;
/// Highest per-facet elevation under a point light (capped with a warning); keeps
/// the in-plane light direction well defined.
pub const MAX_POINT_ELEVATION_DEG: f64 = 85.0;

/// The light the plate is designed for. Plate space (§1), mm and radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Light {
    /// `az`: direction the light comes from in the plate plane (`a = (cos, sin)`);
    /// `el`: grazing angle above the plate.
    Directional { az: f64, el: f64 },
    /// Bulb position in plate space; must be in front of the plate (`w > 0`).
    Point { pos_mm: DVec3 },
}

impl Default for Light {
    fn default() -> Self {
        Light::Directional { az: std::f64::consts::PI, el: 5f64.to_radians() }
    }
}

/// The light as one facet sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellLight {
    /// In-plane unit vector from the facet toward the light.
    pub a: DVec2,
    /// Elevation of the light above the plate plane at this facet (radians).
    pub el: f64,
    /// Relative irradiance (directional: 1; point: `d_min²/d²`, so the nearest
    /// facet gets 1).
    pub irradiance: f64,
}

impl Light {
    /// The v1 DTO light (`light_azimuth`, `light_elevation`).
    pub fn from_params(p: &SquamaParams) -> Self {
        Light::Directional { az: p.light_azimuth, el: p.light_elevation }
    }

    /// Per-facet light at the facet centres `centres` (plate `u, v`), taking the
    /// facet surface at height `w0` (the base `T` is close enough: the light
    /// geometry changes by well under 0.1° over a millimetre of relief).
    pub fn cell_lights(&self, centres: &[DVec2], w0: f64) -> Result<(Vec<CellLight>, Vec<Warning>)> {
        let mut warnings = Vec::new();
        let (lo, hi_dir, hi_pt) =
            (MIN_ELEVATION_DEG.to_radians(), MAX_DIRECTIONAL_ELEVATION_DEG.to_radians(), MAX_POINT_ELEVATION_DEG.to_radians());
        match *self {
            Light::Directional { az, el } => {
                if !(az.is_finite() && el.is_finite()) {
                    return Err(Error::InvalidParams("non-finite light direction".into()));
                }
                if el < 0.0 || el > hi_dir + 1e-9 {
                    return Err(Error::InvalidParams(format!(
                        "elevation {:.3}° outside [{MIN_ELEVATION_DEG}°, {MAX_DIRECTIONAL_ELEVATION_DEG}°]",
                        el.to_degrees()
                    )));
                }
                let el = if el < lo {
                    warnings.push(Warning { code: "elevation_raised", message: "light elevation raised to 1°".into() });
                    lo
                } else {
                    el
                };
                let cl = CellLight { a: DVec2::new(az.cos(), az.sin()), el, irradiance: 1.0 };
                Ok((vec![cl; centres.len()], warnings))
            }
            Light::Point { pos_mm } => {
                if !pos_mm.is_finite() {
                    return Err(Error::InvalidParams("non-finite point-light position".into()));
                }
                if pos_mm.z <= w0 {
                    return Err(Error::InvalidParams(format!(
                        "point light at w = {:.1} mm is not in front of the plate (w > {w0:.1} mm)",
                        pos_mm.z
                    )));
                }
                let mut out = Vec::with_capacity(centres.len());
                let (mut raised, mut capped) = (0usize, 0usize);
                let mut d2min = f64::INFINITY;
                for c in centres {
                    let l = pos_mm - DVec3::new(c.x, c.y, w0);
                    let horiz = l.truncate().length();
                    let mut el = l.z.atan2(horiz);
                    if el < lo {
                        el = lo;
                        raised += 1;
                    } else if el > hi_pt {
                        el = hi_pt;
                        capped += 1;
                    }
                    let a = if horiz > 1e-9 { l.truncate() / horiz } else { DVec2::X };
                    let d2 = l.length_squared();
                    d2min = d2min.min(d2);
                    out.push(CellLight { a, el, irradiance: d2 });
                }
                for cl in &mut out {
                    cl.irradiance = d2min / cl.irradiance;
                }
                if raised > 0 {
                    warnings.push(Warning {
                        code: "elevation_raised",
                        message: format!("light elevation raised to {MIN_ELEVATION_DEG}° at {raised} facets"),
                    });
                }
                if capped > 0 {
                    warnings.push(Warning {
                        code: "elevation_capped",
                        message: format!("light nearly overhead: elevation capped at {MAX_POINT_ELEVATION_DEG}° at {capped} facets"),
                    });
                }
                Ok((out, warnings))
            }
        }
    }

    /// In-plane direction at the plate centre `(W/2, H/2)` (used for export
    /// orientation and calibration under a point light).
    pub fn reference(&self, w_mm: f64, h_mm: f64, w0: f64) -> Result<CellLight> {
        let (v, _) = self.cell_lights(&[DVec2::new(w_mm / 2.0, h_mm / 2.0)], w0)?;
        Ok(v[0])
    }
}

/// Distance from the plate centre (along the centre's light direction, same
/// elevation) at which a point light's `1/d²` falloff over a plate of diagonal
/// `diag_mm` stays within `max_falloff` (e.g. 0.2 = the far corner gets 80 %).
/// A rough guide for the UI's lamp-distance suggestion.
pub fn suggest_lamp_distance(diag_mm: f64, max_falloff: f64) -> f64 {
    let f = max_falloff.clamp(1e-3, 0.99);
    // (d + diag/2)² / d² ≤ 1/(1−f)  ⇒  d ≥ (diag/2) / (1/√(1−f) − 1).
    0.5 * diag_mm / (1.0 / (1.0 - f).sqrt() - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directional_is_uniform() {
        let l = Light::Directional { az: 0.3, el: 10f64.to_radians() };
        let (v, w) = l.cell_lights(&[DVec2::ZERO, DVec2::new(50.0, 20.0)], 3.0).unwrap();
        assert!(w.is_empty());
        assert_eq!(v[0], v[1]);
        assert!((v[0].a - DVec2::new(0.3f64.cos(), 0.3f64.sin())).length() < 1e-15);
        assert!(Light::Directional { az: 0.0, el: 50f64.to_radians() }.cell_lights(&[DVec2::ZERO], 3.0).is_err());
        let (v, w) = Light::Directional { az: 0.0, el: 0.0 }.cell_lights(&[DVec2::ZERO], 3.0).unwrap();
        assert_eq!(w[0].code, "elevation_raised");
        assert!((v[0].el - 1f64.to_radians()).abs() < 1e-15);
    }

    #[test]
    fn point_light_direction_and_falloff() {
        // Bulb 100 mm to the left of the plate's left edge, 30 mm out.
        let l = Light::Point { pos_mm: DVec3::new(-100.0, 50.0, 33.0) };
        let cs = [DVec2::new(0.0, 50.0), DVec2::new(100.0, 50.0), DVec2::new(0.0, 0.0)];
        let (v, _) = l.cell_lights(&cs, 3.0).unwrap();
        // Light comes from the left (a ≈ −u) at every facet on the centre line.
        assert!((v[0].a - DVec2::new(-1.0, 0.0)).length() < 1e-12);
        assert!((v[0].el - (30f64 / 100.0).atan()).abs() < 1e-12);
        assert!(v[1].el < v[0].el, "farther facet sees a lower light");
        assert_eq!(v[0].irradiance, 1.0);
        assert!((v[1].irradiance - (100.0f64.powi(2) + 900.0) / (200.0f64.powi(2) + 900.0)).abs() < 1e-12);
        assert!(v[2].a.y > 0.0, "facet below the bulb's line sees light from above-left");
        assert!(Light::Point { pos_mm: DVec3::new(0.0, 0.0, 2.0) }.cell_lights(&cs, 3.0).is_err());
    }

    #[test]
    fn lamp_distance_guide() {
        let d = suggest_lamp_distance(300.0, 0.2);
        let ratio = ((d + 150.0) / d).powi(2);
        assert!((1.0 / ratio - 0.8).abs() < 1e-9);
    }
}
