//! `pack.toml`: who made the pack, what it is, and what it needs.

use serde::{Deserialize, Serialize};

use crate::version::{Version, VersionReq};

/// The pack's identity and requirements.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackManifest {
    /// Stable machine id, lowercase `[a-z0-9_-]`; namespaces every content
    /// id in the pack (`rebirth`).
    pub id: String,
    /// Display name (`Legend of Legaia: Rebirth`).
    pub name: String,
    /// A short label for places too narrow for [`Self::name`] (the
    /// launcher's Expansion button, a picker row): one line of at most
    /// [`MAX_SHORT_NAME_LEN`] characters (`Rebirth`). Unset, those places
    /// show the name. Schema 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_name: Option<String>,
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
    /// Packs that must be loaded alongside this one, each a [`PackDep`]
    /// string (`base`, `base@^1.2`). A content id written `other:thing` may
    /// only name a pack listed here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Packs that must not be loaded alongside this one, in the same form; a
    /// range limits the conflict to those versions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    /// The pack's cover image: a PNG under `assets/`, named by its path from
    /// the pack root (`assets/cover.png`). Shown by the play page's pack
    /// library, the launcher and the workbench. Schema 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// The oldest engine version the pack runs on, semver
    /// ([`ENGINE_VERSION`] is this build's). An older engine refuses the
    /// pack with a message naming both. Schema 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_engine: Option<String>,
}

/// The newest pack schema (pack-format version) this crate reads, and the
/// one a new pack is written against.
pub const SCHEMA_VERSION: u32 = 3;

/// The oldest pack schema this crate still reads. Every schema from here to
/// [`SCHEMA_VERSION`] loads; schemas 2 and 3 only added optional manifest
/// keys (2: `cover`, `min_engine`; 3: `short_name`).
pub const MIN_SCHEMA_VERSION: u32 = 1;

/// This engine's version, as a pack's `min_engine` is compared against: the
/// workspace version every engine crate shares.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The longest [`PackManifest::short_name`] accepted, in characters.
pub const MAX_SHORT_NAME_LEN: usize = 24;

/// The largest cover image accepted.
pub const MAX_COVER_LEN: usize = 1 << 20;

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
    Version::parse(v).is_some()
}

/// One `requires` / `conflicts` entry: a pack id, optionally `@` and a
/// version range (`base`, `base@^1.2`, `base@>=1.2 <2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackDep {
    pub id: String,
    /// `None` = any version.
    pub version: Option<VersionReq>,
}

impl PackDep {
    /// Parse an entry. The error names what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (id, range) = match text.split_once('@') {
            Some((id, r)) => (id.trim(), Some(r)),
            None => (text.trim(), None),
        };
        if !is_valid_pack_id(id) {
            return Err(format!(
                "{id:?} is not a pack id ([a-z0-9_-], at most {MAX_ID_LEN} characters)"
            ));
        }
        let version = range
            .map(VersionReq::parse)
            .transpose()
            .map_err(|e| format!("{text:?}: {e}"))?;
        Ok(Self {
            id: id.to_string(),
            version,
        })
    }

    /// Does pack `id` at `version` satisfy this entry.
    pub fn matches(&self, id: &str, version: &str) -> bool {
        self.id == id
            && match (&self.version, Version::parse(version)) {
                (None, _) => true,
                (Some(r), Some(v)) => r.matches(&v),
                (Some(_), None) => false,
            }
    }
}

impl std::fmt::Display for PackDep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.version {
            Some(r) => write!(f, "{}@{r}", self.id),
            None => f.write_str(&self.id),
        }
    }
}

/// The pack id an entry names, whether or not the rest of it parses (the
/// part before `@`).
pub fn dep_id(text: &str) -> &str {
    text.split_once('@').map_or(text, |(id, _)| id).trim()
}

/// Why this engine cannot load a pack, before anything else of it is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incompatible {
    /// The pack is written in a pack format newer than this engine reads.
    NewerFormat { schema: u32 },
    /// The pack names a schema older than any this engine reads.
    OlderFormat { schema: u32 },
    /// The pack needs a newer engine.
    NewerEngine { needs: String },
    /// `min_engine` is not a semantic version.
    BadMinEngine { text: String },
}

