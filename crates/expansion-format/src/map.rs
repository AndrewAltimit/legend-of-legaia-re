//! `maps/<id>.toml`: a custom map.

use serde::{Deserialize, Serialize};

/// One custom map.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapDef {}
