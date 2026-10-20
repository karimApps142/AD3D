//! 3MF (core specification 1.3) writer and reader.
//!
//! Writer (design §14.4):
//! - `unit="millimeter"`, one `<object>` per part, one `<item>` per part in `<build>`.
//! - Vertex data is the part's mesh mapped through its orientation [`Xform`] with
//!   `Xform::apply_f32` and printed with Rust's shortest round-trip `f32`
//!   formatting, so the 3MF carries exactly the values the STL writer writes and
//!   `validate()` stage 2 checked. Bit-identical written positions are welded, the
//!   same weld stage 2 does, so the 3MF topology is the one that was validated.
//! - Build items carry the bed layout as pure translations ([`pack_on_bed`]):
//!   parts sit on `z = 0`, inside the bed margin, without overlapping. Parts that
//!   do not fit on one bed go to further "beds" laid out along +X.
//! - Metadata: Title, Designer, Description, Application, CreationDate, plus
//!   `ad3d:*` entries (namespace [`AD3D_NS`]) such as the params JSON.
//! - Streaming: the model XML goes straight into a deflating zip entry through a
//!   64 KiB staging buffer; no whole-file string is ever built.
//!
//! Reader: enough of the core spec to re-read our own files and typical slicer
//! exports (objects with meshes, build items with transforms, units, metadata).
//! Components and extensions are refused or ignored with an error message.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufReader, Read, Seek, Write};
use std::path::Path;

use quick_xml::events::{BytesStart, Event};

use crate::cancel::CancelToken;
use crate::error::{Error, Result};
use crate::io::zip::{Method, ZipArchive, ZipWriter};
use crate::mesh::{Bounds, FaceKind, Mesh, weld_key};
use crate::orient::Xform;
use crate::profiles::Bed;

pub const CORE_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
/// Namespace for our own metadata names (`ad3d:params`, ...).
pub const AD3D_NS: &str = "urn:ad3d:3mf:2026";
const MODEL_PATH: &str = "3D/3dmodel.model";
const THUMB_PATH: &str = "Metadata/thumbnail.png";
const REL_MODEL: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";
const REL_THUMB: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
const FLUSH_AT: usize = 64 * 1024;

/// Package metadata. `custom` names are written as `ad3d:<name>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub title: Option<String>,
    pub designer: Option<String>,
    pub description: Option<String>,
    /// ISO 8601 date, e.g. `2026-10-20`. Left out by default so output is reproducible.
    pub creation_date: Option<String>,
    pub custom: Vec<(String, String)>,
}

/// What one written object contains.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectInfo {
    pub id: u32,
    pub name: String,
    /// Bounds of the written (transformed) vertices.
    pub bounds: Option<Bounds>,
    pub vertices: usize,
    pub triangles: usize,
}

/// One `<item>` in `<build>`: `p_bed = transform · p_object`.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildItem {
    pub object_id: u32,
    pub transform: Xform,
    pub part_number: Option<String>,
}

/// Escapes text for XML attribute values and character data.
pub fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            // XML 1.0 forbids most control characters even escaped; drop them.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

/// 3MF `ST_Matrix3D` string (row-vector convention: `x' = x·m00 + y·m10 + z·m20 + m30`).
pub fn matrix3d(x: &Xform) -> String {
    let m = &x.m;
    let v = [
        m[0][0], m[1][0], m[2][0], m[0][1], m[1][1], m[2][1], m[0][2], m[1][2], m[2][2], x.t[0], x.t[1], x.t[2],
    ];
    v.iter().map(|f| format!("{}", f + 0.0)).collect::<Vec<_>>().join(" ")
}

/// Parses an `ST_Matrix3D` string into an [`Xform`].
pub fn parse_matrix3d(s: &str) -> Result<Xform> {
    let v: Vec<f64> = s.split_whitespace().map(|t| t.parse::<f64>()).collect::<std::result::Result<_, _>>()
        .map_err(|_| Error::Io(format!("3mf: bad transform {s:?}")))?;
    if v.len() != 12 || v.iter().any(|x| !x.is_finite()) {
        return Err(Error::Io(format!("3mf: transform needs 12 finite numbers, got {s:?}")));
    }
    Ok(Xform {
        m: [[v[0], v[3], v[6]], [v[1], v[4], v[7]], [v[2], v[5], v[8]]],
        t: [v[9], v[10], v[11]],
    })
}

