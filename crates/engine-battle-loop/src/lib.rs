//! The battle loop's carried state, minus `World`: the battle session
//! ([`battle_state::BattleState`]), the cast band
//! ([`cast_fx_state::CastFxState`] and the records it holds), the round flow,
//! the battle frame clock, the message banner and the Auto attack's state.
//!
//! `engine-core`'s `World` holds these as `world.battle` / `world.casting`
//! and its battle loop steps them; a method here reads or steps its own
//! state alone. Doc comments name `engine-core` items in plain code spans,
//! since rustdoc cannot resolve a link into a dependent crate. See the crate
//! README for the module map and the split line.

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
