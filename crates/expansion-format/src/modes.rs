//! `modes.toml`: new game modes.

use serde::{Deserialize, Serialize};

/// The `modes.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModeSection {}
