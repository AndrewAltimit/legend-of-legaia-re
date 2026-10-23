//! Monster-turn decisioning: the action picker, confuse retargeting, physical /
//! cast arming, and monster-side target-class resolution.

use super::*;

mod action_pick;
mod mystic_shield;
mod status_procs;
mod targeting;
mod turn_arming;

pub(in crate::world) use status_procs::module_line;

#[cfg(test)]
use status_procs::{ImpactStatusProc, MASTER_GUARD_BIT, ROT_GUARD_BIT, enemy_impact_status_proc};

#[cfg(test)]
mod impact_proc_tests;

#[cfg(test)]
mod capture_special_tests;
