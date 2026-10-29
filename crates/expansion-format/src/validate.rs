//! Pack validation: id syntax, cross-references, and the no-Sony-bytes
//! rule.
//!
//! [`validate`] runs over a parsed [`ExpansionPack`]; [`validate_files`]
//! runs over the raw file tree first (layout, file names, parse errors) and
//! then over the parsed pack. Both return every issue found rather than
//! stopping at the first, so the workbench can list them all.
//!
//! # How a section adds its checks
//!
//! Each content section implements [`SectionRules`]. Validation runs in two
//! passes over every section: [`SectionRules::declare`] registers the
//! content ids the section defines ([`Checker::declare`]), then
//! [`SectionRules::check`] checks its fields and names what it references
//! ([`Checker::reference`] for pack content, [`Checker::disc_ref`] for disc
//! coordinates). References resolve after both passes, so a quest may name a
//! boss defined in `battle.toml` regardless of section order.
//!
//! The empty `impl SectionRules` lines at the bottom of this file are
//! placeholders: a section that grows checks moves its impl into its own
//! module and deletes the line here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::manifest::{
    SCHEMA_VERSION, SUPPORTED_DISC, is_valid_content_id, is_valid_pack_id, is_valid_semver,
};
use crate::refs::{DiscRef, DiscResolver};
use crate::{ExpansionPack, PackFiles, modes};

/// How bad an issue is. An `Error` stops the engine loading the pack; a
/// `Warning` does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

/// One validation finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// The pack file the issue is in (`pack.toml`, `assets/hero.png`).
    pub path: String,
    /// Where in that file, as a dotted field path (`monsters.boss.hp`);
    /// empty when the issue is about the whole file.
    pub field: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        if self.field.is_empty() {
            write!(f, "{sev}: {}: {}", self.path, self.message)
        } else {
            write!(f, "{sev}: {} [{}]: {}", self.path, self.field, self.message)
        }
    }
}

/// Does `issues` contain an error.
pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

/// The checks a content section contributes. Both passes default to
/// nothing.
pub trait SectionRules {
    /// Pass 1: register the content ids this section defines.
    fn declare(&self, _cx: &mut Checker<'_>) {}
    /// Pass 2: check the section's own fields and record its references.
    fn check(&self, _cx: &mut Checker<'_>) {}
}

/// A content reference waiting for every section's declarations.
struct PendingRef {
    kind: &'static str,
    id: String,
    path: String,
    field: String,
}

/// The validation context a section's rules write into. It knows which file
/// is being checked, so a section reports by field only.
pub struct Checker<'a> {
    pack: &'a ExpansionPack,
    file: String,
    issues: Vec<Issue>,
    /// kind -> local id -> the file that declared it.
    declared: BTreeMap<&'static str, BTreeMap<String, String>>,
    pending: Vec<PendingRef>,
    disc_refs: Vec<(String, String, DiscRef)>,
    /// `(file, field, scene, mesh index)`: geometry-pack indices to check
    /// against the scene's pack once a resolver is supplied.
    geometry_refs: Vec<(String, String, String, u16)>,
}

impl<'a> Checker<'a> {
    fn new(pack: &'a ExpansionPack) -> Self {
        Self {
            pack,
            file: String::new(),
            issues: Vec::new(),
            declared: BTreeMap::new(),
            pending: Vec::new(),
            disc_refs: Vec::new(),
            geometry_refs: Vec::new(),
        }
    }

    /// The pack under validation.
    pub fn pack(&self) -> &ExpansionPack {
        self.pack
    }

    /// The file currently being checked.
    pub fn file(&self) -> &str {
        &self.file
    }

    fn push(&mut self, severity: Severity, path: &str, field: &str, message: String) {
        self.issues.push(Issue {
            severity,
            path: path.to_string(),
            field: field.to_string(),
            message,
        });
    }

