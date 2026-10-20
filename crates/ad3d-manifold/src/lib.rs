//! Safe Rust wrapper around [manifold](https://github.com/elalish/manifold)
//! (Apache-2.0), a robust mesh-boolean library, through its C API.
//!
//! The C++ sources are vendored under `vendor/` and built by `build.rs` with
//! CMake as static libraries (serial, no TBB, no network). See
//! `vendor/VERSIONS` for the pinned versions and checksums.
//!
//! Units are whatever the caller uses; AD3D passes millimetres. Triangles are
//! counter-clockwise seen from outside. Positions are `f32` (as in STL); manifold
//! works in `f64` internally.
//!
//! Booleans are lazy, as in manifold itself: [`Manifold::difference`] and
//! friends only record the operation. The work happens on the next query
//! ([`Manifold::status`], [`Manifold::num_tri`], [`Manifold::to_meshgl`], ...).
//! To make that work cancellable, attach an [`ExecutionContext`] with
//! [`Manifold::with_context`] and call [`Manifold::status`] on the result.
//! Cancellation is checked between booleans and between the phases of a mesh
//! import, not in the middle of a single boolean. A cancelled tree stays
//! `Cancelled` for good (its nodes are shared); rebuild it from the inputs,
//! which are unaffected, to retry.

mod ffi;

use std::fmt;
use std::os::raw::{c_int, c_void};
use std::ptr::NonNull;

/// Version of the vendored manifold sources, read from
/// `vendor/manifold/CMakeLists.txt` at build time.
pub const MANIFOLD_VERSION: &str = env!("AD3D_MANIFOLD_VERSION");

/// manifold's status codes (`manifold::Manifold::Error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Status {
    NoError,
    NonFiniteVertex,
    NotManifold,
    VertexIndexOutOfBounds,
    PropertiesWrongLength,
    MissingPositionProperties,
    MergeVectorsDifferentLengths,
    MergeIndexOutOfBounds,
    TransformWrongLength,
    RunIndexWrongLength,
    FaceIdWrongLength,
    InvalidConstruction,
    ResultTooLarge,
    InvalidTangents,
    Cancelled,
    /// A code this wrapper does not know yet (newer manifold).
    Unknown(i32),
}

impl Status {
    fn from_raw(code: c_int) -> Self {
        match code {
            ffi::MANIFOLD_NO_ERROR => Self::NoError,
            ffi::MANIFOLD_NON_FINITE_VERTEX => Self::NonFiniteVertex,
            ffi::MANIFOLD_NOT_MANIFOLD => Self::NotManifold,
            ffi::MANIFOLD_VERTEX_INDEX_OUT_OF_BOUNDS => Self::VertexIndexOutOfBounds,
            ffi::MANIFOLD_PROPERTIES_WRONG_LENGTH => Self::PropertiesWrongLength,
            ffi::MANIFOLD_MISSING_POSITION_PROPERTIES => Self::MissingPositionProperties,
            ffi::MANIFOLD_MERGE_VECTORS_DIFFERENT_LENGTHS => Self::MergeVectorsDifferentLengths,
            ffi::MANIFOLD_MERGE_INDEX_OUT_OF_BOUNDS => Self::MergeIndexOutOfBounds,
            ffi::MANIFOLD_TRANSFORM_WRONG_LENGTH => Self::TransformWrongLength,
            ffi::MANIFOLD_RUN_INDEX_WRONG_LENGTH => Self::RunIndexWrongLength,
            ffi::MANIFOLD_FACE_ID_WRONG_LENGTH => Self::FaceIdWrongLength,
            ffi::MANIFOLD_INVALID_CONSTRUCTION => Self::InvalidConstruction,
            ffi::MANIFOLD_RESULT_TOO_LARGE => Self::ResultTooLarge,
            ffi::MANIFOLD_INVALID_TANGENTS => Self::InvalidTangents,
            ffi::MANIFOLD_CANCELLED => Self::Cancelled,
            other => Self::Unknown(other),
        }
    }

