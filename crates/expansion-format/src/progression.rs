//! `progression.toml`: level curve, XP and rewards to level 99.

use serde::{Deserialize, Serialize};

/// The `progression.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgressionSection {}
