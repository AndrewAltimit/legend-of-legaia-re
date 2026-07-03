//! Move-table opcode VM, ported clean-room from `FUN_80023070` (main VM in
//! `SCUS_942.54`) and `FUN_801D362C` (extension VM in the town overlay).
//!
//! PORT: FUN_80023070, FUN_801D362C, FUN_8001A6C8, FUN_8001A78C, FUN_8001A8DC
//! PORT: FUN_80024C80, FUN_801E45BC
//!
//! See `docs/subsystems/move-vm.md` for the byte-level reference. The VM drives
//! per-actor animation, motion, and combat moves (Tactical Arts) - distinct
//! from the actor / sprite VM in [`super`] and the field / event VM in
//! [`super::field`]. It is invoked every frame from the actor tick
//! (`FUN_80021DF4`) on a per-actor "move buffer" that the field VM's `EXEC_MOVE`
//! opcode (`0x22`) staged via `FUN_800204F8`.
//!
//! ## Bytecode layout
//!
//! Operand stream is **u16-aligned**. PC is also tracked in u16 units (matching
//! how the original stores it as a signed 16-bit value in `actor[+0x70]`):
//!
//! ```text
//!   *(actor + 0x48 + actor[+0x70] * 2) = u16 opcode
//!   *(actor + 0x48 + (actor[+0x70] + 1) * 2) = u16 operand_0
//!   *(actor + 0x48 + (actor[+0x70] + 2) * 2) = u16 operand_1
//!   ...
//! ```
//!
//! Each handler advances PC by an opcode-specific number of u16 words.
//! Out-of-range opcodes (`>= 0x47`) silently terminate the loop, matching the
//! `sltiu v0, v1, 0x47` bound check in the original dispatcher.
//!
//! ## Two-layer dispatch
//!
//! - The main VM has 71 opcodes (`0x00..=0x46`).
//! - Opcode `0x2F` escapes to a per-overlay extension dispatcher
//!   (`FUN_801D362C` in the town overlay), with 61 sub-opcodes
//!   (`0x00..=0x3C`). The sub-opcode is the u16 at `op[1]`.
//!
//! Both are wired through the [`MoveHost`] trait - extension sub-handlers that
//! don't fit a clean Rust idiom are hooked through `host.ext_*` callbacks.
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` or any overlay live in this crate. The Ghidra
//! decompilation at `ghidra/scripts/funcs/80023070.txt` and
//! `ghidra/scripts/funcs/overlay_0897_801d362c.txt` are the *spec*, not source.
//! The [`MoveHost`] trait abstracts every call the original made into the
//! engine layer - implementations live in `crates/engine-core` (or wherever
//! the actor pool is modeled).
//!
//! Tests use hand-authored synthetic bytecode (no Sony bytes).
//! REF: FUN_80017888, FUN_800204F8, FUN_80021B04
//! REF: FUN_80021DF4, FUN_800583C8, FUN_8005842C, FUN_80058490, FUN_801D31B0
//! REF: FUN_80020DE0
//!
//! The implementation is split across sibling submodules; every public item
//! is re-exported here so the external `move_vm::…` paths are unchanged:
//!
//! - [`state`] - [`ActorState`], [`StepResult`], [`MoveOpcode`], [`MoveExtResult`].
//! - [`host`] - the [`MoveHost`] callback trait.
//! - [`color`] - the RGB<->HSV helpers used by ext sub-ops 0x1F / 0x20.
//! - [`ext`] - the opcode-0x2F extension dispatcher.
//! - [`dispatch`] - the main [`step`] loop + [`actor_tick`] gate.
//! - [`spawn`] - the [`spawn_move_actor`] entry point (`FUN_80021B04`).

#![allow(clippy::too_many_arguments)]

mod color;
mod dispatch;
mod ext;
mod host;
mod spawn;
mod state;

pub(crate) use color::*;
pub use dispatch::*;
pub(crate) use ext::*;
pub use host::*;
pub use spawn::*;
pub use state::*;

#[cfg(test)]
mod spawn_tests;

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests;
