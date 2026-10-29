//! `pack.toml`: who made the pack, what it is, and what it needs.

use serde::{Deserialize, Serialize};

/// The pack's identity and requirements.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackManifest {
    /// Stable machine id, lowercase `[a-z0-9_-]`; namespaces every content
    /// id in the pack (`rebirth`).
    pub id: String,
    /// Display name (`Legend of Legaia: Rebirth`).
    pub name: String,
    /// Pack version, semver text.
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: String,
    /// The pack schema version this pack was written against
    /// ([`SCHEMA_VERSION`]).
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// The disc the pack's [`crate::refs::DiscRef`]s are coordinates into.
    /// Only the NA release (`SCUS-94254`) is supported.
    #[serde(default = "default_disc")]
    pub disc: String,
}

/// The schema version this crate reads.
pub const SCHEMA_VERSION: u32 = 1;

fn default_schema() -> u32 {
    SCHEMA_VERSION
}

fn default_disc() -> String {
    "SCUS-94254".to_string()
}
