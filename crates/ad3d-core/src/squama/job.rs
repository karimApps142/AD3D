//! The M1 request (§12.1 DTO) and its resolved, validated form [`SquamaJob`].
//!
//! The v1 DTO fields stay required and unchanged; every M1 field is optional with
//! a default, so v1 payloads (the spike harness) still parse and give the M0 plate.

use std::time::Duration;

use glam::DVec3;
use serde::{Deserialize, Serialize};

use super::layout::{HexOrientation, LayoutKind};
use super::light::Light;
use super::optimise::OptimiserOpts;
use super::response::Response;
use super::solve::{Limits, Quality};
use super::{SquamaParams, SquamaParamsDto};
use crate::error::{Error, Result};
use crate::image::ImageOpts;

/// §12.1 `quality`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualityDto {
    #[default]
    Preview,
    Final,
}

impl From<QualityDto> for Quality {
    fn from(q: QualityDto) -> Quality {
        match q {
            QualityDto::Preview => Quality::Preview,
            QualityDto::Final => Quality::Final,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HexOrientationDto {
    Pointy,
    Flat,
}

/// §12.1 `layout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LayoutDto {
    Square,
    Hex { orientation: HexOrientationDto },
}

impl LayoutDto {
    pub fn kind(self) -> LayoutKind {
        match self {
            LayoutDto::Square => LayoutKind::Square,
            LayoutDto::Hex { orientation: HexOrientationDto::Pointy } => {
                LayoutKind::Hex { orientation: HexOrientation::Pointy }
            }
            LayoutDto::Hex { orientation: HexOrientationDto::Flat } => LayoutKind::Hex { orientation: HexOrientation::Flat },
        }
    }
}

/// §12.1 `light` (overrides the v1 directional light fields).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LightDto {
    Point {
        #[serde(rename = "posMm")]
        pos_mm: [f64; 3],
    },
}

/// §12.1 `response` (tilts in degrees).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ResponseDto {
    Lambert {
        #[serde(default)]
        ambient: f64,
    },
    Measured {
        #[serde(rename = "tiltDeg")]
        tilt_deg: Vec<f64>,
        brightness: Vec<f64>,
    },
}

/// §12.1 `optimiser`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum OptimiserDto {
    None,
    Shadow {
        #[serde(default, rename = "maxSweeps", skip_serializing_if = "Option::is_none")]
        max_sweeps: Option<u32>,
    },
}

/// Most sweeps a request may ask for.
pub const MAX_SWEEPS: u32 = 64;

/// A validated request, ready for the pipeline (millimetres, radians).
#[derive(Debug, Clone, PartialEq)]
pub struct SquamaJob {
    pub job_id: u32,
    /// The v1 parameters (directional light fields, limits, sizes).
    pub params: SquamaParams,
    pub layout: LayoutKind,
    pub light: Light,
    pub limits: Limits,
    pub response: Response,
    /// `None`: no shadow optimiser (Lambert / measured inverse only).
    pub optimiser: Option<OptimiserOpts>,
    pub image_opts: ImageOpts,
    pub quality: Quality,
    /// `"synthetic"` or a path.
    pub image: String,
}

impl SquamaJob {
    /// A job from v1 parameters only: square cells, directional light, Lambert, no optimiser.
    pub fn from_params(p: &SquamaParams) -> SquamaJob {
        SquamaJob {
            job_id: 0,
            params: *p,
            layout: LayoutKind::Square,
            light: Light::Directional { az: p.light_azimuth, el: p.light_elevation.max(1f64.to_radians()) },
            limits: Limits::from_params(p),
            response: Response::default(),
            optimiser: None,
            image_opts: ImageOpts::default(),
            quality: Quality::Preview,
            image: "synthetic".into(),
        }
    }

    pub fn is_synthetic(&self) -> bool {
        self.image == "synthetic"
    }

    /// The same job at another quality.
    pub fn with_quality(&self, q: Quality) -> SquamaJob {
        SquamaJob { quality: q, ..self.clone() }
    }
}

