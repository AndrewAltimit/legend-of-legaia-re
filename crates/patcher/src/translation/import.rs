//! Import: apply a filled language pack to a disc image.
//!
//! Only entries whose `translation` is non-empty are touched; everything
//! else stays byte-identical. Every write is same-size in place:
//!
//! - `scus:str:*` / `scus:party:*` - encoded bytes + NUL terminator written
//!   over the original string (budget = the original's span);
//! - `man:*` - the segment inside the decompressed scene MAN is overwritten
//!   and space-padded (`0x20`) to its exact original length (the pager walks
//!   segments byte-by-byte, so the framing must not move), then the whole
//!   MAN is recompressed and must fit its original compressed footprint.
//!   A line longer than its span, or a padded scene that no longer
//!   recompresses, goes through the relocator instead (every line at its own
//!   length, crossing references moved - [`man_edit::apply_text_edits`]),
//!   still inside the same footprint;
//! - `raw:*` - same space-padded overwrite, directly in the PROT entry; the
//!   ten streaming dungeon scenes (an uncompressed MAN leading a typed-chunk
//!   stream - [`super::stream_man`]) also get the generalized rewriter: a
//!   longer line grows the MAN chunk, shifts the chunks after it, and either
//!   fits the entry's own sector slack or (with relayout) grows the entry.
//!
//! Before writing, each target is verified:
//!
//! - a **working pack** (one that carries `source:`) is checked against it -
//!   if the disc bytes already equal the translation the entry counts as
//!   already applied (idempotent re-import); if they match neither, the entry
//!   is skipped with a warning (wrong disc / conflicting patch);
//! - a **distributable pack** (translation-only - see [`super::pack`]) has no
//!   source to check against, so the target is measured *on the disc being
//!   patched*: the string's own span / the segment's own `0x1F .. 0x00`
//!   framing is the byte budget, and the pack's `budget` hint must agree with
//!   it. A disagreement means the pack wasn't built for this image and the
//!   entry is skipped rather than written blind.
//!
//! Encode failures (non-Latin characters, over-budget text) are reported per
//! entry with per-character positions and leave the disc untouched.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use legaia_asset::man_edit::{self, TextEdit, TextSite};
use legaia_asset::{item_names, new_game, scene_asset_table, worldmap_menu};

use crate::disc::DiscPatcher;

use super::export::SceneManText;
use super::markup::{self, Target};
use super::monster_names;
use super::name_pool::{Layout, NamePool};
use super::pack::{Entry, LanguagePack};
use super::segments;
use super::stream_man::StreamManText;
use super::ui;

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
    /// (see [`super::name_pool`]). Counted in [`Self::applied`] too.
    pub relocated_names: usize,
    /// Monster names that outgrew their record's name slot and were given
    /// room by growing the record (see [`super::monster_names`]). Counted in
    /// [`Self::applied`] too.
    pub grown_monster_names: usize,
    /// What the import measured and decided on the way, per key and per
    /// carrier - the numbers the space report ([`super::space`]) publishes,
    /// recorded where the importer computes them rather than re-derived.
    pub trace: ImportTrace,
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
    /// Key -> `(old VA, new VA)` of a name moved into the pools' free bytes.
    pub moved: BTreeMap<String, (u32, u32)>,
    /// Monster-name keys whose record grew.
    pub grown_monsters: BTreeSet<String>,
    /// The SCUS name pools' compaction after the import: the one relocation
    /// applied, or (no name grew) the layout of the pools as written.
    pub name_layout: Option<Layout>,
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
        self.scenes.extend(other.scenes);
        self.carriers.extend(other.carriers);
    }
}

/// Whole sectors a relayout adds for a MAN `overflow` bytes past its
/// footprint (the grow [`build_grown_entry_payload`] stages).
pub(crate) fn sectors_for_overflow(overflow: usize) -> u32 {
    overflow.div_ceil(SECTOR) as u32
}

