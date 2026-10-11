//! The field dialog layer's `World`-free kernels: the dialog pager (panel,
//! row window, typewriter pacing, picker slide), the one-line text balloon,
//! and the state of the two spawned script contexts that put dialog on
//! screen - an actor's inline interaction script and the modal cutscene
//! timeline.
//!
//! Every module's dependency closure inside the engine is this crate, the
//! field VM (`legaia-engine-vm`) and the MES / font parsers, so it sits below
//! `legaia-engine-menus`. Doc comments name items of a dependent crate in
//! plain code spans, since rustdoc cannot resolve a link into a dependent
//! crate. See the crate README for the module map.

#![forbid(unsafe_code)]

pub mod cursor_sprite;
pub mod dialog;
pub mod dialog_pacing;
pub mod dialog_picker_slide;
pub mod dialog_window;
pub mod text_balloon;