    pub fn is_ok(self) -> bool {
        self == Self::NoError
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(code) => write!(f, "unknown manifold status {code}"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// Errors returned by this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The buffers have the wrong shape; checked in Rust before calling manifold.
    InvalidInput(&'static str),
    /// manifold rejected the mesh or the operation (status other than `NoError`).
    Manifold(Status),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(msg) => write!(f, "invalid mesh input: {msg}"),
            Self::Manifold(status) => write!(f, "manifold status: {status}"),
        }
    }
}

impl std::error::Error for Error {}

/// A boolean operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoolOp {
    Union,
    Difference,
    Intersection,
}

impl BoolOp {
    fn to_raw(self) -> c_int {
        match self {
            Self::Union => ffi::MANIFOLD_ADD,
            Self::Difference => ffi::MANIFOLD_SUBTRACT,
            Self::Intersection => ffi::MANIFOLD_INTERSECT,
        }
    }
}

/// An owned manifold `MeshGL`: `f32` vertex properties (x, y, z first, then
/// any extra channels) and `u32` triangle indices.
pub struct MeshGL {
    ptr: NonNull<ffi::ManifoldMeshGL>,
}

// SAFETY: a MeshGL exclusively owns its C++ object, and manifold objects have
// no thread affinity, so moving one to another thread is fine. It is not Sync.
unsafe impl Send for MeshGL {}

impl MeshGL {
    /// A mesh from packed `[x, y, z, ...]` positions and `[a, b, c, ...]`
    /// triangles. The data is copied.
    pub fn new(positions: &[f32], triangles: &[u32]) -> Result<Self, Error> {
        Self::with_properties(positions, 3, triangles)
    }

    /// Like [`MeshGL::new`] with `num_prop` floats per vertex (x, y, z first).
    pub fn with_properties(vert_props: &[f32], num_prop: usize, triangles: &[u32]) -> Result<Self, Error> {
        if num_prop < 3 {
            return Err(Error::InvalidInput("num_prop must be at least 3 (x, y, z)"));
        }
        if vert_props.len() % num_prop != 0 {
            return Err(Error::InvalidInput("vertex property length is not a multiple of num_prop"));
        }
        if triangles.len() % 3 != 0 {
            return Err(Error::InvalidInput("triangle index length is not a multiple of 3"));
        }
        let n_verts = vert_props.len() / num_prop;
        let n_tris = triangles.len() / 3;
        // SAFETY: `mem` is fresh storage from manifold_alloc_meshgl, constructed
        // exactly once. manifold_meshgl only reads n_verts * num_prop floats and
        // n_tris * 3 indices through the (non-const in C) pointers, which is
        // exactly the length of each slice; it copies them into its own vectors.
        // Out-of-range indices are reported later by status(), never read here.
        let ptr = unsafe {
            let mem = ffi::manifold_alloc_meshgl();
            ffi::manifold_meshgl(
                mem.cast::<c_void>(),
                vert_props.as_ptr().cast_mut(),
                n_verts,
                num_prop,
                triangles.as_ptr().cast_mut(),
                n_tris,
            )
        };
        Ok(Self::from_raw(ptr))
    }

    fn from_raw(ptr: *mut ffi::ManifoldMeshGL) -> Self {
        Self {
            ptr: NonNull::new(ptr).expect("manifold returned a null MeshGL"),
        }
    }

    fn raw(&self) -> *mut ffi::ManifoldMeshGL {
        self.ptr.as_ptr()
    }

    pub fn num_vert(&self) -> usize {
        // SAFETY: self.ptr is a live MeshGL for the lifetime of &self.
        unsafe { ffi::manifold_meshgl_num_vert(self.raw()) }
    }

    pub fn num_tri(&self) -> usize {
        // SAFETY: as above.
        unsafe { ffi::manifold_meshgl_num_tri(self.raw()) }
    }

    pub fn num_prop(&self) -> usize {
        // SAFETY: as above.
        unsafe { ffi::manifold_meshgl_num_prop(self.raw()) }
    }

    /// All vertex properties, `num_prop` floats per vertex.
    pub fn vert_properties(&self) -> Vec<f32> {
        // SAFETY: the buffer has capacity for exactly the length manifold reports,
        // manifold_meshgl_vert_properties memcpy's that many floats into it, and
        // set_len happens only after the copy.
        unsafe {
            let len = ffi::manifold_meshgl_vert_properties_length(self.raw());
            let mut out = Vec::<f32>::with_capacity(len);
            ffi::manifold_meshgl_vert_properties(out.as_mut_ptr().cast::<c_void>(), self.raw());
            out.set_len(len);
            out
        }
    }

