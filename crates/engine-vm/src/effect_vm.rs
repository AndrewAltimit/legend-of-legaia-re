//! Battle-effect VM, ported clean-room from the `0898_xxx_dat` battle overlay.
//!
//! PORT: FUN_801DE914, FUN_801DFDF8, FUN_801E0088
//! PORT: FUN_801DFDF0 (the spawn API's dump entry point - the dump stem
//! `overlay_battle_action_801dfdf0` places the entry 8 bytes before the
//! `801DFDF8` name the docs/READMEs use; same function, both addresses
//! resolve to [`Pool::spawn`])
//!
//! See [`docs/subsystems/effect-vm.md`](../../../docs/subsystems/effect-vm.md)
//! for the authoritative byte-level reference. This crate ports the high-
//! confidence pieces - the slot pool layout, the per-effect script header
//! parser, and the public spawn API - and exposes a [`EffectHost`] trait that
//! lets the engine extend the per-frame walker incrementally.
//!
//! ## Why no opcode table
//!
//! The retail per-frame walker (`FUN_801E0088`, 600+ instructions) does state
//! transitions inline - there's no central `switch (state)` to translate into
//! a clean Rust dispatch. The port models the **walker as a state-machine
//! frame** (slot iteration + state-byte countdown + child-slot allocation)
//! and delegates per-state logic to the host. Engines wire whatever runtime
//! they have for animation playback, GPU primitive emission, and RNG.
//!
//! ## Three retail entry points
//!
//! | Function | Role | Status |
//! |---|---|---|
//! | `0x801DE914` | Init / pack-fixup | Ported as [`Pool::init`] |
//! | `0x801DFDF8` | Public spawn API: `(byte effect_id, short* world_pos, ushort angle)` | Ported as [`Pool::spawn`] |
//! | `0x801E0088` | Per-frame walker | [`Pool::tick`] (skeleton) + host hooks |
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` or any overlay live in this crate. The Ghidra
//! decompilation at `ghidra/scripts/funcs/overlay_battle_801de914.txt`,
//! `overlay_battle_801dfdf8.txt`, and `overlay_battle_801e0088.txt` is the
//! *spec*, not source. Tests use hand-authored synthetic scripts (no Sony
//! bytes).
//! REF: FUN_801D8DE8

#![allow(clippy::too_many_arguments)]

mod catalog;
mod host;
mod pool;

pub use catalog::*;
pub use host::*;
pub use pool::*;

#[cfg(test)]
mod tests;
