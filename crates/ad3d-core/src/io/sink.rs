//! Export sinks (design §12 `PartSink`, §14.4): generators hand finished parts
//! to a sink one at a time; the sink streams them to disk.
//!
//! - [`StlDirSink`]: one binary STL per part, `<dir>/<part name>.stl`.
//! - [`ThreeMfSink`]: one 3MF with an object per part, laid out on the bed.
//! - [`FanOut`]: several sinks at once (STL and 3MF from one run).
//!
//! `squama::export` has its own richer `PartSink` (parts with validation
//! reports, abort); `squama::export::IoSink` adapts any sink here to it.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{Error, Result};
use crate::io::stl::{partial_path, write_stl_cancellable};
use crate::io::threemf::{BuildItem, Meta, Placement, ThreeMfWriter, pack_on_bed};
use crate::mesh::{Bounds, Mesh};
use crate::orient::Xform;
use crate::profiles::Bed;

/// What a part is, for naming and for the export report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartKind {
    Plate,
    Tile,
    Splint,
    Stand,
    Calibration,
    Other,
}

/// One part ready to write: plate-space mesh plus its orientation transform.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportPart {
    pub name: String,
    pub kind: PartKind,
    pub mesh: Mesh,
    pub transform: Xform,
}

/// File formats for print exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExportFormat {
    #[serde(rename = "stl")]
    Stl,
    #[serde(rename = "3mf")]
    ThreeMf,
}

impl ExportFormat {
    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Stl => "stl",
            ExportFormat::ThreeMf => "3mf",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stl" => Ok(ExportFormat::Stl),
            "3mf" => Ok(ExportFormat::ThreeMf),
            o => Err(Error::InvalidParams(format!("unknown export format {o:?} (stl, 3mf)"))),
        }
    }
}

/// A file a sink wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct WrittenFile {
    pub path: PathBuf,
    pub format: ExportFormat,
    pub bytes: u64,
    /// Part names in the file.
    pub parts: Vec<String>,
    /// 3MF only: where each part landed, in part order.
    pub placements: Vec<Placement>,
}

/// Receives parts one at a time. `finish` closes every file and reports them.
pub trait PartSink {
    fn put(&mut self, part: &ExportPart) -> Result<()>;
    fn finish(&mut self) -> Result<Vec<WrittenFile>>;
}

/// File-system-safe version of a part name: ASCII letters, digits, `-`, `_`, `.`.
pub fn safe_file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    let s = s.trim_matches('.').to_string();
    if s.is_empty() { "part".into() } else { s }
}

/// One binary STL per part in `dir`.
pub struct StlDirSink {
    dir: PathBuf,
    cancel: CancelToken,
    written: Vec<WrittenFile>,
}

impl StlDirSink {
    pub fn new(dir: &Path, cancel: CancelToken) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        Ok(StlDirSink { dir: dir.into(), cancel, written: Vec::new() })
    }
}

impl PartSink for StlDirSink {
    fn put(&mut self, part: &ExportPart) -> Result<()> {
        let path = self.dir.join(format!("{}.stl", safe_file_name(&part.name)));
        if self.written.iter().any(|w| w.path == path) {
            return Err(Error::InvalidParams(format!("two parts map to the file {}", path.display())));
        }
        write_stl_cancellable(&path, &part.mesh, &part.transform, &self.cancel)?;
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        self.written.push(WrittenFile {
            path,
            format: ExportFormat::Stl,
            bytes,
            parts: vec![part.name.clone()],
            placements: Vec::new(),
        });
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<WrittenFile>> {
        Ok(std::mem::take(&mut self.written))
    }
}

/// All parts in one 3MF, streamed to a temporary file and renamed on finish.
pub struct ThreeMfSink {
    path: PathBuf,
    tmp: PathBuf,
    bed: Option<Bed>,
    w: Option<ThreeMfWriter<BufWriter<File>>>,
    names: Vec<String>,
}

impl ThreeMfSink {
    pub fn new(path: &Path, meta: &Meta, bed: Option<Bed>, thumbnail_png: Option<&[u8]>, cancel: CancelToken) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        }
        let tmp = partial_path(path);
        let f = File::create(&tmp).map_err(|e| Error::Io(format!("{}: {e}", tmp.display())))?;
        let w = ThreeMfWriter::with_cancel(BufWriter::with_capacity(1 << 20, f), meta, thumbnail_png, cancel)?;
        Ok(ThreeMfSink { path: path.into(), tmp, bed, w: Some(w), names: Vec::new() })
    }
}

