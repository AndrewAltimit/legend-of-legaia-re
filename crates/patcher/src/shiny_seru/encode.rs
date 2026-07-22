//! MIPS R3000 instruction encoders (little-endian) and register aliases used to
//! hand-assemble the shiny-Seru detour routines.
//!
//! These are now the shared encoders in [`crate::mips`]; this module re-exports
//! them so the sibling modules that `use super::encode::*` keep working.

pub(crate) use crate::mips::*;
