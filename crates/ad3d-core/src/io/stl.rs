//! Binary STL. Writers stream through a `BufWriter`; the reader returns a
//! triangle soup (three vertices per triangle) for `validate()` to weld.
//!
//! - [`write_stl_to`] / [`write_stl`]: a whole [`Mesh`] mapped through an [`Xform`]
//!   (the same `Xform::apply_f32` values that `validate()` stage 2 checks).
//!   [`write_stl`] writes to a temporary file next to the target and renames it
//!   into place, so a failed or cancelled export never leaves a half file.
//! - [`StlWriter`]: incremental, for producers that emit triangles one at a time
//!   without knowing the count up front (the count is patched on `finish`).
//! - [`read_stl`] / [`read_stl_from`]: binary, plus ASCII as a fallback.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::cancel::CancelToken;
use crate::error::{Error, Result};
use crate::mesh::{FaceKind, Mesh};
use crate::orient::Xform;

const HEADER_TAG: &[u8] = b"AD3D binary STL (mm)";

fn header() -> [u8; 80] {
    let mut h = [0u8; 80];
    h[..HEADER_TAG.len()].copy_from_slice(HEADER_TAG);
    h
}

/// Facet normal computed in f64 from the written f32 corners (zero if degenerate).
#[inline]
fn normal(p: &[[f32; 3]; 3]) -> [f32; 3] {
    let d = |a: [f32; 3], b: [f32; 3]| [b[0] as f64 - a[0] as f64, b[1] as f64 - a[1] as f64, b[2] as f64 - a[2] as f64];
    let (e1, e2) = (d(p[0], p[1]), d(p[0], p[2]));
    let c = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
    if l > 0.0 { [(c[0] / l) as f32, (c[1] / l) as f32, (c[2] / l) as f32] } else { [0.0; 3] }
}

#[inline]
fn record(p: &[[f32; 3]; 3]) -> [u8; 50] {
    let mut rec = [0u8; 50];
    let n = normal(p);
    let mut o = 0;
    for v in n.iter().copied().chain(p.iter().flat_map(|q| q.iter().copied())) {
        rec[o..o + 4].copy_from_slice(&v.to_le_bytes());
        o += 4;
    }
    rec
}

/// The three written corners of triangle `t`, winding flipped for mirror transforms.
#[inline]
pub fn written_triangle(m: &Mesh, x: &Xform, flip: bool, t: usize) -> [[f32; 3]; 3] {
    let mut tri = m.triangle(t);
    if flip {
        tri.swap(1, 2);
    }
    tri.map(|i| x.apply_f32(m.vertex(i)))
}

/// Writes `m` mapped through `x`. Normals are computed in f64 from the written values.
pub fn write_stl_to<W: Write>(w: &mut W, m: &Mesh, x: &Xform) -> Result<()> {
    write_stl_to_cancellable(w, m, x, &CancelToken::new())
}

/// [`write_stl_to`] that checks `cancel` every 64k triangles.
pub fn write_stl_to_cancellable<W: Write>(w: &mut W, m: &Mesh, x: &Xform, cancel: &CancelToken) -> Result<()> {
    w.write_all(&header())?;
    let n = u32::try_from(m.triangle_count()).map_err(|_| Error::InvalidParams("too many triangles".into()))?;
    w.write_all(&n.to_le_bytes())?;
    let flip = x.det() < 0.0;
    for t in 0..m.triangle_count() {
        if t & 0xFFFF == 0 {
            cancel.check()?;
        }
        w.write_all(&record(&written_triangle(m, x, flip, t)))?;
    }
    Ok(())
}

/// A temporary sibling path for atomic writes (`name.ext.partial`).
pub(crate) fn partial_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".partial");
    PathBuf::from(s)
}