    /// Triangle indices, three per triangle, counter-clockwise from outside.
    pub fn tri_verts(&self) -> Vec<u32> {
        // SAFETY: same contract as vert_properties.
        unsafe {
            let len = ffi::manifold_meshgl_tri_length(self.raw());
            let mut out = Vec::<u32>::with_capacity(len);
            ffi::manifold_meshgl_tri_verts(out.as_mut_ptr().cast::<c_void>(), self.raw());
            out.set_len(len);
            out
        }
    }

    /// Packed `[x, y, z, ...]` positions, without any extra properties.
    pub fn positions(&self) -> Vec<f32> {
        let props = self.vert_properties();
        let n = self.num_prop();
        if n == 3 {
            return props;
        }
        props.chunks_exact(n).flat_map(|v| [v[0], v[1], v[2]]).collect()
    }

    /// A copy whose merge vectors join coincident vertices along open edges,
    /// so that a triangle soup (as read from STL) can become a [`Manifold`].
    pub fn merge(&self) -> MeshGL {
        // SAFETY: fresh storage from manifold_alloc_meshgl, constructed once by
        // manifold_meshgl_merge (a copy of self plus MeshGL::Merge()).
        let ptr = unsafe {
            let mem = ffi::manifold_alloc_meshgl();
            ffi::manifold_meshgl_merge(mem.cast::<c_void>(), self.raw())
        };
        Self::from_raw(ptr)
    }
}

impl Drop for MeshGL {
    fn drop(&mut self) {
        // SAFETY: we own the object; it was allocated by manifold_alloc_meshgl.
        unsafe { ffi::manifold_delete_meshgl(self.raw()) }
    }
}

impl fmt::Debug for MeshGL {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeshGL")
            .field("num_vert", &self.num_vert())
            .field("num_tri", &self.num_tri())
            .field("num_prop", &self.num_prop())
            .finish()
    }
}

/// A closed, oriented 2-manifold solid, or an empty one carrying an error
/// [`Status`]. Cloning is cheap (the CSG tree is shared).
pub struct Manifold {
    ptr: NonNull<ffi::ManifoldManifold>,
}

// SAFETY: a Manifold exclusively owns its handle and manifold objects have no
// thread affinity. Clones share an internal CSG node, but manifold guards lazy
// evaluation of shared nodes with mutexes (csg_tree.h, manifold.h), so clones
// may be evaluated on different threads. &Manifold is not shared (not Sync).
unsafe impl Send for Manifold {}

impl Manifold {
    /// Runs a C constructor into fresh storage from manifold_alloc_manifold.
    fn construct(f: impl FnOnce(*mut c_void) -> *mut ffi::ManifoldManifold) -> Self {
        // SAFETY: manifold_alloc_manifold returns storage sized for a Manifold;
        // every caller passes it to exactly one C constructor.
        let mem = unsafe { ffi::manifold_alloc_manifold() };
        let ptr = f(mem.cast::<c_void>());
        Self {
            ptr: NonNull::new(ptr).expect("manifold returned a null Manifold"),
        }
    }

    fn raw(&self) -> *mut ffi::ManifoldManifold {
        self.ptr.as_ptr()
    }

    /// Imports a mesh. Never fails here: an invalid mesh gives an empty
    /// Manifold whose [`status`](Self::status) says why. See
    /// [`try_from_meshgl`](Self::try_from_meshgl).
    pub fn from_meshgl(mesh: &MeshGL) -> Self {
        // SAFETY: mesh is a live MeshGL; the constructor copies what it needs.
        Self::construct(|mem| unsafe { ffi::manifold_of_meshgl(mem, mesh.raw()) })
    }

    /// Imports a mesh, returning `Err` with manifold's status if it is not a
    /// closed, consistently oriented 2-manifold.
    pub fn try_from_meshgl(mesh: &MeshGL) -> Result<Self, Error> {
        Self::from_meshgl(mesh).into_result()
    }

    /// Imports a mesh, observing `ctx` for cancellation and progress.
    pub fn from_meshgl_with_context(mesh: &MeshGL, ctx: &ExecutionContext) -> Self {
        // SAFETY: mesh and ctx are live for the duration of the call.
        Self::construct(|mem| unsafe { ffi::manifold_execution_context_of_meshgl(mem, ctx.raw(), mesh.raw()) })
    }

