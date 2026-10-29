//! Field collision grid, region tables, floor sampling, wall/interact probes, NPC/actor motion, direction decoding, and free-movement locomotion.

use super::*;

mod action_movement;
mod locomotion;
mod npc_motion;
use legaia_engine_vm::field_ledge_hop_arc as hop_arc;

mod ambient_probe;
mod collision_grid;
mod direction_probes;
mod walk_reachability;

pub use action_movement::{ACTION_DASH_BUTTON, ACTION_JUMP_BUTTON};
use ambient_probe::*;

#[cfg(test)]
mod face_target_tests;

#[cfg(test)]
mod seat_tests;
