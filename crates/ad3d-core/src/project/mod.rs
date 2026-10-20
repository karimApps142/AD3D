//! `.ad3d` project files (FEATURES APP-FILES).
//!
//! A project is a zip with:
//! - `project.json`: [`ProjectDoc`] (`schemaVersion`, `generator`, the generator's
//!   params DTO exactly as the UI sends it, `imageOpts`, export settings and the
//!   printer profile it was made for);
//! - `image/<name>`: the original input image bytes, unchanged (optional);
//! - `thumbnail.png`: a small preview (optional).
//!
//! Generators are pure and deterministic, so reopening a project regenerates the
//! same mesh; no mesh is stored. Loading checks the schema version (refuses files
//! from a newer AD3D, migrates older ones), the generator id, the params' required
//! fields and the embedded image's size and hash.

pub mod thumbnail;

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::image::LinearLuma;
use crate::io::hash::fnv64_hex;
use crate::io::sink::{ExportFormat, safe_file_name};
use crate::io::zip::{Method, ZipArchive, ZipWriter};
use crate::orient::{Orientation, QuarterTurns};
use crate::profiles::{Bed, PrinterProfile, default_printer};

/// Current `schemaVersion`. Bump it, and add a step to [`migrate`], whenever
/// `project.json` changes in a way an older reader would misread.
pub const SCHEMA_VERSION: u32 = 1;
pub const FILE_EXTENSION: &str = "ad3d";
/// Generators this build can open.
pub const KNOWN_GENERATORS: &[&str] = &["squama"];
/// `params.image` value meaning "use the image embedded in the project".
pub const EMBEDDED_IMAGE: &str = "embedded";

const PROJECT_JSON: &str = "project.json";
const THUMBNAIL: &str = "thumbnail.png";
const MAX_JSON: u64 = 16 << 20;
const MAX_IMAGE: u64 = 512 << 20;
const MAX_THUMB: u64 = 16 << 20;

/// Image tone and crop options (design §12.1 `imageOpts`, §16).
///
/// MERGE: replace with the image module's `ImageOpts` DTO once it exists; the
/// JSON shape is the §12.1 one, so stored projects stay readable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOptsDto {
    /// Normalised `x0, y0, x1, y1` (y down).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gamma: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invert: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_levels: Option<[f64; 2]>,
}

impl ImageOptsDto {
    pub fn is_default(&self) -> bool {
        *self == ImageOptsDto::default()
    }
}

/// Export orientation (design §9, §12.1 `SquamaExport.orientation`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrientationDto {
    #[default]
    #[serde(rename = "standing-auto")]
    StandingAuto,
    #[serde(rename = "standing-q0")]
    StandingQ0,
    #[serde(rename = "standing-q1")]
    StandingQ1,
    #[serde(rename = "standing-q2")]
    StandingQ2,
    #[serde(rename = "standing-q3")]
    StandingQ3,
    #[serde(rename = "flat")]
    Flat,
}

impl OrientationDto {
    pub fn orientation(self) -> Orientation {
        match self {
            OrientationDto::StandingAuto => Orientation::Standing(QuarterTurns::Auto),
            OrientationDto::StandingQ0 => Orientation::Standing(QuarterTurns::Q0),
            OrientationDto::StandingQ1 => Orientation::Standing(QuarterTurns::Q1),
            OrientationDto::StandingQ2 => Orientation::Standing(QuarterTurns::Q2),
            OrientationDto::StandingQ3 => Orientation::Standing(QuarterTurns::Q3),
            OrientationDto::Flat => Orientation::Flat,
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        serde_json::from_value(Value::String(s.to_string()))
            .or_else(|_| if s == "standing" { Ok(OrientationDto::StandingAuto) } else { Err(()) })
            .map_err(|_| Error::InvalidParams(format!("unknown orientation {s:?} (standing, standing-q0..q3, flat)")))
    }
}

/// Alignment keys between tiles (§14.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeysDto {
    #[default]
    None,
    Pegs,
}

/// Tiling choice: `"auto"` (fit the bed), `"none"` (one part), or `[nx, ny]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TilesDto {
    Mode(TileMode),
    Grid([u32; 2]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TileMode {
    Auto,
    None,
}

impl Default for TilesDto {
    fn default() -> Self {
        TilesDto::Mode(TileMode::Auto)
    }
}

impl TilesDto {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(TilesDto::Mode(TileMode::Auto)),
            "none" | "1x1" => Ok(TilesDto::Mode(TileMode::None)),
            g => {
                let p: Vec<&str> = g.split(['x', '×']).collect();
                let n = |t: &str| t.trim().parse::<u32>().ok().filter(|&v| (1..=64).contains(&v));
                match (p.len(), p.first().and_then(|t| n(t)), p.get(1).and_then(|t| n(t))) {
                    (2, Some(a), Some(b)) => Ok(TilesDto::Grid([a, b])),
                    _ => Err(Error::InvalidParams(format!("tiles must be auto, none or NxM (1..64), got {s:?}"))),
                }
            }
        }
    }
}