impl SquamaParamsDto {
    /// Resolves and validates the M1 fields (degrees → radians, defaults filled in).
    pub fn job(&self) -> Result<SquamaJob> {
        let p = self.params();
        let image_opts = self.image_opts.unwrap_or_default();
        image_opts.check()?;
        let light = match self.light {
            Some(LightDto::Point { pos_mm }) => {
                if !pos_mm.iter().all(|v| v.is_finite()) {
                    return Err(Error::InvalidParams("lamp position must be finite".into()));
                }
                Light::Point { pos_mm: DVec3::from_array(pos_mm) }
            }
            // The solver raises elevations under 1° with a warning; do the same here so
            // the optimiser and forward model see the light that was solved for.
            None => Light::Directional { az: p.light_azimuth, el: p.light_elevation.max(1f64.to_radians()) },
        };
        let response = match &self.response {
            None => Response::default(),
            Some(ResponseDto::Lambert { ambient }) => Response::Lambert { ambient: *ambient },
            Some(ResponseDto::Measured { tilt_deg, brightness }) => {
                if tilt_deg.len() != brightness.len() || tilt_deg.len() < 2 {
                    return Err(Error::InvalidParams("measured response needs ≥ 2 (tilt, brightness) pairs".into()));
                }
                // Ascending tilts (the forward model interpolates the raw table).
                let mut pts: Vec<(f64, f64)> =
                    tilt_deg.iter().map(|t| t.to_radians()).zip(brightness.iter().copied()).collect();
                pts.sort_by(|a, b| a.0.total_cmp(&b.0));
                Response::Measured { tilt_rad: pts.iter().map(|x| x.0).collect(), brightness: pts.iter().map(|x| x.1).collect() }
            }
        };
        let optimiser = match self.optimiser {
            None | Some(OptimiserDto::None) => None,
            Some(OptimiserDto::Shadow { max_sweeps }) => {
                let d = OptimiserOpts::default();
                let n = max_sweeps.unwrap_or(d.max_sweeps);
                if n > MAX_SWEEPS {
                    return Err(Error::InvalidParams(format!("maxSweeps {n} above {MAX_SWEEPS}")));
                }
                Some(OptimiserOpts { max_sweeps: n, time_budget: Some(Duration::from_secs(30)), ..d })
            }
        };
        if let Some(r) = self.max_relief_mm
            && !(r.is_finite() && r > 0.0)
        {
            return Err(Error::InvalidParams(format!("maxReliefMm {r} must be positive")));
        }
        Ok(SquamaJob {
            job_id: self.job_id,
            params: p,
            layout: self.layout.map_or(LayoutKind::Square, LayoutDto::kind),
            light,
            limits: Limits { max_tilt: p.max_tilt, base_mm: p.base_mm, max_relief_mm: self.max_relief_mm },
            response,
            optimiser,
            image_opts,
            quality: self.quality.unwrap_or_default().into(),
            image: self.image.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_payload_parses_to_the_m0_job() {
        let v1 = r#"{"jobId":3,"widthMm":300,"cellMm":6,"lightAzimuthDeg":180,"lightElevationDeg":5,
                     "maxTiltDeg":10,"baseMm":3,"image":"synthetic"}"#;
        let d: SquamaParamsDto = serde_json::from_str(v1).unwrap();
        let j = d.job().unwrap();
        assert_eq!(j.job_id, 3);
        assert_eq!(j.layout, LayoutKind::Square);
        assert_eq!(j.quality, Quality::Preview);
        assert!(j.optimiser.is_none());
        assert_eq!(j.response, Response::default());
        assert!(matches!(j.light, Light::Directional { .. }));
        assert_eq!(j.image_opts, ImageOpts::default());
    }

    #[test]
    fn m1_fields_parse() {
        let m1 = r#"{"jobId":4,"quality":"final","widthMm":200,"heightMm":150,"cellMm":5,
            "layout":{"kind":"hex","orientation":"flat"},"lightAzimuthDeg":180,"lightElevationDeg":8,
            "light":{"kind":"point","posMm":[-500,75,300]},"maxTiltDeg":12,"baseMm":3,"maxReliefMm":2.5,
            "image":"/tmp/x.png","imageOpts":{"crop":[0.1,0.1,0.9,0.9],"gamma":1.2,"autoLevels":null,"rotate":90,"mirror":true},
            "response":{"kind":"measured","tiltDeg":[10,-10,0],"brightness":[0.9,0.1,0.5]},
            "optimiser":{"kind":"shadow","maxSweeps":3}}"#;
        let d: SquamaParamsDto = serde_json::from_str(m1).unwrap();
        let j = d.job().unwrap();
        assert_eq!(j.quality, Quality::Final);
        assert_eq!(j.layout, LayoutKind::Hex { orientation: HexOrientation::Flat });
        assert!(matches!(j.light, Light::Point { pos_mm } if pos_mm.x == -500.0));
        assert_eq!(j.limits.max_relief_mm, Some(2.5));
        assert_eq!(j.image_opts.rotate_deg, 90);
        assert!(j.image_opts.mirror);
        match &j.response {
            Response::Measured { tilt_rad, brightness } => {
                assert!(tilt_rad.windows(2).all(|w| w[1] > w[0]));
                assert_eq!(brightness, &vec![0.1, 0.5, 0.9]);
            }
            r => panic!("{r:?}"),
        }
        assert_eq!(j.optimiser.unwrap().max_sweeps, 3);
        // Round trip keeps the M1 fields.
        let back: SquamaParamsDto = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn bad_m1_fields_are_refused() {
        let base = r#""jobId":1,"widthMm":100,"cellMm":5,"lightAzimuthDeg":180,"lightElevationDeg":5,"maxTiltDeg":10,"baseMm":3,"image":"synthetic""#;
        for extra in [
            r#""imageOpts":{"gamma":9}"#,
            r#""maxReliefMm":-1"#,
            r#""response":{"kind":"measured","tiltDeg":[1],"brightness":[1]}"#,
            r#""optimiser":{"kind":"shadow","maxSweeps":1000}"#,
        ] {
            let d: SquamaParamsDto = serde_json::from_str(&format!("{{{base},{extra}}}")).unwrap();
            assert!(d.job().is_err(), "{extra}");
        }
    }
}
