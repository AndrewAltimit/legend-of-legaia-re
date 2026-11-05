//! The retail kernel differential: every module runs a routine off the user's
//! disc against the Rust kernel that claims to port it. Skips without
//! `LEGAIA_DISC_BIN`, so CI runs without disc data.

mod arts_gauge;
mod arts_queue;
mod arts_roll;
mod arts_voice;
mod auto_combo;
mod baka_fight;
mod battle_item;
mod battle_leaves;
mod battle_seed;
mod best_equipment;
mod cast_damage;
mod common;
mod confuse;
mod dance_floor;
mod death_spoils;
mod dialog_pager;
mod encounter_roll;
mod enemy_impact;
mod escape;
mod field_vm;
mod field_walls;
mod fishing_cadence;
mod fishing_fight;
mod inn;
mod inventory;
mod item_apply;
mod level_up;
mod magic_level;
mod melee_hit;
mod monster_pick;
mod muscle_contest;
mod queue_head;
mod round_boundary;
mod round_open;
mod seru_debuff;
mod slot_machine;
mod spirit_cleanup;
mod spoils;
mod stat_aggregator;
mod validator;
mod walk_step;