/// Export settings (design §12.1 `SquamaExport` without `params` and `outDir`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSettings {
    pub formats: Vec<ExportFormat>,
    #[serde(default)]
    pub orientation: OrientationDto,
    /// Overrides the printer profile's build volume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_mm: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_mm: Option<f64>,
    #[serde(default)]
    pub tiles: TilesDto,
    #[serde(default)]
    pub keys: KeysDto,
    #[serde(default)]
    pub stand: bool,
    #[serde(default)]
    pub calibration_tile: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        ExportSettings {
            formats: vec![ExportFormat::ThreeMf],
            orientation: OrientationDto::default(),
            bed_mm: None,
            margin_mm: None,
            tiles: TilesDto::default(),
            keys: KeysDto::None,
            stand: false,
            calibration_tile: false,
        }
    }
}

impl ExportSettings {
    /// The bed to tile and lay out on: `bedMm`/`marginMm` override the printer.
    pub fn bed(&self, printer: &PrinterProfile) -> Bed {
        let margin = self.margin_mm.unwrap_or(printer.margin_mm);
        match self.bed_mm {
            Some([x, y, z]) => Bed { x_mm: x, y_mm: y, z_mm: z, margin_mm: margin },
            None => printer.bed_with_margin(margin),
        }
    }
}

/// Reference to the embedded image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    /// Zip entry, `image/<file name>`.
    pub entry: String,
    /// The file name the user picked.
    pub original_name: String,
    pub size: u64,
    /// FNV-1a 64 of the bytes (integrity, not security).
    pub fnv64: String,
}

/// `project.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDoc {
    pub schema_version: u32,
    pub generator: String,
    /// AD3D version that wrote the file.
    pub app_version: String,
    /// The generator's params DTO as the UI sends it (design §12.1 for Squama),
    /// stored verbatim so newer optional fields survive a round trip.
    pub params: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_opts: Option<ImageOptsDto>,
    #[serde(default)]
    pub export: ExportSettings,
    #[serde(default = "default_printer")]
    pub printer: PrinterProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
}

/// A project in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub doc: ProjectDoc,
    pub image_bytes: Option<Vec<u8>>,
    pub thumbnail_png: Option<Vec<u8>>,
}

/// Squama v1 params that must be present (design §12.1).
const SQUAMA_REQUIRED: &[&str] =
    &["widthMm", "cellMm", "lightAzimuthDeg", "lightElevationDeg", "maxTiltDeg", "baseMm", "image"];

fn check_params(generator: &str, params: &Value) -> Result<()> {
    let obj = params.as_object().ok_or_else(|| Error::InvalidParams("project params must be a JSON object".into()))?;
    if generator == "squama" {
        for k in SQUAMA_REQUIRED {
            if !obj.contains_key(*k) {
                return Err(Error::InvalidParams(format!("project params are missing {k:?}")));
            }
        }
        // Typed check of the v1 fields (extra M1 fields are allowed).
        let mut p = params.clone();
        p.as_object_mut().unwrap().entry("jobId").or_insert(Value::from(0));
        serde_json::from_value::<crate::squama::SquamaParamsDto>(p)
            .map_err(|e| Error::InvalidParams(format!("project params: {e}")))?;
    }
    Ok(())
}

/// Upgrades an older `project.json` value to [`SCHEMA_VERSION`] in place.
fn migrate(doc: &mut Value, from: u32) -> Result<()> {
    // v1 is the first schema; future steps go here as `if from < 2 { ... }`.
    if from == 0 {
        return Err(Error::InvalidParams("project schemaVersion 0 is not valid".into()));
    }
    doc["schemaVersion"] = Value::from(SCHEMA_VERSION);
    Ok(())
}

