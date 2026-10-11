//! Per-frame time/RNG/pad, the top-level tick dispatcher, and the minigame ticks (tile board, dance, fishing, slot machine, baka fighter, muscle dome).
//!
//! The frame-time sampler behind the adaptive cadence
//! ([`World::resolve_frame_step`]) - retail's `VSync(1)` reading, returned by
//! the dev profiler HUD.
//! REF: FUN_800173BC

use super::*;

mod escape_timer;
mod field_eases;
mod frame_inputs;
mod mode_tick;
mod play_clock;
mod sound_release;
mod staged_warp;
mod three_actor_talk;
mod walk_regen;

mod tile_board_tick;

mod minigame_sessions;
