//! GTE-style fixed-point math primitives.
//!
//! PORT: FUN_8002735C
//!
//! The retail TMD renderer at `FUN_8002735c` (60 GTE coprocessor ops) uses
//! signed 16-bit fixed-point matrix coefficients (`q12.4` in the rotation
//! matrix, `q14.16` for translation) to transform a vertex from object →
//! world → view → screen. This module mirrors the multiply-add accumulator
//! shape with tested arithmetic so engines and downstream tooling have a
//! single place to reproduce per-vertex GTE behaviour.
//!
//! ## What lives here
//!
//! * [`GteVec3`] / [`GteMat3`] - fixed-point vector + 3×3 rotation matrix in
//!   q3.12 storage with i64-widened multiply-add (`mul_vec`).
//! * [`Camera`] - rotation matrix + q19.12 translation + projection focal
//!   length `h` (the GTE register named `H`).
//! * [`Camera::transform`] - PSX GTE `RTPT` (rotate-translate-perspective):
//!   `screen = perspective_divide(rot * v + trans, h)`. Returns the
//!   screen-space coordinate plus the post-rotation Z used for depth.
//! * [`nclip`] - the GTE `NCLIP` operation: signed area of the screen-space
//!   triangle. Negative ⇒ back-face under PSX winding rules.
//! * [`avsz3`] / [`avsz4`] - average screen-Z helpers used by the OT-bucket
//!   selector.
//! * [`screen_to_pixel`] - clamps GTE screen coords (q.0 fixed-point in
//!   pixels) to a render target, with the GTE's saturation behaviour.
//! * A small CPU rasterizer scaffold under [`raster`] that plugs the above
//!   together - useful for offline regression checks against captured
//!   GTE traces.
//!
//! Production rendering still goes through wgpu's f32 pipeline (see
//! `Renderer::set_psx_mode`); this module is the source of truth when
//! something needs **pixel-exact** PSX behaviour, and supplies the f32
//! pipeline with the same constants (focal length, screen half-width)
//! the GTE used at runtime.
//!
//! ## Units (matching `docs/subsystems/renderer.md`)
//!
//! - World-position vertices: q19.12 (3D coordinate, signed)
//! - Rotation matrix: q3.12 (3×3 unit-vector basis, scaled by 4096)
//! - Translation: q19.12
//! - Projection focal length `h`: q.0 (PSX uses 320 for the standard 320×240 frame)
//! - Output screen-space: q.0 pixel coordinates, signed; clamped to viewport

mod math;
pub use math::*;

mod camera;
pub use camera::*;

mod gte_core;
pub use gte_core::*;

mod lighting;
mod registers;
mod transform;

mod mem;
pub use mem::*;

pub mod raster;

#[cfg(test)]
mod tests;