    /// Report an error at `field` of the current file.
    pub fn error(&mut self, field: &str, message: impl Into<String>) {
        let file = self.file.clone();
        self.push(Severity::Error, &file, field, message.into());
    }

    /// Report a warning at `field` of the current file.
    pub fn warning(&mut self, field: &str, message: impl Into<String>) {
        let file = self.file.clone();
        self.push(Severity::Warning, &file, field, message.into());
    }

    /// The pack-qualified form of a local content id (`rebirth:tower`).
    pub fn qualified(&self, id: &str) -> String {
        format!("{}:{id}", self.pack.manifest.id)
    }

    /// Register content id `id` of `kind` (`monster`, `quest`, `card`, ...)
    /// as defined at `field`. Checks the id's syntax and that no other
    /// definition of the same kind uses it. Declared ids are local; the
    /// pack id is their namespace.
    pub fn declare(&mut self, kind: &'static str, id: &str, field: &str) {
        if id.contains(':') {
            self.error(
                field,
                format!(
                    "{kind} id {id:?} is pack-qualified; define it unqualified (the pack id namespaces it)"
                ),
            );
            return;
        }
        if !is_valid_content_id(id) {
            self.error(
                field,
                format!("{kind} id {id:?} is not [a-z0-9_], at most 48 characters"),
            );
            return;
        }
        let file = self.file.clone();
        match self.declared.entry(kind).or_default().entry(id.to_string()) {
            std::collections::btree_map::Entry::Occupied(prev) => {
                let prev = prev.get().clone();
                self.error(
                    field,
                    format!("{kind} id {id:?} is already defined in {prev}"),
                );
            }
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(file);
            }
        }
    }

    /// Record that `field` names content `id` of `kind`. A bare id (or one
    /// qualified with this pack's own id) must be declared somewhere in the
    /// pack; `other:id` must name a pack listed in `requires`, and is
    /// resolved by the engine once that pack is loaded.
    pub fn reference(&mut self, kind: &'static str, id: &str, field: &str) {
        self.pending.push(PendingRef {
            kind,
            id: id.to_string(),
            path: self.file.clone(),
            field: field.to_string(),
        });
    }

    /// Record a disc coordinate at `field`: range-checked now, and handed to
    /// a [`DiscResolver`] when one is supplied.
    pub fn disc_ref(&mut self, field: &str, r: &DiscRef) {
        if let Err(e) = r.check_range() {
            self.error(field, e);
        } else {
            self.disc_refs
                .push((self.file.clone(), field.to_string(), r.clone()));
        }
    }

    /// Record that `field` names mesh `index` of scene `scene`'s environment
    /// geometry pack: checked against the pack's length when the resolver
    /// knows it ([`DiscResolver::scene_geometry_len`]).
    pub fn geometry_index(&mut self, field: &str, scene: &str, index: u16) {
        self.geometry_refs.push((
            self.file.clone(),
            field.to_string(),
            scene.to_string(),
            index,
        ));
    }

    fn resolve_references(&mut self) {
        let own = self.pack.manifest.id.clone();
        let requires: BTreeSet<&str> = self
            .pack
            .manifest
            .requires
            .iter()
            .map(String::as_str)
            .collect();
        let pending = std::mem::take(&mut self.pending);
        for p in pending {
            let local = match p.id.split_once(':') {
                Some((pack, id)) if pack == own => id,
                Some((pack, _)) => {
                    if !requires.contains(pack) {
                        self.push(
                            Severity::Error,
                            &p.path,
                            &p.field,
                            format!(
                                "{} {:?} names pack {pack:?}, which is not in pack.toml `requires`",
                                p.kind, p.id
                            ),
                        );
                    }
                    continue;
                }
                None => p.id.as_str(),
            };
            let found = self
                .declared
                .get(p.kind)
                .is_some_and(|ids| ids.contains_key(local));
            if !found {
                self.push(
                    Severity::Error,
                    &p.path,
                    &p.field,
                    format!("{} {:?} is not defined in this pack", p.kind, p.id),
                );
            }
        }
    }