    /// Shorthand for `MeshGL::new` followed by `try_from_meshgl`.
    pub fn from_mesh(positions: &[f32], triangles: &[u32]) -> Result<Self, Error> {
        Self::try_from_meshgl(&MeshGL::new(positions, triangles)?)
    }

    pub fn empty() -> Self {
        // SAFETY: plain constructor.
        Self::construct(|mem| unsafe { ffi::manifold_empty(mem) })
    }

    /// An axis-aligned box from the origin to `size`, or centred on the origin.
    pub fn cube(size: [f64; 3], center: bool) -> Self {
        // SAFETY: plain constructor.
        Self::construct(|mem| unsafe { ffi::manifold_cube(mem, size[0], size[1], size[2], c_int::from(center)) })
    }

    /// A cylinder (or cone) along +Z with `segments` sides; `segments <= 2`
    /// lets manifold choose from the radius.
    pub fn cylinder(height: f64, radius_low: f64, radius_high: f64, segments: i32, center: bool) -> Self {
        // SAFETY: plain constructor.
        Self::construct(|mem| unsafe {
            ffi::manifold_cylinder(mem, height, radius_low, radius_high, segments, c_int::from(center))
        })
    }

    pub fn translate(&self, offset: [f64; 3]) -> Self {
        // SAFETY: self is live; the result is a new object.
        Self::construct(|mem| unsafe { ffi::manifold_translate(mem, self.raw(), offset[0], offset[1], offset[2]) })
    }

    pub fn boolean(&self, other: &Self, op: BoolOp) -> Self {
        // SAFETY: both operands are live; the result is a new object.
        Self::construct(|mem| unsafe { ffi::manifold_boolean(mem, self.raw(), other.raw(), op.to_raw()) })
    }

    pub fn union(&self, other: &Self) -> Self {
        // SAFETY: as boolean().
        Self::construct(|mem| unsafe { ffi::manifold_union(mem, self.raw(), other.raw()) })
    }

    pub fn difference(&self, other: &Self) -> Self {
        // SAFETY: as boolean().
        Self::construct(|mem| unsafe { ffi::manifold_difference(mem, self.raw(), other.raw()) })
    }

    pub fn intersection(&self, other: &Self) -> Self {
        // SAFETY: as boolean().
        Self::construct(|mem| unsafe { ffi::manifold_intersection(mem, self.raw(), other.raw()) })
    }

    /// A copy that evaluates its pending booleans under `ctx` on the next
    /// [`status`](Self::status) call. Upstream idiom: `(a - b).with_context(ctx).status()`.
    pub fn with_context(&self, ctx: &ExecutionContext) -> Self {
        // SAFETY: both live; the C++ side keeps its own reference-counted copy of
        // the context, so ctx may be dropped before the result.
        Self::construct(|mem| unsafe { ffi::manifold_with_context(mem, self.raw(), ctx.raw()) })
    }

    /// Evaluates any pending booleans and returns the status.
    pub fn status(&self) -> Status {
        // SAFETY: self is live.
        Status::from_raw(unsafe { ffi::manifold_status(self.raw()) })
    }

    /// `Ok(self)` if the status is `NoError`.
    pub fn into_result(self) -> Result<Self, Error> {
        match self.status() {
            Status::NoError => Ok(self),
            status => Err(Error::Manifold(status)),
        }
    }

    pub fn is_empty(&self) -> bool {
        // SAFETY: self is live.
        unsafe { ffi::manifold_is_empty(self.raw()) != 0 }
    }

    /// Volume in cubic units of the input (mm³ in AD3D).
    pub fn volume(&self) -> f64 {
        // SAFETY: self is live.
        unsafe { ffi::manifold_volume(self.raw()) }
    }

    pub fn surface_area(&self) -> f64 {
        // SAFETY: self is live.
        unsafe { ffi::manifold_surface_area(self.raw()) }
    }

    pub fn num_vert(&self) -> usize {
        // SAFETY: self is live.
        unsafe { ffi::manifold_num_vert(self.raw()) }
    }

    pub fn num_tri(&self) -> usize {
        // SAFETY: self is live.
        unsafe { ffi::manifold_num_tri(self.raw()) }
    }

