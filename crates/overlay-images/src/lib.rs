//! The code-overlay image formats: how a PROT entry is recognised as MIPS
//! code, where each runtime overlay loads, how a slot-B cast / summon module
//! is laid out, and the data tables resident in the battle and menu overlay
//! images.
//!
//! Every module here reads an overlay image handed to it as bytes and names
//! only its siblings, `legaia-bytes` and `legaia-lzs` - none of the asset
//! dispatcher, the pack / bundle formats or the scene formats - so the crate
//! sits below `legaia-asset`, which re-exports each module at its old path.
//! Doc links that pointed up at a dependent crate are plain code spans, since
//! rustdoc cannot resolve a link into a dependent crate. See the crate README
//! for the module map.

#![forbid(unsafe_code)]

pub mod cast_effect_pool;
pub mod menu_windows;
pub mod mips_overlay;
pub mod move_power;
pub mod overlay_ptr_table;
pub mod slot_b_module;
pub mod static_overlay;
pub mod summon_overlay;
pub mod widget_script;
