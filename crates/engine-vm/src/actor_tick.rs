//! Per-actor physics tick - port of `FUN_80021DF4`.
//!
//! PORT: FUN_80021DF4, FUN_800250D4, FUN_801D79E8
//!
//! `FUN_8002519C` walks the per-frame actor list and calls this function on
//! every active record. The dispatcher is **not** an "animation interpreter"
//! the way [`legaia_anm::AnimPlayer`] is - it advances per-actor position /
//! velocity / acceleration state, emits a positional sound cue when the
//! dispatch byte selects the SFX-emitter variant, and (only for the keyframe
//! dispatch byte) writes the interpolated pose into the renderer's output
//! buffer at `actor[+0x4C]`.
//!
//! ## Dispatch ladder
//!
//! The dispatch byte at `actor[+0x5A]` (see [`anim_vm::DispatchByte`]) selects
//! a layered set of side-effects:
//!
//! 1. **Common pre-update** - runs unconditionally. Drains the per-frame
//!    timer at `+0x54` and the rotation accumulator at `+0x22`.
//! 2. **Keyframe accel** (dispatch `2` / `6`). Adds `+0xC0..+0xCA` * scalars
//!    >> 6 into the shake envelopes at `+0xB4..+0xC8`.
//! 3. **Positional SFX emitter** (dispatch `5`). Either ramps a fade between
//!    `(+0x90, +0x92)` and `(+0x94 + +0x98, +0x96 + +0x9A)` over `+0xBC`
//!    frames, or simply integrates `+0x98 / +0x9A` into `+0x90 / +0x92`.
//!    Audio cues surface as [`TickEvent`] entries.
//! 4. **Path interpolation** (dispatch `3`). Adds `+0x96 / +0x98 / +0x9A`
//!    velocities into `+0x90 / +0x92 / +0x94`, advances the zoom envelope at
//!    `+0x68` (clamped at `0x100`).
//! 5. **Default movement** (every dispatch byte except `3` and `5`). Adds
//!    `+0x80..+0x84` into `+0x24..+0x28`, runs the trig-LUT-driven
//!    world-position update via [`apply_world_rotation`], and accumulates
//!    the camera-shake envelopes at `+0x72 / +0x78 / +0x7A`.
//! 6. **Common late-update** - caps the envelopes, kicks the move VM,
//!    fires the per-arm render event (line draws for `4`, scene-graph
//!    triangle for `7`), and for dispatch `6` writes the keyframe pose.
//!
//! ## What this port covers vs what it doesn't
//!
//! - **Covered.** The dispatch ladder, the per-arm position / velocity /
//!   acceleration math (with field offsets matching retail), and host
//!   callbacks for cross-cutting effects (audio cues, render submissions,
//!   move-VM kicks). Tests verify the arithmetic shape against the
//!   decompiled C reference for each arm.
//! - **Out of scope.** Bit-exact MIPS-cycle behaviour (the retail dispatcher
//!   leans on the `1F800380`-region scratchpad register file for nearly
//!   every multiply-add - we use straight i64 multiplication and a single
//!   round-down shift, which matches the source's `>> 6` / `>> 18` when
//!   neither operand is `i32::MIN`). The trig-LUT contents are supplied by
//!   the caller via [`apply_world_rotation`].
//!
//! The dispatcher reads many of these fields at different widths from
//! different arms (e.g. `+0xB8` is read as `i32` by the SFX-emitter arm and
//! as `i16` by the keyframe arm). Both views are kept in sync via the
//! `path_active` (i32) and `kf_shake[2]` (i16) fields - touching either
//! field via the public API keeps the other in lockstep.
//! REF: FUN_800204F8, FUN_8002519C, FUN_80065034, FUN_800657D0
//! Tick-cadence resolver + its `VSync(1)` frame-time source; see
//! [`FrameCadence`].
//! REF: FUN_80016B6C, FUN_800173BC
//! Per-mode `DAT_8007B9D8` cadence-floor installers.
//! REF: FUN_801D6704, FUN_801CFDA0, FUN_801DC6B4, FUN_801DE234, FUN_801DD35C, FUN_801CF678, FUN_801D362C

use crate::anim_vm::DispatchByte;

mod cadence;
pub use cadence::*;
mod physics;
pub use physics::*;
mod tick;
pub use tick::*;
mod clip_fraction;
pub use clip_fraction::*;
mod clip_cursor;
pub use clip_cursor::*;

#[cfg(test)]
mod tests;
