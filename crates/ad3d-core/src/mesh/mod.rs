//! Indexed triangle mesh: `f32` positions (mm) and `u32` triangle indices,
//! counter-clockwise seen from outside, plus an optional face kind per triangle.

pub mod intersect;
pub mod validate;

/// What a triangle belongs to. Used by `validate()` (facet height and back-face
/// checks) and later by the viewport (M1 payload flag bit 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FaceKind {
    Facet = 0,
    Wall = 1,
    Back = 2,
    Key = 3,
    Stand = 4,
    Other = 255,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// `x, y, z` per vertex, millimetres.
    pub positions: Vec<f32>,
    /// Three indices per triangle.
    pub indices: Vec<u32>,
    /// One kind per triangle, or empty when unknown.
    pub face_kind: Vec<FaceKind>,
}

/// Axis-aligned bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub fn size(&self) -> [f32; 3] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1], self.max[2] - self.min[2]]
    }
}

impl Mesh {
    pub fn with_capacity(vertices: usize, triangles: usize) -> Self {
        Mesh {
            positions: Vec::with_capacity(vertices * 3),
            indices: Vec::with_capacity(triangles * 3),
            face_kind: Vec::with_capacity(triangles),
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    #[inline]
    pub fn vertex(&self, i: u32) -> [f32; 3] {
        let i = i as usize * 3;
        [self.positions[i], self.positions[i + 1], self.positions[i + 2]]
    }

    #[inline]
    pub fn triangle(&self, t: usize) -> [u32; 3] {
        [self.indices[3 * t], self.indices[3 * t + 1], self.indices[3 * t + 2]]
    }

    /// Appends a vertex and returns its index.
    #[inline]
    pub fn push_vertex(&mut self, p: [f32; 3]) -> u32 {
        let i = self.vertex_count() as u32;
        self.positions.extend_from_slice(&p);
        i
    }

    /// Appends a triangle. `face_kind` is kept in step only when it is in use.
    #[inline]
    pub fn push_triangle(&mut self, t: [u32; 3], kind: FaceKind) {
        self.indices.extend_from_slice(&t);
        self.face_kind.push(kind);
    }

    /// Bounds of all vertices, or `None` for an empty mesh. NaNs are ignored.
    pub fn bounds(&self) -> Option<Bounds> {
        if self.positions.is_empty() {
            return None;
        }
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in self.positions.chunks_exact(3) {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        Some(Bounds { min, max })
    }

    /// Signed volume (positive for an outward-facing closed mesh), in f64,
    /// measured relative to the bounding-box centre to limit cancellation.
    pub fn signed_volume(&self) -> f64 {
        let Some(b) = self.bounds() else { return 0.0 };
        let c = [
            (b.min[0] as f64 + b.max[0] as f64) * 0.5,
            (b.min[1] as f64 + b.max[1] as f64) * 0.5,
            (b.min[2] as f64 + b.max[2] as f64) * 0.5,
        ];
        let p = |i: u32| {
            let v = self.vertex(i);
            [v[0] as f64 - c[0], v[1] as f64 - c[1], v[2] as f64 - c[2]]
        };
        let mut vol = 0.0;
        for t in self.indices.chunks_exact(3) {
            vol += triple(p(t[0]), p(t[1]), p(t[2]));
        }
        vol / 6.0
    }

    /// Returns a copy with every vertex mapped through `f`.
    pub fn map_positions(&self, f: impl Fn([f32; 3]) -> [f32; 3]) -> Mesh {
        let mut out = self.clone();
        for p in out.positions.chunks_exact_mut(3) {
            let q = f([p[0], p[1], p[2]]);
            p.copy_from_slice(&q);
        }
        out
    }
}

#[inline]
pub(crate) fn triple(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

/// Bit pattern used for exact welding: `-0.0` is treated as `0.0`.
#[inline]
pub fn weld_key(p: [f32; 3]) -> [u32; 3] {
    [(p[0] + 0.0).to_bits(), (p[1] + 0.0).to_bits(), (p[2] + 0.0).to_bits()]
}

/// Welds bit-identical positions. Returns the welded mesh and the number of
/// vertices merged. Triangle order and face kinds are kept.
pub fn weld_exact(m: &Mesh) -> (Mesh, usize) {
    use std::collections::HashMap;
    let n = m.vertex_count();
    let mut map: HashMap<[u32; 3], u32> = HashMap::with_capacity(n);
    let mut remap = Vec::with_capacity(n);
    let mut out = Mesh { positions: Vec::with_capacity(n * 3), indices: Vec::new(), face_kind: m.face_kind.clone() };
    for i in 0..n as u32 {
        let p = m.vertex(i);
        let id = *map.entry(weld_key(p)).or_insert_with(|| {
            let id = (out.positions.len() / 3) as u32;
            out.positions.extend_from_slice(&[p[0] + 0.0, p[1] + 0.0, p[2] + 0.0]);
            id
        });
        remap.push(id);
    }
    out.indices = m.indices.iter().map(|&i| remap[i as usize]).collect();
    let merged = n - out.vertex_count();
    (out, merged)
}
