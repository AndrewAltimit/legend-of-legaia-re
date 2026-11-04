//! The retail kernel differential: every module runs a routine off the user's
//! disc against the Rust kernel that claims to port it. Skips without
//! `LEGAIA_DISC_BIN`, so CI runs without disc data.

mod arts_gauge;
mod arts_queue;
mod arts_roll;
mod auto_combo;
mod battle_item;
mod battle_seed;
mod cast_damage;
mod common;
mod death_spoils;
mod encounter_roll;
mod escape;
mod field_walls;
mod inventory;
mod item_apply;
mod level_up;
mod magic_level;
mod melee_hit;
mod monster_pick;
mod round_boundary;
mod round_open;
mod spirit_cleanup;
mod spoils;
mod stat_aggregator;
mod validator;
mod walk_step;