    fn resolve_disc(&mut self, resolver: &dyn DiscResolver) {
        let refs = std::mem::take(&mut self.disc_refs);
        for (path, field, r) in refs {
            if let Err(e) = resolver.resolve(&r) {
                self.push(Severity::Error, &path, &field, format!("{r}: {e}"));
            }
        }
        for (path, field, scene, index) in std::mem::take(&mut self.geometry_refs) {
            if let Some(len) = resolver.scene_geometry_len(&scene)
                && usize::from(index) >= len
            {
                self.push(
                    Severity::Error,
                    &path,
                    &field,
                    format!("scene `{scene}`'s geometry pack has {len} meshes; mesh {index} is past the last"),
                );
            }
        }
    }
}

/// Validate a parsed pack with no disc: everything but disc-backed
/// resolution.
pub fn validate(pack: &ExpansionPack) -> Vec<Issue> {
    validate_with(pack, None)
}

/// Validate a parsed pack, resolving every [`DiscRef`] through `resolver`
/// when one is supplied.
pub fn validate_with(pack: &ExpansionPack, resolver: Option<&dyn DiscResolver>) -> Vec<Issue> {
    let mut cx = Checker::new(pack);
    cx.file = "pack.toml".into();
    check_manifest(&mut cx);
    for (path, bytes) in &pack.assets {
        check_asset(&mut cx, &format!("assets/{path}"), bytes);
    }
    check_asset_budget(&mut cx);
    for name in pack.maps.keys() {
        cx.file = format!("maps/{name}.toml");
        cx.declare("map", name, "");
    }

    let sections = sections(pack);
    for (file, rules) in &sections {
        cx.file.clone_from(file);
        rules.declare(&mut cx);
    }
    for (file, rules) in &sections {
        cx.file.clone_from(file);
        rules.check(&mut cx);
    }
    cx.resolve_references();
    if let Some(resolver) = resolver {
        cx.resolve_disc(resolver);
    }
    cx.issues
}

/// Validate a raw pack tree: file layout and names, parsing, then
/// everything [`validate_with`] checks.
pub fn validate_files(files: &PackFiles, resolver: Option<&dyn DiscResolver>) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut issue = |severity, path: &str, message: String| {
        issues.push(Issue {
            severity,
            path: path.to_string(),
            field: String::new(),
            message,
        })
    };
    for path in files.keys() {
        if !crate::bundle::is_valid_path(path) {
            issue(Severity::Error, path, "not a valid pack path".into());
        } else if let Some(why) = disc_file_name(path) {
            issue(Severity::Error, path, why);
        } else if !crate::is_known_path(path) {
            issue(
                Severity::Warning,
                path,
                "not part of the pack layout; the loader ignores it".into(),
            );
        }
    }
    match ExpansionPack::from_files(files) {
        Ok(pack) => issues.extend(validate_with(&pack, resolver)),
        Err(e) => issue(Severity::Error, "pack.toml", format!("{e:#}")),
    }
    issues
}

fn sections(pack: &ExpansionPack) -> Vec<(String, &dyn SectionRules)> {
    let mut out: Vec<(String, &dyn SectionRules)> = vec![
        ("battle.toml".into(), &pack.battle),
        ("progression.toml".into(), &pack.progression),
        ("quests.toml".into(), &pack.quests),
        ("dialog.toml".into(), &pack.dialog),
        ("cards.toml".into(), &pack.cards),
        ("modes.toml".into(), &pack.modes),
    ];
    for (name, map) in &pack.maps {
        out.push((format!("maps/{name}.toml"), map));
    }
    out
}

