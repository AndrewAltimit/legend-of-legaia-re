//! Item/spell/art catalogs, tactical-arts rows, item use, stat raises, battle stat setters, XP/loot/steal, and shop flow.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

mod arts_rows;
mod battle_spoils;
mod catalog_install;
mod item_use;
mod shop_arming;

pub use battle_spoils::*;
