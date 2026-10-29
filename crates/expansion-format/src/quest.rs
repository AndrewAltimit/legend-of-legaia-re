//! `quest.toml`: quests and challenges.

use serde::{Deserialize, Serialize};

/// The `quest.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestSection {}
