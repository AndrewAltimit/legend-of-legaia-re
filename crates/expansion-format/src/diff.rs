//! Compare two versions of a pack, file by file and entry by entry, and
//! draft a changelog from the comparison.
//!
//! Each content file is compared through its TOML value, so a diff speaks
//! the file's own vocabulary (`monsters.mist_hound.hp`) and is blind to
//! comments, key order and formatting. Within a file every top-level key is
//! a **collection** when it holds a table of entries (`monsters`,
//! `formations`) or an array of tables that each carry an `id` (`quests`,
//! `cards`); its entries are matched by key or id and reported as added,
//! removed or changed, a change listing each field that differs. Any other
//! top-level key (a scalar, a plain list) is one entry of the file's
//! `settings`. A map file is one entry of the `maps` collection, and the
//! manifest one entry of its own. Assets compare by content.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::ExpansionPack;
use crate::version::Version;

/// What happened to one entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "change", content = "fields")]
pub enum Change {
    Added,
    Removed,
    /// The fields that differ.
    Changed(Vec<FieldChange>),
}

/// One field that differs, as compact TOML text (`None` where absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldChange {
    /// Dotted path inside the entry (`hp`, `rewards.gold`, `objectives[2]`);
    /// empty when the entry is a single value.
    pub field: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// One added, removed or changed entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryDiff {
    /// The collection the entry is in (`monsters`, `quests`, `maps`,
    /// `settings`, `assets`, `manifest`).
    pub collection: String,
    pub id: String,
    #[serde(flatten)]
    pub change: Change,
}

/// Every difference in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub entries: Vec<EntryDiff>,
}

/// A whole comparison, old pack to new.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackDiff {
    pub old_version: String,
    pub new_version: String,
    /// Only files with a difference, in path order.
    pub files: Vec<FileDiff>,
}

/// The semver bump a diff calls for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Bump {
    None,
    Patch,
    Minor,
    Major,
}

impl PackDiff {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Every entry with its file, in file order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &EntryDiff)> {
        self.files
            .iter()
            .flat_map(|f| f.entries.iter().map(move |e| (f.path.as_str(), e)))
    }

    /// `(added, removed, changed)` content-entry counts (the manifest's own
    /// fields not among them).
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut c = (0, 0, 0);
        for (_, e) in self.entries().filter(|(_, e)| e.collection != "manifest") {
            match e.change {
                Change::Added => c.0 += 1,
                Change::Removed => c.1 += 1,
                Change::Changed(_) => c.2 += 1,
            }
        }
        c
    }

    /// The bump the diff calls for: removing content breaks saves and
    /// dependent packs (major), adding is minor, changing only is a patch.
    /// The manifest's own fields (its version among them) do not count.
    pub fn bump(&self) -> Bump {
        let content = self.entries().filter(|(_, e)| e.collection != "manifest");
        content.fold(Bump::None, |b, (_, e)| {
            b.max(match e.change {
                Change::Removed => Bump::Major,
                Change::Added => Bump::Minor,
                Change::Changed(_) => Bump::Patch,
            })
        })
    }

    /// The version the new pack should carry: `old_version` bumped by
    /// [`Self::bump`] (below `1.0.0` a breaking change bumps the minor, as
    /// semver's `0.y.z` convention has it). `None` when `old_version` is not
    /// semver or nothing changed.
    pub fn suggested_version(&self) -> Option<String> {
        let v = Version::parse(&self.old_version)?;
        let (maj, min, pat) = (v.major, v.minor, v.patch);
        let next = match (self.bump(), maj) {
            (Bump::None, _) => return None,
            (Bump::Major, 0) | (Bump::Minor, _) => Version::new(maj, min + 1, 0),
            (Bump::Major, _) => Version::new(maj + 1, 0, 0),
            (Bump::Patch, _) => Version::new(maj, min, pat + 1),
        };
        Some(next.to_string())
    }

    /// A Markdown changelog section for the new version, grouped Added /
    /// Changed / Removed, ready to paste above the older sections of
    /// `CHANGELOG.md`.
    pub fn changelog(&self) -> String {
        let heading = if self.new_version == self.old_version {
            self.suggested_version()
                .unwrap_or_else(|| self.new_version.clone())
        } else {
            self.new_version.clone()
        };
        let mut out = format!("## {heading}\n\nChanges since {}.\n", self.old_version);
        if self.is_empty() {
            out.push_str("\nNo content changes.\n");
            return out;
        }
        let groups: [(&str, ChangePick); 3] = [
            ("Added", |c| matches!(c, Change::Added)),
            ("Changed", |c| matches!(c, Change::Changed(_))),
            ("Removed", |c| matches!(c, Change::Removed)),
        ];
        for (title, pick) in groups {
            let lines: Vec<String> = self
                .entries()
                .filter(|(_, e)| pick(&e.change) && e.collection != "manifest")
                .map(|(path, e)| changelog_line(path, e))
                .collect();
            if !lines.is_empty() {
                out.push_str(&format!("\n### {title}\n\n"));
                for l in lines {
                    out.push_str(&format!("- {l}\n"));
                }
            }
        }
        out
    }
}