/// Streaming 3MF writer: [`new`](Self::new), [`add_object`](Self::add_object)
/// per part, then [`finish`](Self::finish) with the build items.
pub struct ThreeMfWriter<W: Write + Seek> {
    zip: ZipWriter<W>,
    buf: String,
    objects: Vec<ObjectInfo>,
    cancel: CancelToken,
}

impl<W: Write + Seek> ThreeMfWriter<W> {
    pub fn new(w: W, meta: &Meta, thumbnail_png: Option<&[u8]>) -> Result<Self> {
        Self::with_cancel(w, meta, thumbnail_png, CancelToken::new())
    }

    /// As [`new`](Self::new); `cancel` is checked every 64k vertices/triangles.
    pub fn with_cancel(w: W, meta: &Meta, thumbnail_png: Option<&[u8]>, cancel: CancelToken) -> Result<Self> {
        let mut zip = ZipWriter::new(w)?;
        let mut ct = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n\
             <Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\n",
        );
        if thumbnail_png.is_some() {
            ct.push_str("<Default Extension=\"png\" ContentType=\"image/png\"/>\n");
        }
        ct.push_str("</Types>\n");
        zip.add("[Content_Types].xml", Method::Deflate, ct.as_bytes())?;
        let mut rels = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n\
             <Relationship Target=\"/{MODEL_PATH}\" Id=\"rel0\" Type=\"{REL_MODEL}\"/>\n"
        );
        if thumbnail_png.is_some() {
            rels.push_str(&format!("<Relationship Target=\"/{THUMB_PATH}\" Id=\"rel1\" Type=\"{REL_THUMB}\"/>\n"));
        }
        rels.push_str("</Relationships>\n");
        zip.add("_rels/.rels", Method::Deflate, rels.as_bytes())?;
        if let Some(png) = thumbnail_png {
            zip.add(THUMB_PATH, Method::Stored, png)?;
        }
        zip.begin(MODEL_PATH, Method::Deflate)?;
        let mut s = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"{CORE_NS}\" xmlns:ad3d=\"{AD3D_NS}\">\n"
        );
        let mut md = |name: &str, v: &str| {
            s.push_str(&format!(" <metadata name=\"{}\">{}</metadata>\n", xml_escape(name), xml_escape(v)));
        };
        if let Some(t) = &meta.title {
            md("Title", t);
        }
        if let Some(t) = &meta.designer {
            md("Designer", t);
        }
        if let Some(t) = &meta.description {
            md("Description", t);
        }
        md("Application", concat!("AD3D ", env!("CARGO_PKG_VERSION")));
        if let Some(t) = &meta.creation_date {
            md("CreationDate", t);
        }
        for (k, v) in &meta.custom {
            md(&format!("ad3d:{k}"), v);
        }
        s.push_str(" <resources>\n");
        let mut w = ThreeMfWriter { zip, buf: String::with_capacity(FLUSH_AT + 4096), objects: Vec::new(), cancel };
        w.buf.push_str(&s);
        Ok(w)
    }

    fn flush_if_full(&mut self) -> Result<()> {
        if self.buf.len() >= FLUSH_AT {
            self.zip.write_all(self.buf.as_bytes())?;
            self.buf.clear();
        }
        Ok(())
    }

    /// Writes `mesh` mapped through `x` as a new object and returns its info.
    /// Fails if two corners of a triangle become the same written vertex.
    pub fn add_object(&mut self, name: &str, mesh: &Mesh, x: &Xform) -> Result<ObjectInfo> {
        let id = self.objects.len() as u32 + 1;
        let flip = x.det() < 0.0;
        let nv = mesh.vertex_count();
        let mut map: HashMap<[u32; 3], u32> = HashMap::with_capacity(nv);
        let mut remap: Vec<u32> = Vec::with_capacity(nv);
        let mut bmin = [f32::INFINITY; 3];
        let mut bmax = [f32::NEG_INFINITY; 3];
        let _ = writeln!(self.buf, "  <object id=\"{id}\" type=\"model\" name=\"{}\">\n   <mesh>\n    <vertices>", xml_escape(name));
        let mut unique = 0u32;
        for i in 0..nv as u32 {
            if i & 0xFFFF == 0 {
                self.cancel.check()?;
            }
            let p = x.apply_f32(mesh.vertex(i));
            if p.iter().any(|v| !v.is_finite()) {
                return Err(Error::InvalidParams(format!("3mf: non-finite vertex in {name}")));
            }
            let next = unique;
            let idx = *map.entry(weld_key(p)).or_insert(next);
            if idx == next {
                unique += 1;
                for k in 0..3 {
                    bmin[k] = bmin[k].min(p[k]);
                    bmax[k] = bmax[k].max(p[k]);
                }
                let _ = writeln!(self.buf, "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", p[0], p[1], p[2]);
                self.flush_if_full()?;
            }
            remap.push(idx);
        }
        drop(map);
        self.buf.push_str("    </vertices>\n    <triangles>\n");
        let nt = mesh.triangle_count();
        for t in 0..nt {
            if t & 0xFFFF == 0 {
                self.cancel.check()?;
            }
            let mut tri = mesh.triangle(t).map(|v| remap[v as usize]);
            if flip {
                tri.swap(1, 2);
            }
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                return Err(Error::InvalidParams(format!(
                    "3mf: triangle {t} of {name} collapses after the f32 transform (validate it first)"
                )));
            }
            let _ = writeln!(self.buf, "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", tri[0], tri[1], tri[2]);
            self.flush_if_full()?;
        }
        self.buf.push_str("    </triangles>\n   </mesh>\n  </object>\n");
        self.flush_if_full()?;
        let info = ObjectInfo {
            id,
            name: name.into(),
            bounds: (unique > 0).then_some(Bounds { min: bmin, max: bmax }),
            vertices: unique as usize,
            triangles: nt,
        };
        self.objects.push(info.clone());
        Ok(info)
    }

    pub fn objects(&self) -> &[ObjectInfo] {
        &self.objects
    }

    /// Writes `<build>` and closes the package. Every object should get an item.
    pub fn finish(mut self, items: &[BuildItem]) -> Result<W> {
        self.buf.push_str(" </resources>\n <build>\n");
        for it in items {
            if it.object_id == 0 || it.object_id as usize > self.objects.len() {
                return Err(Error::InvalidParams(format!("3mf: build item references unknown object {}", it.object_id)));
            }
            let _ = write!(self.buf, "  <item objectid=\"{}\"", it.object_id);
            if it.transform != Xform::IDENTITY {
                let _ = write!(self.buf, " transform=\"{}\"", matrix3d(&it.transform));
            }
            if let Some(pn) = &it.part_number {
                let _ = write!(self.buf, " partnumber=\"{}\"", xml_escape(pn));
            }
            self.buf.push_str("/>\n");
        }
        self.buf.push_str(" </build>\n</model>\n");
        self.zip.write_all(self.buf.as_bytes())?;
        self.buf.clear();
        self.zip.end()?;
        self.zip.finish()
    }
}