impl Project {
    /// A new Squama project. `params` is the §12.1 DTO; `image` is the original
    /// file name and bytes, or `None` for the synthetic target.
    pub fn new_squama(
        mut params: Value,
        image_opts: Option<ImageOptsDto>,
        image: Option<(&str, Vec<u8>)>,
        export: ExportSettings,
        printer: PrinterProfile,
    ) -> Result<Self> {
        let obj = params.as_object_mut().ok_or_else(|| Error::InvalidParams("params must be a JSON object".into()))?;
        // Per-run fields do not belong in a saved document.
        obj.remove("jobId");
        obj.remove("quality");
        let image_ref = match &image {
            Some((name, bytes)) => {
                obj.insert("image".into(), Value::from(EMBEDDED_IMAGE));
                let original = Path::new(name).file_name().and_then(|s| s.to_str()).unwrap_or("image").to_string();
                Some(ImageRef {
                    entry: format!("image/{}", safe_file_name(&original)),
                    original_name: original,
                    size: bytes.len() as u64,
                    fnv64: fnv64_hex(bytes),
                })
            }
            None => {
                obj.insert("image".into(), Value::from("synthetic"));
                None
            }
        };
        let image_opts = image_opts.filter(|o| !o.is_default());
        let doc = ProjectDoc {
            schema_version: SCHEMA_VERSION,
            generator: "squama".into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            params,
            image_opts,
            export,
            printer,
            image: image_ref,
        };
        check_params(&doc.generator, &doc.params)?;
        Ok(Project { doc, image_bytes: image.map(|(_, b)| b), thumbnail_png: None })
    }

    /// The params DTO ready for the generator: `jobId` filled in and `imageOpts`
    /// merged back in (design §12.1 keeps them inside the params object).
    pub fn params_dto(&self, job_id: u32) -> Value {
        let mut p = self.doc.params.clone();
        if let Some(o) = p.as_object_mut() {
            o.insert("jobId".into(), Value::from(job_id));
            if let Some(io) = &self.doc.image_opts {
                o.insert("imageOpts".into(), serde_json::to_value(io).unwrap_or(Value::Null));
            }
        }
        p
    }

    pub fn is_synthetic(&self) -> bool {
        self.doc.params.get("image").and_then(Value::as_str) == Some("synthetic")
    }

    /// Decodes the embedded image (EXIF orientation applied, linearised).
    pub fn luma(&self) -> Result<Option<LinearLuma>> {
        match &self.image_bytes {
            Some(b) => luma_from_bytes(b).map(Some),
            None => Ok(None),
        }
    }

    /// Writes the project to `w`.
    pub fn write_to<W: Write + Seek>(&self, w: W) -> Result<W> {
        let mut doc = self.doc.clone();
        doc.schema_version = SCHEMA_VERSION;
        check_params(&doc.generator, &doc.params)?;
        if let (Some(r), Some(b)) = (&mut doc.image, &self.image_bytes) {
            r.size = b.len() as u64;
            r.fnv64 = fnv64_hex(b);
        } else if doc.image.is_some() != self.image_bytes.is_some() {
            return Err(Error::InvalidParams("project image reference and bytes disagree".into()));
        }
        let mut z = ZipWriter::new(w)?;
        let json = serde_json::to_vec_pretty(&doc).map_err(|e| Error::Internal(format!("project json: {e}")))?;
        z.add(PROJECT_JSON, Method::Deflate, &json)?;
        if let (Some(r), Some(b)) = (&doc.image, &self.image_bytes) {
            // Photos are already compressed; storing keeps saves fast.
            z.add(&r.entry, Method::Stored, b)?;
        }
        if let Some(t) = &self.thumbnail_png {
            z.add(THUMBNAIL, Method::Stored, t)?;
        }
        z.finish()
    }