const SECTOR: usize = 2048;

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
    fn issue(&mut self, key: &str, msg: impl Into<String>) {
        self.issue_as(key, IssueKind::Other, msg);
    }

    fn issue_as(&mut self, key: &str, kind: IssueKind, msg: impl Into<String>) {
        self.issues.push((key.to_string(), msg.into()));
        self.trace.issue_kinds.insert(key.to_string(), kind);
    }

    fn room(&mut self, key: &str, room: usize) {
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
        self.trace.merge(other.trace);
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

/// Parsed provenance key.
pub(crate) enum Key {
    ScusStr {
        va: u32,
    },
    ScusParty {
        slot: usize,
    },
    /// A fixed `0x20`-byte NUL-padded SCUS cell (world-map place names).
    ScusCell {
        va: u32,
    },
    Man {
        entry: usize,
        off: usize,
    },
    Raw {
        entry: usize,
        off: usize,
    },
    Ui {
        prot: usize,
        va: u32,
    },
    /// A monster name inside its record in the monster archive.
    Mon {
        id: u16,
    },
}

pub(crate) fn parse_key(key: &str) -> Option<Key> {
    let mut it = key.split(':');
    match it.next()? {
        "scus" => match it.next()? {
            "str" => {
                let va = it.next()?.strip_prefix("0x")?;
                Some(Key::ScusStr {
                    va: u32::from_str_radix(va, 16).ok()?,
                })
            }
            "party" => Some(Key::ScusParty {
                slot: it.next()?.parse().ok()?,
            }),
            "cell" => {
                let va = it.next()?.strip_prefix("0x")?;
                Some(Key::ScusCell {
                    va: u32::from_str_radix(va, 16).ok()?,
                })
            }
            _ => None,
        },
        kind @ ("man" | "raw") => {
            let entry = it.next()?.parse().ok()?;
            let off = usize::from_str_radix(it.next()?.strip_prefix("0x")?, 16).ok()?;
            Some(if kind == "man" {
                Key::Man { entry, off }
            } else {
                Key::Raw { entry, off }
            })
        }
        "ui" => {
            let prot = it.next()?.parse().ok()?;
            let va = u32::from_str_radix(it.next()?.strip_prefix("0x")?, 16).ok()?;
            Some(Key::Ui { prot, va })
        }
        "mon" => Some(Key::Mon {
            id: monster_names::key_id(key)?,
        }),
        _ => None,
    }
}

/// Encode the pack's `source` for `target`. `Ok(None)` = the pack is a
/// distributable (translation-only) one and carries no source; `Err(())` = a
/// diagnostic was recorded.
#[allow(clippy::result_unit_err)]
fn encode_source(
    entry: &Entry,
    target: Target,
    report: &mut ImportReport,
) -> Result<Option<Vec<u8>>, ()> {
    if entry.source.is_empty() {
        return Ok(None);
    }
    match markup::encode(&entry.source, target) {
        Ok(b) => Ok(Some(b)),
        Err(issues) => {
            report.issue(
                &entry.key,
                format!(
                    "pack source doesn't encode (corrupted pack?): {}",
                    issues[0]
                ),
            );
            Err(())
        }
    }
}

/// Encode `entry.translation` for `target`. `None` = a diagnostic was recorded.
fn encode_translation(entry: &Entry, target: Target, report: &mut ImportReport) -> Option<Vec<u8>> {
    match markup::encode(&entry.translation, target) {
        Ok(b) => {
            report.trace.encoded.insert(entry.key.clone(), b.len());
            Some(b)
        }
        Err(issues) => {
            let detail: Vec<String> = issues.iter().map(ToString::to_string).collect();
            report.issue_as(
                &entry.key,
                IssueKind::NotEncodable,
                format!("translation not encodable: {}", detail.join("; ")),
            );
            None
        }
    }
}

/// Budget check against the byte span actually measured on the disc.
fn fits(entry: &Entry, translated: &[u8], budget: usize, report: &mut ImportReport) -> bool {
    if translated.len() > budget {
        report.issue_as(
            &entry.key,
            IssueKind::OverBudget,
            format!(
                "translation needs {} bytes but the in-place budget is {budget} \
                 (shorten the text)",
                translated.len()
            ),
        );
        return false;
    }
    true
}

/// Wrong-disc guard for a source-less entry: the target's real length on the
/// disc must equal the pack's `budget` hint, which was measured off the disc
/// the pack was authored against. (A working pack proves the same thing, more
/// strongly, by comparing the bytes.)
fn hint_agrees(entry: &Entry, disc_len: usize, report: &mut ImportReport) -> bool {
    if entry.budget != disc_len {
        report.issue_as(
            &entry.key,
            IssueKind::Mismatch,
            format!(
                "this disc's text is {disc_len} bytes but the pack expects {} - the pack \
                 was not built for this image (or another patch already moved the text) \
                 - skipped",
                entry.budget
            ),
        );
        return false;
    }
    true
}

/// `translated`, space-padded to exactly `len` bytes (dialog-segment form).
fn pad_segment(translated: &[u8], len: usize) -> Vec<u8> {
    let mut v = translated.to_vec();
    v.resize(len, 0x20);
    v
}

/// Longest SCUS string the reader will follow before calling the pointer bogus.
const MAX_SCUS_STRLEN: usize = 512;

/// Text bytes writable at `off` on **this disc**: the string's own length plus
/// the zero alignment padding that follows its terminator (the name pools are
/// 4-byte aligned; see `export::ScusCollector::padding_slack`). Measured, not
/// asserted - the run must actually be zeros - so a pack can never talk the
/// importer into writing over a neighbouring string.
fn scus_writable_span(scus: &[u8], off: usize, cur_len: usize) -> usize {
    let end = off + cur_len; // the NUL
    let aligned = (end + 4) & !3;
    let mut u = end + 1;
    while u < aligned && scus.get(u) == Some(&0) {
        u += 1;
    }
    (u - 1) - off
}

/// What [`plan_scus_str`] decided for one name.
enum ScusStrPlan {
    /// Same-size in place: `(file_offset, bytes)`.
    Write(usize, Vec<u8>),
    /// Longer than its span but movable: relocate these bytes (no terminator).
    Grow(Vec<u8>),
}

/// Plan one SCUS-string write, or `None` when the entry was resolved without a
/// write (diagnostic / already applied). A translation over its in-place
/// budget comes back as [`ScusStrPlan::Grow`] when `pool` can move the string
/// (see [`super::name_pool`]); otherwise it is diagnosed here.
fn plan_scus_str(
    scus: &[u8],
    entry: &Entry,
    va: u32,
    pool: &NamePool,
    report: &mut ImportReport,
) -> Option<ScusStrPlan> {
    let source = encode_source(entry, Target::CString, report).ok()?;
    let translated = encode_translation(entry, Target::CString, report)?;
    let Some(off) = item_names::file_offset_for_va(scus, va) else {
        report.issue(&entry.key, "VA not in the SCUS data segment");
        return None;
    };
    // The string as it stands on this disc, up to (not including) its NUL.
    // A strict system-text pool reads token-aware: `{c1:00}` carries a `0x00`
    // argument that is not the terminator.
    let Some(tail) = scus.get(off..) else {
        report.issue(&entry.key, "string span past end of SCUS");
        return None;
    };
    let strict = ui::pool_for(usize::MAX, va).is_some_and(|p| p.strict);
    let Some(cur_len) = ui::pool_strlen(scus, off, strict).filter(|&l| l <= MAX_SCUS_STRLEN) else {
        report.issue(&entry.key, "no NUL-terminated string at this VA - skipped");
        return None;
    };
    let cur = &tail[..cur_len];
    if cur == translated.as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return None;
    }
    // The write may never leave the string's own dead span on THIS disc: its
    // bytes plus the zero padding after its terminator. The pack's budget is
    // clamped to that, so a bad/tampered budget can't reach a neighbour.
    let writable = scus_writable_span(scus, off, cur_len);
    report.room(&entry.key, entry.budget.min(writable));
    if let Some(src) = &source
        && cur != src.as_slice()
    {
        report.issue_as(
            &entry.key,
            IssueKind::Mismatch,
            "disc bytes don't match the pack source (different disc revision or \
             a conflicting patch) - skipped",
        );
        return None;
    }
    // Source-less pack: the measured span is the only wrong-disc guard there is.
    if source.is_none() && !hint_agrees(entry, writable, report) {
        return None;
    }
    if translated.len() > entry.budget.min(writable) && pool.is_movable(va) {
        return Some(ScusStrPlan::Grow(translated));
    }
    if !fits(entry, &translated, entry.budget.min(writable), report) {
        return None;
    }
    // Re-terminate and zero the rest of the old string's span: nothing reads
    // past a terminator, but a stale printable tail is what a pool scanner
    // (and any reader that skips zero padding to the next string - the combo
    // string that follows an arts description) would pick up.
    let mut bytes = translated;
    bytes.push(0);
    if bytes.len() < cur_len + 1 {
        bytes.resize(cur_len + 1, 0);
    }
    Some(ScusStrPlan::Write(off, bytes))
}

/// Plan one party-name write (fixed 10-byte NUL-padded field).
fn plan_scus_party(
    scus: &[u8],
    entry: &Entry,
    slot: usize,
    report: &mut ImportReport,
) -> Option<(usize, Vec<u8>)> {
    let source = encode_source(entry, Target::CString, report).ok()?;
    let translated = encode_translation(entry, Target::CString, report)?;
    report.room(&entry.key, entry.budget.min(new_game::NAME_LEN - 1));
    if !fits(entry, &translated, entry.budget, report) {
        return None;
    }
    if slot >= new_game::PARTY_RECORDS {
        report.issue(&entry.key, "party slot out of range");
        return None;
    }
    if translated.len() > new_game::NAME_LEN - 1 {
        report.issue_as(
            &entry.key,
            IssueKind::OverBudget,
            "party name must fit 9 bytes",
        );
        return None;
    }
    let va = new_game::PARTY_TEMPLATE_VA + (slot * new_game::RECORD_STRIDE) as u32 + 16;
    let Some(off) = item_names::file_offset_for_va(scus, va) else {
        report.issue(&entry.key, "party record outside the SCUS data segment");
        return None;
    };
    let Some(field) = scus.get(off..off + new_game::NAME_LEN) else {
        report.issue(&entry.key, "party record past end of SCUS");
        return None;
    };
    let cur_len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    if &field[..cur_len] == translated.as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return None;
    }
    // Fixed-width field: the write is bounded by the field itself, so a
    // source-less (distributable) pack needs no extra guard here.
    if let Some(src) = &source
        && &field[..cur_len] != src.as_slice()
    {
        report.issue_as(
            &entry.key,
            IssueKind::Mismatch,
            "disc bytes don't match the pack source - skipped",
        );
        return None;
    }
    let mut bytes = translated;
    bytes.resize(new_game::NAME_LEN, 0);
    Some((off, bytes))
}

/// Plan one fixed-cell write: a `0x20`-byte NUL-padded `SCUS_942.54` field
/// (the world-map quick-travel place names, `legaia_asset::worldmap_menu`).
/// The write is bounded by the cell itself, like a party-name field.
fn plan_scus_cell(
    scus: &[u8],
    entry: &Entry,
    va: u32,
    report: &mut ImportReport,
) -> Option<(usize, Vec<u8>)> {
    const CELL: usize = worldmap_menu::NAME_STRIDE;
    let source = encode_source(entry, Target::CString, report).ok()?;
    let translated = encode_translation(entry, Target::CString, report)?;
    report.room(&entry.key, entry.budget.min(CELL - 1));
    if !fits(entry, &translated, entry.budget.min(CELL - 1), report) {
        return None;
    }
    let table_end = worldmap_menu::NAME_TABLE_ADDR + (worldmap_menu::NAME_COUNT * CELL) as u32;
    if va < worldmap_menu::NAME_TABLE_ADDR
        || va >= table_end
        || !((va - worldmap_menu::NAME_TABLE_ADDR) as usize).is_multiple_of(CELL)
    {
        report.issue(&entry.key, "not a place-name cell address - skipped");
        return None;
    }
    let Some(off) = item_names::file_offset_for_va(scus, va) else {
        report.issue(&entry.key, "cell outside the SCUS data segment");
        return None;
    };
    let Some(field) = scus.get(off..off + CELL) else {
        report.issue(&entry.key, "cell past end of SCUS");
        return None;
    };
    let cur_len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    if &field[..cur_len] == translated.as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return None;
    }
    if let Some(src) = &source
        && &field[..cur_len] != src.as_slice()
    {
        report.issue_as(
            &entry.key,
            IssueKind::Mismatch,
            "disc bytes don't match the pack source - skipped",
        );
        return None;
    }
    let mut bytes = translated;
    bytes.resize(CELL, 0);
    Some((off, bytes))
}

