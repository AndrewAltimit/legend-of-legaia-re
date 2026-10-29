//! The battle loop's state and the steps that need nothing else, minus
//! `World`: the battle session ([`battle_state::BattleState`]), the cast band
//! ([`cast_fx_state::CastFxState`] and the records it holds), the round flow,
//! the battle frame clock, and the methods and free kernels that read or
//! write that state alone (the stat block, the tutorial flow, the HUD glides,
//! the clip ladder, the module stagers).
//!
//! `engine-core`'s `World` holds the two states as `world.battle` /
//! `world.casting`; a step that needs any other `World` field stays an
//! `impl World` there. Doc comments name `engine-core` items in plain code
//! spans, since rustdoc cannot resolve a link into a dependent crate. See the
//! crate README for the module map and the split line.

#![forbid(unsafe_code)]

pub mod auto_combo;
pub mod barra_flight;
pub mod battle_state;
pub mod cast_band;
pub mod cast_fx_state;
pub mod cast_seat_map;
pub mod clip_ladder;
pub mod frame_clock;
pub mod hud_glides;
pub mod message_banner;
pub mod module_presentation;
pub mod module_seat_arm;
pub mod module_stagers;
pub mod nova_motion;
pub mod orb_stager;
pub mod round_flow;
pub mod seat_stats;
pub mod status_word;
pub mod tutorial_flow;
pub mod ui_cues;
