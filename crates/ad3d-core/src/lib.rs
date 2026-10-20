//! AD3D geometry core.
//!
//! Units are millimetres; angles are radians in code (the DTOs carry degrees).
//! Generators are pure functions of `(params, inputs)` that take a [`CancelToken`]
//! and return a [`Mesh`]. Nothing here depends on a UI.
//!
//! Spike entry points for the app crate:
//! - [`squama::SquamaParamsDto`] → [`squama::SquamaParams`] (JSON from JS, degrees → radians)
//! - [`squama::run`] (params + image source → mesh, layout, warnings, timings)
//! - [`payload::encode`] (mesh → the binary `AD3M` buffer the viewport reads)
//! - [`mesh::validate::validate`] and [`io::stl::write_stl`] for export.

pub mod cancel;
pub mod error;
pub mod image;
pub mod io;
pub mod mesh;
pub mod orient;
pub mod payload;
pub mod profiles;
pub mod project;
pub mod relief;
pub mod squama;
pub mod synth;

pub use cancel::CancelToken;
pub use error::{Error, Result};
pub use mesh::{FaceKind, Mesh};
