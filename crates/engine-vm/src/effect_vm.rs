//! Battle-effect VM, ported from the `0898_xxx_dat` battle overlay.
//!
//! PORT: FUN_801DE914, FUN_801DFDF0, FUN_801E0088
//!
//! The spawn API's entry is `0x801DFDF0`: its first two words load the
//! pool-ready byte `0x8007BD58` ahead of the `addiu sp,sp,-0x30` at
//! `0x801DFDF8`, and every one of the disc's `jal`s to the routine names
//! `0x801DFDF0` (none names `0x801DFDF8`). Older pages cite the routine by
//! its prologue word `0x801DFDF8`; that is an interior address, not a
//! second entry.
//!
//! See [`docs/subsystems/effect-vm.md`](../../../docs/subsystems/effect-vm.md)
//! for the authoritative byte-level reference. This crate ports the slot pool
//! layout, the per-effect script header parser, the public spawn API, and the
//! full per-frame walker algebra ([`Pool::tick_retail`] pass 1 +
//! [`Pool::child_billboards`] pass 2). The [`EffectHost`] trait supplies the
//! RNG and the summon routing; the faithful walker is the only per-frame
//! path (`engine-core::World::tick_effects` drives it each retail frame).
//!
//! ## Why no opcode table
//!
//! The retail per-frame walker (`FUN_801E0088`, 600+ instructions) has no
//! opcode byte anywhere: the per-slot "state" bytes are 5.3 fixed-point
//! **wait counters**, and the lifecycle is a pair of countdown-driven cursor
//! walks - the master spawn cadence over 14-byte pack1 records and the child
//! anim/motion walk over 6-byte pack0 frames. [`Pool::tick_retail`] executes
//! that algebra operator-for-operator.
//!
//! ## Three retail entry points
//!
//! | Function | Role | Status |
//! |---|---|---|
//! | `0x801DE914` | Init / pack-fixup | Ported as [`Pool::init`] |
//! | `0x801DFDF0` | Public spawn API: `(byte effect_id, short* world_pos, ushort angle)` | Ported as [`Pool::spawn`] |
//! | `0x801E0088` | Per-frame walker | [`Pool::tick_retail`] (pass 1) + [`Pool::child_billboards`] (pass 2) |
//!
//! ## Port boundary
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