/// Plan one overlay UI-string write into a PROT overlay entry buffer:
/// `(file_offset, bytes)`, or `None` when resolved without a write. Same
/// mechanism as [`plan_scus_str`] (NUL-terminated, span + zero-padding budget,
/// wrong-disc guard) but keyed by VA into the overlay at `va - base_va`. The
/// returned bytes fully cover the old string's span so a shorter translation
/// leaves no stale tail behind for the pool scanner to pick up.
fn plan_ui(
    entry: &[u8],
    base_va: u32,
    e: &Entry,
    va: u32,
    pool_strict: bool,
    report: &mut ImportReport,
) -> Option<(usize, Vec<u8>)> {
    let source = encode_source(e, Target::CString, report).ok()?;
    let translated = encode_translation(e, Target::CString, report)?;
    let Some(off) = va.checked_sub(base_va).map(|d| d as usize) else {
        report.issue(&e.key, "VA is before the overlay load base");
        return None;
    };
    let Some(tail) = entry.get(off..) else {
        report.issue(&e.key, "VA past end of the overlay entry");
        return None;
    };
    let strict = pool_strict;
    let Some(cur_len) = ui::pool_strlen(entry, off, strict).filter(|&l| l <= MAX_SCUS_STRLEN)
    else {
        report.issue(&e.key, "no NUL-terminated string at this VA - skipped");
        return None;
    };
    let cur = &tail[..cur_len];
    if cur == translated.as_slice() {
        report.already_applied += 1;
        report.already_keys.push(e.key.clone());
        return None;
    }
    let writable = ui::writable_span(entry, off, cur_len);
    report.room(&e.key, e.budget.min(writable));
    if let Some(src) = &source
        && cur != src.as_slice()
    {
        report.issue_as(
            &e.key,
            IssueKind::Mismatch,
            "disc bytes don't match the pack source (different disc revision or \
             a conflicting patch) - skipped",
        );
        return None;
    }
    if source.is_none() && !hint_agrees(e, writable, report) {
        return None;
    }
    if !fits(e, &translated, e.budget.min(writable), report) {
        return None;
    }
    // Cover the whole old string span (its bytes + terminator) so a shorter
    // translation zero-fills the leftover instead of leaving a printable tail.
    let mut bytes = translated;
    bytes.push(0);
    if bytes.len() < cur_len + 1 {
        bytes.resize(cur_len + 1, 0);
    }
    Some((off, bytes))
}

/// A dialog segment inside a scene MAN, validated against the disc and ready
/// to write: its decompressed-domain offset, current on-disc byte length (the
/// `0x1F .. 0x00` framing span), and the encoded translated bytes.
struct ReadyMan<'a> {
    off: usize,
    old_len: usize,
    translated: Vec<u8>,
    entry: &'a Entry,
}

/// Outcome of validating one dialog segment against the disc before a write.
enum SegPrep {
    /// Ready to write; `old_len` is its current framing span.
    Ready { old_len: usize },
    /// The translation is already on the disc (counted; no write needed).
    Already,
    /// A diagnostic was recorded; skip.
    Skip,
}

/// Validate a dialog segment (in a decompressed MAN or a raw PROT entry)
/// against the bytes actually on the disc - framing, already-applied,
/// wrong-disc guard - *without* mutating, returning the current framing span
/// so the caller can choose the same-size or grow path. The segment's byte
/// budget is its own `0x1F <text> 0x00` framing on this disc, never a number
/// the pack asserts, so a bad pack can't overrun the text pool it edits.
fn prepare_segment(
    buf: &[u8],
    entry: &Entry,
    off: usize,
    source: Option<&[u8]>,
    translated: &[u8],
    report: &mut ImportReport,
) -> SegPrep {
    let framed = off > 0
        && buf.get(off - 1) == Some(&0x1F)
        && segments::walk_to_terminator(buf, off).is_some_and(|t| buf[t] == 0x00);
    if !framed {
        report.issue(
            &entry.key,
            "segment framing not found at the keyed offset - skipped",
        );
        return SegPrep::Skip;
    }
    let term = segments::walk_to_terminator(buf, off).expect("framing checked");
    let old_len = term - off;
    report.room(&entry.key, old_len);
    let cur = &buf[off..term];
    // Already applied: exact (a grown/shrunk write) or space-padded (same-size).
    if cur == translated || cur == pad_segment(translated, old_len).as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return SegPrep::Already;
    }
    match source {
        Some(src) if cur != src => {
            report.issue_as(
                &entry.key,
                IssueKind::Mismatch,
                "disc bytes don't match the pack source (different disc revision or \
                 a conflicting patch) - skipped",
            );
            SegPrep::Skip
        }
        None if !hint_agrees(entry, old_len, report) => SegPrep::Skip,
        _ => SegPrep::Ready { old_len },
    }
}

/// Validate every keyed line of one carrier against the disc
/// ([`prepare_segment`]) and return the ready set.
///
/// A **translation-only** pack has no source text to prove a line is the one
/// it was written for - only the segment's framing and its length hint - so
/// once any keyed line of a carrier fails that check, the carrier's text is
/// not where the pack expects it (an earlier import relocated the scene, or
/// another patch moved it), and a line that still happens to pass may be a
/// *neighbour* of the right length. The whole carrier is then skipped: that
/// is what makes re-importing onto an already translated image a no-op
/// rather than a write into shifted text. A working pack keeps the per-line
/// source comparison, which cannot land on a neighbour.
fn collect_ready<'a>(
    buf: &[u8],
    entry_idx: usize,
    edits: &[(usize, &'a Entry)],
    report: &mut ImportReport,
) -> Vec<ReadyMan<'a>> {
    let mut ready = Vec::new();
    let mut mismatched = 0usize;
    for (off, en) in edits {
        let Ok(source) = encode_source(en, Target::Segment, report) else {
            continue;
        };
        let Some(translated) = encode_translation(en, Target::Segment, report) else {
            continue;
        };
        match prepare_segment(buf, en, *off, source.as_deref(), &translated, report) {
            SegPrep::Ready { old_len } => ready.push(ReadyMan {
                off: *off,
                old_len,
                translated,
                entry: en,
            }),
            SegPrep::Skip if source.is_none() => mismatched += 1,
            _ => {}
        }
    }
    if mismatched > 0 && !ready.is_empty() {
        for r in &ready {
            report.issue_as(
                &r.entry.key,
                IssueKind::Mismatch,
                format!(
                    "PROT entry {entry_idx}: {mismatched} other line(s) of this scene no longer \
                     sit where the pack expects them (an earlier import or another patch moved \
                     its text) - the whole scene is skipped so no line lands on a neighbour"
                ),
            );
        }
        return Vec::new();
    }
    ready
}

/// Diagnostic for a keyed line whose `0x1F` framing is a coincidental byte
/// run inside a decoded instruction's operands.
const OPERAND_RUN_MSG: &str = "the text framing at this offset is a coincidence inside a \
     decoded instruction's operands (an actor index followed by printable bytes), not a \
     dialog segment - skipped (a write here would corrupt the script)";

/// Same-size lines pre-applied into a decompressed MAN: `(offset, previous
/// bytes, entry)`, the rollback shape the same-size path uses.
type PreApplied<'a> = Vec<(usize, Vec<u8>, &'a Entry)>;

/// Partition the ready lines of one decompressed MAN by [`TextSite`]: lines on
/// a clean-walk text segment come back as the relocatable `ready` set; a line
/// whose framing is an instruction's operand bytes is refused with a
/// diagnostic; a line the walk does not reach is same-size only - applied into
/// `decoded` here (space-padded) when it fits, and reported when it does not.
/// The pre-applied `(offset, previous bytes, entry)` triples are returned so
/// the caller counts them once a write happens and can roll them back.
fn gate_text_sites<'a>(
    decoded: &mut [u8],
    ready: Vec<ReadyMan<'a>>,
    report: &mut ImportReport,
) -> (Vec<ReadyMan<'a>>, PreApplied<'a>) {
    let mut walked = Vec::with_capacity(ready.len());
    let mut applied = Vec::new();
    for r in ready {
        match man_edit::text_site(decoded, r.off) {
            TextSite::Segment => walked.push(r),
            TextSite::Operand => report.issue(&r.entry.key, OPERAND_RUN_MSG),
            TextSite::Unreached | TextSite::NoRecord => {
                if r.translated.len() > r.old_len {
                    report.issue_as(
                        &r.entry.key,
                        IssueKind::OverBudget,
                        format!(
                            "translation needs {} bytes but the in-place budget is {} and \
                             the script walk does not reach this line, so it cannot be \
                             relocated (shorten this line)",
                            r.translated.len(),
                            r.old_len
                        ),
                    );
                    continue;
                }
                let before = decoded[r.off..r.off + r.old_len].to_vec();
                decoded[r.off..r.off + r.old_len]
                    .copy_from_slice(&pad_segment(&r.translated, r.old_len));
                applied.push((r.off, before, r.entry));
            }
        }
    }
    (walked, applied)
}