impl std::fmt::Display for Incompatible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewerFormat { schema } => write!(
                f,
                "the pack is written in pack format {schema}, newer than this engine reads \
                 (formats {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION}); update the engine to play it"
            ),
            Self::OlderFormat { schema } => write!(
                f,
                "the pack names pack format {schema}, older than any this engine reads \
                 (formats {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION})"
            ),
            Self::NewerEngine { needs } => write!(
                f,
                "the pack needs engine {needs} or newer; this engine is {ENGINE_VERSION}"
            ),
            Self::BadMinEngine { text } => {
                write!(
                    f,
                    "min_engine {text:?} is not a version (MAJOR.MINOR.PATCH)"
                )
            }
        }
    }
}

/// The keys of `pack.toml` that decide whether the rest can be read, taken
/// without the strict parse: a pack from a newer format may carry keys this
/// build does not know, and the reader should say "update the engine", not
/// "unknown field".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestPeek {
    pub id: String,
    pub name: String,
    pub version: String,
    pub schema: u32,
    pub min_engine: Option<String>,
}

impl ManifestPeek {
    /// Read the gate keys from `pack.toml` text. `None` when the text is not
    /// TOML at all (the strict parse then says why).
    pub fn read(text: &str) -> Option<Self> {
        let table: toml::Table = toml::from_str(text).ok()?;
        let s = |k: &str| {
            table
                .get(k)
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        Some(Self {
            id: s("id"),
            name: s("name"),
            version: s("version"),
            schema: match table.get("schema") {
                Some(v) => v
                    .as_integer()
                    .and_then(|i| u32::try_from(i).ok())
                    .unwrap_or(u32::MAX),
                None => SCHEMA_VERSION,
            },
            min_engine: table
                .get("min_engine")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
        })
    }

    /// Can this engine load the pack.
    pub fn compatibility(&self) -> Result<(), Incompatible> {
        check_compatibility(self.schema, self.min_engine.as_deref())
    }
}

/// Can this engine load a pack of `schema` that needs `min_engine`.
pub fn check_compatibility(schema: u32, min_engine: Option<&str>) -> Result<(), Incompatible> {
    if schema > SCHEMA_VERSION {
        return Err(Incompatible::NewerFormat { schema });
    }
    if schema < MIN_SCHEMA_VERSION {
        return Err(Incompatible::OlderFormat { schema });
    }
    if let Some(text) = min_engine {
        let needs = Version::parse(text).ok_or_else(|| Incompatible::BadMinEngine {
            text: text.to_string(),
        })?;
        let have = Version::parse(ENGINE_VERSION).expect("the workspace version is semver");
        if needs > have {
            return Err(Incompatible::NewerEngine {
                needs: text.to_string(),
            });
        }
    }
    Ok(())
}

impl PackManifest {
    /// The label for a narrow place: [`Self::short_name`] when set, else
    /// [`Self::name`].
    pub fn short_label(&self) -> &str {
        match self.short_name.as_deref().map(str::trim) {
            Some(s) if !s.is_empty() => s,
            _ => &self.name,
        }
    }

    /// Can this engine load the pack ([`check_compatibility`]).
    pub fn compatibility(&self) -> Result<(), Incompatible> {
        check_compatibility(self.schema, self.min_engine.as_deref())
    }

    /// The `requires` entries, parsed; malformed ones are skipped (the
    /// validator reports them).
    pub fn required(&self) -> Vec<PackDep> {
        self.requires
            .iter()
            .filter_map(|r| PackDep::parse(r).ok())
            .collect()
    }

    /// The `conflicts` entries, parsed.
    pub fn conflicting(&self) -> Vec<PackDep> {
        self.conflicts
            .iter()
            .filter_map(|r| PackDep::parse(r).ok())
            .collect()
    }
}

/// One way a pack's dependencies are not met by a set of installed packs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyProblem {
    /// A required pack is not installed.
    Missing { dep: String },
    /// A required pack is installed at a version outside the range.
    WrongVersion { dep: String, installed: String },
    /// A conflicting pack is installed.
    Conflict { dep: String, installed: String },
}

impl std::fmt::Display for DependencyProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { dep } => write!(f, "needs pack {dep}, which is not installed"),
            Self::WrongVersion { dep, installed } => {
                write!(f, "needs pack {dep}; version {installed} is installed")
            }
            Self::Conflict { dep, installed } => {
                write!(f, "conflicts with pack {dep} ({installed} is installed)")
            }
        }
    }
}

