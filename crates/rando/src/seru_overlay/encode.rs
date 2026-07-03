//! Low-level MIPS R3000 instruction encoders (little-endian words), register
//! aliases, and the `lui`/`ori` immediate-split helpers used by the routine
//! builders.
//!
//! These are now the shared encoders + register aliases in [`crate::mips`]; this
//! module re-exports them so the sibling modules that `use super::*` (which pulls
//! in `use encode::*`) keep working unchanged.

pub(crate) use crate::mips::*;
