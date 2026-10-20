//! Binary mesh payload, Rust → JS (shared spike contract, little-endian):
//!
//! | offset | type | field |
//! |---|---|---|
//! | 0 | u32 | magic `0x4D334441` ("AD3M") |
//! | 4 | u32 | version = 1 |
//! | 8 | u32 | vertex_count |
//! | 12 | u32 | index_count (triangles = index_count / 3) |
//! | 16 | f32×3 | bbox_min |
//! | 28 | f32×3 | bbox_max |
//! | 40 | f32 | gen_ms |
//! | 44 | f32 | encode_ms |
//! | 48 | u32 | job_id |
//! | 52 | u32 | flags (0) |
//! | 56 | u32×2 | reserved |
//! | 64 | f32×3·V | positions (three.js x = u, y = v, z = w) |
//! | … | u32×I | indices |

use std::time::Instant;

use crate::mesh::Mesh;

pub const MAGIC: u32 = 0x4D33_4441;
pub const VERSION: u32 = 1;
pub const HEADER_BYTES: usize = 64;

/// Encodes `m`. `encode_ms` is measured here and written into the header.
pub fn encode(m: &Mesh, job_id: u32, gen_ms: f32) -> Vec<u8> {
    let t0 = Instant::now();
    let nv = m.vertex_count();
    let ni = m.indices.len();
    let mut out = Vec::with_capacity(HEADER_BYTES + 12 * nv + 4 * ni);
    let b = m.bounds().unwrap_or(crate::mesh::Bounds { min: [0.0; 3], max: [0.0; 3] });
    let put_u32 = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    put_u32(&mut out, MAGIC);
    put_u32(&mut out, VERSION);
    put_u32(&mut out, nv as u32);
    put_u32(&mut out, ni as u32);
    for v in b.min.iter().chain(b.max.iter()) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&gen_ms.to_le_bytes());
    out.extend_from_slice(&0f32.to_le_bytes()); // encode_ms, patched below
    put_u32(&mut out, job_id);
    put_u32(&mut out, 0); // flags
    put_u32(&mut out, 0);
    put_u32(&mut out, 0);
    debug_assert_eq!(out.len(), HEADER_BYTES);
    #[cfg(target_endian = "little")]
    {
        out.extend_from_slice(bytemuck::cast_slice(&m.positions));
        out.extend_from_slice(bytemuck::cast_slice(&m.indices));
    }
    #[cfg(not(target_endian = "little"))]
    {
        for v in &m.positions {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in &m.indices {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    let ms = t0.elapsed().as_secs_f64() as f32 * 1e3;
    out[44..48].copy_from_slice(&ms.to_le_bytes());
    out
}

/// Decodes a payload (tests and tools). Returns `(mesh, job_id, gen_ms, encode_ms)`.
pub fn decode(buf: &[u8]) -> Option<(Mesh, u32, f32, f32)> {
    let rd = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(buf.get(o..o + 4)?.try_into().ok()?)) };
    if rd(0)? != MAGIC || rd(4)? != VERSION {
        return None;
    }
    let nv = rd(8)? as usize;
    let ni = rd(12)? as usize;
    if buf.len() != HEADER_BYTES + 12 * nv + 4 * ni {
        return None;
    }
    let f = |o: usize| f32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
    let mut m = Mesh::default();
    m.positions = (0..3 * nv).map(|k| f(HEADER_BYTES + 4 * k)).collect();
    let io = HEADER_BYTES + 12 * nv;
    m.indices = (0..ni).map(|k| u32::from_le_bytes(buf[io + 4 * k..io + 4 * k + 4].try_into().unwrap())).collect();
    Some((m, rd(48)?, f(40), f(44)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::FaceKind;

    #[test]
    fn roundtrip_and_header() {
        let mut m = Mesh::default();
        m.push_vertex([0.0, 1.0, 2.0]);
        m.push_vertex([3.0, -1.0, 0.5]);
        m.push_vertex([1.0, 4.0, 0.0]);
        m.push_triangle([0, 1, 2], FaceKind::Other);
        let b = encode(&m, 42, 1.5);
        assert_eq!(&b[0..4], b"AD3M");
        assert_eq!(b.len(), 64 + 36 + 12);
        let (d, job, g, _) = decode(&b).unwrap();
        assert_eq!(job, 42);
        assert_eq!(g, 1.5);
        assert_eq!(d.positions, m.positions);
        assert_eq!(d.indices, m.indices);
        assert_eq!(f32::from_le_bytes(b[16..20].try_into().unwrap()), 0.0);
        assert_eq!(f32::from_le_bytes(b[32..36].try_into().unwrap()), 4.0);
    }
}
