//! Image replacement on a user-supplied disc: field / scene TIM textures
//! (with their palette contexts), the player battle-texture sections, the
//! monster archive's textures and the save-slot portrait sheet. Each module
//! decodes the retail image, encodes a replacement PNG against the same
//! palette and page budget, and writes it back through `legaia-disc-patch`'s
//! `DiscPatcher`.
//!
//! `legaia-patcher` sits above this crate. Doc comments name its items in
//! plain code spans, since rustdoc cannot resolve a link into a dependent
//! crate.

#![forbid(unsafe_code)]

pub mod battle_texture;
pub mod monster_texture;
pub mod save_icon;
pub mod texture;
pub mod texture_palettes;
