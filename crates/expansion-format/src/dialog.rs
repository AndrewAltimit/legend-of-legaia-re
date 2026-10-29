//! `dialog.toml`: dialogue lines.

use serde::{Deserialize, Serialize};

/// The `dialog.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DialogSection {}
