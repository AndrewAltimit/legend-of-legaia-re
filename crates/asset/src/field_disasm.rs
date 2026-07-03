//! Field-VM bytecode disassembler.
//!
//! Walks a field-VM bytecode buffer (the per-frame opcode stream consumed by
//! the field VM's `step` loop) and yields one [`Insn`] per source-encoded
//! instruction. The decoder mirrors the *width* logic of the field VM's `step`
//!   - it only computes how many bytes each instruction occupies plus a
//!     mnemonic, never executing host calls or mutating ctx state.
//!
//! This is a side-effect-free width/format decoder for the script bytecode, so
//! it lives in the Track-1 asset crate alongside the other format parsers; the
//! engine's executing field VM (`legaia_engine_vm::field::step`) re-uses the
//! same width logic and re-exports this module.
//!
//! For control-flow instructions (jumps, conditional jumps, BBOX tests),
//! the decoder always emits the **encoded** byte length, so a linear walk
//! traverses the script body exactly once. Branch / jump targets are
//! surfaced via the [`InsnInfo`] discriminator for callers that want to
//! follow control flow.
//!
//! For sub-dispatched opcodes (`0x4C`, `0x43`, `0x49`, `0x45`, `0x4E`,
//! `0x34`) where a particular sub-op variant isn't yet ported in the engine's
//! `field::step`, the decoder returns [`DisasmError::UnknownSubOp`]. Callers
//! typically print a `.byte` line for the leading byte and resume one byte
//! later.
//!
//! ## Why not call `step` directly?
//!
//! The engine's `field::step` interleaves width computation with side effects on
//! `ctx` and the `FieldHost` trait, and several opcodes return `StepResult`
//! variants (`Halt`, `Yield`) that don't carry the encoded width. A separate
//! width decoder keeps the disassembler side-effect-free and lets us produce
//! a stable encoded-width answer for every opcode the VM understands.
//!
//! ## Cross-context dispatch
//!
//! When the leading byte's high bit is set, the next byte is a target
//! script ID. Width math accounts for the 2-byte header in those cases;
//! the `extended` field on [`Insn`] surfaces the target ID for callers.
//!
//! This module is a thin coordinator over the cohesive submodules under
//! `field_disasm/`; every public item is re-exported here so external paths
//! (`legaia_asset::field_disasm::<Item>`, and the `legaia-engine-vm`
//! re-export) keep resolving.

mod decode;
mod decode_subops;
mod packet;
mod render;
mod types;
mod walker;

pub use decode::*;
pub use packet::*;
pub use render::*;
pub use types::*;
pub use walker::*;

// `decode_subops` holds only the `pub(super)` sub-op decoders (and a private
// MES walker); a plain glob brings them into scope so `decode`'s `super::*`
// can reach them, without a public re-export that would have nothing public
// to export.
use decode_subops::*;

#[cfg(test)]
mod tests;