fn check_manifest(cx: &mut Checker<'_>) {
    // Copy the `&'a` pack out so reading the manifest does not borrow `cx`.
    let pack = cx.pack;
    let m = &pack.manifest;
    if !is_valid_pack_id(&m.id) {
        cx.error(
            "id",
            format!(
                "pack id {:?} is not [a-z0-9_-], at most 48 characters",
                m.id
            ),
        );
    }
    if m.name.trim().is_empty() {
        cx.error("name", "the pack has no display name");
    }
    if !is_valid_semver(&m.version) {
        cx.error(
            "version",
            format!("version {:?} is not semver (MAJOR.MINOR.PATCH)", m.version),
        );
    }
    match m.schema {
        s if s == SCHEMA_VERSION => {}
        s if s > SCHEMA_VERSION => cx.error(
            "schema",
            format!(
                "schema {s} is newer than this build reads ({SCHEMA_VERSION}); update the engine"
            ),
        ),
        s => cx.error("schema", format!("schema {s} is not supported")),
    }
    if m.disc != SUPPORTED_DISC {
        cx.error(
            "disc",
            format!(
                "disc {:?} is not supported; coordinates are into {SUPPORTED_DISC}",
                m.disc
            ),
        );
    }
    if m.description.trim().is_empty() {
        cx.warning("description", "the pack has no description");
    }
    if m.authors.is_empty() {
        cx.warning("authors", "the pack names no authors");
    }
    if m.license.trim().is_empty() {
        cx.warning(
            "license",
            "the pack states no licence; others cannot know if they may share it",
        );
    }
    if let Some(h) = &m.homepage
        && !(h.starts_with("https://") || h.starts_with("http://"))
    {
        cx.warning("homepage", format!("homepage {h:?} is not an http(s) URL"));
    }
    check_id_list(cx, "tags", &m.tags, &m.id);
    check_id_list(cx, "requires", &m.requires, &m.id);
    check_id_list(cx, "conflicts", &m.conflicts, &m.id);
    for both in m.requires.iter().filter(|r| m.conflicts.contains(r)) {
        cx.error(
            "conflicts",
            format!("{both:?} is listed in both `requires` and `conflicts`"),
        );
    }
}

fn check_id_list(cx: &mut Checker<'_>, field: &str, ids: &[String], own: &str) {
    let mut seen = BTreeSet::new();
    for (i, id) in ids.iter().enumerate() {
        let at = format!("{field}[{i}]");
        if !is_valid_pack_id(id) {
            cx.error(
                &at,
                format!("{id:?} is not [a-z0-9_-], at most 48 characters"),
            );
        } else if field != "tags" && id == own {
            cx.error(&at, "a pack cannot name itself");
        } else if !seen.insert(id.as_str()) {
            cx.warning(&at, format!("{id:?} is listed twice"));
        }
    }
}

// ---------------------------------------------------------------------------
// The no-Sony-bytes rule.
// ---------------------------------------------------------------------------

/// Largest single file under `assets/`.
pub const MAX_ASSET_LEN: usize = 8 << 20;
/// Largest total of every file under `assets/`.
pub const MAX_ASSETS_TOTAL: usize = 64 << 20;

/// The file types a pack may carry under `assets/`: the modder's own art,
/// models, sound and text, in formats the disc does not use.
pub const ASSET_EXTENSIONS: &[&str] = &["png", "gltf", "glb", "ogg", "wav", "txt", "md", "toml"];

/// Exact file names (any case) of disc files and images.
const DISC_FILE_NAMES: &[&str] = &[
    "prot.dat",
    "dmy.dat",
    "scus_942.54",
    "system.cnf",
    "cdname.txt",
];

/// Extensions of disc images and of the disc's own formats.
const DISC_EXTENSIONS: &[&str] = &[
    "bin", "iso", "cue", "img", "ccd", "sub", "mdf", "mds", "chd", "ecm", "pbp", "str", "xa",
    "dat", "exe", "tim", "tmd", "vab", "vag", "seq", "lzs", "man", "mes", "anm", "mdt", "dpk",
    "spk", "pch",
];

fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}