/// Why [`relocate_and_pack`] could not produce a stream.
enum GrowFail {
    /// The relocator refused the edit set, or the rewrite is not the same
    /// program relocated - nothing about *which* lines are kept changes that.
    NotPreserved,
    /// The relocated MAN recompresses `n` bytes past the footprint.
    Overflow(usize),
}

/// Attempt the **generalized rewriter** path for one scene MAN: grow/shrink
/// every given segment to its exact translated bytes, relocate all crossing
/// references ([`man_edit::apply_text_edits`]), verify the rewrite is the same
/// program relocated ([`man_edit::text_edits_preserve_scripts`]), and recompress
/// within the MAN's on-disc footprint (fast greedy parse, then the optimal
/// parse when it just misses - same policy as `repack`). Returns
/// `(recompressed_stream, new_decompressed_size)`.
fn relocate_and_pack(
    man: &SceneManText,
    ready: &[&ReadyMan],
) -> std::result::Result<(Vec<u8>, u32), GrowFail> {
    let edits: Vec<TextEdit> = ready
        .iter()
        .map(|r| TextEdit {
            offset: r.off,
            old_len: r.old_len,
            new_bytes: r.translated.clone(),
        })
        .collect();
    let grown =
        man_edit::apply_text_edits(&man.decoded, &edits).map_err(|_| GrowFail::NotPreserved)?;
    if !man_edit::text_edits_preserve_scripts(&man.decoded, &grown) {
        return Err(GrowFail::NotPreserved);
    }
    let stream = legaia_lzs::compress(&grown);
    if stream.len() <= man.compressed_budget {
        return Ok((stream, grown.len() as u32));
    }
    let opt = legaia_lzs::compress_optimal(&grown);
    if opt.len() <= man.compressed_budget {
        return Ok((opt, grown.len() as u32));
    }
    Err(GrowFail::Overflow(opt.len() - man.compressed_budget))
}

/// [`relocate_and_pack`] over every ready line; the error says why the
/// growth can't be done safely / won't fit (caller falls back to relayout or
/// same-size).
fn try_grow_man(man: &SceneManText, ready: &[ReadyMan]) -> Result<(Vec<u8>, u32), GrowFail> {
    let all: Vec<&ReadyMan> = ready.iter().collect();
    relocate_and_pack(man, &all)
}

/// A same-size scene write the relocator fitted into the MAN's footprint:
/// the stream, its decompressed size, and which `ready` lines it carries
/// (`kept`) versus left English (`dropped`).
struct FittedMan {
    stream: Vec<u8>,
    size: u32,
    kept: Vec<usize>,
    dropped: Vec<usize>,
}

/// What [`fit_man_in_footprint`] found.
struct FitOutcome {
    /// The fitted write, or `None` when the relocator refused the scene or
    /// no subset fits.
    fitted: Option<FittedMan>,
    /// Compressed bytes over the footprint of the first attempt (every line
    /// at its exact length), when it overflowed.
    first_overflow: Option<usize>,
    /// The relocator refused the edit set.
    refused: bool,
}

/// Fit as many `ready` lines as possible into the MAN's **own** compressed
/// footprint through the relocator (every line at its exact length - a
/// shorter translation shrinks the MAN, a longer one grows it), rolling
/// lines back to the source text only when the scene still overflows.
///
/// This is the same-size image's best path whenever the space-padded write
/// does not recompress: padding every shorter line back to the English
/// length spends bytes the footprint does not have, while the exact-length
/// rewrite gives them back. The rollback drops the lines that grow the MAN
/// most first, a batch at a time sized by the measured overflow, so a scene
/// that is a few bytes over loses a line or two rather than every line
/// longer than the one that tipped it. No fit when the relocator refuses the
/// scene outright or no subset fits (the caller keeps the padded path).
fn fit_man_in_footprint(man: &SceneManText, ready: &[ReadyMan]) -> FitOutcome {
    use std::cmp::Reverse;
    let growth = |r: &ReadyMan| r.translated.len() as isize - r.old_len as isize;
    // Drop order: biggest growth first, then the longest translation.
    let mut order: Vec<usize> = (0..ready.len()).collect();
    order.sort_by_key(|&i| {
        (
            Reverse(growth(&ready[i])),
            Reverse(ready[i].translated.len()),
            i,
        )
    });
    let mut out = FitOutcome {
        fitted: None,
        first_overflow: None,
        refused: false,
    };
    let mut keep = vec![true; ready.len()];
    let mut cursor = 0usize;
    loop {
        let kept: Vec<usize> = (0..ready.len()).filter(|&i| keep[i]).collect();
        if kept.is_empty() {
            return out;
        }
        let refs: Vec<&ReadyMan> = kept.iter().map(|&i| &ready[i]).collect();
        match relocate_and_pack(man, &refs) {
            Ok((stream, size)) => {
                let dropped = (0..ready.len()).filter(|&i| !keep[i]).collect();
                out.fitted = Some(FittedMan {
                    stream,
                    size,
                    kept,
                    dropped,
                });
                return out;
            }
            Err(GrowFail::NotPreserved) => {
                out.refused = true;
                return out;
            }
            Err(GrowFail::Overflow(over)) => {
                out.first_overflow.get_or_insert(over);
                // Raw bytes restored are an upper bound on the compressed
                // bytes saved, so a batch whose growth covers the overflow is
                // the fewest lines that can possibly fit - never more.
                let mut freed = 0usize;
                while freed < over && cursor < order.len() {
                    let i = order[cursor];
                    cursor += 1;
                    keep[i] = false;
                    freed += growth(&ready[i]).max(1) as usize;
                }
            }
        }
    }
}

/// Stage a [`fit_man_in_footprint`] result: the stream, the MAN descriptor's
/// new size word, the carried lines (plus the same-size `pre_applied` ones
/// already in the decoded MAN) as applied, and a rollback diagnostic for
/// every line left in the source language.
#[allow(clippy::too_many_arguments)]
fn stage_fitted_man(
    entry_idx: usize,
    man: &SceneManText,
    ready: &[ReadyMan],
    pre_applied: &PreApplied,
    fit: FittedMan,
    report: &mut ImportReport,
    plan: &mut ScenePlan,
    trace: &mut SceneTrace,
) {
    trace.path = WritePath::Relocated;
    trace.written_len = Some(fit.stream.len());
    plan.writes.push((man.man_offset as u64, fit.stream));
    plan.writes.push(size_word_write(man, fit.size));
    plan.applied
        .extend(fit.kept.iter().map(|&i| ready[i].entry.key.clone()));
    plan.applied
        .extend(pre_applied.iter().map(|(_, _, en)| en.key.clone()));
    for &i in &fit.dropped {
        let key = &ready[i].entry.key;
        report.issue_as(
            key,
            IssueKind::RolledBack,
            format!(
                "scene {entry_idx}: rolled back - the scene's dialog no longer \
                 recompresses into its {} byte footprint (shorten this line)",
                man.compressed_budget
            ),
        );
        trace.rolled_back.push(key.clone());
    }
}

/// The MAN descriptor's `(type << 24) | size` word rewritten for a MAN of
/// `size` decompressed bytes, as an entry write.
fn size_word_write(man: &SceneManText, size: u32) -> (u64, Vec<u8>) {
    (
        man.man_descriptor_off as u64,
        scene_asset_table::encode_size_word(0x03, size)
            .to_le_bytes()
            .to_vec(),
    )
}

