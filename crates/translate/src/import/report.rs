//! What an import reports back: the per-section counts and issue list, the per-
//! scene and per-carrier write trace, and the phase selector.

use super::super::code_strings::CodeLayout;
use super::super::name_pool::Layout;
use super::super::pack::LanguagePack;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Import outcome counters + per-entry diagnostics.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// Entries written to the image.
    pub applied: usize,
    /// Entries whose translation was already on the disc (no write).
    pub already_applied: usize,
    /// Entries with an empty translation (left vanilla).
    pub untranslated: usize,
    /// Per-entry problems: `(key, message)`. Errors never abort the whole
    /// import - the entry is skipped and the rest proceeds.
    pub issues: Vec<(String, String)>,
    /// Keys of the entries counted in [`Self::applied`].
    pub applied_keys: Vec<String>,
    /// Keys of the entries counted in [`Self::already_applied`].
    pub already_keys: Vec<String>,
    /// Scene MAN PROT entries grown by a whole-sector **disc relayout** (only
    /// when relayout is enabled). Each is a MAN whose full-length dialog would
    /// not fit its original compressed footprint and was given `+N` sectors.
    pub relayout_entries: usize,
    /// Total sectors added across all relayout-grown entries.
    pub relayout_sectors_added: u32,
    /// Names whose translation outgrew their in-place span and were moved into
    /// free bytes of the executable's name tables, their pointers repointed
    /// (see [`super::super::name_pool`]). Counted in [`Self::applied`] too.
    pub relocated_names: usize,
    /// Monster names that outgrew their record's name slot and were given
    /// room by growing the record (see [`super::super::monster_names`]). Counted in
    /// [`Self::applied`] too.
    pub grown_monster_names: usize,
    /// `ui_menu` / `system_text` strings that outgrew their span and were
    /// moved - into the bytes their pools' compaction frees, a translation
    /// region of the [space ledger](legaia_disc_patch::space_ledger), or (in the
    /// executable) the name pools' free runs - with every reference
    /// rewritten (see [`super::super::code_strings`]). Counted in [`Self::applied`]
    /// too.
    pub relocated_strings: usize,
    /// What the import measured and decided on the way, per key and per
    /// carrier - the numbers the space report ([`super::super::space`]) publishes,
    /// recorded where the importer computes them rather than re-derived.
    pub trace: ImportTrace,
    /// What the pack's accent mode did to its text before encoding
    /// ([`super::super::accents::prepared`]).
    pub accents: super::super::accents::AccentStats,
    /// The accent-font write, for a pack with `accents: font` (the names
    /// phase does it, once).
    pub accent_font: Option<super::super::accents::AccentFontReport>,
}

/// The class of an [`ImportReport::issues`] diagnostic, for tools that group
/// skips without parsing the message ([`ImportTrace::issue_kinds`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// A character outside the retail glyph set (or a byte the target
    /// cannot carry).
    NotEncodable,
    /// Longer than its in-place room, and no growth path took it.
    OverBudget,
    /// A longer SCUS name the pools had no free run for.
    NoFreeRun,
    /// A dialog line rolled back so its scene recompresses into its
    /// footprint.
    RolledBack,
    /// A growth path refused the edit: a monster record past the retail
    /// bounds, or a name past the longest retail name.
    Refused,
    /// The disc does not carry what the pack expects here (wrong disc, a
    /// conflicting patch, a moved scene, a budget hint that disagrees).
    Mismatch,
    /// Anything else (an unknown key, an unreadable entry, an operand run).
    Other,
}

/// How a carrier's accepted lines reached the image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritePath {
    /// Nothing was written.
    #[default]
    None,
    /// Same-size: every line space-padded to its own span.
    Padded,
    /// Through the relocator at exact lengths, inside the original footprint
    /// (a scene MAN's LZS footprint, or a streaming entry's sector slack).
    Relocated,
    /// Staged for the whole-sector disc relayout.
    Relayout,
}