/// Why `path` is named like a disc file or disc image, if it is.
pub fn disc_file_name(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if DISC_FILE_NAMES.contains(&name.as_str()) {
        return Some(format!(
            "{name} is a retail disc file; a pack names disc content by coordinate, never by copy"
        ));
    }
    let ext = extension(path)?;
    DISC_EXTENSIONS
        .contains(&ext.as_str())
        .then(|| format!(".{ext} is a disc image or retail disc format; packs may not carry one"))
}

/// If `bytes` look like a retail disc format, which one.
pub fn sniff_disc_format(bytes: &[u8]) -> Option<&'static str> {
    const SECTOR_SYNC: [u8; 12] = [
        0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0,
    ];
    let word = bytes
        .get(..4)
        .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")));
    match word {
        Some(0x0000_0010) => return Some("a PSX TIM image"),
        Some(0x8000_0002) => return Some("a Legaia TMD model"),
        Some(0x0000_0041) => return Some("a PSX TMD model"),
        _ => {}
    }
    let head: &[(&[u8], &str)] = &[
        (b"pBAV", "a VAB sound bank"),
        (b"pQES", "a SEQ sequence"),
        (b"VAGp", "a VAG sample"),
        (b"PS-X EXE", "a PS-X executable"),
        (&SECTOR_SYNC, "a raw CD sector"),
    ];
    if let Some((_, what)) = head.iter().find(|(m, _)| bytes.starts_with(m)) {
        return Some(what);
    }
    if bytes.get(0x8001..0x8006) == Some(b"CD001") || bytes.get(1..6) == Some(b"CD001") {
        return Some("an ISO 9660 image");
    }
    // Long markers are distinctive enough to look for anywhere, which
    // catches a disc file appended to or embedded in an otherwise valid one.
    let embedded: &[(&[u8], &str)] = &[
        (b"PS-X EXE", "an embedded PS-X executable"),
        (&SECTOR_SYNC, "an embedded raw CD sector"),
    ];
    embedded
        .iter()
        .find(|(m, _)| bytes.windows(m.len()).any(|w| w == *m))
        .map(|(_, what)| *what)
}

/// Does `bytes` have the signature of an asset of type `ext`.
fn matches_type(ext: &str, bytes: &[u8]) -> bool {
    match ext {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "glb" => bytes.starts_with(b"glTF") && bytes.get(4..8) == Some(&2u32.to_le_bytes()),
        "ogg" => bytes.starts_with(b"OggS"),
        "wav" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "gltf" => std::str::from_utf8(bytes).is_ok_and(|t| t.trim_start().starts_with('{')),
        "txt" | "md" | "toml" => std::str::from_utf8(bytes).is_ok_and(|t| !t.contains('\0')),
        _ => false,
    }
}

