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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The pack schema version this pack was written against
    /// ([`SCHEMA_VERSION`]).
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// The disc the pack's [`crate::refs::DiscRef`]s are coordinates into.
    /// Only the NA release (`SCUS-94254`) is supported.
    #[serde(default = "default_disc")]
    pub disc: String,
    /// The licence the pack's own content is shared under: an SPDX
    /// expression (`CC-BY-4.0`) or free text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    /// Where the pack is published (a project page or repository URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Free-form discovery tags (`endgame`, `bosses`, `minigame`), each with
    /// the same syntax as a pack id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Pack ids that must be loaded alongside this one. A content id written
    /// `other:thing` may only name a pack listed here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Pack ids that must not be loaded alongside this one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
}

/// The schema version this crate reads.
pub const SCHEMA_VERSION: u32 = 1;

/// The only disc a pack's coordinates may point into.
pub const SUPPORTED_DISC: &str = "SCUS-94254";

/// The longest pack id, tag or content id accepted.
pub const MAX_ID_LEN: usize = 48;

fn default_schema() -> u32 {
    SCHEMA_VERSION
}

fn default_disc() -> String {
    SUPPORTED_DISC.to_string()
}

impl PackManifest {
    /// A manifest with the required fields set and every other field at its
    /// default (the current schema, the supported disc).
    pub fn new(id: &str, name: &str, version: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            schema: SCHEMA_VERSION,
            disc: SUPPORTED_DISC.to_string(),
            ..Self::default()
        }
    }
}

/// Is `id` a well-formed pack id or tag: 1..=[`MAX_ID_LEN`] characters of
/// `[a-z0-9_-]`, starting with a letter or digit.
pub fn is_valid_pack_id(id: &str) -> bool {
    is_id(id, |c| c == '-' || c == '_')
}

/// Is `id` a well-formed local content id (`tower_floor_1`): 1..=
/// [`MAX_ID_LEN`] characters of `[a-z0-9_]`, starting with a letter or
/// digit. Content ids are namespaced by the pack id, so they carry no `:`.
pub fn is_valid_content_id(id: &str) -> bool {
    is_id(id, |c| c == '_')
}

fn is_id(id: &str, extra: impl Fn(char) -> bool) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    id.len() <= MAX_ID_LEN
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || extra(c))
}

/// Is `v` a semantic version: `MAJOR.MINOR.PATCH` with no leading zeros,
/// then an optional `-pre.release` and `+build` suffix.
pub fn is_valid_semver(v: &str) -> bool {
    let (rest, build) = match v.split_once('+') {
        Some((r, b)) => (r, Some(b)),
        None => (v, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (rest, None),
    };
    let numeric = |s: &str| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
    };
    let ident_list = |s: &str| {
        s.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| numeric(p))
        && pre.is_none_or(ident_list)
        && build.is_none_or(ident_list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_syntax() {
        assert!(is_valid_pack_id("rebirth"));
        assert!(is_valid_pack_id("my-pack_2"));
        assert!(!is_valid_pack_id(""));
        assert!(!is_valid_pack_id("Rebirth"));
        assert!(!is_valid_pack_id("-lead"));
        assert!(!is_valid_pack_id("a:b"));
        assert!(!is_valid_pack_id(&"a".repeat(MAX_ID_LEN + 1)));
        assert!(is_valid_content_id("tower_floor_1"));
        assert!(!is_valid_content_id("tower-floor"));
    }

    #[test]
    fn semver_syntax() {
        for ok in [
            "0.1.0",
            "1.20.3",
            "1.0.0-alpha.1",
            "1.0.0+build.5",
            "2.0.0-rc-1+x",
        ] {
            assert!(is_valid_semver(ok), "{ok}");
        }
        for bad in [
            "", "1", "1.0", "01.0.0", "1.0.0-", "1.0.0+", "1.a.0", "1.0.0.0",
        ] {
            assert!(!is_valid_semver(bad), "{bad}");
        }
    }

    #[test]
    fn minimal_manifest_parses_with_defaults() {
        let m: PackManifest =
            toml::from_str("id = \"x\"\nname = \"X\"\nversion = \"0.1.0\"").unwrap();
        assert_eq!(m, PackManifest::new("x", "X", "0.1.0"));
    }
}