    /// Saves atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        }
        crate::io::stl::write_atomic(path, |w| self.write_to(w).map(|_| ()))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(self.write_to(Cursor::new(Vec::new()))?.into_inner())
    }

    /// Opens and checks a project file.
    pub fn load(path: &Path) -> Result<Self> {
        let f = File::open(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        Self::read_from(BufReader::new(f)).map_err(|e| match e {
            Error::Io(m) => Error::Io(format!("{}: {m}", path.display())),
            Error::InvalidParams(m) => Error::InvalidParams(format!("{}: {m}", path.display())),
            e => e,
        })
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        Self::read_from(Cursor::new(b))
    }

    pub fn read_from<R: Read + Seek>(r: R) -> Result<Self> {
        let mut z = ZipArchive::new(r).map_err(|e| match e {
            Error::Io(m) => Error::Io(format!("not an AD3D project ({m})")),
            e => e,
        })?;
        let json = z.read(PROJECT_JSON, MAX_JSON).map_err(|_| Error::Io("not an AD3D project (no project.json)".into()))?;
        let mut v: Value = serde_json::from_slice(&json).map_err(|e| Error::Io(format!("project.json: {e}")))?;
        let ver = v
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::InvalidParams("project.json has no schemaVersion".into()))?;
        if ver > SCHEMA_VERSION as u64 {
            return Err(Error::InvalidParams(format!(
                "project was saved by a newer AD3D (schema {ver}, this build reads up to {SCHEMA_VERSION}); update AD3D to open it"
            )));
        }
        if (ver as u32) < SCHEMA_VERSION {
            migrate(&mut v, ver as u32)?;
        } else if ver == 0 {
            return Err(Error::InvalidParams("project schemaVersion 0 is not valid".into()));
        }
        let doc: ProjectDoc = serde_json::from_value(v).map_err(|e| Error::InvalidParams(format!("project.json: {e}")))?;
        if !KNOWN_GENERATORS.contains(&doc.generator.as_str()) {
            return Err(Error::InvalidParams(format!(
                "project uses generator {:?}, which this build does not have ({})",
                doc.generator,
                KNOWN_GENERATORS.join(", ")
            )));
        }
        check_params(&doc.generator, &doc.params)?;
        let image_bytes = match &doc.image {
            Some(r) => {
                let b = z.read(&r.entry, MAX_IMAGE)?;
                if b.len() as u64 != r.size || fnv64_hex(&b) != r.fnv64 {
                    return Err(Error::Io(format!("embedded image {} does not match its recorded size/hash", r.entry)));
                }
                Some(b)
            }
            None => {
                if doc.params.get("image").and_then(Value::as_str) == Some(EMBEDDED_IMAGE) {
                    return Err(Error::InvalidParams("params refer to an embedded image but the project has none".into()));
                }
                None
            }
        };
        let thumbnail_png = match z.find(THUMBNAIL) {
            Some(_) => Some(z.read(THUMBNAIL, MAX_THUMB)?),
            None => None,
        };
        Ok(Project { doc, image_bytes, thumbnail_png })
    }
}

