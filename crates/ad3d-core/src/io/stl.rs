//! Binary STL. The writer streams through a `BufWriter`; the reader returns a
//! triangle soup (three vertices per triangle) for `validate()` to weld.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use crate::error::{Error, Result};
use crate::mesh::{FaceKind, Mesh};
use crate::orient::Xform;

/// Writes `m` mapped through `x` (the same `Xform::apply_f32` that `validate()`
/// stage 2 checks). Normals are computed in f64 from the written values.
pub fn write_stl_to<W: Write>(w: &mut W, m: &Mesh, x: &Xform) -> Result<()> {
    let mut header = [0u8; 80];
    let tag = b"AD3D binary STL (mm)";
    header[..tag.len()].copy_from_slice(tag);
    w.write_all(&header)?;
    let n = u32::try_from(m.triangle_count()).map_err(|_| Error::InvalidParams("too many triangles".into()))?;
    w.write_all(&n.to_le_bytes())?;
    let flip = x.det() < 0.0;
    let mut rec = [0u8; 50];
    for t in 0..m.triangle_count() {
        let mut tri = m.triangle(t);
        if flip {
            tri.swap(1, 2);
        }
        let p = tri.map(|i| x.apply_f32(m.vertex(i)));
        let d = |a: [f32; 3], b: [f32; 3]| [b[0] as f64 - a[0] as f64, b[1] as f64 - a[1] as f64, b[2] as f64 - a[2] as f64];
        let (e1, e2) = (d(p[0], p[1]), d(p[0], p[2]));
        let c = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        let nrm = if l > 0.0 { [c[0] / l, c[1] / l, c[2] / l] } else { [0.0; 3] };
        let mut o = 0;
        for v in nrm.iter().map(|&v| v as f32).chain(p.iter().flat_map(|q| q.iter().copied())) {
            rec[o..o + 4].copy_from_slice(&v.to_le_bytes());
            o += 4;
        }
        rec[48] = 0;
        rec[49] = 0;
        w.write_all(&rec)?;
    }
    Ok(())
}

/// Streams `m` to `path` as binary STL.
pub fn write_stl(path: &Path, m: &Mesh, x: &Xform) -> Result<()> {
    let f = File::create(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    let mut w = BufWriter::with_capacity(1 << 20, f);
    write_stl_to(&mut w, m, x)?;
    w.flush()?;
    Ok(())
}

/// Reads a binary STL as a soup: vertex `3t + k` is corner `k` of triangle `t`.
pub fn read_stl(path: &Path) -> Result<Mesh> {
    let f = File::open(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    let len = f.metadata()?.len();
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut header = [0u8; 84];
    r.read_exact(&mut header).map_err(|e| Error::Io(format!("{}: short header: {e}", path.display())))?;
    let n = u32::from_le_bytes(header[80..84].try_into().unwrap()) as u64;
    if 84 + 50 * n != len {
        return Err(Error::Io(format!(
            "{}: not a binary STL ({} triangles declared, file is {len} bytes; ASCII STL is not supported)",
            path.display(),
            n
        )));
    }
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