/// Where one part goes on the bed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// 0 for the first bed; later beds are laid out along +X.
    pub bed: u32,
    /// Translation applied to the part's written coordinates.
    pub translate: [f64; 3],
    /// The part fits inside the bed volume minus margins.
    pub fits: bool,
}

/// Gap between consecutive beds when parts spill over (mm).
pub const BED_GAP_MM: f64 = 20.0;

/// Shelf-packs part footprints (written-space bounds) onto beds: rows along X,
/// tallest first, `max(margin, 2)` mm apart, each part lifted to `z = 0`.
/// Deterministic; no rotation, so the written vertex values stay unchanged.
pub fn pack_on_bed(bounds: &[Bounds], bed: &Bed) -> Vec<Placement> {
    let gap = bed.margin_mm.max(2.0);
    let (ux0, uy0) = (bed.margin_mm, bed.margin_mm);
    let (ux1, uy1) = (bed.x_mm - bed.margin_mm, bed.y_mm - bed.margin_mm);
    let size = |b: &Bounds| {
        let s = b.size();
        [s[0] as f64, s[1] as f64, s[2] as f64]
    };
    let mut order: Vec<usize> = (0..bounds.len()).collect();
    order.sort_by(|&a, &b| size(&bounds[b])[1].total_cmp(&size(&bounds[a])[1]).then(a.cmp(&b)));
    let mut out = vec![Placement { bed: 0, translate: [0.0; 3], fits: false }; bounds.len()];
    let (mut bed_i, mut x, mut y, mut row_h) = (0u32, ux0, uy0, 0.0f64);
    let mut used_on_bed = false;
    for &i in &order {
        let b = &bounds[i];
        let s = size(b);
        // XY keeps the margin (brim, skirt); Z only has to stay under the gantry.
        let fits = s[0] <= ux1 - ux0 + 1e-9 && s[1] <= uy1 - uy0 + 1e-9 && s[2] <= bed.z_mm + 1e-9;
        if used_on_bed && x + s[0] > ux1 + 1e-9 {
            x = ux0;
            y += row_h + gap;
            row_h = 0.0;
        }
        if used_on_bed && y + s[1] > uy1 + 1e-9 {
            bed_i += 1;
            x = ux0;
            y = uy0;
            row_h = 0.0;
        }
        let off = bed_i as f64 * (bed.x_mm + BED_GAP_MM);
        out[i] = Placement {
            bed: bed_i,
            translate: [off + x - b.min[0] as f64, y - b.min[1] as f64, -(b.min[2] as f64)],
            fits,
        };
        used_on_bed = true;
        x += s[0] + gap;
        row_h = row_h.max(s[1]);
    }
    out
}

