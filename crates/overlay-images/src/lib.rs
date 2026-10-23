//! Code images: how a PROT entry is recognised as MIPS code, which entry
//! each loader picks and where it loads, and how a slot-B cast / summon
//! module image is laid out band by band.
//!
//! The rule against `legaia-game-tables`: this crate describes the image, that
//! one decodes the data tables found in an image (`SCUS_942.54` or an
//! overlay). The dependency runs one way - `legaia-game-tables` takes an
//! overlay's PROT index, load base and window bounds from here; nothing here
//! names a table. [`summon_overlay`] and [`cast_effect_pool`] stay on this
//! side because they are bands of the slot-B layout: [`slot_b_module`] and
//! [`summon_overlay`] call each other.
//!
//! Every module reads an image handed to it as bytes and names only its
//! siblings, `legaia-bytes` and `legaia-lzs`. A doc reference to a crate that
//! depends on this one is a plain code span, since rustdoc cannot resolve a
//! link upward. See the crate README for the module map.

#![forbid(unsafe_code)]

pub mod boot_overlay;
pub mod cast_effect_pool;
pub mod cast_presentation;
pub mod mips_overlay;
pub mod overlay_ptr_table;
pub mod slot_b_module;
pub mod static_overlay;
pub mod string_refs;
pub mod summon_overlay;