/// Check `m`'s `requires` and `conflicts` against the packs installed beside
/// it, `(id, version)` each (the pack itself may be among them).
pub fn check_dependencies(m: &PackManifest, installed: &[(&str, &str)]) -> Vec<DependencyProblem> {
    let mut out = Vec::new();
    let others = || installed.iter().filter(|(id, _)| *id != m.id);
    for dep in m.required() {
        match others().find(|(id, _)| *id == dep.id) {
            None => out.push(DependencyProblem::Missing {
                dep: dep.to_string(),
            }),
            Some((id, v)) if !dep.matches(id, v) => out.push(DependencyProblem::WrongVersion {
                dep: dep.to_string(),
                installed: v.to_string(),
            }),
            Some(_) => {}
        }
    }
    for dep in m.conflicting() {
        if let Some((_, v)) = others().find(|(id, v)| dep.matches(id, v)) {
            out.push(DependencyProblem::Conflict {
                dep: dep.to_string(),
                installed: v.to_string(),
            });
        }
    }
    out
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
    fn dependency_entries() {
        let d = PackDep::parse("base@^1.2").unwrap();
        assert_eq!(d.id, "base");
        assert!(d.matches("base", "1.4.0"));
        assert!(!d.matches("base", "2.0.0"));
        assert!(!d.matches("other", "1.4.0"));
        assert_eq!(d.to_string(), "base@^1.2");
        assert!(PackDep::parse("base").unwrap().matches("base", "9.0.0"));
        assert!(PackDep::parse("Base@1").is_err());
        assert!(PackDep::parse("base@").is_err());
        assert!(PackDep::parse("base@>=1 <2").is_ok());
        assert_eq!(dep_id("base@nonsense"), "base");
    }

    #[test]
    fn compatibility_gate() {
        assert!(check_compatibility(SCHEMA_VERSION, None).is_ok());
        assert!(
            check_compatibility(1, Some("0.0.1")).is_ok(),
            "old packs load"
        );
        assert_eq!(
            check_compatibility(SCHEMA_VERSION + 1, None),
            Err(Incompatible::NewerFormat {
                schema: SCHEMA_VERSION + 1
            })
        );
        assert!(matches!(
            check_compatibility(0, None),
            Err(Incompatible::OlderFormat { .. })
        ));
        let e = check_compatibility(1, Some("99.0.0")).unwrap_err();
        assert!(e.to_string().contains("99.0.0"), "{e}");
        assert!(e.to_string().contains(ENGINE_VERSION), "{e}");
        assert!(matches!(
            check_compatibility(1, Some("soon")),
            Err(Incompatible::BadMinEngine { .. })
        ));
    }

    #[test]
    fn peek_reads_a_future_manifest_the_strict_parse_refuses() {
        let text = "id = \"x\"\nname = \"X\"\nversion = \"1.0.0\"\nschema = 9\nfuture_key = 1\n";
        assert!(toml::from_str::<PackManifest>(text).is_err());
        let p = ManifestPeek::read(text).unwrap();
        assert_eq!((p.id.as_str(), p.schema), ("x", 9));
        assert!(matches!(
            p.compatibility(),
            Err(Incompatible::NewerFormat { schema: 9 })
        ));
    }

    #[test]
    fn dependencies_against_installed_packs() {
        let mut m = PackManifest::new("me", "Me", "1.0.0");
        m.requires = vec!["base@^1".into(), "extra".into()];
        m.conflicts = vec!["rival@<2".into()];
        let p = check_dependencies(
            &m,
            &[("me", "1.0.0"), ("base", "2.1.0"), ("rival", "1.5.0")],
        );
        assert_eq!(
            p,
            vec![
                DependencyProblem::WrongVersion {
                    dep: "base@^1".into(),
                    installed: "2.1.0".into()
                },
                DependencyProblem::Missing {
                    dep: "extra".into()
                },
                DependencyProblem::Conflict {
                    dep: "rival@<2".into(),
                    installed: "1.5.0".into()
                },
            ]
        );
        assert!(
            check_dependencies(
                &m,
                &[("base", "1.3.0"), ("extra", "0.1.0"), ("rival", "2.0.0")]
            )
            .is_empty()
        );
    }

    #[test]
    fn minimal_manifest_parses_with_defaults() {
        let m: PackManifest =
            toml::from_str("id = \"x\"\nname = \"X\"\nversion = \"0.1.0\"").unwrap();
        assert_eq!(m, PackManifest::new("x", "X", "0.1.0"));
    }
}
