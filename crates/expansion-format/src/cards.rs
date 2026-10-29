//! `cards.toml`: trading-card definitions.

use serde::{Deserialize, Serialize};

/// The `cards.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CardSection {}
