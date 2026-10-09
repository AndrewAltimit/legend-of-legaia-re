//! The game's static data tables, parsed out of `SCUS_942.54` and the
//! battle / field / STR overlays: item names, effects and equipment
//! bonuses, accessory passives, spells and their anim pairs, level-up
//! curves, steal and element-affinity tables, the Seru side-effect table
//! and trade offers, the new-game template, the game-mode and FMV tables,
//! SFX / XA cue tables, victory poses, the battle camera tables, the
//! world-map quick-travel menu and the Seru-absorb caption.
//!
//! `legaia-asset` re-exports every module here at its old path. Doc links
//! that pointed back up at it are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate.

#![forbid(unsafe_code)]

pub mod absorb_caption;
pub mod accessory_passive;
pub mod battle_attack_camera_table;
pub mod battle_camera_table;
pub mod element_affinity;
pub mod equip_stats;
pub mod fmv_dispatch;
pub mod item_effect;
pub mod item_names;
pub mod level_up_tables;
pub mod mode_table;
pub mod new_game;
pub mod seru_side_effect;
pub mod seru_trade;
pub mod sfx_table;
pub mod spell_anim_pairs;
pub mod spell_names;
pub mod steal_table;
pub mod str_fmv_table;
pub mod victory_pose;
pub mod worldmap_menu;
pub mod xa_cue_table;