/// One scene MAN's import, as the importer measured it
/// ([`ImportTrace::scenes`]).
#[derive(Debug, Clone, Default)]
pub struct SceneTrace {
    /// Bytes the recompressed MAN may occupy at its LBA.
    pub footprint: usize,
    /// Length of the MAN's compressed stream on this disc.
    pub disc_len: usize,
    /// Filled keyed lines the pack carries for this scene.
    pub keyed: usize,
    /// How the scene was written.
    pub path: WritePath,
    /// Length of the compressed stream written (for a relayout, the stream
    /// the grown entry carries).
    pub written_len: Option<usize>,
    /// Compressed bytes over the footprint with **every** ready line at its
    /// exact length (the relocator's first attempt), when that overflowed.
    pub full_overflow: Option<usize>,
    /// The padded same-size MAN did not recompress into the footprint.
    pub padded_overflow: Option<usize>,
    /// The relocator refused the edit set (it would not be the same program
    /// relocated), so only the same-size path was open.
    pub refused: bool,
    /// Keys rolled back to the source text to make the scene fit.
    pub rolled_back: Vec<String>,
    /// Whole sectors the staged relayout grows the entry by.
    pub relayout_sectors: Option<u32>,
    /// Whole sectors a relayout would add, when the full-length dialog
    /// overflowed and relayout was off.
    pub relayout_would_add: Option<u32>,
}

/// One raw carrier's import ([`ImportTrace::carriers`]).
#[derive(Debug, Clone, Default)]
pub struct CarrierTrace {
    /// The entry leads with an uncompressed streaming MAN (the ten dungeon
    /// scenes), so its lines can grow.
    pub streaming: bool,
    /// The entry's true footprint in bytes.
    pub footprint: usize,
    /// Streaming only: the footprint's zero padding a grown MAN may take
    /// without a relayout (`StreamManText::sector_slack`).
    pub sector_slack: Option<usize>,
    /// Filled keyed lines the pack carries for this entry.
    pub keyed: usize,
    /// How the entry was written.
    pub path: WritePath,
    /// Whole sectors the grown payload adds past the footprint (0 = it fit
    /// the slack).
    pub grown_sectors: Option<u32>,
}

/// Per-key and per-carrier measurements an import records
/// ([`ImportReport::trace`]). Every value is the one the importer used for
/// its decision, on the disc it was patching.
#[derive(Debug, Clone, Default)]
pub struct ImportTrace {
    /// Key -> the in-place room measured on the disc (the string's span and
    /// its zero padding, the segment's framing, the fixed field, the monster
    /// record's slot).
    pub rooms: BTreeMap<String, usize>,
    /// Key -> encoded length of the translation.
    pub encoded: BTreeMap<String, usize>,
    /// Key -> class of its diagnostic.
    pub issue_kinds: BTreeMap<String, IssueKind>,
    /// Key -> `(old VA, new VA)` of a name moved into the pools' free
    /// bytes, or a `ui_menu` / `system_text` string moved with its references.
    pub moved: BTreeMap<String, (u32, u32)>,
    /// Monster-name keys whose record grew.
    pub grown_monsters: BTreeSet<String>,
    /// The SCUS name pools' compaction after the import: the one relocation
    /// applied, or (no name grew) the layout of the pools as written.
    pub name_layout: Option<Layout>,
    /// Code-string image (a PROT overlay entry, `usize::MAX` for the
    /// executable) -> the layout of its `ui_menu` / `system_text` strings
    /// after the moves, when any string grew.
    pub code_layouts: BTreeMap<usize, CodeLayout>,
    /// Scene MAN PROT entry -> its import.
    pub scenes: BTreeMap<usize, SceneTrace>,
    /// Raw carrier PROT entry -> its import.
    pub carriers: BTreeMap<usize, CarrierTrace>,
}

impl ImportTrace {
    fn merge(&mut self, other: ImportTrace) {
        self.rooms.extend(other.rooms);
        self.encoded.extend(other.encoded);
        self.issue_kinds.extend(other.issue_kinds);
        self.moved.extend(other.moved);
        self.grown_monsters.extend(other.grown_monsters);
        if other.name_layout.is_some() {
            self.name_layout = other.name_layout;
        }
        self.code_layouts.extend(other.code_layouts);
        self.scenes.extend(other.scenes);
        self.carriers.extend(other.carriers);
    }
}

/// Whole sectors a relayout adds for a MAN `overflow` bytes past its
/// footprint (the grow [`build_grown_entry_payload`] stages).
pub(crate) fn sectors_for_overflow(overflow: usize) -> u32 {
    overflow.div_ceil(SECTOR) as u32
}