/// Runs `f` on a buffered temporary file and renames it to `path` on success.
/// On error the temporary file is removed and `path` is left untouched.
pub(crate) fn write_atomic(path: &Path, f: impl FnOnce(&mut BufWriter<File>) -> Result<()>) -> Result<()> {
    let tmp = partial_path(path);
    let res = (|| {
        let file = File::create(&tmp).map_err(|e| Error::Io(format!("{}: {e}", tmp.display())))?;
        let mut w = BufWriter::with_capacity(1 << 20, file);
        f(&mut w)?;
        w.flush()?;
        w.get_ref().sync_all().ok();
        drop(w);
        std::fs::rename(&tmp, path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Streams `m` to `path` as binary STL (atomically: temp file + rename).
pub fn write_stl(path: &Path, m: &Mesh, x: &Xform) -> Result<()> {
    write_stl_cancellable(path, m, x, &CancelToken::new())
}

/// [`write_stl`] with cancellation; a cancelled export leaves no file behind.
pub fn write_stl_cancellable(path: &Path, m: &Mesh, x: &Xform, cancel: &CancelToken) -> Result<()> {
    write_atomic(path, |w| write_stl_to_cancellable(w, m, x, cancel))
}

/// Incremental binary STL writer: push triangles, then `finish` patches the count.
pub struct StlWriter<W: Write + Seek> {
    w: W,
    start: u64,
    n: u64,
}

impl<W: Write + Seek> StlWriter<W> {
    pub fn new(mut w: W) -> Result<Self> {
        let start = w.stream_position()?;
        w.write_all(&header())?;
        w.write_all(&0u32.to_le_bytes())?;
        Ok(StlWriter { w, start, n: 0 })
    }

    /// Appends one triangle (counter-clockwise seen from outside).
    pub fn push(&mut self, tri: [[f32; 3]; 3]) -> Result<()> {
        if self.n >= u32::MAX as u64 {
            return Err(Error::InvalidParams("too many triangles for binary STL".into()));
        }
        self.w.write_all(&record(&tri))?;
        self.n += 1;
        Ok(())
    }

    /// Appends every triangle of `m` mapped through `x`.
    pub fn push_mesh(&mut self, m: &Mesh, x: &Xform) -> Result<()> {
        let flip = x.det() < 0.0;
        for t in 0..m.triangle_count() {
            self.push(written_triangle(m, x, flip, t))?;
        }
        Ok(())
    }

    pub fn triangles(&self) -> u64 {
        self.n
    }

    /// Patches the triangle count and returns the writer (flushed).
    pub fn finish(mut self) -> Result<W> {
        let end = self.w.stream_position()?;
        self.w.seek(SeekFrom::Start(self.start + 80))?;
        self.w.write_all(&(self.n as u32).to_le_bytes())?;
        self.w.seek(SeekFrom::Start(end))?;
        self.w.flush()?;
        Ok(self.w)
    }
}

/// Reads a binary (or ASCII) STL as a soup: vertex `3t + k` is corner `k` of triangle `t`.
pub fn read_stl(path: &Path) -> Result<Mesh> {
    let f = File::open(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    read_stl_from(BufReader::with_capacity(1 << 20, f)).map_err(|e| match e {
        Error::Io(m) => Error::Io(format!("{}: {m}", path.display())),
        e => e,
    })
}

/// [`read_stl`] from any seekable reader.
pub fn read_stl_from<R: Read + Seek>(mut r: R) -> Result<Mesh> {
    let len = r.seek(SeekFrom::End(0))?;
    r.seek(SeekFrom::Start(0))?;
    let mut header = [0u8; 84];
    if len >= 84 {
        r.read_exact(&mut header)?;
        let n = u32::from_le_bytes(header[80..84].try_into().unwrap()) as u64;
        if 84 + 50 * n == len {
            return read_binary_body(&mut r, n);
        }
    }
    r.seek(SeekFrom::Start(0))?;
    let mut probe = [0u8; 5];
    let k = r.read(&mut probe)?;
    r.seek(SeekFrom::Start(0))?;
    if k == 5 && probe.eq_ignore_ascii_case(b"solid") {
        return read_ascii(BufReader::new(r));
    }
    Err(Error::Io(format!("not an STL file ({len} bytes; neither binary nor ASCII)")))
}

fn read_binary_body<R: Read>(r: &mut R, n: u64) -> Result<Mesh> {
    let mut m = Mesh::with_capacity(3 * n as usize, n as usize);
    let mut rec = [0u8; 50];
    for t in 0..n as u32 {
        r.read_exact(&mut rec)?;
        let f = |o: usize| f32::from_le_bytes(rec[o..o + 4].try_into().unwrap());
        for k in 0..3 {
            let o = 12 + 12 * k;
            m.positions.extend_from_slice(&[f(o), f(o + 4), f(o + 8)]);
        }
        m.push_triangle([3 * t, 3 * t + 1, 3 * t + 2], FaceKind::Other);
    }
    Ok(m)
}

fn read_ascii<R: BufRead>(r: R) -> Result<Mesh> {
    let mut m = Mesh::default();
    let mut corner = 0u32;
    for (ln, line) in r.lines().enumerate() {
        let line = line?;
        let mut it = line.split_whitespace();
        match it.next() {
            Some(k) if k.eq_ignore_ascii_case("vertex") => {
                let mut v = [0f32; 3];
                for c in &mut v {
                    *c = it
                        .next()
                        .and_then(|s| s.parse().ok())
                        .ok_or_else(|| Error::Io(format!("ASCII STL line {}: bad vertex", ln + 1)))?;
                }
                m.positions.extend_from_slice(&v);
                corner += 1;
            }
            Some(k) if k.eq_ignore_ascii_case("endloop") => {
                if corner % 3 != 0 {
                    return Err(Error::Io(format!("ASCII STL line {}: facet without 3 vertices", ln + 1)));
                }
                let t = corner / 3 - 1;
                m.push_triangle([3 * t, 3 * t + 1, 3 * t + 2], FaceKind::Other);
            }
            _ => {}
        }
    }
    if m.triangle_count() as u32 * 3 != corner {
        return Err(Error::Io("ASCII STL: truncated facet".into()));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn tet() -> Mesh {
        let mut m = Mesh::default();
        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
            m.push_vertex(p);
        }
        for t in [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]] {
            m.push_triangle(t, FaceKind::Other);
        }
        m
    }

    #[test]
    fn incremental_writer_matches_whole_writer() {
        let m = tet();
        let mut a = Vec::new();
        write_stl_to(&mut a, &m, &Xform::IDENTITY).unwrap();
        let mut w = StlWriter::new(Cursor::new(Vec::new())).unwrap();
        w.push_mesh(&m, &Xform::IDENTITY).unwrap();
        assert_eq!(w.triangles(), 4);
        let b = w.finish().unwrap().into_inner();
        assert_eq!(a, b);
        let back = read_stl_from(Cursor::new(b)).unwrap();
        assert_eq!(back.triangle_count(), 4);
    }

    #[test]
    fn ascii_fallback() {
        let s = "solid t\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 0\n   vertex 1 0 0\n   vertex 0 1 0\n  endloop\n endfacet\nendsolid t\n";
        let m = read_stl_from(Cursor::new(s.as_bytes().to_vec())).unwrap();
        assert_eq!(m.triangle_count(), 1);
        assert_eq!(m.vertex(1), [1.0, 0.0, 0.0]);
        assert!(read_stl_from(Cursor::new(b"garbage!".to_vec())).is_err());
    }

    #[test]
    fn cancelled_write_leaves_no_file() {
        let dir = std::env::temp_dir().join(format!("ad3d-stl-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.stl");
        let c = CancelToken::new();
        c.cancel();
        assert_eq!(write_stl_cancellable(&path, &tet(), &Xform::IDENTITY, &c), Err(Error::Cancelled));
        assert!(!path.exists() && !partial_path(&path).exists());
        write_stl(&path, &tet(), &Xform::IDENTITY).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 84 + 4 * 50);
        std::fs::remove_dir_all(&dir).ok();
    }
}
