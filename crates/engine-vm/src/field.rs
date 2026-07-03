//! Field / event script VM, ported clean-room from `FUN_801DE840`.
//!
//! PORT: FUN_801DE840, FUN_8003CE08, FUN_8003CE34, FUN_8003CE64, FUN_8003C83C, FUN_8003CF04
//! PORT: FUN_801DAA50, FUN_801DAB90, FUN_801DBC20, FUN_801DE004, FUN_801DC0BC, FUN_801DDF48
//! PORT: FUN_801DE190, FUN_8003C5F0, FUN_801D77F4, FUN_801D8280, FUN_801E57F0, FUN_801E3614
//!
//! `FUN_801DE840` lives in PROT entry `0897_xxx_dat` (the town/field overlay,
//! see `docs/subsystems/script-vm.md`). It drives Legaia's overworld scripting - NPC
//! movement, dialog triggers, cutscene sequencing, story flag manipulation.
//! 17.5 KB, 357 outgoing calls - the largest function in the corpus.
//!
//! Unlike the small fixed-width actor VM in [`super`], the field VM has
//! variable-length opcodes (1 to many bytes), a rich per-script context
//! struct, and dispatches into hundreds of SCUS helpers. This module starts
//! with a foundation: the simplest opcodes ported faithfully, with stubs and
//! a `Pending` return for the rest. As the opcode reference fills in, this
//! module grows.
//!
//! ## Bytecode layout
//!
//! Each instruction starts with one opcode byte:
//!
//! ```text
//!   *(buffer + pc) = opcode
//! ```
//!
//! The high bit (0x80) is the **extended** flag. When set, the next byte is
//! a target script ID; the VM resolves it through the host and operates on
//! that script's context instead of the caller's. The low 7 bits are the
//! actual opcode (range `0x21..=0x4F` with gaps at `0x27..=0x2A`).
//!
//! Operands follow the opcode byte (or the script-ID byte if extended).
//! Operand width is per-opcode and ranges from 0 to ~14 bytes.
//!
//! Execution does NOT loop internally. The VM dispatches a single instruction
//! per call, returning a [`StepResult`] that tells the caller whether to
//! advance and where, or to halt.
//!
//! ## Cross-context dispatch
//!
//! When the high bit is set, the VM operates on a *different* script's
//! context than the caller's. The caller is responsible for resolving the
//! target script ID (via [`peek_extended`]) before invoking [`step`] - the
//! `ctx` parameter should already point at the target's context. This mirrors
//! the original's `func_0x8003C83C(target_id)` lookup, lifted into the host
//! layer to keep the VM borrow-free.
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` or any overlay live in this crate. The Ghidra
//! decompilation at `ghidra/scripts/funcs/overlay_0897_801de840.txt` and the
//! reference at `docs/subsystems/script-vm.md` are the *spec*, not source. The
//! [`FieldHost`] trait abstracts every call the original made into SCUS - its
//! implementation lives in the engine layer.
//!
//! Tests use hand-authored synthetic bytecode (no Sony bytes).
//!
//! PORT: FUN_801D5630, FUN_801D596C, FUN_801D65D8, FUN_801D835C, FUN_801DB8EC
//! PORT: FUN_801DD9D4, FUN_801DDE34, FUN_801DDFE4, FUN_801DE084, FUN_801DE2B0
//! PORT: FUN_801DE3E0, FUN_801DE698, FUN_801DE754, FUN_801DE7BC, FUN_801E4C58
//! PORT: FUN_801E573C, FUN_801E5668, FUN_801F8004, FUN_801F88FC, FUN_801F8D4C
//! PORT: FUN_801F8E6C, FUN_801F8F28
//!
//! REF: FUN_8003AEB0, FUN_8003C764, FUN_8003CA38, FUN_8003CE9C, FUN_8003CF04
//! REF: FUN_80042EE0, FUN_80056798, FUN_80058104, FUN_800583C8, FUN_8005842C, FUN_801D2D38
//! REF: FUN_801E3620
//! REF: FUN_8001EBEC, FUN_80039B7C, FUN_801D84D0

#![allow(clippy::too_many_arguments)]

mod ctx;
mod helpers;
mod host;
mod types;

pub use ctx::*;
pub use helpers::peek_extended;
use helpers::{grid_to_world, rel_jump, walk_mes_bytecode};
pub use host::*;
pub use types::*;

pub use step::{step, step_with_caller};

mod step;

#[cfg(test)]
mod tests;