pub(super) const SECTOR: usize = 2048;

/// Per-section outcome row (see [`ImportReport::section_counts`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionCounts {
    /// Section name (the pack serialization name, e.g. `scene_dialog`).
    pub name: &'static str,
    /// Entries in the pack's section.
    pub total: usize,
    /// Entries a translator filled in (the ones import acts on).
    pub filled: usize,
    /// Filled entries written to the image.
    pub applied: usize,
    /// Filled entries whose translation was already on the disc.
    pub already_applied: usize,
    /// Filled entries skipped with a diagnostic (see the issues list).
    pub skipped: usize,
}

impl ImportReport {
    pub(super) fn issue(&mut self, key: &str, msg: impl Into<String>) {
        self.issue_as(key, IssueKind::Other, msg);
    }

    pub(super) fn issue_as(&mut self, key: &str, kind: IssueKind, msg: impl Into<String>) {
        self.issues.push((key.to_string(), msg.into()));
        self.trace.issue_kinds.insert(key.to_string(), kind);
    }

    pub(super) fn room(&mut self, key: &str, room: usize) {
        self.trace.rooms.insert(key.to_string(), room);
    }

    /// Absorb another report's counters + diagnostics (used to combine the
    /// two-phase dialog / name imports into one user-facing report).
    pub fn merge(&mut self, other: ImportReport) {
        self.applied += other.applied;
        self.already_applied += other.already_applied;
        self.untranslated += other.untranslated;
        self.issues.extend(other.issues);
        self.applied_keys.extend(other.applied_keys);
        self.already_keys.extend(other.already_keys);
        self.relayout_entries += other.relayout_entries;
        self.relayout_sectors_added += other.relayout_sectors_added;
        self.relocated_names += other.relocated_names;
        self.grown_monster_names += other.grown_monster_names;
        self.relocated_strings += other.relocated_strings;
        self.trace.merge(other.trace);
        if other.accent_font.is_some() {
            self.accent_font = other.accent_font;
        }
        // Both phases prepare the whole pack; keep one count.
        if self.accents == Default::default() {
            self.accents = other.accents;
        }
    }

    /// Fold this report against the pack it came from into per-section
    /// applied / already-applied / skipped counts. `skipped` counts filled
    /// entries that produced a diagnostic; a filled entry the report never
    /// saw (e.g. a phase import that excluded its section) counts in none of
    /// the outcome columns.
    pub fn section_counts(&self, pack: &LanguagePack) -> Vec<SectionCounts> {
        use std::collections::HashSet;
        let applied: HashSet<&str> = self.applied_keys.iter().map(String::as_str).collect();
        let already: HashSet<&str> = self.already_keys.iter().map(String::as_str).collect();
        let skipped: HashSet<&str> = self.issues.iter().map(|(k, _)| k.as_str()).collect();
        pack.sections
            .iter()
            .map(|(name, entries)| {
                let mut row = SectionCounts {
                    name,
                    total: entries.len(),
                    ..Default::default()
                };
                for e in entries {
                    if !e.is_filled() {
                        continue;
                    }
                    row.filled += 1;
                    if applied.contains(e.key.as_str()) {
                        row.applied += 1;
                    } else if already.contains(e.key.as_str()) {
                        row.already_applied += 1;
                    } else if skipped.contains(e.key.as_str()) {
                        row.skipped += 1;
                    }
                }
                row
            })
            .collect()
    }
}

/// Which key population an import pass touches. The site patcher splits a
/// pack in two: dialog first (its `man:` offsets predate any record
/// relocation by the door / starting-bag randomizers), SCUS name tables last
/// (so randomizer passes that classify items by their **English** names -
/// the equipment-drop gear pool - still see the retail names). See
/// `docs/tooling/translation/dialog-import.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportPhase {
    /// Every entry (the CLI single-shot import).
    All,
    /// Only `man:` / `raw:` dialog-segment entries.
    DialogOnly,
    /// Only `scus:` string / party-name / `ui:` overlay entries. Overlay UI
    /// strings ride with the names phase: nothing in the randomizer relocates
    /// or classifies an overlay string, so writing them last is always safe.
    NamesOnly,
}