/// A part to export: a mesh in its own (plate) space plus the orientation transform.
#[derive(Debug, Clone, PartialEq)]
pub struct Part3mf<'a> {
    pub name: &'a str,
    pub mesh: &'a Mesh,
    pub transform: Xform,
}

/// What [`write_3mf`] produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreeMfReport {
    pub objects: Vec<ObjectInfo>,
    pub placements: Vec<Placement>,
    pub beds: u32,
}

/// Writes `parts` to `path` (atomically) with a bed layout when `bed` is given
/// (otherwise every item sits at its written coordinates).
pub fn write_3mf(
    path: &Path,
    parts: &[Part3mf<'_>],
    meta: &Meta,
    bed: Option<&Bed>,
    thumbnail_png: Option<&[u8]>,
    cancel: &CancelToken,
) -> Result<ThreeMfReport> {
    let mut report = None;
    crate::io::stl::write_atomic(path, |w| {
        let r = write_3mf_to(w, parts, meta, bed, thumbnail_png, cancel)?;
        report = Some(r);
        Ok(())
    })?;
    Ok(report.expect("set on success"))
}

/// [`write_3mf`] into any seekable writer.
pub fn write_3mf_to<W: Write + Seek>(
    w: W,
    parts: &[Part3mf<'_>],
    meta: &Meta,
    bed: Option<&Bed>,
    thumbnail_png: Option<&[u8]>,
    cancel: &CancelToken,
) -> Result<ThreeMfReport> {
    let mut tw = ThreeMfWriter::with_cancel(w, meta, thumbnail_png, cancel.clone())?;
    for p in parts {
        cancel.check()?;
        tw.add_object(p.name, p.mesh, &p.transform)?;
    }
    let objects = tw.objects().to_vec();
    let empty = Bounds { min: [0.0; 3], max: [0.0; 3] };
    let bounds: Vec<Bounds> = objects.iter().map(|o| o.bounds.unwrap_or(empty)).collect();
    let placements = match bed {
        Some(b) => pack_on_bed(&bounds, b),
        None => vec![Placement { bed: 0, translate: [0.0; 3], fits: true }; objects.len()],
    };
    let items: Vec<BuildItem> = objects
        .iter()
        .zip(&placements)
        .map(|(o, pl)| BuildItem {
            object_id: o.id,
            transform: Xform { t: pl.translate, ..Xform::IDENTITY },
            part_number: Some(o.name.clone()),
        })
        .collect();
    tw.finish(&items)?;
    let beds = placements.iter().map(|p| p.bed + 1).max().unwrap_or(0);
    Ok(ThreeMfReport { objects, placements, beds })
}

/// A mesh object read back from a 3MF.
#[derive(Debug, Clone, PartialEq)]
pub struct Object3mf {
    pub id: u32,
    pub name: String,
    /// Positions in millimetres (converted from the file's unit).
    pub mesh: Mesh,
}

/// A parsed 3MF model.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model3mf {
    pub unit: String,
    pub metadata: Vec<(String, String)>,
    pub objects: Vec<Object3mf>,
    pub items: Vec<BuildItem>,
    pub has_thumbnail: bool,
}

impl Model3mf {
    pub fn meta(&self, name: &str) -> Option<&str> {
        self.metadata.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    pub fn object(&self, id: u32) -> Option<&Object3mf> {
        self.objects.iter().find(|o| o.id == id)
    }
}

fn unit_scale(u: &str) -> Result<f64> {
    Ok(match u {
        "micron" => 0.001,
        "millimeter" => 1.0,
        "centimeter" => 10.0,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" => 1000.0,
        _ => return Err(Error::Io(format!("3mf: unknown unit {u:?}"))),
    })
}

/// Reads a 3MF package from disk.
pub fn read_3mf(path: &Path) -> Result<Model3mf> {
    let f = File::open(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    read_3mf_from(BufReader::new(f)).map_err(|e| match e {
        Error::Io(m) => Error::Io(format!("{}: {m}", path.display())),
        e => e,
    })
}

fn rels_model_path(rels: &str) -> Option<String> {
    // Small and ours or a slicer's: scan Relationship elements for the 3D model type.
    let mut r = quick_xml::Reader::from_str(rels);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == b"Relationship" => {
                let (mut target, mut ty) = (None, None);
                for a in e.attributes().flatten() {
                    let v = a.decode_and_unescape_value(r.decoder()).ok()?.into_owned();
                    match a.key.local_name().as_ref() {
                        b"Target" => target = Some(v),
                        b"Type" => ty = Some(v),
                        _ => {}
                    }
                }
                if ty.as_deref() == Some(REL_MODEL) {
                    return target.map(|t| t.trim_start_matches('/').to_string());
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn attr(e: &BytesStart<'_>, dec: quick_xml::encoding::Decoder, name: &[u8]) -> Result<Option<String>> {
    for a in e.attributes() {
        let a = a.map_err(|er| Error::Io(format!("3mf: bad attribute: {er}")))?;
        if a.key.as_ref() == name {
            let v = a.decode_and_unescape_value(dec).map_err(|er| Error::Io(format!("3mf: bad attribute: {er}")))?;
            return Ok(Some(v.into_owned()));
        }
    }
    Ok(None)
}

fn num<T: std::str::FromStr>(e: &BytesStart<'_>, dec: quick_xml::encoding::Decoder, name: &[u8]) -> Result<T> {
    attr(e, dec, name)?
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| Error::Io(format!("3mf: missing or bad attribute {}", String::from_utf8_lossy(name))))
}

/// Reads a 3MF package from any seekable reader. The model XML is streamed.
pub fn read_3mf_from<R: Read + Seek>(r: R) -> Result<Model3mf> {
    let mut zip = ZipArchive::new(r)?;
    let model_path = match zip.find("_rels/.rels") {
        Some(_) => {
            let rels = zip.read("_rels/.rels", 1 << 20)?;
            rels_model_path(&String::from_utf8_lossy(&rels)).unwrap_or_else(|| MODEL_PATH.into())
        }
        None => MODEL_PATH.into(),
    };
    let has_thumbnail = zip.find(THUMB_PATH).is_some();
    let entry = zip.open(&model_path)?;
    let mut xr = quick_xml::Reader::from_reader(BufReader::with_capacity(1 << 16, entry));
    let mut model = Model3mf { has_thumbnail, unit: "millimeter".into(), ..Default::default() };
    let mut scale = 1.0;
    let mut buf = Vec::new();
    let mut cur: Option<Object3mf> = None;
    let mut meta_name: Option<String> = None;
    let mut meta_text = String::new();
    let err = |e: quick_xml::Error| Error::Io(format!("3mf: XML error: {e}"));
    loop {
        let ev = xr.read_event_into(&mut buf).map_err(err)?;
        let dec = xr.decoder();
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                match e.local_name().as_ref() {
                    b"model" => {
                        if let Some(u) = attr(e, dec, b"unit")? {
                            scale = unit_scale(&u)?;
                            model.unit = u;
                        }
                        if let Some(req) = attr(e, dec, b"requiredextensions")? {
                            if !req.trim().is_empty() {
                                return Err(Error::Io(format!("3mf: required extensions not supported: {req}")));
                            }
                        }
                    }
                    b"metadata" if cur.is_none() => {
                        let n = attr(e, dec, b"name")?.unwrap_or_default();
                        if empty {
                            model.metadata.push((n, String::new()));
                        } else {
                            meta_name = Some(n);
                            meta_text.clear();
                        }
                    }
                    b"object" => {
                        let id = num(e, dec, b"id")?;
                        let name = attr(e, dec, b"name")?.unwrap_or_default();
                        cur = Some(Object3mf { id, name, mesh: Mesh::default() });
                    }
                    b"vertex" => {
                        let o = cur.as_mut().ok_or_else(|| Error::Io("3mf: vertex outside object".into()))?;
                        let p: [f64; 3] = [num(e, dec, b"x")?, num(e, dec, b"y")?, num(e, dec, b"z")?];
                        o.mesh.push_vertex(p.map(|v| if scale == 1.0 { v as f32 } else { (v * scale) as f32 }));
                    }
                    b"triangle" => {
                        let o = cur.as_mut().ok_or_else(|| Error::Io("3mf: triangle outside object".into()))?;
                        let t: [u32; 3] = [num(e, dec, b"v1")?, num(e, dec, b"v2")?, num(e, dec, b"v3")?];
                        let n = o.mesh.vertex_count() as u32;
                        if t.iter().any(|&v| v >= n) {
                            return Err(Error::Io(format!("3mf: object {} triangle index out of range", o.id)));
                        }
                        o.mesh.push_triangle(t, FaceKind::Other);
                    }
                    b"components" => {
                        return Err(Error::Io("3mf: component objects are not supported".into()));
                    }
                    b"item" => {
                        let object_id = num(e, dec, b"objectid")?;
                        let mut transform = match attr(e, dec, b"transform")? {
                            Some(s) => parse_matrix3d(&s)?,
                            None => Xform::IDENTITY,
                        };
                        for t in &mut transform.t {
                            *t *= scale;
                        }
                        let part_number = attr(e, dec, b"partnumber")?;
                        model.items.push(BuildItem { object_id, transform, part_number });
                    }
                    _ => {}
                }
            }
            Event::Text(t) if meta_name.is_some() => {
                meta_text.push_str(&t.decode().map_err(|e| Error::Io(format!("3mf: {e}")))?);
            }
            Event::GeneralRef(r) if meta_name.is_some() => {
                let ch = match r.resolve_char_ref().map_err(err)? {
                    Some(c) => c,
                    None => match r.decode().map_err(|e| Error::Io(format!("3mf: {e}")))?.as_ref() {
                        "amp" => '&',
                        "lt" => '<',
                        "gt" => '>',
                        "quot" => '"',
                        "apos" => '\'',
                        other => return Err(Error::Io(format!("3mf: unknown entity &{other};"))),
                    },
                };
                meta_text.push(ch);
            }
            Event::End(ref e) => match e.local_name().as_ref() {
                b"metadata" => {
                    if let Some(n) = meta_name.take() {
                        model.metadata.push((n, std::mem::take(&mut meta_text)));
                    }
                }
                b"object" => {
                    if let Some(o) = cur.take() {
                        model.objects.push(o);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    for it in &model.items {
        if model.object(it.object_id).is_none() {
            return Err(Error::Io(format!("3mf: build item references missing object {}", it.object_id)));
        }
    }
    Ok(model)
}

/// Streams the model XML part of a package into `w` (for golden checksums).
pub fn copy_model_xml<R: Read + Seek, W: Write>(r: R, w: &mut W) -> Result<u64> {
    let mut zip = ZipArchive::new(r)?;
    let mut e = zip.open(MODEL_PATH)?;
    Ok(std::io::copy(&mut e, w)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn cube(s: f32) -> Mesh {
        let mut m = Mesh::default();
        for k in 0..8u32 {
            m.push_vertex([(k & 1) as f32 * s, ((k >> 1) & 1) as f32 * s, ((k >> 2) & 1) as f32 * s]);
        }
        for t in [
            [0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4],
            [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5],
        ] {
            m.push_triangle(t, FaceKind::Other);
        }
        m
    }

    #[test]
    fn matrix_round_trip() {
        let x = Xform { m: [[0.0, -1.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]], t: [10.0, 0.5, -3.25] };
        assert_eq!(parse_matrix3d(&matrix3d(&x)).unwrap(), x);
        let p = [1.0, 2.0, 3.0];
        assert_eq!(x.apply(p), [8.0, -2.5, -2.25]);
    }

    #[test]
    fn writes_and_reads_two_parts() {
        let a = cube(10.0);
        let b = cube(20.0);
        let bed = Bed { x_mm: 235.0, y_mm: 235.0, z_mm: 250.0, margin_mm: 5.0 };
        let meta = Meta {
            title: Some("t & <x>".into()),
            custom: vec![("params".into(), "{\"a\":\"<1>\"}".into())],
            ..Default::default()
        };
        let parts = [
            Part3mf { name: "a", mesh: &a, transform: Xform::IDENTITY },
            Part3mf { name: "b \"big\"", mesh: &b, transform: Xform::IDENTITY },
        ];
        let mut out = Cursor::new(Vec::new());
        let rep = write_3mf_to(&mut out, &parts, &meta, Some(&bed), Some(b"\x89PNG"), &CancelToken::new()).unwrap();
        assert_eq!(rep.beds, 1);
        assert!(rep.placements.iter().all(|p| p.fits));
        let m = read_3mf_from(Cursor::new(out.into_inner())).unwrap();
        assert_eq!(m.unit, "millimeter");
        assert!(m.has_thumbnail);
        assert_eq!(m.meta("Title"), Some("t & <x>"));
        assert_eq!(m.meta("ad3d:params"), Some("{\"a\":\"<1>\"}"));
        assert_eq!(m.objects.len(), 2);
        assert_eq!(m.objects[1].name, "b \"big\"");
        assert_eq!(m.objects[0].mesh.vertex_count(), 8);
        assert_eq!(m.objects[0].mesh.triangle_count(), 12);
        // Placed footprints do not overlap and sit inside the margin.
        let mut rects = Vec::new();
        for it in &m.items {
            let o = m.object(it.object_id).unwrap();
            let bb = o.mesh.map_positions(|p| it.transform.apply_f32(p)).bounds().unwrap();
            assert!(bb.min[0] >= 5.0 && bb.min[1] >= 5.0 && bb.min[2] == 0.0);
            assert!(bb.max[0] <= 230.0 && bb.max[1] <= 230.0);
            rects.push(bb);
        }
        let (r0, r1) = (rects[0], rects[1]);
        let overlap = r0.min[0] < r1.max[0] && r1.min[0] < r0.max[0] && r0.min[1] < r1.max[1] && r1.min[1] < r0.max[1];
        assert!(!overlap);
    }

    #[test]
    fn mirror_transform_flips_winding_and_welds() {
        let a = cube(5.0);
        let mirror = Xform { m: [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };
        let mut out = Cursor::new(Vec::new());
        write_3mf_to(&mut out, &[Part3mf { name: "m", mesh: &a, transform: mirror }], &Meta::default(), None, None, &CancelToken::new())
            .unwrap();
        let m = read_3mf_from(Cursor::new(out.into_inner())).unwrap();
        assert!(m.objects[0].mesh.signed_volume() > 0.0);
    }

    #[test]
    fn collapsed_triangle_is_refused() {
        let mut m = cube(1.0);
        m.positions[3..6].copy_from_slice(&[0.0, 0.0, 0.0]); // vertex 1 onto vertex 0
        let mut tw = ThreeMfWriter::new(Cursor::new(Vec::new()), &Meta::default(), None).unwrap();
        assert!(tw.add_object("x", &m, &Xform::IDENTITY).is_err());
    }

    #[test]
    fn packing_spills_to_second_bed() {
        let bed = Bed { x_mm: 100.0, y_mm: 100.0, z_mm: 100.0, margin_mm: 5.0 };
        let b = Bounds { min: [0.0; 3], max: [60.0, 60.0, 10.0] };
        let p = pack_on_bed(&[b, b, b], &bed);
        assert_eq!(p.iter().map(|q| q.bed).collect::<Vec<_>>(), vec![0, 1, 2]);
        let big = Bounds { min: [0.0; 3], max: [200.0, 10.0, 10.0] };
        assert!(!pack_on_bed(&[big], &bed)[0].fits);
    }
}
