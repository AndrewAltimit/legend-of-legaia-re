//! Battle kernels: the renderer-free, `World`-free half of the battle engine.
//!
//! Every module here is a pure kernel or a self-contained state type - the
//! monster AI script, death spoils, the AP gauge, the stat aggregator, the
//! spell / monster / move-power catalogs, encounter tables and triggers,
//! level-up, Seru learning and trading, Tactical Arts tracking, and the
//! per-frame battle-animation / CLUT / side-band passes. None of them reads
//! or writes `World`: the stateful seat (`legaia_engine_core::world` and its
//! `world::battle` submodules) calls in and owns the composition, so this
//! crate sits strictly below `legaia-engine-core` with no cycle.
//!
//! `legaia-engine-core` re-exports each module at its old path
//! (`legaia_engine_core::monster_ai`, ...), so a consumer may name either.
//! See the crate README for the module map and the rule for what belongs
//! here.

pub mod accessory_passives;
pub mod ap_gauge;
pub mod art_strike;
pub mod arts_command_input;
pub mod battle_afterimage;
pub mod battle_anim;
pub mod battle_arts;
pub mod battle_body_blend;
pub mod battle_effect_clut;
pub mod battle_events;
pub mod battle_magic;
pub mod battle_return_flags;
pub mod battle_seats;
pub mod battle_sideband;
pub mod battle_stats;
pub mod battle_status_clut;
pub mod battle_steal;
pub mod encounter;
pub mod encounter_man;
pub mod encounter_record;
pub mod encounter_registry;
pub mod levelup;
pub mod magic_xp;
pub mod monster_ai;
pub mod monster_catalog;
pub mod move_power;
pub mod region_encounter;
pub mod retail_magic;
pub mod seru_learning;
pub mod seru_stats;
pub mod seru_trade;
pub mod spells;
pub mod tactical_arts;
pub mod tactical_arts_editor;
pub mod target_picker;