/// Build entry `entry_idx`'s new **full-footprint payload**, growing the scene
/// MAN by whole sectors so its full-length dialog fits. Used only when the
/// in-place grow ([`try_grow_man`]) overflowed the MAN's compressed footprint and
/// a disc relayout is permitted.
///
/// Steps (see `docs/tooling/pal-localizations.md`): relocate all crossing
/// references + verify the program is preserved, recompress with no budget cap,
/// then within the entry insert `ceil(overflow / 2048)` sectors after the MAN's
/// compressed region, shift every later sub-asset, and bump their
/// `scene_asset_table` descriptor `data_offset`s (+ the MAN decompressed-size
/// word). Returns `(new_footprint_payload, grown_sectors, stream_len)`, or
/// `None` when the grow can't be done safely.
fn build_grown_entry_payload(
    patcher: &DiscPatcher,
    entry_idx: usize,
    man: &SceneManText,
    ready: &[ReadyMan],
) -> Option<(Vec<u8>, u32, usize)> {
    let edits: Vec<TextEdit> = ready
        .iter()
        .map(|r| TextEdit {
            offset: r.off,
            old_len: r.old_len,
            new_bytes: r.translated.clone(),
        })
        .collect();
    let grown = man_edit::apply_text_edits(&man.decoded, &edits).ok()?;
    if !man_edit::text_edits_preserve_scripts(&man.decoded, &grown) {
        return None;
    }
    let greedy = legaia_lzs::compress(&grown);
    let optimal = legaia_lzs::compress_optimal(&grown);
    let stream = if optimal.len() < greedy.len() {
        optimal
    } else {
        greedy
    };

    let extra = stream.len().checked_sub(man.compressed_budget)?;
    if extra == 0 {
        return None; // fits in place; caller should have used try_grow_man
    }
    let grown_sectors = sectors_for_overflow(extra) as usize;
    let insert = grown_sectors * SECTOR;

    // The entry's TRUE footprint bytes (the real per-scene allocation), not the
    // indexed-size over-read `read_entry` returns.
    let foot = patcher.read_entry_footprint(entry_idx).ok()?;
    let table = scene_asset_table::detect(&foot)?;
    let man_i = table.descriptor_index(0x03)?;
    let man_off = man.man_offset;
    let next_sub_off = man_off.checked_add(man.compressed_budget)?;
    if next_sub_off > foot.len() || man_off + stream.len() > next_sub_off + insert {
        return None;
    }

    // Insert `insert` blank bytes at the MAN's compressed-region end; later
    // sub-assets shift up by `insert`.
    let mut new = Vec::with_capacity(foot.len() + insert);
    new.extend_from_slice(&foot[..next_sub_off]);
    new.resize(new.len() + insert, 0);
    new.extend_from_slice(&foot[next_sub_off..]);
    new[man_off..man_off + stream.len()].copy_from_slice(&stream);

    // Bump the descriptor `data_offset` of every sub-asset after the MAN.
    for (i, d) in table.used().iter().enumerate() {
        if d.data_offset as usize > man_off {
            let off = scene_asset_table::SceneAssetTable::size_word_offset(i) + 4;
            if off + 4 > new.len() {
                return None;
            }
            let nv = d.data_offset + insert as u32;
            new[off..off + 4].copy_from_slice(&nv.to_le_bytes());
        }
    }
    // Rewrite the MAN descriptor's decompressed-size word.
    let sw = scene_asset_table::SceneAssetTable::size_word_offset(man_i);
    if sw + 4 > new.len() {
        return None;
    }
    let size_word = scene_asset_table::encode_size_word(0x03, grown.len() as u32);
    new[sw..sw + 4].copy_from_slice(&size_word.to_le_bytes());

    debug_assert_eq!(new.len(), foot.len() + insert);
    debug_assert_eq!(new.len() % SECTOR, 0);
    Some((new, grown_sectors as u32, stream.len()))
}

/// Build a streaming scene entry's new **full-footprint payload** with its
/// leading MAN chunk grown to carry every ready line at full length (see
/// [`StreamManText`]). `ready` offsets are entry offsets (the `raw:` key
/// space); each must lie inside the MAN chunk. Returns `(payload,
/// grown_sectors)` - `grown_sectors == 0` when the entry's own trailing sector
/// slack absorbs the growth, so the payload can be written in place - or
/// `None` when the rewrite can't be done safely (a line outside the MAN, a
/// record the relocator refuses, a program the round-trip doesn't preserve,
/// or the loader arena bound).
fn build_grown_stream_payload(
    patcher: &DiscPatcher,
    entry_idx: usize,
    sm: &StreamManText,
    ready: &[ReadyMan],
) -> Option<(Vec<u8>, u32)> {
    let range = sm.man_range();
    let mut edits = Vec::with_capacity(ready.len());
    for r in ready {
        if r.off < range.start || r.off + r.old_len > range.end {
            return None;
        }
        edits.push(TextEdit {
            offset: r.off - range.start,
            old_len: r.old_len,
            new_bytes: r.translated.clone(),
        });
    }
    let grown = man_edit::apply_text_edits(&sm.man, &edits).ok()?;
    if !man_edit::text_edits_preserve_scripts(&sm.man, &grown) {
        return None;
    }
    let foot = patcher.read_entry_footprint(entry_idx).ok()?;
    // The rebuilt payload is never shorter than the footprint (a shrunken
    // MAN keeps its sectors), so the difference is the whole-sector growth.
    let payload = sm.rebuild(&foot, &grown)?;
    let grown_sectors = (payload.len() / SECTOR).checked_sub(foot.len() / SECTOR)?;
    Some((payload, grown_sectors as u32))
}

/// Apply `pack` to the patcher's image. Untranslated entries are untouched.
/// No disc relayout: MANs whose full-length dialog overflows their compressed
/// footprint fall back to same-size + abbreviation.
pub fn import_pack(patcher: &mut DiscPatcher, pack: &LanguagePack) -> Result<ImportReport> {
    import_pack_phase(patcher, pack, ImportPhase::All, false)
}

/// [`import_pack`] with the whole-sector **disc relayout** enabled: a scene MAN
/// whose full-length dialog can't fit its compressed footprint is given `+N`
/// sectors (the PROT entry grows, the disc is relaid out) so the dialog imports
/// byte-faithfully instead of being abbreviated. See [`ImportReport`]'s
/// `relayout_*` counters.
pub fn import_pack_relayout(
    patcher: &mut DiscPatcher,
    pack: &LanguagePack,
) -> Result<ImportReport> {
    import_pack_phase(patcher, pack, ImportPhase::All, true)
}

