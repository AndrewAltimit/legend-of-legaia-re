//! The retail kernel differential: every module runs a routine off the user's
//! disc against the Rust kernel that claims to port it. Skips without
//! `LEGAIA_DISC_BIN`, so CI runs without disc data.

mod arts_roll;
mod battle_seed;
mod common;
mod escape;
mod item_apply;
mod level_up;
mod melee_hit;
mod spoils;
mod stat_aggregator;
