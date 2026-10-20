//! Hand-written declarations for the subset of manifold's C API
//! (vendor/manifold/bindings/c/include/manifold/manifoldc.h, v3.5.x) that the
//! safe wrapper uses. No bindgen, so the build needs no libclang.
//!
//! Memory model of the C API: `manifold_alloc_*` returns uninitialised
//! storage of the right size, every constructor placement-news into the `mem`
//! it is given and returns it, and `manifold_delete_*` runs the destructor and
//! frees. C enums are passed as `c_int`, so an enumerator added upstream can
//! never become an invalid Rust enum value.
//!
//! To add a function: copy its prototype from manifoldc.h, map `size_t` to
//! `usize`, `int` and enums to `c_int`, and `T*` to `*mut T`.

#![allow(non_camel_case_types, dead_code)]

use std::marker::{PhantomData, PhantomPinned};
use std::os::raw::{c_int, c_void};

macro_rules! opaque {
    ($($name:ident),* $(,)?) => {$(
        #[repr(C)]
        pub struct $name {
            _data: [u8; 0],
            _marker: PhantomData<(*mut u8, PhantomPinned)>,
        }
    )*};
}

opaque!(
    ManifoldManifold,
    ManifoldMeshGL,
    ManifoldBox,
    ManifoldExecutionContext,
);

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManifoldVec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

// enum ManifoldOpType
pub const MANIFOLD_ADD: c_int = 0;
pub const MANIFOLD_SUBTRACT: c_int = 1;
pub const MANIFOLD_INTERSECT: c_int = 2;

// enum ManifoldError
pub const MANIFOLD_NO_ERROR: c_int = 0;
pub const MANIFOLD_NON_FINITE_VERTEX: c_int = 1;
pub const MANIFOLD_NOT_MANIFOLD: c_int = 2;
pub const MANIFOLD_VERTEX_INDEX_OUT_OF_BOUNDS: c_int = 3;
pub const MANIFOLD_PROPERTIES_WRONG_LENGTH: c_int = 4;
pub const MANIFOLD_MISSING_POSITION_PROPERTIES: c_int = 5;
pub const MANIFOLD_MERGE_VECTORS_DIFFERENT_LENGTHS: c_int = 6;
pub const MANIFOLD_MERGE_INDEX_OUT_OF_BOUNDS: c_int = 7;
pub const MANIFOLD_TRANSFORM_WRONG_LENGTH: c_int = 8;
pub const MANIFOLD_RUN_INDEX_WRONG_LENGTH: c_int = 9;
pub const MANIFOLD_FACE_ID_WRONG_LENGTH: c_int = 10;
pub const MANIFOLD_INVALID_CONSTRUCTION: c_int = 11;
pub const MANIFOLD_RESULT_TOO_LARGE: c_int = 12;
pub const MANIFOLD_INVALID_TANGENTS: c_int = 13;
pub const MANIFOLD_CANCELLED: c_int = 14;