    /// Topological genus: 0 for a ball, 1 for a ring.
    pub fn genus(&self) -> i32 {
        // SAFETY: self is live.
        unsafe { ffi::manifold_genus(self.raw()) }
    }

    /// `(min, max)` corners, or `None` for an empty solid.
    pub fn bounding_box(&self) -> Option<([f64; 3], [f64; 3])> {
        if self.is_empty() {
            return None;
        }
        // SAFETY: fresh Box storage constructed once by manifold_bounding_box,
        // read, then deleted.
        unsafe {
            let mem = ffi::manifold_alloc_box();
            let b = ffi::manifold_bounding_box(mem.cast::<c_void>(), self.raw());
            let (lo, hi) = (ffi::manifold_box_min(b), ffi::manifold_box_max(b));
            ffi::manifold_delete_box(b);
            Some(([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]))
        }
    }

    /// The triangle mesh (positions only, `num_prop == 3`, unless properties
    /// were added upstream). Vertices are shared, so the mesh is indexed.
    pub fn to_meshgl(&self) -> MeshGL {
        // SAFETY: fresh MeshGL storage constructed once by manifold_get_meshgl.
        let ptr = unsafe {
            let mem = ffi::manifold_alloc_meshgl();
            ffi::manifold_get_meshgl(mem.cast::<c_void>(), self.raw())
        };
        MeshGL::from_raw(ptr)
    }
}

impl Clone for Manifold {
    fn clone(&self) -> Self {
        // SAFETY: self is live; manifold_copy shares the CSG tree.
        Self::construct(|mem| unsafe { ffi::manifold_copy(mem, self.raw()) })
    }
}

impl Drop for Manifold {
    fn drop(&mut self) {
        // SAFETY: we own the object; it was allocated by manifold_alloc_manifold.
        unsafe { ffi::manifold_delete_manifold(self.raw()) }
    }
}

impl fmt::Debug for Manifold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Querying forces evaluation of pending booleans.
        f.debug_struct("Manifold")
            .field("status", &self.status())
            .field("num_vert", &self.num_vert())
            .field("num_tri", &self.num_tri())
            .finish()
    }
}

/// Cooperative cancellation and progress for manifold work (manifold >= 3.5.1).
/// Sticky: once cancelled, it stays cancelled. Share it with `Arc`.
pub struct ExecutionContext {
    ptr: NonNull<ffi::ManifoldExecutionContext>,
}

// SAFETY: upstream documents ExecutionContext as safe to read and write from
// any thread (manifoldc.h, common.h); its state is atomics behind a shared_ptr.
unsafe impl Send for ExecutionContext {}
// SAFETY: as above; every method takes &self and maps to a thread-safe call.
unsafe impl Sync for ExecutionContext {}

impl ExecutionContext {
    pub fn new() -> Self {
        // SAFETY: fresh storage, constructed exactly once.
        let ptr = unsafe {
            let mem = ffi::manifold_alloc_execution_context();
            ffi::manifold_execution_context(mem.cast::<c_void>())
        };
        Self {
            ptr: NonNull::new(ptr).expect("manifold returned a null ExecutionContext"),
        }
    }

    fn raw(&self) -> *mut ffi::ManifoldExecutionContext {
        self.ptr.as_ptr()
    }

    /// Requests cancellation. Callable from any thread; idempotent.
    pub fn cancel(&self) {
        // SAFETY: self is live; thread-safe upstream.
        unsafe { ffi::manifold_execution_context_cancel(self.raw()) }
    }

    pub fn is_cancelled(&self) -> bool {
        // SAFETY: self is live; thread-safe upstream.
        unsafe { ffi::manifold_execution_context_cancelled(self.raw()) != 0 }
    }

    /// Progress of the evaluation observing this context, in `[0, 1]`.
    pub fn progress(&self) -> f64 {
        // SAFETY: self is live; thread-safe upstream.
        unsafe { ffi::manifold_execution_context_progress(self.raw()) }
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ExecutionContext {
    fn drop(&mut self) {
        // SAFETY: we own the handle; it was allocated by manifold_alloc_execution_context.
        unsafe { ffi::manifold_delete_execution_context(self.raw()) }
    }
}

impl fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("cancelled", &self.is_cancelled())
            .field("progress", &self.progress())
            .finish()
    }
}
