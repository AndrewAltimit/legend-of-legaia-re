//! `battle.toml`: monsters, formations, encounter overrides and bosses.

use serde::{Deserialize, Serialize};

/// The `battle.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BattleSection {}