/// Picks one changelog group's changes.
type ChangePick = fn(&Change) -> bool;

fn changelog_line(path: &str, e: &EntryDiff) -> String {
    let what = match e.collection.as_str() {
        "settings" => format!("`{}`", e.id),
        "assets" => format!("asset `{}`", e.id),
        c => format!("{} `{}`", singular(c), e.id),
    };
    match &e.change {
        Change::Changed(fields) => {
            let mut names: Vec<&str> = fields
                .iter()
                .map(|f| f.field.as_str())
                .filter(|f| !f.is_empty())
                .collect();
            names.dedup();
            let shown = names.len().min(6);
            let mut list = names[..shown].join(", ");
            if names.len() > shown {
                list.push_str(&format!(", and {} more", names.len() - shown));
            }
            if list.is_empty() {
                format!("{what} ({path})")
            } else {
                format!("{what}: {list} ({path})")
            }
        }
        _ => format!("{what} ({path})"),
    }
}

/// `monsters` -> `monster`, `bosses` -> `boss`, `quests` -> `quest`.
fn singular(c: &str) -> String {
    if let Some(s) = c.strip_suffix("sses") {
        return format!("{s}ss");
    }
    if let Some(s) = c.strip_suffix("ies") {
        return format!("{s}y");
    }
    c.strip_suffix('s').unwrap_or(c).to_string()
}

/// Compare `old` to `new`.
pub fn diff_packs(old: &ExpansionPack, new: &ExpansionPack) -> PackDiff {
    let mut files = Vec::new();
    let mut push = |path: String, entries: Vec<EntryDiff>| {
        if !entries.is_empty() {
            files.push(FileDiff { path, entries });
        }
    };

    // The manifest, as one entry.
    let (a, b) = (to_value(&old.manifest), to_value(&new.manifest));
    let mut fields = Vec::new();
    field_changes("", Some(&a), Some(&b), &mut fields);
    if !fields.is_empty() {
        push(
            "pack.toml".into(),
            vec![EntryDiff {
                collection: "manifest".into(),
                id: new.manifest.id.clone(),
                change: Change::Changed(fields),
            }],
        );
    }

    for (path, a, b) in section_values(old)
        .into_iter()
        .zip(section_values(new))
        .map(|((p, a), (_, b))| (p, a, b))
    {
        push(path.to_string(), diff_section(&a, &b));
    }

    // Maps: one entry each, under the file it lives in.
    let ids: BTreeSet<&String> = old.maps.keys().chain(new.maps.keys()).collect();
    for id in ids {
        let (a, b) = (
            old.maps.get(id).map(to_value),
            new.maps.get(id).map(to_value),
        );
        if let Some(e) = entry_diff("maps", id, a.as_ref(), b.as_ref()) {
            push(format!("maps/{id}.toml"), vec![e]);
        }
    }

    // Assets and documentation, by content.
    let mut assets = Vec::new();
    let paths: BTreeSet<&String> = old.assets.keys().chain(new.assets.keys()).collect();
    for p in paths {
        let (a, b) = (old.assets.get(p), new.assets.get(p));
        let change = match (a, b) {
            (None, Some(_)) => Change::Added,
            (Some(_), None) => Change::Removed,
            (Some(x), Some(y)) if x != y => Change::Changed(vec![FieldChange {
                field: "bytes".into(),
                old: Some(format!("{} bytes", x.len())),
                new: Some(format!("{} bytes", y.len())),
            }]),
            _ => continue,
        };
        assets.push(EntryDiff {
            collection: "assets".into(),
            id: p.clone(),
            change,
        });
    }
    push("assets/".into(), assets);

    files.sort_by(|a, b| a.path.cmp(&b.path));
    PackDiff {
        old_version: old.manifest.version.clone(),
        new_version: new.manifest.version.clone(),
        files,
    }
}