/// [`import_pack`] restricted to one key population (see [`ImportPhase`]).
/// Entries outside the phase are ignored entirely - they appear in none of
/// the report's counters - so running `DialogOnly` then `NamesOnly` and
/// [`ImportReport::merge`]-ing the two reports counts every entry exactly
/// once, identically to a single `All` run.
pub fn import_pack_phase(
    patcher: &mut DiscPatcher,
    pack: &LanguagePack,
    phase: ImportPhase,
    allow_relayout: bool,
) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    // Scene MAN entries whose full-length dialog overflows their compressed
    // footprint and needs a whole-sector disc relayout. Collected across the MAN
    // loop, then applied in one relayout pass so the PROT index space (and every
    // later index-keyed edit) is preserved.
    let mut pending_growth: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    let mut pending_meta: Vec<(usize, Vec<String>, u32)> = Vec::new();

    // Group the work by write mechanism.
    let mut scus_work: Vec<&Entry> = Vec::new();
    let mut man_work: BTreeMap<usize, Vec<(usize, &Entry)>> = BTreeMap::new();
    let mut raw_work: BTreeMap<usize, Vec<(usize, &Entry)>> = BTreeMap::new();
    let mut ui_work: BTreeMap<usize, Vec<(u32, &Entry)>> = BTreeMap::new();
    let mut mon_work: Vec<(u16, &Entry)> = Vec::new();
    for (_, entries) in pack.sections.iter() {
        for e in entries {
            let key = parse_key(&e.key);
            let in_phase = match (&key, phase) {
                (_, ImportPhase::All) => true,
                (Some(Key::Man { .. }) | Some(Key::Raw { .. }), ImportPhase::DialogOnly) => true,
                (
                    Some(Key::ScusStr { .. })
                    | Some(Key::ScusParty { .. })
                    | Some(Key::ScusCell { .. })
                    | Some(Key::Ui { .. })
                    | Some(Key::Mon { .. }),
                    ImportPhase::NamesOnly,
                ) => true,
                // Unrecognized keys are diagnosed once, in the names (last)
                // phase, so a dialog+names pair reports them exactly once.
                (None, ImportPhase::NamesOnly) => true,
                _ => false,
            };
            if !in_phase {
                continue;
            }
            if e.translation.trim().is_empty() {
                report.untranslated += 1;
                continue;
            }
            match key {
                Some(Key::ScusStr { .. })
                | Some(Key::ScusParty { .. })
                | Some(Key::ScusCell { .. }) => scus_work.push(e),
                Some(Key::Man { entry, off }) => {
                    man_work.entry(entry).or_default().push((off, e));
                }
                Some(Key::Raw { entry, off }) => {
                    raw_work.entry(entry).or_default().push((off, e));
                }
                Some(Key::Ui { prot, va }) => {
                    ui_work.entry(prot).or_default().push((va, e));
                }
                Some(Key::Mon { id }) => mon_work.push((id, e)),
                None => report.issue(&e.key, "unrecognized key shape - skipped"),
            }
        }
    }

    // SCUS strings. Read the file once; every write lands in the local copy
    // (so later verifications stay coherent with earlier writes) and is then
    // mirrored onto the disc.
    if !scus_work.is_empty() {
        let mut scus = patcher
            .read_named_file("SCUS_942.54")
            .context("SCUS_942.54 not found in disc image")?;
        // Measured before any write, so every span is the retail one.
        let pool = NamePool::build(&scus);
        for (off, len) in apply_scus_work(&mut scus, &pool, &scus_work, &mut report) {
            patcher.patch_named_file("SCUS_942.54", off as u64, &scus[off..off + len])?;
        }
    }

    // Scene-bundle MANs: one decompress -> N segment edits -> one repack per
    // PROT entry.
    for (entry_idx, edits) in man_work {
        let plan = plan_scene_man(patcher, entry_idx, &edits, allow_relayout, &mut report);
        for (off, bytes) in &plan.writes {
            patcher.patch_prot_entry(entry_idx, *off, bytes)?;
        }
        report.applied += plan.applied.len();
        report.applied_keys.extend(plan.applied);
        if let Some((payload, keys, grown_sectors)) = plan.growth {
            pending_growth.insert(entry_idx, payload);
            pending_meta.push((entry_idx, keys, grown_sectors));
        }
    }

    // Raw carriers: one read per PROT entry. A streaming dungeon scene (an
    // uncompressed MAN leading a typed-chunk stream) gets the generalized
    // rewriter - grow the MAN chunk, shift the later chunks, fit the entry's
    // own sector slack or (with relayout) grow the entry by whole sectors;
    // anything else, and any line the rewriter can't carry, is a same-size
    // in-place write.
    for (entry_idx, edits) in raw_work {
        let mut ct = CarrierTrace {
            footprint: patcher
                .entry_true_footprint_sectors(entry_idx)
                .map_or(0, |n| n as usize * SECTOR),
            keyed: edits.len(),
            ..Default::default()
        };
        'carrier: {
            let mut window = match patcher.read_entry(entry_idx) {
                Ok(b) => b,
                Err(e) => {
                    for (_, en) in &edits {
                        report.issue(&en.key, format!("PROT entry unreadable: {e}"));
                    }
                    break 'carrier;
                }
            };
            // Carrier gate: the `0x1F <text> 0x00` framing occurs by coincidence
            // throughout binary asset banks, so refuse to write into any entry
            // that isn't a genuine, prose-dense dialog carrier on the disc being
            // patched - writing a "translation" over a coincidental hit corrupts
            // the asset and freezes the game. Real event-script / dungeon-MAN
            // scenes clear the bar with a wide margin (see
            // [`segments::is_dialog_carrier`]).
            if !segments::is_dialog_carrier(&window) {
                for (_, en) in &edits {
                    report.issue(
                        &en.key,
                        format!(
                            "PROT entry {entry_idx} is not a dialog carrier on this disc \
                             (binary asset bank - writing here would corrupt it) - skipped"
                        ),
                    );
                }
                break 'carrier;
            }
            let sm = StreamManText::locate(&window);
            if let Some(sm) = &sm {
                ct.streaming = true;
                ct.sector_slack = patcher
                    .read_entry_footprint(entry_idx)
                    .ok()
                    .map(|foot| sm.sector_slack(&foot));
            }
            // Validate each segment against the disc once (framing /
            // already-applied / wrong-disc guard), collecting the ready set
            // with its current span.
            let ready = collect_ready(&window, entry_idx, &edits, &mut report);
            if ready.is_empty() {
                break 'carrier;
            }

            // Streaming dungeon MAN: the same structural gate as the LZS MANs
            // (keyed offsets are entry offsets; the MAN sits at `man_range()`),
            // and any line the walk does not reach stays same-size only.
            let (ready, blind) = match &sm {
                Some(sm) => {
                    let start = sm.man_range().start;
                    let mut walked = Vec::new();
                    let mut blind = Vec::new();
                    for r in ready {
                        match r
                            .off
                            .checked_sub(start)
                            .map(|o| man_edit::text_site(&sm.man, o))
                        {
                            Some(TextSite::Segment) => walked.push(r),
                            Some(TextSite::Operand) => report.issue(&r.entry.key, OPERAND_RUN_MSG),
                            _ => blind.push(r),
                        }
                    }
                    (walked, blind)
                }
                None => (Vec::new(), ready),
            };

            // Escape hatch: a line longer than its span grows the streaming
            // MAN. In the entry's own slack it is a same-size-image write
            // (PPF-safe); past it, a whole-sector grow staged for the relayout
            // pass. The same-size (blind) lines are written first so the grown
            // payload, built from a fresh read of the entry, carries them too.
            for r in &blind {
                if r.translated.len() <= r.old_len {
                    let padded = pad_segment(&r.translated, r.old_len);
                    window[r.off..r.off + r.old_len].copy_from_slice(&padded);
                    patcher.patch_prot_entry(entry_idx, r.off as u64, &padded)?;
                    report.applied += 1;
                    report.applied_keys.push(r.entry.key.clone());
                    ct.path = WritePath::Padded;
                }
            }
            if ready.iter().any(|r| r.translated.len() > r.old_len)
                && let Some(sm) = StreamManText::locate(&window)
                && let Some((payload, grown_sectors)) =
                    build_grown_stream_payload(patcher, entry_idx, &sm, &ready)
            {
                ct.grown_sectors = Some(grown_sectors);
                if grown_sectors == 0 {
                    patcher.patch_prot_entry(entry_idx, 0, &payload)?;
                    report.applied += ready.len();
                    report
                        .applied_keys
                        .extend(ready.iter().map(|r| r.entry.key.clone()));
                    ct.path = WritePath::Relocated;
                    break 'carrier;
                }
                if allow_relayout {
                    pending_growth.insert(entry_idx, payload);
                    pending_meta.push((
                        entry_idx,
                        ready.iter().map(|r| r.entry.key.clone()).collect(),
                        grown_sectors,
                    ));
                    ct.path = WritePath::Relayout;
                    break 'carrier;
                }
            }

            // Same-size path: the fitting lines in place, space-padded; the
            // rest reported (the carrier couldn't be grown to fit them).
            for r in ready
                .iter()
                .chain(blind.iter().filter(|r| r.translated.len() > r.old_len))
            {
                if r.translated.len() > r.old_len {
                    report.issue_as(
                        &r.entry.key,
                        IssueKind::OverBudget,
                        format!(
                            "translation needs {} bytes but the in-place budget is {} and the \
                             scene MAN could not be grown to fit it (shorten this line)",
                            r.translated.len(),
                            r.old_len
                        ),
                    );
                    continue;
                }
                let padded = pad_segment(&r.translated, r.old_len);
                window[r.off..r.off + r.old_len].copy_from_slice(&padded);
                patcher.patch_prot_entry(entry_idx, r.off as u64, &padded)?;
                report.applied += 1;
                report.applied_keys.push(r.entry.key.clone());
                ct.path = WritePath::Padded;
            }
        }
        report.trace.carriers.insert(entry_idx, ct);
    }

    // Apply all staged whole-sector grows (scene-bundle MANs and streaming
    // carriers alike) in one disc relayout, so the PROT index space (and every
    // later index-keyed edit) is preserved. Same-size edits above are already
    // in the image and carried through the rebuild.
    if !pending_growth.is_empty() {
        match patcher.grow_prot_entries(&pending_growth) {
            Ok(()) => {
                for (_, keys, grown_sectors) in &pending_meta {
                    report.relayout_entries += 1;
                    report.relayout_sectors_added += grown_sectors;
                    report.applied += keys.len();
                    report.applied_keys.extend(keys.iter().cloned());
                }
            }
            Err(e) => {
                for (entry_idx, keys, _) in &pending_meta {
                    for k in keys {
                        report.issue(k, format!("scene {entry_idx}: disc relayout failed: {e}"));
                    }
                    if let Some(t) = report.trace.scenes.get_mut(entry_idx) {
                        t.path = WritePath::None;
                    }
                    if let Some(t) = report.trace.carriers.get_mut(entry_idx) {
                        t.path = WritePath::None;
                    }
                }
            }
        }
    }

    // Overlay UI menu strings: same-size NUL-terminated writes into the menu /
    // battle overlay PROT entries. One read per overlay entry; each write is
    // mirrored into the local copy so later verifications stay coherent.
    for (prot, edits) in ui_work {
        let Some(base_va) = ui::overlay_base_va(prot) else {
            for (_, en) in &edits {
                report.issue(&en.key, "PROT entry is not a mapped UI overlay - skipped");
            }
            continue;
        };
        let mut buf = match patcher.read_entry(prot) {
            Ok(b) => b,
            Err(e) => {
                for (_, en) in &edits {
                    report.issue(&en.key, format!("PROT entry unreadable: {e}"));
                }
                continue;
            }
        };
        for (va, en) in edits {
            let strict = ui::pool_for(prot, va).is_some_and(|p| p.strict);
            if let Some((off, bytes)) = plan_ui(&buf, base_va, en, va, strict, &mut report) {
                patcher.patch_prot_entry(prot, off as u64, &bytes)?;
                buf[off..off + bytes.len()].copy_from_slice(&bytes);
                report.applied += 1;
                report.applied_keys.push(en.key.clone());
            }
        }
    }

    if !mon_work.is_empty() {
        import_monster_names(patcher, &mon_work, &mut report)?;
    }

    Ok(report)
}

