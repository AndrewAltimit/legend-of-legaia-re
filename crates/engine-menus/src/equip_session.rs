//! Equipment-session state machine.
//!
//! Drives the "browse character → pick slot → pick item → confirm swap"
//! flow shared between the field menu's Equip screen and the
//! shop-buy-then-equip flow.
//!
//! The retail engine threads equip state through `FUN_80042558` (the
//! battle-stat aggregator) on commit; this module mirrors that:
//! `EquipSession::commit` writes the new equip id into the character
//! record and re-runs [`compute_battle_stats`] so the world's resolved
//! stats stay in sync.

use crate::equipment::{DiscEquipInfo, EquipSlot, engine_slot_disc_category};
use legaia_engine_battle::battle_stats::{
    BattleStats, EquipmentTable, ItemModifier, StatRecord, StatusModifiers, compute_battle_stats,
};
use legaia_engine_battle_vm::status_effects::StatusKind;

mod best_equipment;
mod commit;
mod input;
mod session;
mod slot_map;

pub use best_equipment::*;
pub use session::*;
pub use slot_map::*;

#[cfg(test)]
mod tests;