fn to_value<T: Serialize>(v: &T) -> toml::Value {
    toml::Value::try_from(v).unwrap_or_else(|_| toml::Value::Table(Default::default()))
}

/// Each section file and its TOML value, in [`crate::SECTION_FILES`] order.
fn section_values(p: &ExpansionPack) -> Vec<(&'static str, toml::Value)> {
    vec![
        ("battle.toml", to_value(&p.battle)),
        ("progression.toml", to_value(&p.progression)),
        ("quests.toml", to_value(&p.quests)),
        ("dialog.toml", to_value(&p.dialog)),
        ("cards.toml", to_value(&p.cards)),
        ("cutscenes.toml", to_value(&p.cutscenes)),
        ("modes.toml", to_value(&p.modes)),
        ("items.toml", to_value(&p.items)),
        ("audio.toml", to_value(&p.audio)),
        ("arts.toml", to_value(&p.arts)),
        ("spells.toml", to_value(&p.spells)),
    ]
}

/// The entries of a collection value, keyed: a table's keys, or the `id`
/// of each table in an array. `None` when the value is not a collection.
fn keyed(v: &toml::Value) -> Option<BTreeMap<String, &toml::Value>> {
    match v {
        toml::Value::Table(t) if t.values().all(toml::Value::is_table) => {
            Some(t.iter().map(|(k, v)| (k.clone(), v)).collect())
        }
        toml::Value::Array(a) if !a.is_empty() => {
            let mut out = BTreeMap::new();
            for item in a {
                let id = item.get("id")?.as_str()?;
                if out.insert(id.to_string(), item).is_some() {
                    return None;
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// [`keyed`], with an absent value or an empty array as no entries.
fn coll(v: Option<&toml::Value>) -> Option<BTreeMap<String, &toml::Value>> {
    match v {
        None => Some(BTreeMap::new()),
        Some(toml::Value::Array(a)) if a.is_empty() => Some(BTreeMap::new()),
        Some(v) => keyed(v),
    }
}

fn diff_section(a: &toml::Value, b: &toml::Value) -> Vec<EntryDiff> {
    let empty = toml::Table::new();
    let ta = a.as_table().unwrap_or(&empty);
    let tb = b.as_table().unwrap_or(&empty);
    let keys: BTreeSet<&String> = ta.keys().chain(tb.keys()).collect();
    let mut out = Vec::new();
    for key in keys {
        let (va, vb) = (ta.get(key), tb.get(key));
        if va == vb {
            continue;
        }
        // An absent or empty value reads as a collection with no entries,
        // so the first quest added is "quest x added", not a changed list.
        match (coll(va), coll(vb)) {
            (Some(ea), Some(eb)) if !(ea.is_empty() && eb.is_empty()) => {
                let ids: BTreeSet<&String> = ea.keys().chain(eb.keys()).collect();
                for id in ids {
                    if let Some(e) = entry_diff(key, id, ea.get(id).copied(), eb.get(id).copied()) {
                        out.push(e);
                    }
                }
            }
            _ => out.extend(entry_diff("settings", key, va, vb)),
        }
    }
    out
}

fn entry_diff(
    collection: &str,
    id: &str,
    a: Option<&toml::Value>,
    b: Option<&toml::Value>,
) -> Option<EntryDiff> {
    let change = match (a, b) {
        (None, None) => return None,
        (None, Some(_)) => Change::Added,
        (Some(_), None) => Change::Removed,
        (Some(x), Some(y)) if x == y => return None,
        (Some(x), Some(y)) => {
            let mut fields = Vec::new();
            field_changes("", Some(x), Some(y), &mut fields);
            Change::Changed(fields)
        }
    };
    Some(EntryDiff {
        collection: collection.to_string(),
        id: id.to_string(),
        change,
    })
}

/// Longest value text shown in a field change.
const SHOW_MAX: usize = 120;

fn show(v: &toml::Value) -> String {
    let s = match v {
        toml::Value::String(s) => format!("{s:?}"),
        toml::Value::Table(t) => format!(
            "{{ {} }}",
            toml::to_string(t)
                .unwrap_or_default()
                .trim()
                .replace('\n', ", ")
        ),
        other => other.to_string(),
    };
    if s.chars().count() > SHOW_MAX {
        let cut: String = s.chars().take(SHOW_MAX - 3).collect();
        format!("{cut}...")
    } else {
        s
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// Every leaf that differs between `a` and `b`, as dotted paths. Tables
/// recurse; arrays of id'd tables match by id (`objectives[kill_boss]`),
/// arrays of equal length compare element by element, and any other array
/// is one value.
fn field_changes(
    path: &str,
    a: Option<&toml::Value>,
    b: Option<&toml::Value>,
    out: &mut Vec<FieldChange>,
) {
    if a == b {
        return;
    }
    match (a, b) {
        (Some(toml::Value::Table(ta)), Some(toml::Value::Table(tb))) => {
            let keys: BTreeSet<&String> = ta.keys().chain(tb.keys()).collect();
            for k in keys {
                field_changes(&join(path, k), ta.get(k), tb.get(k), out);
            }
        }
        (Some(x @ toml::Value::Array(xa)), Some(y @ toml::Value::Array(ya))) => {
            if let (Some(ka), Some(kb)) = (keyed(x), keyed(y)) {
                let ids: BTreeSet<&String> = ka.keys().chain(kb.keys()).collect();
                for id in ids {
                    let at = format!("{path}[{id}]");
                    field_changes(&at, ka.get(id).copied(), kb.get(id).copied(), out);
                }
            } else if xa.len() == ya.len() && xa.iter().any(toml::Value::is_table) {
                for (i, (p, q)) in xa.iter().zip(ya).enumerate() {
                    field_changes(&format!("{path}[{i}]"), Some(p), Some(q), out);
                }
            } else {
                out.push(FieldChange {
                    field: path.to_string(),
                    old: Some(show(x)),
                    new: Some(show(y)),
                });
            }
        }
        _ => out.push(FieldChange {
            field: path.to_string(),
            old: a.map(show),
            new: b.map(show),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PackManifest;

    fn pack(version: &str, toml_files: &[(&str, &str)]) -> ExpansionPack {
        let mut files = crate::PackFiles::new();
        files.insert(
            "pack.toml".into(),
            format!("id = \"t\"\nname = \"T\"\nversion = \"{version}\"\n").into_bytes(),
        );
        for (p, t) in toml_files {
            files.insert((*p).into(), t.as_bytes().to_vec());
        }
        ExpansionPack::from_files(&files).unwrap()
    }

    const CARDS_V1: &str = r#"
[[cards]]
id = "ember"
name = "Ember"
rarity = "common"
[[cards]]
id = "frost"
name = "Frost"
rarity = "rare"
"#;
    const CARDS_V2: &str = r#"
[[cards]]
id = "ember"
name = "Ember Wisp"
rarity = "uncommon"
[[cards]]
id = "gale"
name = "Gale"
rarity = "common"
"#;

    #[test]
    fn entries_are_matched_by_id() {
        let a = pack("1.0.0", &[("cards.toml", CARDS_V1)]);
        let b = pack("1.1.0", &[("cards.toml", CARDS_V2)]);
        let d = diff_packs(&a, &b);
        let cards = d.files.iter().find(|f| f.path == "cards.toml").unwrap();
        let by_id: BTreeMap<&str, &Change> = cards
            .entries
            .iter()
            .map(|e| (e.id.as_str(), &e.change))
            .collect();
        assert_eq!(by_id["gale"], &Change::Added);
        assert_eq!(by_id["frost"], &Change::Removed);
        let Change::Changed(f) = by_id["ember"] else {
            panic!()
        };
        let names: Vec<&str> = f.iter().map(|c| c.field.as_str()).collect();
        assert_eq!(names, ["name", "rarity"]);
        assert_eq!(f[0].old.as_deref(), Some("\"Ember\""));
        assert_eq!(f[0].new.as_deref(), Some("\"Ember Wisp\""));
        assert_eq!(d.counts(), (1, 1, 1));
        assert_eq!(d.bump(), Bump::Major);
        // The manifest's version change is listed but does not count.
        assert!(d.files.iter().any(|f| f.path == "pack.toml"));
    }

    #[test]
    fn the_first_entries_of_a_section_read_as_added() {
        let a = pack("0.1.0", &[]);
        let b = pack("0.1.0", &[("cards.toml", CARDS_V1)]);
        let d = diff_packs(&a, &b);
        assert_eq!(d.counts(), (2, 0, 0), "{d:#?}");
        assert_eq!(d.suggested_version().as_deref(), Some("0.2.0"));
        let log = d.changelog();
        assert!(log.starts_with("## 0.2.0\n"), "{log}");
        assert!(
            log.contains("### Added\n\n- card `ember` (cards.toml)\n"),
            "{log}"
        );
        assert!(!log.contains("### Removed"));
    }

    #[test]
    fn identical_packs_have_no_diff_and_bumps_follow_semver() {
        let a = pack("1.2.3", &[("cards.toml", CARDS_V1)]);
        assert!(diff_packs(&a, &a.clone()).is_empty());
        assert_eq!(diff_packs(&a, &a).suggested_version(), None);
        let mut b = a.clone();
        b.cards.cards[0].name = "Changed".into();
        let d = diff_packs(&a, &b);
        assert_eq!(d.bump(), Bump::Patch);
        assert_eq!(d.suggested_version().as_deref(), Some("1.2.4"));
        b.cards.cards.pop();
        assert_eq!(
            diff_packs(&a, &b).suggested_version().as_deref(),
            Some("2.0.0")
        );
        assert!(
            diff_packs(&a, &b)
                .changelog()
                .contains("### Removed\n\n- card `frost`")
        );
    }

    #[test]
    fn maps_assets_and_settings() {
        let mut a = ExpansionPack {
            manifest: PackManifest::new("t", "T", "1.0.0"),
            ..Default::default()
        };
        a.maps.insert("hall".into(), crate::map::MapDef::default());
        a.assets.insert("a.png".into(), vec![1, 2]);
        let mut b = a.clone();
        b.maps.get_mut("hall").unwrap().name = "Great Hall".into();
        b.maps.insert("crypt".into(), crate::map::MapDef::default());
        b.assets.insert("a.png".into(), vec![1, 2, 3]);
        b.progression.xp_mul = Some(2.0);
        let d = diff_packs(&a, &b);
        let paths: Vec<&str> = d.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "assets/",
                "maps/crypt.toml",
                "maps/hall.toml",
                "progression.toml"
            ]
        );
        let hall = &d.files[2].entries[0];
        assert_eq!(hall.collection, "maps");
        let Change::Changed(f) = &hall.change else {
            panic!()
        };
        assert_eq!(f[0].field, "name");
        let prog = &d.files[3].entries[0];
        assert_eq!(
            (prog.collection.as_str(), prog.id.as_str()),
            ("settings", "xp_mul")
        );
    }
}