/// Plan and apply every SCUS write (`scus:str` / `scus:party` / `scus:cell`
/// entries) into `scus`, in place: the same-size writes first, then the names
/// that outgrew their span moved into the pools' free bytes
/// ([`NamePool::relocate`]). Returns the `(file offset, len)` ranges changed,
/// to mirror onto the disc. `pool` must be built from the executable before
/// any write. Import runs this over the disc's executable; the space report's
/// name fitter ([`super::space::NameFitter`]) runs it over a copy, so a live
/// editor sees the importer's own decision for every name.
pub(crate) fn apply_scus_work(
    scus: &mut [u8],
    pool: &NamePool,
    work: &[&Entry],
    report: &mut ImportReport,
) -> Vec<(usize, usize)> {
    let mut written = Vec::new();
    let mut grow: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut grow_entries: BTreeMap<u32, &Entry> = BTreeMap::new();
    for e in work {
        let plan = match parse_key(&e.key) {
            Some(Key::ScusStr { va }) => match plan_scus_str(scus, e, va, pool, report) {
                Some(ScusStrPlan::Grow(bytes)) => {
                    grow.push((va, bytes));
                    grow_entries.insert(va, e);
                    None
                }
                Some(ScusStrPlan::Write(off, bytes)) => Some((off, bytes)),
                None => None,
            },
            Some(Key::ScusParty { slot }) => plan_scus_party(scus, e, slot, report),
            Some(Key::ScusCell { va }) => plan_scus_cell(scus, e, va, report),
            _ => {
                report.issue(&e.key, "not a SCUS key - skipped");
                None
            }
        };
        if let Some((off, bytes)) = plan {
            scus[off..off + bytes.len()].copy_from_slice(&bytes);
            written.push((off, bytes.len()));
            report.applied += 1;
            report.applied_keys.push(e.key.clone());
        }
    }
    // Names that outgrew their span: move them into the pools' free bytes
    // (the in-place writes above already freed every shortened tail). With
    // nothing to move this only measures the pools as written.
    let reloc = pool.relocate(scus, &grow);
    written.extend(reloc.written);
    for m in &reloc.moved {
        let e = grow_entries[&m.from];
        report.applied += 1;
        report.relocated_names += 1;
        report.applied_keys.push(e.key.clone());
        report.trace.moved.insert(e.key.clone(), (m.from, m.to));
    }
    for va in reloc.no_room {
        let e = grow_entries[&va];
        let need = grow
            .iter()
            .find(|(v, _)| *v == va)
            .map_or(0, |(_, b)| b.len());
        report.issue_as(
            &e.key,
            IssueKind::NoFreeRun,
            format!(
                "translation needs {need} bytes but the in-place budget is {} and the \
                 name tables have no free run that long to move it into (shorten this \
                 name, or others in the same tables to free room)",
                e.budget
            ),
        );
    }
    report.trace.name_layout = Some(reloc.layout);
    written
}

/// What one scene MAN's import decided ([`plan_scene_man`]): the writes to
/// land in its PROT entry, the keys they carry, and a staged whole-sector
/// grow.
#[derive(Default)]
pub(crate) struct ScenePlan {
    /// `(entry offset, bytes)` writes inside the entry's footprint.
    pub writes: Vec<(u64, Vec<u8>)>,
    /// Keys the writes carry (counted applied once they land).
    pub applied: Vec<String>,
    /// A staged relayout: `(full-footprint payload, keys, grown sectors)`.
    pub growth: Option<(Vec<u8>, Vec<String>, u32)>,
}

/// Plan one scene MAN's dialog edits (`edits` = `(decompressed offset,
/// entry)` pairs of its filled `man:` keys) without touching the disc: the
/// relocator at full length, a staged relayout, a fitted rollback, or the
/// padded same-size write, in that order. Records the scene's
/// [`SceneTrace`] and every per-key diagnostic in `report`. Import applies
/// the plan; [`super::space::scene_fit`] only reads it.
pub(crate) fn plan_scene_man(
    patcher: &DiscPatcher,
    entry_idx: usize,
    edits: &[(usize, &Entry)],
    allow_relayout: bool,
    report: &mut ImportReport,
) -> ScenePlan {
    let mut plan = ScenePlan::default();
    let entry_bytes = match patcher.read_entry(entry_idx) {
        Ok(b) => b,
        Err(e) => {
            for (_, en) in edits {
                report.issue(&en.key, format!("PROT entry unreadable: {e}"));
            }
            return plan;
        }
    };
    let Some(mut man) = SceneManText::locate(&entry_bytes) else {
        for (_, en) in edits {
            report.issue(&en.key, "scene MAN not found in this PROT entry - skipped");
        }
        return plan;
    };
    let mut trace = SceneTrace {
        footprint: man.compressed_budget,
        disc_len: man.compressed_len,
        keyed: edits.len(),
        ..Default::default()
    };
    plan_man_edits(
        patcher,
        entry_idx,
        &mut man,
        edits,
        allow_relayout,
        report,
        &mut plan,
        &mut trace,
    );
    report.trace.scenes.insert(entry_idx, trace);
    plan
}