fn check_asset(cx: &mut Checker<'_>, path: &str, bytes: &[u8]) {
    let mut err = |message: String| {
        cx.push(Severity::Error, path, "", message);
    };
    if let Some(why) = disc_file_name(path) {
        return err(why);
    }
    let Some(ext) = extension(path).filter(|e| ASSET_EXTENSIONS.contains(&e.as_str())) else {
        return err(format!(
            "assets may only be {}",
            ASSET_EXTENSIONS
                .iter()
                .map(|e| format!(".{e}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    if bytes.len() > MAX_ASSET_LEN {
        return err(format!(
            "{} bytes is over the {MAX_ASSET_LEN}-byte asset limit",
            bytes.len()
        ));
    }
    if let Some(what) = sniff_disc_format(bytes) {
        return err(format!(
            "the bytes are {what}; a pack carries no disc data - name it with a DiscRef instead"
        ));
    }
    if !matches_type(&ext, bytes) {
        err(format!("the bytes are not a .{ext} file"));
    }
}

fn check_asset_budget(cx: &mut Checker<'_>) {
    let total: usize = cx.pack.assets.values().map(Vec::len).sum();
    if total > MAX_ASSETS_TOTAL {
        cx.push(
            Severity::Error,
            "assets/",
            "",
            format!("assets total {total} bytes, over the {MAX_ASSETS_TOTAL}-byte budget"),
        );
    }
}

// Placeholder rules; see the module docs.
impl SectionRules for modes::ModeSection {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PackManifest;

    fn good_pack() -> ExpansionPack {
        let mut manifest = PackManifest::new("test", "Test", "0.1.0");
        manifest.description = "d".into();
        manifest.authors = vec!["me".into()];
        manifest.license = "CC0-1.0".into();
        ExpansionPack {
            manifest,
            ..Default::default()
        }
    }

    fn errors(issues: &[Issue]) -> Vec<String> {
        issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn a_clean_pack_has_no_issues() {
        assert_eq!(validate(&good_pack()), vec![]);
    }

    #[test]
    fn manifest_errors() {
        let mut p = good_pack();
        p.manifest.id = "Bad Id".into();
        p.manifest.version = "1.0".into();
        p.manifest.schema = SCHEMA_VERSION + 1;
        p.manifest.disc = "SLES-02375".into();
        p.manifest.requires = vec!["base".into(), "Bad".into()];
        p.manifest.conflicts = vec!["base".into()];
        let e = errors(&validate(&p));
        for field in [
            "[id]",
            "[version]",
            "[schema]",
            "[disc]",
            "[requires[1]]",
            "[conflicts]",
        ] {
            assert!(e.iter().any(|m| m.contains(field)), "{field} in {e:?}");
        }
    }

    #[test]
    fn missing_metadata_only_warns() {
        let p = ExpansionPack {
            manifest: PackManifest::new("test", "Test", "0.1.0"),
            ..Default::default()
        };
        let issues = validate(&p);
        assert!(!has_errors(&issues));
        assert_eq!(issues.len(), 3, "{issues:?}");
    }

    #[test]
    fn fake_tim_and_tmd_assets_are_rejected() {
        let mut p = good_pack();
        let mut tim = vec![0x10, 0, 0, 0, 0x08, 0, 0, 0];
        tim.resize(64, 0);
        p.assets.insert("art/hero.png".into(), tim);
        let mut tmd = 0x8000_0002u32.to_le_bytes().to_vec();
        tmd.resize(64, 0);
        p.assets.insert("models/hero.glb".into(), tmd);
        p.assets
            .insert("sfx/bank.wav".into(), b"pBAV\0\0\0\0".to_vec());
        p.assets.insert("raw.tim".into(), vec![0x10, 0, 0, 0]);
        p.assets.insert("bonus.zip".into(), b"PK\x03\x04".to_vec());
        let e = errors(&validate(&p));
        assert_eq!(e.len(), 5, "{e:#?}");
        assert!(
            e.iter()
                .any(|m| m.contains("hero.png") && m.contains("TIM"))
        );
        assert!(
            e.iter()
                .any(|m| m.contains("hero.glb") && m.contains("TMD"))
        );
        assert!(
            e.iter()
                .any(|m| m.contains("bank.wav") && m.contains("VAB"))
        );
        assert!(
            e.iter()
                .any(|m| m.contains("raw.tim") && m.contains(".tim"))
        );
        assert!(
            e.iter()
                .any(|m| m.contains("bonus.zip") && m.contains("may only be"))
        );
    }

    #[test]
    fn real_modder_assets_pass() {
        let mut p = good_pack();
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0; 32]);
        p.assets.insert("art/a.png".into(), png);
        p.assets.insert("notes.md".into(), b"# notes".to_vec());
        let mut wav = b"RIFF\x24\0\0\0WAVEfmt ".to_vec();
        wav.resize(44, 0);
        p.assets.insert("sfx/a.wav".into(), wav);
        assert_eq!(validate(&p), vec![]);
    }

    #[test]
    fn embedded_and_oversized_assets_are_rejected() {
        let mut p = good_pack();
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(b"....PS-X EXE....");
        p.assets.insert("a.png".into(), png);
        let mut big = b"\x89PNG\r\n\x1a\n".to_vec();
        big.resize(MAX_ASSET_LEN + 1, 0);
        p.assets.insert("b.png".into(), big);
        let e = errors(&validate(&p));
        assert!(
            e.iter()
                .any(|m| m.contains("a.png") && m.contains("executable"))
        );
        assert!(e.iter().any(|m| m.contains("b.png") && m.contains("limit")));
    }

    #[test]
    fn disc_named_files_are_refused_anywhere() {
        let mut files = PackFiles::new();
        files.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"0.1.0\"\nauthors = [\"a\"]\ndescription = \"d\"\nlicense = \"CC0-1.0\"\n".to_vec(),
        );
        for p in [
            "PROT.DAT",
            "extra/SCUS_942.54",
            "Legaia.BIN",
            "x/MV1.STR",
            "XA1.XA",
        ] {
            files.insert(p.into(), vec![0]);
        }
        files.insert("notes/todo.txt".into(), b"x".to_vec());
        let issues = validate_files(&files, None);
        assert_eq!(errors(&issues).len(), 5, "{issues:#?}");
        assert!(
            issues
                .iter()
                .any(|i| i.path == "notes/todo.txt" && i.severity == Severity::Warning)
        );
    }

    #[test]
    fn parse_errors_become_issues() {
        let mut files = PackFiles::new();
        files.insert("pack.toml".into(), b"id = ".to_vec());
        assert!(has_errors(&validate_files(&files, None)));
        assert!(has_errors(&validate_files(&PackFiles::new(), None)));
    }

    /// Exercise the two-pass registry with a section defined only here.
    #[test]
    fn registry_resolves_across_sections() {
        struct Defines;
        impl SectionRules for Defines {
            fn declare(&self, cx: &mut Checker<'_>) {
                cx.declare("boss", "tower_king", "bosses[0]");
                cx.declare("boss", "tower_king", "bosses[1]");
                cx.declare("boss", "base:other", "bosses[2]");
                cx.declare("boss", "Bad", "bosses[3]");
            }
        }
        struct Uses;
        impl SectionRules for Uses {
            fn check(&self, cx: &mut Checker<'_>) {
                cx.reference("boss", "tower_king", "a");
                cx.reference("boss", "test:tower_king", "b");
                cx.reference("boss", "missing", "c");
                cx.reference("boss", "base:thing", "d");
                cx.reference("boss", "stranger:thing", "e");
                cx.disc_ref("f", &DiscRef::Monster { id: 0 });
                cx.disc_ref("g", &DiscRef::Monster { id: 5 });
            }
        }
        struct NoMonsterFive;
        impl DiscResolver for NoMonsterFive {
            fn resolve(&self, r: &DiscRef) -> Result<(), String> {
                match r {
                    DiscRef::Monster { id: 5 } => Err("absent".into()),
                    _ => Ok(()),
                }
            }
        }
        let mut pack = good_pack();
        pack.manifest.requires = vec!["base".into()];
        let mut cx = Checker::new(&pack);
        let sections: [(&str, &dyn SectionRules); 2] = [("q.toml", &Uses), ("b.toml", &Defines)];
        for (f, s) in sections {
            cx.file = f.into();
            s.declare(&mut cx);
        }
        for (f, s) in sections {
            cx.file = f.into();
            s.check(&mut cx);
        }
        cx.resolve_references();
        cx.resolve_disc(&NoMonsterFive);
        let fields: Vec<String> = cx
            .issues
            .iter()
            .map(|i| format!("{}:{}", i.path, i.field))
            .collect();
        assert_eq!(
            fields,
            [
                "b.toml:bosses[1]",
                "b.toml:bosses[2]",
                "b.toml:bosses[3]",
                "q.toml:f",
                "q.toml:c",
                "q.toml:e",
                "q.toml:g",
            ]
        );
    }
}
