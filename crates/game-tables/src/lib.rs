//! The game's decoded data tables, wherever they are resident: in
//! `SCUS_942.54` (item names, effects and equipment bonuses, accessory
//! passives, spells, level-up curves, the steal table, the new-game template,
//! the game-mode table, SFX / XA cue tables, victory poses, the world-map
//! quick-travel menu) or in an overlay (the battle overlay's move-power,
//! element-affinity, camera, spell anim-pair and Seru side-effect tables and
//! absorb caption; the menu overlay's window descriptors and window scripts;
//! the STR overlay's FMV dispatch table).
//!
//! The rule against `legaia-overlay-images`: that crate describes a code
//! image (detection, load base, slot-B module layout), this one decodes the
//! tables inside an image. A table module takes its overlay's PROT index and
//! load base from `legaia_overlay_images::static_overlay` rather than
//! restating them, and the dependency never runs the other way.
//!
//! A doc reference to a crate that depends on this one is a plain code span,
//! since rustdoc cannot resolve a link upward.

#![forbid(unsafe_code)]

pub mod absorb_caption;
pub mod accessory_passive;
pub mod battle_attack_camera_table;
pub mod battle_camera_table;
#[cfg(test)]
mod battle_table_neighbour_tests;
pub mod element_affinity;
pub mod equip_stats;
pub mod fmv_dispatch;
pub mod item_effect;
pub mod item_names;
pub mod level_up_tables;
pub mod menu_pool;
pub mod menu_string_refs;
pub mod menu_windows;
pub mod mode_table;
pub mod move_power;
pub mod new_game;
pub mod seru_side_effect;
pub mod seru_trade;
pub mod sfx_table;
pub mod spell_anim_pairs;
pub mod spell_names;
pub mod steal_table;
pub mod str_fmv_table;
pub mod victory_pose;
pub mod widget_script;
pub mod worldmap_menu;
pub mod xa_cue_table;