/// The body of [`plan_scene_man`] once the MAN is located.
#[allow(clippy::too_many_arguments)]
fn plan_man_edits(
    patcher: &DiscPatcher,
    entry_idx: usize,
    man: &mut SceneManText,
    edits: &[(usize, &Entry)],
    allow_relayout: bool,
    report: &mut ImportReport,
    plan: &mut ScenePlan,
    trace: &mut SceneTrace,
) {
    // Validate each segment against the disc once (framing / already-applied
    // / wrong-disc guard), collecting the ready set with its current span.
    let ready = collect_ready(&man.decoded, entry_idx, edits, report);
    // Structural gate: a line is dialog only when its `0x1F` lead is the
    // text an instruction on the record's clean script walk carries. A
    // coincidental `1F .. 00` inside an instruction's operands is refused on
    // every path (writing "text" there corrupts the script); a line the walk
    // does not reach is written same-size only, pre-applied here so the grown
    // MAN carries it and the same-size rollback covers it.
    let (ready, mut applied) = gate_text_sites(&mut man.decoded, ready, report);
    if ready.is_empty() && applied.is_empty() {
        return;
    }
    let any_over = ready.iter().any(|r| r.translated.len() > r.old_len);

    // Escape hatch: if any line overflows its own byte span, try to grow the
    // whole MAN (budget = the MAN's own on-disc footprint, not each string).
    // On success the segment framing moves but every crossing reference is
    // relocated, so the pager still walks it correctly.
    if any_over {
        match try_grow_man(man, &ready) {
            Ok((stream, new_size)) => {
                trace.path = WritePath::Relocated;
                trace.written_len = Some(stream.len());
                plan.writes.push((man.man_offset as u64, stream));
                plan.writes.push(size_word_write(man, new_size));
                plan.applied
                    .extend(ready.iter().map(|r| r.entry.key.clone()));
                plan.applied
                    .extend(applied.iter().map(|(_, _, en)| en.key.clone()));
                return;
            }
            Err(GrowFail::Overflow(n)) => trace.full_overflow = Some(n),
            Err(GrowFail::NotPreserved) => trace.refused = true,
        }
        // The full-length dialog overflows the MAN's compressed footprint.
        // With relayout enabled, stage a whole-sector grow of this PROT entry
        // (applied together, after the loop) instead of abbreviating.
        if allow_relayout
            && let Some((payload, grown_sectors, stream_len)) =
                build_grown_entry_payload(patcher, entry_idx, man, &ready)
        {
            trace.path = WritePath::Relayout;
            trace.relayout_sectors = Some(grown_sectors);
            trace.written_len = Some(stream_len);
            plan.growth = Some((
                payload,
                ready
                    .iter()
                    .map(|r| r.entry.key.clone())
                    .chain(applied.iter().map(|(_, _, en)| en.key.clone()))
                    .collect(),
                grown_sectors,
            ));
            return;
        }
        if !allow_relayout && let Some(n) = trace.full_overflow {
            trace.relayout_would_add = Some(sectors_for_overflow(n));
        }

        // A line is over its span and neither the whole-set grow nor a
        // relayout took the scene: fit the most lines the footprint holds
        // through the relocator before falling back to padding (which could
        // only drop every over-span line).
        let fit = fit_man_in_footprint(man, &ready);
        trace.refused |= fit.refused;
        if let Some(fit) = fit.fitted {
            stage_fitted_man(entry_idx, man, &ready, &applied, fit, report, plan, trace);
            return;
        }
    }

    // Same-size (fast, byte-identical) path: apply the fitting lines in
    // place, report the over-budget ones (the MAN couldn't be grown to fit
    // them), then recompress with a longest-first rollback if the scene's
    // dialog no longer fits its compressed footprint.
    let pre_applied = applied.len();
    for r in &ready {
        if r.translated.len() > r.old_len {
            report.issue_as(
                &r.entry.key,
                IssueKind::OverBudget,
                format!(
                    "translation needs {} bytes but the in-place budget is {} and the \
                     scene MAN could not be grown to fit it (shorten this line)",
                    r.translated.len(),
                    r.old_len
                ),
            );
            continue;
        }
        let before = man.decoded[r.off..r.off + r.old_len].to_vec();
        let padded = pad_segment(&r.translated, r.old_len);
        man.decoded[r.off..r.off + r.old_len].copy_from_slice(&padded);
        applied.push((r.off, before, r.entry));
    }
    if applied.is_empty() {
        return;
    }

    // The MAN must recompress into its original footprint. Translated text
    // is less repetitive than the source, so a scene can overflow by a few
    // bytes; roll back the costliest lines (longest first) one at a time
    // rather than losing the whole scene's dialog. `pop()` takes from the
    // vector's tail, so an ASCENDING sort puts the longest line there.
    let mut stream = man.repack_measured();
    if let Err(over) = stream {
        trace.padded_overflow = Some(over);
    }
    // Space padding spends bytes the footprint may not have: every shorter
    // translation padded back to the source length recompresses worse than
    // the same line at its own length. Before rolling lines back, undo the
    // padding and let the relocator fit the scene at exact lengths (the
    // shape a translation-only pack of in-budget lines hits).
    if stream.is_err() && !any_over && applied.len() > pre_applied {
        for (off, before, _) in applied.drain(pre_applied..).rev() {
            man.decoded[off..off + before.len()].copy_from_slice(&before);
        }
        let fit = fit_man_in_footprint(man, &ready);
        trace.refused |= fit.refused;
        if trace.full_overflow.is_none() {
            trace.full_overflow = fit.first_overflow;
        }
        if let Some(fit) = fit.fitted {
            stage_fitted_man(entry_idx, man, &ready, &applied, fit, report, plan, trace);
            return;
        }
        for r in &ready {
            let before = man.decoded[r.off..r.off + r.old_len].to_vec();
            man.decoded[r.off..r.off + r.old_len]
                .copy_from_slice(&pad_segment(&r.translated, r.old_len));
            applied.push((r.off, before, r.entry));
        }
    }
    if stream.is_err() {
        applied.sort_by_key(|(_, before, _)| before.len());
        while stream.is_err()
            && let Some((off, before, en)) = applied.pop()
        {
            man.decoded[off..off + before.len()].copy_from_slice(&before);
            report.issue_as(
                &en.key,
                IssueKind::RolledBack,
                format!(
                    "scene {entry_idx}: rolled back - the scene's dialog no longer \
                     recompresses into its {} byte footprint (shorten this line)",
                    man.compressed_budget
                ),
            );
            trace.rolled_back.push(en.key.clone());
            if applied.is_empty() {
                break;
            }
            stream = man.repack_measured();
        }
    }
    if let Ok(stream) = stream
        && !applied.is_empty()
    {
        trace.path = WritePath::Padded;
        trace.written_len = Some(stream.len());
        plan.writes.push((man.man_offset as u64, stream));
        plan.applied
            .extend(applied.iter().map(|(_, _, en)| en.key.clone()));
    }
}

/// Monster names: one decode / rewrite / re-pack per touched record, into its
/// fixed slot of the archive (see [`monster_names`]). The budget is re-derived
/// from the archive on this disc, never taken from the pack.
fn import_monster_names(
    patcher: &mut DiscPatcher,
    work: &[(u16, &Entry)],
    report: &mut ImportReport,
) -> Result<()> {
    let mut archive = match patcher.read_entry(crate::disc::MONSTER_ARCHIVE_ENTRY) {
        Ok(b) => b,
        Err(e) => {
            for (_, en) in work {
                report.issue(&en.key, format!("monster archive unreadable: {e}"));
            }
            return Ok(());
        }
    };
    let fields = monster_names::fields(&archive);
    let budgets: BTreeMap<u16, usize> = monster_names::budgets(&fields).into_iter().collect();
    let current: BTreeMap<u16, Vec<u8>> = fields.into_iter().map(|(id, f)| (id, f.bytes)).collect();
    for &(id, en) in work {
        let (Some(cur), Some(&budget)) = (current.get(&id), budgets.get(&id)) else {
            report.issue(&en.key, "no named monster record at this id - skipped");
            continue;
        };
        let Ok(source) = encode_source(en, Target::CString, report) else {
            continue;
        };
        let Some(translated) = encode_translation(en, Target::CString, report) else {
            continue;
        };
        if !translated.iter().all(|&b| (0x20..0x7F).contains(&b)) {
            report.issue_as(
                &en.key,
                IssueKind::NotEncodable,
                "a monster name takes printable glyphs only (the loader reads the \
                 record's name as plain text, markup escapes included)",
            );
            continue;
        }
        if cur == &translated {
            report.already_applied += 1;
            report.already_keys.push(en.key.clone());
            continue;
        }
        if let Some(src) = &source
            && src != cur
        {
            report.issue_as(
                &en.key,
                IssueKind::Mismatch,
                "disc bytes don't match the pack source (different disc revision or \
                 a conflicting patch) - skipped",
            );
            continue;
        }
        report.room(&en.key, budget);
        if source.is_none() && !hint_agrees(en, budget, report) {
            continue;
        }
        // Past the record's own room the record grows (up to the longest
        // retail name); `rewrite_slot` enforces both limits.
        match monster_names::rewrite_slot(&archive, id, &translated) {
            Ok((slot, grown)) => {
                let at = monster_names::slot_offset(id);
                patcher.patch_monster_slot(id, &slot)?;
                archive[at..at + slot.len()].copy_from_slice(&slot);
                report.applied += 1;
                report.grown_monster_names += usize::from(grown);
                report.applied_keys.push(en.key.clone());
                if grown {
                    report.trace.grown_monsters.insert(en.key.clone());
                }
            }
            Err(e) => report.issue_as(&en.key, IssueKind::Refused, format!("{e} - skipped")),
        }
    }
    Ok(())
}