impl Drop for ThreeMfSink {
    fn drop(&mut self) {
        if self.w.is_some() {
            // Not finished (error or cancel): leave no partial file behind.
            self.w = None;
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

impl PartSink for ThreeMfSink {
    fn put(&mut self, part: &ExportPart) -> Result<()> {
        let w = self.w.as_mut().ok_or_else(|| Error::Internal("3mf sink already finished".into()))?;
        w.add_object(&part.name, &part.mesh, &part.transform)?;
        self.names.push(part.name.clone());
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<WrittenFile>> {
        let w = self.w.take().ok_or_else(|| Error::Internal("3mf sink already finished".into()))?;
        let objects = w.objects().to_vec();
        let empty = Bounds { min: [0.0; 3], max: [0.0; 3] };
        let bounds: Vec<Bounds> = objects.iter().map(|o| o.bounds.unwrap_or(empty)).collect();
        let placements = match &self.bed {
            Some(b) => pack_on_bed(&bounds, b),
            None => vec![Placement { bed: 0, translate: [0.0; 3], fits: true }; objects.len()],
        };
        let items: Vec<BuildItem> = objects
            .iter()
            .zip(&placements)
            .map(|(o, p)| BuildItem {
                object_id: o.id,
                transform: Xform { t: p.translate, ..Xform::IDENTITY },
                part_number: Some(o.name.clone()),
            })
            .collect();
        let res = (|| {
            let mut bw = w.finish(&items)?;
            std::io::Write::flush(&mut bw)?;
            drop(bw);
            std::fs::rename(&self.tmp, &self.path).map_err(|e| Error::Io(format!("{}: {e}", self.path.display())))
        })();
        if let Err(e) = res {
            let _ = std::fs::remove_file(&self.tmp);
            return Err(e);
        }
        let bytes = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        Ok(vec![WrittenFile {
            path: self.path.clone(),
            format: ExportFormat::ThreeMf,
            bytes,
            parts: std::mem::take(&mut self.names),
            placements,
        }])
    }
}

/// Sends every part to several sinks.
pub struct FanOut(pub Vec<Box<dyn PartSink>>);

impl PartSink for FanOut {
    fn put(&mut self, part: &ExportPart) -> Result<()> {
        for s in &mut self.0 {
            s.put(part)?;
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<WrittenFile>> {
        let mut out = Vec::new();
        for s in &mut self.0 {
            out.extend(s.finish()?);
        }
        Ok(out)
    }
}

/// Collects parts in memory (tests, previews).
#[derive(Default)]
pub struct VecSink(pub Vec<ExportPart>);

impl PartSink for VecSink {
    fn put(&mut self, part: &ExportPart) -> Result<()> {
        self.0.push(part.clone());
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<WrittenFile>> {
        Ok(Vec::new())
    }
}

/// Builds the sink for `formats`: STLs go into `dir`, the 3MF to `dir/<stem>.3mf`.
pub fn sink_for(
    dir: &Path,
    stem: &str,
    formats: &[ExportFormat],
    meta: &Meta,
    bed: Option<Bed>,
    thumbnail_png: Option<&[u8]>,
    cancel: &CancelToken,
) -> Result<FanOut> {
    if formats.is_empty() {
        return Err(Error::InvalidParams("no export format chosen".into()));
    }
    let mut sinks: Vec<Box<dyn PartSink>> = Vec::new();
    for f in formats {
        match f {
            ExportFormat::Stl => sinks.push(Box::new(StlDirSink::new(dir, cancel.clone())?)),
            ExportFormat::ThreeMf => {
                let path = dir.join(format!("{}.3mf", safe_file_name(stem)));
                sinks.push(Box::new(ThreeMfSink::new(&path, meta, bed, thumbnail_png, cancel.clone())?));
            }
        }
    }
    Ok(FanOut(sinks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe() {
        assert_eq!(safe_file_name("tile 0/1"), "tile_0_1");
        assert_eq!(safe_file_name(".."), "part");
        assert_eq!(ExportFormat::parse("3MF").unwrap(), ExportFormat::ThreeMf);
        assert!(ExportFormat::parse("obj").is_err());
    }

    #[test]
    fn three_mf_sink_cleans_up_when_dropped_unfinished() {
        let dir = std::env::temp_dir().join(format!("ad3d-sink-{}", std::process::id()));
        let path = dir.join("x.3mf");
        {
            let _s = ThreeMfSink::new(&path, &Meta::default(), None, None, CancelToken::new()).unwrap();
            assert!(partial_path(&path).exists());
        }
        assert!(!partial_path(&path).exists() && !path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