/// Decodes image bytes, applies EXIF orientation and linearises (design §16 steps 1–2).
///
/// MERGE: move next to `image::LinearLuma::load` (same steps, from memory).
pub fn luma_from_bytes(bytes: &[u8]) -> Result<LinearLuma> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Error::Io(format!("image: {e}")))?;
    let mut dec = reader.into_decoder().map_err(|e| Error::Io(format!("image: {e}")))?;
    let orient = dec.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(dec).map_err(|e| Error::Io(format!("image: {e}")))?;
    img.apply_orientation(orient);
    Ok(LinearLuma::from_dynamic(&img))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn params() -> Value {
        json!({ "jobId": 7, "widthMm": 120.0, "cellMm": 6.0, "lightAzimuthDeg": 180.0, "lightElevationDeg": 5.0,
                "maxTiltDeg": 10.0, "baseMm": 3.0, "image": "/somewhere/photo.png",
                "layout": { "kind": "hex", "orientation": "pointy" } })
    }

    fn png_bytes() -> Vec<u8> {
        let img = image::GrayImage::from_fn(4, 3, |x, y| image::Luma([(x * 60 + y * 10) as u8]));
        let mut c = Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img).write_to(&mut c, image::ImageFormat::Png).unwrap();
        c.into_inner()
    }

    #[test]
    fn round_trip_with_image_and_thumbnail() {
        let opts = ImageOptsDto { gamma: Some(1.2), invert: Some(true), ..Default::default() };
        let mut p = Project::new_squama(params(), Some(opts.clone()), Some(("dir/photo.png", png_bytes())), ExportSettings::default(), default_printer())
            .unwrap();
        p.thumbnail_png = Some(png_bytes());
        let bytes = p.to_bytes().unwrap();
        let q = Project::from_bytes(&bytes).unwrap();
        assert_eq!(q, p);
        assert_eq!(q.doc.params["image"], EMBEDDED_IMAGE);
        assert!(q.doc.params.get("jobId").is_none());
        assert_eq!(q.doc.params["layout"]["kind"], "hex", "unknown M1 fields survive");
        assert_eq!(q.doc.image.as_ref().unwrap().original_name, "photo.png");
        let l = q.luma().unwrap().unwrap();
        assert_eq!((l.w, l.h), (4, 3));
        let dto = q.params_dto(42);
        assert_eq!(dto["jobId"], 42);
        assert_eq!(dto["imageOpts"]["gamma"], 1.2);
        // Deterministic bytes.
        assert_eq!(q.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn synthetic_project_and_export_settings_json() {
        let ex = ExportSettings {
            formats: vec![ExportFormat::Stl, ExportFormat::ThreeMf],
            orientation: OrientationDto::StandingQ1,
            tiles: TilesDto::Grid([2, 1]),
            keys: KeysDto::Pegs,
            stand: true,
            ..Default::default()
        };
        let p = Project::new_squama(params(), None, None, ex, default_printer()).unwrap();
        assert!(p.is_synthetic());
        let s = String::from_utf8(p.to_bytes().unwrap()).unwrap_or_default();
        let _ = s; // binary zip; just make sure it serialises
        let j = serde_json::to_value(&p.doc).unwrap();
        assert_eq!(j["schemaVersion"], 1);
        assert_eq!(j["export"]["formats"], json!(["stl", "3mf"]));
        assert_eq!(j["export"]["orientation"], "standing-q1");
        assert_eq!(j["export"]["tiles"], json!([2, 1]));
        assert_eq!(j["export"]["keys"], "pegs");
        assert_eq!(j["printer"]["id"], "cr6se");
        let q = Project::from_bytes(&p.to_bytes().unwrap()).unwrap();
        assert_eq!(q.doc.export.tiles, TilesDto::Grid([2, 1]));
        assert_eq!(q.doc.export.bed(&q.doc.printer), Bed { x_mm: 235.0, y_mm: 235.0, z_mm: 250.0, margin_mm: 5.0 });
    }

    fn with_json(doc: &Value) -> Vec<u8> {
        let mut z = ZipWriter::new(Cursor::new(Vec::new())).unwrap();
        z.add(PROJECT_JSON, Method::Deflate, &serde_json::to_vec(doc).unwrap()).unwrap();
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn version_and_content_checks() {
        let p = Project::new_squama(params(), None, None, ExportSettings::default(), default_printer()).unwrap();
        let good = serde_json::to_value(&p.doc).unwrap();
        assert!(Project::from_bytes(&with_json(&good)).is_ok());

        let mut newer = good.clone();
        newer["schemaVersion"] = json!(SCHEMA_VERSION + 1);
        let e = Project::from_bytes(&with_json(&newer)).unwrap_err();
        assert!(e.to_string().contains("newer AD3D"), "{e}");

        let mut zero = good.clone();
        zero["schemaVersion"] = json!(0);
        assert!(Project::from_bytes(&with_json(&zero)).is_err());

        let mut missing = good.clone();
        missing.as_object_mut().unwrap().remove("schemaVersion");
        assert!(Project::from_bytes(&with_json(&missing)).is_err());

        let mut gen_ = good.clone();
        gen_["generator"] = json!("lithophane");
        assert!(Project::from_bytes(&with_json(&gen_)).unwrap_err().to_string().contains("generator"));

        let mut bad = good.clone();
        bad["params"].as_object_mut().unwrap().remove("cellMm");
        assert!(Project::from_bytes(&with_json(&bad)).is_err());

        let mut emb = good.clone();
        emb["params"]["image"] = json!(EMBEDDED_IMAGE);
        assert!(Project::from_bytes(&with_json(&emb)).is_err());

        assert!(Project::from_bytes(b"not a zip").is_err());
    }

    #[test]
    fn tampered_image_is_rejected() {
        let p = Project::new_squama(params(), None, Some(("a.png", png_bytes())), ExportSettings::default(), default_printer()).unwrap();
        let mut doc = serde_json::to_value(&p.doc).unwrap();
        doc["image"]["fnv64"] = json!("0000000000000000");
        let mut z = ZipWriter::new(Cursor::new(Vec::new())).unwrap();
        z.add(PROJECT_JSON, Method::Deflate, &serde_json::to_vec(&doc).unwrap()).unwrap();
        z.add("image/a.png", Method::Stored, &png_bytes()).unwrap();
        let bytes = z.finish().unwrap().into_inner();
        assert!(Project::from_bytes(&bytes).unwrap_err().to_string().contains("hash"));
    }

    #[test]
    fn parse_helpers() {
        assert_eq!(TilesDto::parse("auto").unwrap(), TilesDto::Mode(TileMode::Auto));
        assert_eq!(TilesDto::parse("none").unwrap(), TilesDto::Mode(TileMode::None));
        assert_eq!(TilesDto::parse("2x3").unwrap(), TilesDto::Grid([2, 3]));
        assert!(TilesDto::parse("0x3").is_err());
        assert_eq!(OrientationDto::parse("standing").unwrap(), OrientationDto::StandingAuto);
        assert_eq!(OrientationDto::parse("flat").unwrap(), OrientationDto::Flat);
        assert!(OrientationDto::parse("sideways").is_err());
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("ad3d-project-{}", std::process::id()));
        let path = dir.join("p.ad3d");
        let p = Project::new_squama(params(), None, None, ExportSettings::default(), default_printer()).unwrap();
        p.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), p);
        std::fs::remove_dir_all(&dir).ok();
    }
}