unsafe extern "C" {
    // allocation / destruction
    pub fn manifold_alloc_manifold() -> *mut ManifoldManifold;
    pub fn manifold_alloc_meshgl() -> *mut ManifoldMeshGL;
    pub fn manifold_alloc_box() -> *mut ManifoldBox;
    pub fn manifold_alloc_execution_context() -> *mut ManifoldExecutionContext;
    pub fn manifold_delete_manifold(m: *mut ManifoldManifold);
    pub fn manifold_delete_meshgl(m: *mut ManifoldMeshGL);
    pub fn manifold_delete_box(b: *mut ManifoldBox);
    pub fn manifold_delete_execution_context(ctx: *mut ManifoldExecutionContext);

    // MeshGL (f32 properties, u32 indices); the constructor copies its inputs
    pub fn manifold_meshgl(
        mem: *mut c_void,
        vert_props: *mut f32,
        n_verts: usize,
        n_props: usize,
        tri_verts: *mut u32,
        n_tris: usize,
    ) -> *mut ManifoldMeshGL;
    pub fn manifold_meshgl_merge(mem: *mut c_void, m: *mut ManifoldMeshGL) -> *mut ManifoldMeshGL;
    pub fn manifold_meshgl_num_prop(m: *mut ManifoldMeshGL) -> usize;
    pub fn manifold_meshgl_num_vert(m: *mut ManifoldMeshGL) -> usize;
    pub fn manifold_meshgl_num_tri(m: *mut ManifoldMeshGL) -> usize;
    pub fn manifold_meshgl_vert_properties_length(m: *mut ManifoldMeshGL) -> usize;
    pub fn manifold_meshgl_tri_length(m: *mut ManifoldMeshGL) -> usize;
    /// Copies `vert_properties_length` floats into `mem`.
    pub fn manifold_meshgl_vert_properties(mem: *mut c_void, m: *mut ManifoldMeshGL) -> *mut f32;
    /// Copies `tri_length` indices into `mem`.
    pub fn manifold_meshgl_tri_verts(mem: *mut c_void, m: *mut ManifoldMeshGL) -> *mut u32;

    // Manifold construction
    pub fn manifold_of_meshgl(mem: *mut c_void, mesh: *mut ManifoldMeshGL) -> *mut ManifoldManifold;
    pub fn manifold_get_meshgl(mem: *mut c_void, m: *mut ManifoldManifold) -> *mut ManifoldMeshGL;
    pub fn manifold_empty(mem: *mut c_void) -> *mut ManifoldManifold;
    pub fn manifold_copy(mem: *mut c_void, m: *mut ManifoldManifold) -> *mut ManifoldManifold;
    pub fn manifold_cube(mem: *mut c_void, x: f64, y: f64, z: f64, center: c_int) -> *mut ManifoldManifold;
    pub fn manifold_cylinder(
        mem: *mut c_void,
        height: f64,
        radius_low: f64,
        radius_high: f64,
        circular_segments: c_int,
        center: c_int,
    ) -> *mut ManifoldManifold;
    pub fn manifold_translate(
        mem: *mut c_void,
        m: *mut ManifoldManifold,
        x: f64,
        y: f64,
        z: f64,
    ) -> *mut ManifoldManifold;

    // booleans (lazy: evaluated on the next query of the result)
    pub fn manifold_boolean(
        mem: *mut c_void,
        a: *mut ManifoldManifold,
        b: *mut ManifoldManifold,
        op: c_int,
    ) -> *mut ManifoldManifold;
    pub fn manifold_union(mem: *mut c_void, a: *mut ManifoldManifold, b: *mut ManifoldManifold) -> *mut ManifoldManifold;
    pub fn manifold_difference(
        mem: *mut c_void,
        a: *mut ManifoldManifold,
        b: *mut ManifoldManifold,
    ) -> *mut ManifoldManifold;
    pub fn manifold_intersection(
        mem: *mut c_void,
        a: *mut ManifoldManifold,
        b: *mut ManifoldManifold,
    ) -> *mut ManifoldManifold;

    // queries
    pub fn manifold_is_empty(m: *mut ManifoldManifold) -> c_int;
    pub fn manifold_status(m: *mut ManifoldManifold) -> c_int;
    pub fn manifold_num_vert(m: *mut ManifoldManifold) -> usize;
    pub fn manifold_num_tri(m: *mut ManifoldManifold) -> usize;
    pub fn manifold_genus(m: *mut ManifoldManifold) -> c_int;
    pub fn manifold_volume(m: *mut ManifoldManifold) -> f64;
    pub fn manifold_surface_area(m: *mut ManifoldManifold) -> f64;
    pub fn manifold_bounding_box(mem: *mut c_void, m: *mut ManifoldManifold) -> *mut ManifoldBox;
    pub fn manifold_box_min(b: *mut ManifoldBox) -> ManifoldVec3;
    pub fn manifold_box_max(b: *mut ManifoldBox) -> ManifoldVec3;

    // cancellation / progress (manifold >= 3.5.1)
    pub fn manifold_with_context(
        mem: *mut c_void,
        m: *mut ManifoldManifold,
        ctx: *mut ManifoldExecutionContext,
    ) -> *mut ManifoldManifold;
    pub fn manifold_execution_context(mem: *mut c_void) -> *mut ManifoldExecutionContext;
    pub fn manifold_execution_context_cancel(ctx: *mut ManifoldExecutionContext);
    pub fn manifold_execution_context_cancelled(ctx: *mut ManifoldExecutionContext) -> c_int;
    pub fn manifold_execution_context_progress(ctx: *mut ManifoldExecutionContext) -> f64;
    pub fn manifold_execution_context_of_meshgl(
        mem: *mut c_void,
        ec: *mut ManifoldExecutionContext,
        mesh: *mut ManifoldMeshGL,
    ) -> *mut ManifoldManifold;
}
