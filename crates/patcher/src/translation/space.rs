//! Space report: how much room every translatable string has on a disc, how
//! much English uses, and - with a language pack - how much the pack uses and
//! what the importer does with each line.
//!
//! **Every number is the importer's.** A disc-only report reads the room the
//! export measures (the same span / padding / framing rules import clamps
//! to) and the name pools' compaction through [`NamePool::layout`], the
//! function the relocator applies. A disc + pack report runs
//! [`import_pack_phase`] on a scratch copy of the image and reads what it
//! recorded on the way ([`ImportReport::trace`]): the room it measured per
//! key, the encoded length, the outcome, the pools' layout after the moves,
//! and per scene MAN the recompressed size, the overflow, the rollbacks, the
//! relocator's refusals and the relayout sectors. Nothing here re-derives a
//! rule, so a report that says a line fits is a line the import writes.
//!
//! # JSON schema (`legaia-space-v1`)
//!
//! [`SpaceReport`] serializes (serde) to one object. Numbers are byte counts
//! unless named otherwise; VAs are integers; optional fields are `null`
//! when not applicable (a disc-only report has no `pack_*` / `outcome`
//! values). No field carries game text - only keys, lengths and counts.
//!
//! ```text
//! {
//!   "schema": "legaia-space-v1",
//!   "language": "fr" | null,          // pack language; null = disc only
//!   "relayout": false,                // --allow-relayout dry run
//!   "summary": {
//!     "entries": 24000, "filled": 3100,
//!     "outcomes": { "in_place": 2900, "moved": 40, ... },
//!     "name_bytes": 9000, "name_free_english": 12, "name_free_pack": 300,
//!     "scenes": 97, "scenes_rolled_back": 2,
//!     "relayout_entries": 0, "relayout_sectors": 0
//!   },
//!   "name_regions": [ {                // one per compaction region
//!     "index": 0, "start_va": 2147553888, "end_va": 2147555000,
//!     "total": 1112, "strings": 80,
//!     "english_used": 1112, "english_free": 0,
//!     "pack_used": 1040 | null, "pack_free": 72 | null } ],
//!   "names": [ {                       // every SCUS name-table string
//!     "key": "scus:str:0x80012260", "va": 2147557984,
//!     "movable": true, "pin": null | "arts_description" | "tail_shared" |
//!       "lui_materialised" | "word_count_mismatch" | "no_span" |
//!       "not_a_table_string",
//!     "region": 0 | null, "span": 16 | null, "moved_to": null | 2147558000 } ],
//!   "monsters": [ {
//!     "key": "mon:12", "id": 12, "room": 11, "cap": 15, "longest": 15,
//!     "block_len": 70000, "kept_len": 50000,
//!     "max_block": 126464, "max_kept": 93216 } ],
//!   "pools": [ {                       // ui_menu + system_text windows
//!     "index": 0, "section": "ui_menu", "label": "menu", "prot": 899 | null,
//!     "va_start": ..., "va_end": ..., "total": 1116, "strict": false,
//!     "strings": 90, "english_bytes": 800, "room_bytes": 830, "slack": 30,
//!     "pack_bytes": 790 | null } ],
//!   "scenes": [ {                      // scene MANs (`man:` keys)
//!     "prot": 123, "scene": "town01" | null, "lines": 210,
//!     "footprint": 18432, "disc_len": 18430, "slack": 2,
//!     "filled": 40 | null, "path": "none" | "padded" | "relocated" |
//!       "relayout" | null,
//!     "written_len": 18420 | null, "full_overflow": 35 | null,
//!     "padded_overflow": null, "refused": false,
//!     "rolled_back": ["man:123:0x4f0"], "relayout_sectors": null,
//!     "relayout_would_add": 1 | null } ],
//!   "carriers": [ {                    // raw carriers (`raw:` keys)
//!     "prot": 456, "scene": "dolk2" | null, "streaming": true,
//!     "lines": 80, "footprint": 40960, "sector_slack": 900 | null,
//!     "max_footprint": 339968 | null, "filled": 10 | null,
//!     "path": ... | null, "grown_sectors": 0 | null } ],
//!   "entries": [ {                     // flat per-key index
//!     "key": "man:123:0x4f0", "section": "scene_dialog",
//!     "room_kind": "dialog_growable", "room": 42,
//!     "group": "scene:123", "english_len": 42,
//!     "pack_len": 45 | null, "outcome": "relocated" | null,
//!     "issue_kind": null | "over_budget" | ..., "issue": null | "..." } ]
//! }
//! ```
//!
//! `group` names the thing an entry shares room with: `region:<i>` (a
//! movable name, index into `name_regions`), `pool:<i>` (into `pools`),
//! `scene:<prot>` / `carrier:<prot>` (into `scenes` / `carriers` by
//! `prot`), `monster`, `party`, `place`, or `null` (a pinned name).
//!
//! `room_kind` says what `room` bounds:
//!
//! - `string_fixed` - a NUL-terminated string that cannot move (a pinned
//!   name, a `ui_menu` / `system_text` string): `room` is the hard limit;
//! - `name_movable` - a SCUS name: `room` in place, and past it the name
//!   moves if the pools have a free run (`region`'s `pack_free` after the
//!   pack's own moves);
//! - `field` - a fixed NUL-padded field (party name 9, place name 31);
//! - `monster` - `room` in place, up to the record's `cap` by growing it;
//! - `dialog_growable` - a line on its record's clean script walk: `room` is
//!   its own span, and a longer line relocates the scene / streaming MAN,
//!   bounded by the carrier's footprint / sector slack (see `scenes` /
//!   `carriers`);
//! - `dialog_fixed` - a line the script walk does not reach, or an
//!   event-script carrier line: its own span only.
//!
//! `outcome` (pack only): `untranslated`, `in_place`, `moved` (a longer
//! name relocated), `grown` (a monster record grew), `relocated` (the line's
//! scene / streaming MAN was rewritten at exact lengths inside its
//! footprint), `relayout` (its entry grows by whole sectors),
//! `already_applied`, or a skip: `over_budget`, `no_free_run`,
//! `rolled_back`, `refused`, `not_encodable`, `mismatch`, `skipped`.
//!
//! # Fast paths for a live editor
//!
//! A whole report runs a whole-disc export (and a whole dry-run import);
//! keystroke-level feedback needs less:
//!
//! - **Encoded length + per-character errors:** [`encoded_len`] (a thin
//!   wrapper over [`markup::encode`], with [`target_for_key`] picking the
//!   byte space for a key). The dialog / string limits in `entries[].room`
//!   are in these bytes.
//! - **One scene:** [`scene_fit`] re-plans one scene MAN with the pack's
//!   lines through the importer's own scene planner - relocator, relayout
//!   staging, fitted rollback, padded write - without writing anything.
//! - **Names:** [`NameFitter`]. The per-region numbers do **not** let a
//!   client recompute free bytes by summing `align4(len + 1)`: the
//!   compaction keeps an unaligned retail string at its original offset, a
//!   longer name moves into the tightest free run of *any* region (largest
//!   names first), and a name that finds none is dropped and the layout
//!   re-planned. [`NameFitter::fit`] runs the importer's SCUS pass over a
//!   copy of the executable instead, which costs a copy and one compaction.
//! - **Monsters:** `room` / `cap` are exact: a name up to `room` bytes writes
//!   in place, up to `cap` grows the record ([`monster_names::grow_block`]
//!   decided `cap`), past it is refused; a monster name takes printable
//!   glyphs only.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use legaia_asset::man_edit::{self, TextSite};

use crate::disc::DiscPatcher;

use super::export::{SceneManText, export_pack};
use super::import::{
    ImportPhase, ImportReport, IssueKind, Key, WritePath, import_pack_phase, parse_key,
    plan_scene_man,
};
use super::markup::{self, EncodeIssue, Target};
use super::monster_names;
use super::name_pool::{Layout, NamePool, PinReason};
use super::pack::{Entry, LanguagePack};
use super::stream_man::{MAX_GROWN_FOOTPRINT, StreamManText};
use super::ui;

/// Schema tag of a serialized [`SpaceReport`].
pub const SPACE_SCHEMA: &str = "legaia-space-v1";

/// Pack sections whose strings are SCUS name-table strings.
const NAME_SECTIONS: [&str; 5] = [
    "items",
    "item_types",
    "spells",
    "arts",
    "accessory_passives",
];

/// Options for [`space_report`].
#[derive(Debug, Clone, Default)]
pub struct SpaceOptions {
    /// Dry-run the import with the whole-sector relayout enabled
    /// (`translate import --allow-relayout`).
    pub relayout: bool,
}

/// What the importer did (or, disc-only, nothing) with one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Empty translation: the disc keeps its text.
    Untranslated,
    /// Written same-size into its own room.
    InPlace,
    /// A longer SCUS name moved into the pools' free bytes.
    Moved,
    /// A longer monster name; its record grew.
    Grown,
    /// Its scene / streaming MAN was rewritten at exact lengths inside the
    /// footprint.
    Relocated,
    /// Its entry grows by whole sectors (relayout).
    Relayout,
    /// The disc already carries the translation.
    AlreadyApplied,
    /// Skipped: longer than its room and no growth path took it.
    OverBudget,
    /// Skipped: a longer name found no free run.
    NoFreeRun,
    /// Skipped: rolled back so its scene fits its footprint.
    RolledBack,
    /// Skipped: a growth path refused it (monster record bounds).
    Refused,
    /// Skipped: a character the retail glyph set lacks.
    NotEncodable,
    /// Skipped: the disc does not carry what the pack expects.
    Mismatch,
    /// Skipped for another reason (see `issue`).
    Skipped,
}

impl Outcome {
    /// The serialized name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Untranslated => "untranslated",
            Self::InPlace => "in_place",
            Self::Moved => "moved",
            Self::Grown => "grown",
            Self::Relocated => "relocated",
            Self::Relayout => "relayout",
            Self::AlreadyApplied => "already_applied",
            Self::OverBudget => "over_budget",
            Self::NoFreeRun => "no_free_run",
            Self::RolledBack => "rolled_back",
            Self::Refused => "refused",
            Self::NotEncodable => "not_encodable",
            Self::Mismatch => "mismatch",
            Self::Skipped => "skipped",
        }
    }

    /// `true` for an outcome that leaves the translation on the image.
    pub fn lands(self) -> bool {
        matches!(
            self,
            Self::InPlace
                | Self::Moved
                | Self::Grown
                | Self::Relocated
                | Self::Relayout
                | Self::AlreadyApplied
        )
    }

    fn from_issue(kind: IssueKind) -> Self {
        match kind {
            IssueKind::NotEncodable => Self::NotEncodable,
            IssueKind::OverBudget => Self::OverBudget,
            IssueKind::NoFreeRun => Self::NoFreeRun,
            IssueKind::RolledBack => Self::RolledBack,
            IssueKind::Refused => Self::Refused,
            IssueKind::Mismatch => Self::Mismatch,
            IssueKind::Other => Self::Skipped,
        }
    }
}

/// What an entry's `room` bounds (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomKind {
    /// A string that cannot move: `room` is the hard limit.
    StringFixed,
    /// A SCUS name: `room` in place, a free run past it.
    NameMovable,
    /// A fixed NUL-padded field.
    Field,
    /// A monster name: `room` in place, `cap` by growing the record.
    Monster,
    /// A walked dialog line: its span, and past it the carrier's footprint.
    DialogGrowable,
    /// A dialog line held to its own span.
    DialogFixed,
}

/// The whole report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceReport {
    /// [`SPACE_SCHEMA`].
    pub schema: String,
    /// The pack's language; `None` for a disc-only report.
    pub language: Option<String>,
    /// The dry run had the relayout enabled.
    pub relayout: bool,
    /// Totals.
    pub summary: SpaceSummary,
    /// SCUS name compaction regions.
    pub name_regions: Vec<NameRegion>,
    /// Every SCUS name-table string.
    pub names: Vec<NameSpace>,
    /// Every named monster record.
    pub monsters: Vec<MonsterSpace>,
    /// The fixed-room `ui_menu` / `system_text` pools.
    pub pools: Vec<PoolSpace>,
    /// Scene MANs carrying `man:` keys.
    pub scenes: Vec<SceneSpace>,
    /// Raw carriers carrying `raw:` keys.
    pub carriers: Vec<CarrierSpace>,
    /// One row per key.
    pub entries: Vec<EntrySpace>,
}

/// Report totals.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpaceSummary {
    /// Keys in the report.
    pub entries: usize,
    /// Keys the pack fills.
    pub filled: usize,
    /// Outcome -> filled-key count (pack only).
    pub outcomes: BTreeMap<String, usize>,
    /// Bytes across every name compaction region.
    pub name_bytes: usize,
    /// Free bytes the regions hold with English compacted.
    pub name_free_english: usize,
    /// Free bytes left after the pack's writes and moves.
    pub name_free_pack: Option<usize>,
    /// Scene MANs carrying keys.
    pub scenes: usize,
    /// Scenes with at least one rolled-back line.
    pub scenes_rolled_back: usize,
    /// Entries the dry run grew by a relayout.
    pub relayout_entries: usize,
    /// Sectors those grows add.
    pub relayout_sectors: u32,
}

/// One SCUS name compaction region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameRegion {
    /// Position in [`SpaceReport::name_regions`] (the `region:<i>` group).
    pub index: usize,
    /// VA of the first byte.
    pub start_va: u32,
    /// VA one past the last byte.
    pub end_va: u32,
    /// Bytes in the region.
    pub total: usize,
    /// Strings whose retail spans make it up.
    pub strings: usize,
    /// Bytes the English strings use, compacted.
    pub english_used: usize,
    /// Free run English leaves.
    pub english_free: usize,
    /// Bytes used after the pack (in-place writes, then moves).
    pub pack_used: Option<usize>,
    /// Free run left after the pack - what a further longer name can take.
    pub pack_free: Option<usize>,
}

/// One SCUS name-table string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameSpace {
    /// `scus:str:0x<va>`.
    pub key: String,
    /// String VA.
    pub va: u32,
    /// May move when it outgrows its room.
    pub movable: bool,
    /// Why not, when pinned.
    pub pin: Option<PinReason>,
    /// Compaction region (movable names).
    pub region: Option<usize>,
    /// Retail span: text, terminator and zero padding.
    pub span: Option<usize>,
    /// Where the pack's longer translation moved to.
    pub moved_to: Option<u32>,
}

/// One named monster record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterSpace {
    /// `mon:<id>`.
    pub key: String,
    /// Monster id.
    pub id: u16,
    /// In-place room (7, 11 or 15).
    pub room: usize,
    /// Longest name the record takes, growing it if needed.
    pub cap: usize,
    /// The growth ceiling every record shares
    /// ([`monster_names::RETAIL_LONGEST_NAME`]).
    pub longest: usize,
    /// Decoded block size on this disc.
    pub block_len: usize,
    /// Loader-kept head size on this disc.
    pub kept_len: usize,
    /// [`monster_names::RETAIL_MAX_BLOCK`].
    pub max_block: usize,
    /// [`monster_names::RETAIL_MAX_KEPT`].
    pub max_kept: usize,
}

/// One fixed-room string pool (`ui_menu` overlay window or `system_text`
/// SCUS window).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolSpace {
    /// Position in [`SpaceReport::pools`] (the `pool:<i>` group).
    pub index: usize,
    /// `ui_menu` or `system_text`.
    pub section: String,
    /// Pool label.
    pub label: String,
    /// Overlay PROT entry; `None` for `SCUS_942.54`.
    pub prot: Option<usize>,
    /// First VA of the window.
    pub va_start: u32,
    /// One past the last VA.
    pub va_end: u32,
    /// Window size in bytes.
    pub total: usize,
    /// Token-aware (strict) scan.
    pub strict: bool,
    /// Strings exported from the window.
    pub strings: usize,
    /// English bytes, terminators included.
    pub english_bytes: usize,
    /// Room bytes (each string's room plus its terminator).
    pub room_bytes: usize,
    /// `room_bytes - english_bytes`: the padding a longer translation can
    /// spend, spread one string at a time (strings never move).
    pub slack: usize,
    /// Bytes the pack's strings take (English where untranslated or
    /// skipped), terminators included.
    pub pack_bytes: Option<usize>,
}

/// One scene MAN.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSpace {
    /// PROT entry.
    pub prot: usize,
    /// CDNAME scene name.
    pub scene: Option<String>,
    /// Keyed lines (the export's, or the pack's for [`scene_fit`]).
    pub lines: usize,
    /// Bytes the compressed MAN may occupy at its LBA.
    pub footprint: usize,
    /// The compressed MAN on this disc.
    pub disc_len: usize,
    /// `footprint - disc_len`.
    pub slack: usize,
    /// Keys the pack fills here.
    pub filled: Option<usize>,
    /// How the dry run wrote the scene.
    pub path: Option<WritePath>,
    /// Compressed stream written.
    pub written_len: Option<usize>,
    /// Overflow with every line at its exact length.
    pub full_overflow: Option<usize>,
    /// Overflow of the padded same-size MAN.
    pub padded_overflow: Option<usize>,
    /// The relocator refused the scene.
    pub refused: bool,
    /// Keys rolled back to English.
    pub rolled_back: Vec<String>,
    /// Sectors the relayout grows the entry by.
    pub relayout_sectors: Option<u32>,
    /// Sectors `--allow-relayout` would add (relayout off, over-span lines).
    pub relayout_would_add: Option<u32>,
}

/// One raw carrier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarrierSpace {
    /// PROT entry.
    pub prot: usize,
    /// CDNAME scene name.
    pub scene: Option<String>,
    /// A streaming dungeon scene (its MAN can grow).
    pub streaming: bool,
    /// Keyed lines.
    pub lines: usize,
    /// True footprint in bytes.
    pub footprint: usize,
    /// Streaming: bytes a grown MAN takes before a relayout.
    pub sector_slack: Option<usize>,
    /// Streaming: the largest footprint a relayout may grow it to.
    pub max_footprint: Option<usize>,
    /// Keys the pack fills here.
    pub filled: Option<usize>,
    /// How the dry run wrote the entry.
    pub path: Option<WritePath>,
    /// Sectors the grown payload adds (0 = it fit the slack).
    pub grown_sectors: Option<u32>,
}

/// One key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntrySpace {
    /// The pack key.
    pub key: String,
    /// Pack section.
    pub section: String,
    /// What `room` bounds.
    pub room_kind: RoomKind,
    /// In-place room in encoded bytes.
    pub room: usize,
    /// What it shares room with (see the module docs).
    pub group: Option<String>,
    /// Encoded length of the English text.
    pub english_len: Option<usize>,
    /// Encoded length of the pack's translation.
    pub pack_len: Option<usize>,
    /// What the import does with it (pack only).
    pub outcome: Option<Outcome>,
    /// Class of its diagnostic.
    pub issue_kind: Option<IssueKind>,
    /// The importer's diagnostic.
    pub issue: Option<String>,
}

/// The byte space a key's text encodes into.
pub fn target_for_key(key: &str) -> Target {
    if key.starts_with("man:") || key.starts_with("raw:") {
        Target::Segment
    } else {
        Target::CString
    }
}

/// Encoded byte length of `text` for `target`, or every per-character
/// problem at once - the call a live editor makes per keystroke
/// ([`markup::encode`]; import uses the same).
pub fn encoded_len(text: &str, target: Target) -> Result<usize, Vec<EncodeIssue>> {
    markup::encode(text, target).map(|b| b.len())
}

fn section_of_pool(pool: &ui::UiStringPool) -> &'static str {
    if pool.prot_index == usize::MAX {
        "system_text"
    } else {
        "ui_menu"
    }
}

/// Every fixed-room pool in report order: the overlay pools, then the SCUS
/// windows.
fn all_pools() -> impl Iterator<Item = &'static ui::UiStringPool> {
    ui::UI_STRING_POOLS.iter().chain(ui::SCUS_STRING_POOLS)
}

fn pool_index(key: &Key) -> Option<usize> {
    let (prot, va) = match *key {
        Key::Ui { prot, va } => (prot, va),
        Key::ScusStr { va } => (usize::MAX, va),
        _ => return None,
    };
    all_pools().position(|p| p.prot_index == prot && (p.va_start..p.va_end).contains(&va))
}

fn scene_name(cdname: &Option<legaia_prot::cdname::IndexMap>, idx: usize) -> Option<String> {
    cdname
        .as_ref()
        .and_then(|m| legaia_prot::cdname::block_for_extraction_index(m, idx as u32))
        .map(str::to_string)
}

/// The importer's outcome for a filled `key`, from its report.
struct Outcomes<'a> {
    report: &'a ImportReport,
    applied: HashSet<&'a str>,
    already: HashSet<&'a str>,
    issues: HashMap<&'a str, &'a str>,
    /// Keys staged for a relayout.
    relayout: HashSet<&'a str>,
}

impl<'a> Outcomes<'a> {
    fn new(report: &'a ImportReport, relayout_keys: HashSet<&'a str>) -> Self {
        Self {
            report,
            applied: report.applied_keys.iter().map(String::as_str).collect(),
            already: report.already_keys.iter().map(String::as_str).collect(),
            issues: report
                .issues
                .iter()
                .map(|(k, m)| (k.as_str(), m.as_str()))
                .collect(),
            relayout: relayout_keys,
        }
    }

    fn of(&self, key: &str, filled: bool) -> Outcome {
        let t = &self.report.trace;
        if !filled {
            return Outcome::Untranslated;
        }
        if self.already.contains(key) {
            return Outcome::AlreadyApplied;
        }
        if self.applied.contains(key) || self.relayout.contains(key) {
            if t.moved.contains_key(key) {
                return Outcome::Moved;
            }
            if t.grown_monsters.contains(key) {
                return Outcome::Grown;
            }
            let path = match parse_key(key) {
                Some(Key::Man { entry, .. }) => t.scenes.get(&entry).map(|s| s.path),
                Some(Key::Raw { entry, .. }) => t.carriers.get(&entry).map(|c| c.path),
                _ => None,
            };
            return match path {
                Some(WritePath::Relocated) => Outcome::Relocated,
                Some(WritePath::Relayout) => Outcome::Relayout,
                _ => Outcome::InPlace,
            };
        }
        match t.issue_kinds.get(key) {
            Some(k) => Outcome::from_issue(*k),
            None => Outcome::Skipped,
        }
    }

    fn issue(&self, key: &str) -> Option<String> {
        self.issues.get(key).map(|m| m.to_string())
    }
}

/// Encoded length of an export entry's English source.
fn english_len(e: &Entry) -> Option<usize> {
    if e.source.is_empty() {
        return None;
    }
    markup::encode(&e.source, target_for_key(&e.key))
        .ok()
        .map(|b| b.len())
}

/// Build the space report for `patcher`'s disc, and - with `pack` - for that
/// pack imported onto a scratch copy of it (the disc itself is never
/// written). See the module docs for the schema.
pub fn space_report(
    patcher: &DiscPatcher,
    pack: Option<&LanguagePack>,
    opts: SpaceOptions,
) -> Result<SpaceReport> {
    let english = export_pack(patcher)?;
    let dry = match pack {
        Some(p) => {
            let mut scratch = DiscPatcher::open(patcher.image().to_vec())
                .context("open a scratch copy of the disc")?;
            Some(import_pack_phase(
                &mut scratch,
                p,
                ImportPhase::All,
                opts.relayout,
            )?)
        }
        None => None,
    };
    build_report(patcher, &english, pack, dry.as_ref(), opts.relayout)
}

fn build_report(
    patcher: &DiscPatcher,
    english: &LanguagePack,
    pack: Option<&LanguagePack>,
    dry: Option<&ImportReport>,
    relayout: bool,
) -> Result<SpaceReport> {
    let cdname = patcher.cdname();
    // Keys staged for (and landed by) a relayout: the relayout scenes' and
    // carriers' applied keys already sit in `applied_keys`.
    let outcomes = dry.map(|r| Outcomes::new(r, HashSet::new()));
    let filled: BTreeMap<&str, &Entry> = pack
        .map(|p| {
            p.sections
                .iter()
                .flat_map(|(_, es)| es)
                .filter(|e| e.is_filled())
                .map(|e| (e.key.as_str(), e))
                .collect()
        })
        .unwrap_or_default();

    // --- SCUS names -------------------------------------------------------
    let scus = patcher
        .read_named_file("SCUS_942.54")
        .context("SCUS_942.54 not found in disc image")?;
    let pool = NamePool::build(&scus);
    let english_layout = pool.layout(&scus, &BTreeMap::new());
    let pack_layout: Option<&Layout> = dry.and_then(|r| r.trace.name_layout.as_ref());
    let name_regions: Vec<NameRegion> = english_layout
        .regions
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let after = pack_layout.and_then(|l| l.regions.get(i));
            NameRegion {
                index: i,
                start_va: r.start_va,
                end_va: r.end_va,
                total: r.total(),
                strings: r.vas.len(),
                english_used: r.used,
                english_free: r.free,
                pack_used: after.map(|a| a.used).or(dry.map(|_| r.used)),
                pack_free: after.map(|a| a.free).or(dry.map(|_| r.free)),
            }
        })
        .collect();
    let mut names = Vec::new();
    let mut name_group: HashMap<String, Option<usize>> = HashMap::new();
    for (section, entries) in english.sections.iter() {
        if !NAME_SECTIONS.contains(&section) {
            continue;
        }
        for e in entries {
            let Some(Key::ScusStr { va }) = parse_key(&e.key) else {
                continue;
            };
            let pin = pool.pin_reason(va);
            let region = english_layout.region_of(va);
            name_group.insert(e.key.clone(), region);
            names.push(NameSpace {
                key: e.key.clone(),
                va,
                movable: pin.is_none(),
                pin,
                region,
                span: pool.span(va).map(|(_, len)| len),
                moved_to: dry.and_then(|r| r.trace.moved.get(&e.key).map(|m| m.1)),
            });
        }
    }

    // --- Monsters -----------------------------------------------------------
    let mut monsters = Vec::new();
    if let Ok(archive) = patcher.read_entry(crate::disc::MONSTER_ARCHIVE_ENTRY) {
        let fields = monster_names::fields(&archive);
        for ((id, f), (_, room)) in fields.iter().zip(monster_names::budgets(&fields)) {
            let Ok(Some(block)) = legaia_asset::monster_archive::decode_block(&archive, *id) else {
                continue;
            };
            let kept = block
                .get(8..12)
                .map_or(0, |w| u32::from_le_bytes(w.try_into().unwrap()) as usize);
            // The longest name `rewrite_slot` accepts: in the record's own
            // room, or by `grow_block` (monotone in the length, so the first
            // length that works counting down is the cap).
            let longest = monster_names::RETAIL_LONGEST_NAME;
            let cap = (1..=longest)
                .rev()
                .find(|&l| l <= f.room || monster_names::grow_block(&block, f, l).is_ok())
                .unwrap_or(0);
            monsters.push(MonsterSpace {
                key: format!("mon:{id}"),
                id: *id,
                room,
                cap,
                longest,
                block_len: block.len(),
                kept_len: kept,
                max_block: monster_names::RETAIL_MAX_BLOCK,
                max_kept: monster_names::RETAIL_MAX_KEPT,
            });
        }
    }

    // --- Flat index (built alongside the pools / scenes / carriers) ---------
    let mut entries: Vec<EntrySpace> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut scene_lines: BTreeMap<usize, usize> = BTreeMap::new();
    let mut carrier_lines: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut decoded_cache: HashMap<usize, Option<SceneManText>> = HashMap::new();
    let mut stream_cache: HashMap<usize, Option<StreamManText>> = HashMap::new();
    let mut push = |section: &str, key: &str, room_default: usize, eng: Option<usize>| {
        let parsed = parse_key(key);
        let t = dry.map(|r| &r.trace);
        let room = t
            .and_then(|t| t.rooms.get(key).copied())
            .unwrap_or(room_default);
        let (room_kind, group) = match &parsed {
            Some(Key::ScusStr { va }) if NAME_SECTIONS.contains(&section) => {
                match name_group.get(key).copied().flatten() {
                    Some(r) if pool.is_movable(*va) => {
                        (RoomKind::NameMovable, Some(format!("region:{r}")))
                    }
                    _ => (RoomKind::StringFixed, None),
                }
            }
            Some(k @ (Key::ScusStr { .. } | Key::Ui { .. })) => (
                RoomKind::StringFixed,
                pool_index(k).map(|i| format!("pool:{i}")),
            ),
            Some(Key::ScusParty { .. }) => (RoomKind::Field, Some("party".to_string())),
            Some(Key::ScusCell { .. }) => (RoomKind::Field, Some("place".to_string())),
            Some(Key::Mon { .. }) => (RoomKind::Monster, Some("monster".to_string())),
            Some(Key::Man { entry, off }) => {
                *scene_lines.entry(*entry).or_default() += 1;
                let man = decoded_cache.entry(*entry).or_insert_with(|| {
                    patcher
                        .read_entry(*entry)
                        .ok()
                        .and_then(|b| SceneManText::locate(&b))
                });
                let walked = man
                    .as_ref()
                    .is_some_and(|m| man_edit::text_site(&m.decoded, *off) == TextSite::Segment);
                (
                    if walked {
                        RoomKind::DialogGrowable
                    } else {
                        RoomKind::DialogFixed
                    },
                    Some(format!("scene:{entry}")),
                )
            }
            Some(Key::Raw { entry, off }) => {
                carrier_lines.entry(*entry).or_default().push(*off);
                let sm = stream_cache.entry(*entry).or_insert_with(|| {
                    patcher
                        .read_entry(*entry)
                        .ok()
                        .and_then(|b| StreamManText::locate(&b))
                });
                let walked = sm.as_ref().is_some_and(|sm| {
                    off.checked_sub(sm.man_range().start)
                        .is_some_and(|o| man_edit::text_site(&sm.man, o) == TextSite::Segment)
                });
                (
                    if walked {
                        RoomKind::DialogGrowable
                    } else {
                        RoomKind::DialogFixed
                    },
                    Some(format!("carrier:{entry}")),
                )
            }
            None => (RoomKind::StringFixed, None),
        };
        let fe = filled.get(key).copied();
        entries.push(EntrySpace {
            key: key.to_string(),
            section: section.to_string(),
            room_kind,
            room,
            group,
            english_len: eng,
            pack_len: t.and_then(|t| t.encoded.get(key).copied()),
            outcome: outcomes.as_ref().map(|o| o.of(key, fe.is_some())),
            issue_kind: t.and_then(|t| t.issue_kinds.get(key).copied()),
            issue: outcomes.as_ref().and_then(|o| o.issue(key)),
        });
    };
    for (section, es) in english.sections.iter() {
        for e in es {
            if seen.insert(e.key.clone()) {
                push(section, &e.key, e.budget, english_len(e));
            }
        }
    }
    // Pack keys the disc's export does not carry (a stale or foreign key).
    if let Some(p) = pack {
        for (section, es) in p.sections.iter() {
            for e in es {
                if seen.insert(e.key.clone()) {
                    push(section, &e.key, e.budget, None);
                }
            }
        }
    }

    // --- Fixed-room pools ----------------------------------------------------
    let mut pools: Vec<PoolSpace> = all_pools()
        .enumerate()
        .map(|(i, p)| PoolSpace {
            index: i,
            section: section_of_pool(p).to_string(),
            label: p.label.to_string(),
            prot: (p.prot_index != usize::MAX).then_some(p.prot_index),
            va_start: p.va_start,
            va_end: p.va_end,
            total: (p.va_end - p.va_start) as usize,
            strict: p.strict,
            strings: 0,
            english_bytes: 0,
            room_bytes: 0,
            slack: 0,
            pack_bytes: dry.map(|_| 0),
        })
        .collect();
    for e in &entries {
        let Some(i) = e
            .group
            .as_deref()
            .and_then(|g| g.strip_prefix("pool:"))
            .and_then(|i| i.parse::<usize>().ok())
        else {
            continue;
        };
        let p = &mut pools[i];
        let eng = e.english_len.unwrap_or(e.room);
        p.strings += 1;
        p.english_bytes += eng + 1;
        p.room_bytes += e.room + 1;
        if let Some(pb) = p.pack_bytes.as_mut() {
            let lands = e
                .outcome
                .is_some_and(|o| o.lands() && o != Outcome::AlreadyApplied);
            *pb += if lands {
                e.pack_len.unwrap_or(eng)
            } else {
                eng
            } + 1;
        }
    }
    for p in &mut pools {
        p.slack = p.room_bytes.saturating_sub(p.english_bytes);
    }

    // --- Scenes ----------------------------------------------------------------
    let filled_in = |prefix: &str, prot: usize| -> usize {
        filled
            .keys()
            .filter(|k| {
                k.strip_prefix(prefix)
                    .and_then(|r| r.split(':').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    == Some(prot)
            })
            .count()
    };
    let mut scenes = Vec::new();
    for (&prot, &lines) in &scene_lines {
        let Some(Some(man)) = decoded_cache.get(&prot) else {
            continue;
        };
        let t = dry.and_then(|r| r.trace.scenes.get(&prot));
        scenes.push(SceneSpace {
            prot,
            scene: scene_name(&cdname, prot),
            lines,
            footprint: man.compressed_budget,
            disc_len: man.compressed_len,
            slack: man.compressed_budget.saturating_sub(man.compressed_len),
            filled: dry.map(|_| filled_in("man:", prot)),
            path: dry.map(|_| t.map_or(WritePath::None, |t| t.path)),
            written_len: t.and_then(|t| t.written_len),
            full_overflow: t.and_then(|t| t.full_overflow),
            padded_overflow: t.and_then(|t| t.padded_overflow),
            refused: t.is_some_and(|t| t.refused),
            rolled_back: t.map(|t| t.rolled_back.clone()).unwrap_or_default(),
            relayout_sectors: t.and_then(|t| t.relayout_sectors),
            relayout_would_add: t.and_then(|t| t.relayout_would_add),
        });
    }

    // --- Raw carriers ------------------------------------------------------------
    let mut carriers = Vec::new();
    for (&prot, offs) in &carrier_lines {
        let sm = stream_cache.get(&prot).and_then(Option::as_ref);
        let footprint = patcher
            .entry_true_footprint_sectors(prot)
            .map_or(0, |n| n as usize * 2048);
        let sector_slack = sm.and_then(|sm| {
            patcher
                .read_entry_footprint(prot)
                .ok()
                .map(|foot| sm.sector_slack(&foot))
        });
        let t = dry.and_then(|r| r.trace.carriers.get(&prot));
        carriers.push(CarrierSpace {
            prot,
            scene: scene_name(&cdname, prot),
            streaming: sm.is_some(),
            lines: offs.len(),
            footprint,
            sector_slack,
            max_footprint: sm.map(|_| MAX_GROWN_FOOTPRINT),
            filled: dry.map(|_| filled_in("raw:", prot)),
            path: dry.map(|_| t.map_or(WritePath::None, |t| t.path)),
            grown_sectors: t.and_then(|t| t.grown_sectors),
        });
    }

    // --- Summary -------------------------------------------------------------------
    let mut summary = SpaceSummary {
        entries: entries.len(),
        filled: filled.len(),
        name_bytes: name_regions.iter().map(|r| r.total).sum(),
        name_free_english: name_regions.iter().map(|r| r.english_free).sum(),
        name_free_pack: dry.map(|_| name_regions.iter().filter_map(|r| r.pack_free).sum()),
        scenes: scenes.len(),
        scenes_rolled_back: scenes.iter().filter(|s| !s.rolled_back.is_empty()).count(),
        relayout_entries: dry.map_or(0, |r| r.relayout_entries),
        relayout_sectors: dry.map_or(0, |r| r.relayout_sectors_added),
        ..Default::default()
    };
    if dry.is_some() {
        for e in &entries {
            if let Some(o) = e.outcome.filter(|o| *o != Outcome::Untranslated) {
                *summary.outcomes.entry(o.as_str().to_string()).or_default() += 1;
            }
        }
    }

    Ok(SpaceReport {
        schema: SPACE_SCHEMA.to_string(),
        language: pack.map(|p| p.language.clone()),
        relayout,
        summary,
        name_regions,
        names,
        monsters,
        pools,
        scenes,
        carriers,
        entries,
    })
}

impl SpaceReport {
    /// Keep only the rows of pack section `section`: the per-area lists are
    /// narrowed to the areas that section uses, and the summary's per-key
    /// counts (`entries`, `filled`, `outcomes`) are recounted over it (the
    /// byte totals stay whole-disc).
    pub fn retain_section(&mut self, section: &str) {
        self.entries.retain(|e| e.section == section);
        let s = &mut self.summary;
        s.entries = self.entries.len();
        s.filled = self
            .entries
            .iter()
            .filter(|e| e.outcome.is_some_and(|o| o != Outcome::Untranslated))
            .count();
        s.outcomes.clear();
        for o in self.entries.iter().filter_map(|e| e.outcome) {
            if o != Outcome::Untranslated {
                *s.outcomes.entry(o.as_str().to_string()).or_default() += 1;
            }
        }
        let keys: HashSet<&str> = self.entries.iter().map(|e| e.key.as_str()).collect();
        self.names.retain(|n| keys.contains(n.key.as_str()));
        self.monsters.retain(|m| keys.contains(m.key.as_str()));
        let groups: HashSet<&str> = self
            .entries
            .iter()
            .filter_map(|e| e.group.as_deref())
            .collect();
        self.name_regions
            .retain(|r| groups.contains(format!("region:{}", r.index).as_str()));
        self.pools
            .retain(|p| groups.contains(format!("pool:{}", p.index).as_str()));
        self.scenes
            .retain(|s| groups.contains(format!("scene:{}", s.prot).as_str()));
        self.carriers
            .retain(|c| groups.contains(format!("carrier:{}", c.prot).as_str()));
    }

    /// The row for `key`.
    pub fn entry(&self, key: &str) -> Option<&EntrySpace> {
        self.entries.iter().find(|e| e.key == key)
    }
}

/// [`scene_fit`]'s answer: the scene's row and one row per pack key in it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneFit {
    /// The scene (its `lines` = the pack's keys for it).
    pub scene: SceneSpace,
    /// One row per pack key in the scene (`english_len` is `None`: no
    /// export runs).
    pub entries: Vec<EntrySpace>,
}

/// Re-plan scene MAN `prot` with `pack`'s lines for it through the
/// importer's scene planner (`import::plan_scene_man`): nothing is written,
/// and no other scene or section is touched. The fast path for a live
/// editor; it costs one decompress, the relocator, and one or more
/// recompressions of this scene only.
pub fn scene_fit(
    patcher: &DiscPatcher,
    pack: &LanguagePack,
    prot: usize,
    relayout: bool,
) -> SceneFit {
    let mut keyed: Vec<(&str, &Entry)> = Vec::new();
    let mut edits: Vec<(usize, &Entry)> = Vec::new();
    for (section, es) in pack.sections.iter() {
        for e in es {
            if let Some(Key::Man { entry, off }) = parse_key(&e.key)
                && entry == prot
            {
                keyed.push((section, e));
                if e.is_filled() {
                    edits.push((off, e));
                }
            }
        }
    }
    let mut report = ImportReport::default();
    let plan = plan_scene_man(patcher, prot, &edits, relayout, &mut report);
    let staged: HashSet<&str> = plan
        .growth
        .as_ref()
        .map(|(_, keys, _)| keys.iter().map(String::as_str).collect())
        .unwrap_or_default();
    report.applied_keys = plan.applied.clone();
    let outcomes = Outcomes::new(&report, staged);
    let man = patcher
        .read_entry(prot)
        .ok()
        .and_then(|b| SceneManText::locate(&b));
    let t = report.trace.scenes.get(&prot);
    let entries = keyed
        .iter()
        .map(|(section, e)| {
            let walked = man.as_ref().is_some_and(|m| {
                matches!(parse_key(&e.key), Some(Key::Man { off, .. })
                    if man_edit::text_site(&m.decoded, off) == TextSite::Segment)
            });
            EntrySpace {
                key: e.key.clone(),
                section: section.to_string(),
                room_kind: if walked {
                    RoomKind::DialogGrowable
                } else {
                    RoomKind::DialogFixed
                },
                room: report.trace.rooms.get(&e.key).copied().unwrap_or(e.budget),
                group: Some(format!("scene:{prot}")),
                english_len: None,
                pack_len: report.trace.encoded.get(&e.key).copied(),
                outcome: Some(outcomes.of(&e.key, e.is_filled())),
                issue_kind: report.trace.issue_kinds.get(&e.key).copied(),
                issue: outcomes.issue(&e.key),
            }
        })
        .collect();
    let scene = SceneSpace {
        prot,
        scene: scene_name(&patcher.cdname(), prot),
        lines: keyed.len(),
        footprint: man.as_ref().map_or(0, |m| m.compressed_budget),
        disc_len: man.as_ref().map_or(0, |m| m.compressed_len),
        slack: man
            .as_ref()
            .map_or(0, |m| m.compressed_budget.saturating_sub(m.compressed_len)),
        filled: Some(edits.len()),
        path: Some(t.map_or(WritePath::None, |t| t.path)),
        written_len: t.and_then(|t| t.written_len),
        full_overflow: t.and_then(|t| t.full_overflow),
        padded_overflow: t.and_then(|t| t.padded_overflow),
        refused: t.is_some_and(|t| t.refused),
        rolled_back: t.map(|t| t.rolled_back.clone()).unwrap_or_default(),
        relayout_sectors: t.and_then(|t| t.relayout_sectors),
        relayout_would_add: t.and_then(|t| t.relayout_would_add),
    };
    SceneFit { scene, entries }
}

/// The importer's SCUS pass, reusable per keystroke: holds the retail
/// executable and its [`NamePool`] (the expensive part to build), and runs
/// the import's own SCUS planner over a copy for each [`Self::fit`].
pub struct NameFitter {
    scus: Vec<u8>,
    pool: NamePool,
}

/// [`NameFitter::fit`]'s answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamesFit {
    /// Every compaction region after the pack (same indices as
    /// [`SpaceReport::name_regions`]).
    pub regions: Vec<NameRegion>,
    /// One row per filled SCUS key (`scus:str` / `scus:party` /
    /// `scus:cell`): `room`, `pack_len`, `outcome` and `issue`; `group` and
    /// `english_len` are not filled in.
    pub entries: Vec<EntrySpace>,
}

impl NameFitter {
    /// Read the disc's executable and measure its pools.
    pub fn new(patcher: &DiscPatcher) -> Result<Self> {
        let scus = patcher
            .read_named_file("SCUS_942.54")
            .context("SCUS_942.54 not found in disc image")?;
        Ok(Self::from_scus(scus))
    }

    /// Measure the pools of a retail executable.
    pub fn from_scus(scus: Vec<u8>) -> Self {
        let pool = NamePool::build(&scus);
        Self { scus, pool }
    }

    /// Plan every filled SCUS entry of `pack` exactly as import does, on a
    /// copy of the executable.
    pub fn fit(&self, pack: &LanguagePack) -> NamesFit {
        let english = self.pool.layout(&self.scus, &BTreeMap::new());
        let mut work: Vec<&Entry> = Vec::new();
        let mut sections: HashMap<&str, &str> = HashMap::new();
        for (section, es) in pack.sections.iter() {
            for e in es.iter().filter(|e| e.is_filled()) {
                if matches!(
                    parse_key(&e.key),
                    Some(Key::ScusStr { .. } | Key::ScusParty { .. } | Key::ScusCell { .. })
                ) {
                    work.push(e);
                    sections.insert(&e.key, section);
                }
            }
        }
        let mut scus = self.scus.clone();
        let mut report = ImportReport::default();
        super::import::apply_scus_work(&mut scus, &self.pool, &work, &mut report);
        let outcomes = Outcomes::new(&report, HashSet::new());
        let after = report.trace.name_layout.as_ref();
        let regions = english
            .regions
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let a = after.and_then(|l| l.regions.get(i));
                NameRegion {
                    index: i,
                    start_va: r.start_va,
                    end_va: r.end_va,
                    total: r.total(),
                    strings: r.vas.len(),
                    english_used: r.used,
                    english_free: r.free,
                    pack_used: Some(a.map_or(r.used, |a| a.used)),
                    pack_free: Some(a.map_or(r.free, |a| a.free)),
                }
            })
            .collect();
        let entries = work
            .iter()
            .map(|e| {
                let movable = matches!(parse_key(&e.key), Some(Key::ScusStr { va }) if self.pool.is_movable(va));
                EntrySpace {
                    key: e.key.clone(),
                    section: sections[e.key.as_str()].to_string(),
                    room_kind: match parse_key(&e.key) {
                        Some(Key::ScusStr { .. }) if movable => RoomKind::NameMovable,
                        Some(Key::ScusStr { .. }) => RoomKind::StringFixed,
                        _ => RoomKind::Field,
                    },
                    room: report.trace.rooms.get(&e.key).copied().unwrap_or(e.budget),
                    group: None,
                    english_len: None,
                    pack_len: report.trace.encoded.get(&e.key).copied(),
                    outcome: Some(outcomes.of(&e.key, true)),
                    issue_kind: report.trace.issue_kinds.get(&e.key).copied(),
                    issue: outcomes.issue(&e.key),
                }
            })
            .collect();
        NamesFit { regions, entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_follow_the_key_shape() {
        assert_eq!(target_for_key("man:10:0x40"), Target::Segment);
        assert_eq!(target_for_key("raw:10:0x40"), Target::Segment);
        assert_eq!(target_for_key("scus:str:0x80012260"), Target::CString);
        assert_eq!(target_for_key("ui:899:0x801ce81c"), Target::CString);
        assert_eq!(target_for_key("mon:3"), Target::CString);
    }

    #[test]
    fn encoded_len_counts_tokens_as_their_bytes() {
        assert_eq!(encoded_len("Abc", Target::CString), Ok(3));
        assert_eq!(encoded_len("{c2:79}x", Target::Segment), Ok(3));
        let err = encoded_len("\u{00e9}t\u{00e9}", Target::Segment).unwrap_err();
        assert_eq!(err.len(), 2, "every bad character reported at once");
    }

    #[test]
    fn outcomes_serialize_as_their_names() {
        for o in [
            Outcome::InPlace,
            Outcome::NoFreeRun,
            Outcome::AlreadyApplied,
        ] {
            let json = serde_json::to_string(&o).unwrap();
            assert_eq!(json, format!("\"{}\"", o.as_str()));
        }
        assert!(Outcome::Moved.lands() && !Outcome::RolledBack.lands());
    }

    #[test]
    fn pool_index_finds_overlay_and_scus_windows() {
        let first = &ui::UI_STRING_POOLS[0];
        assert_eq!(
            pool_index(&Key::Ui {
                prot: first.prot_index,
                va: first.va_start
            }),
            Some(0)
        );
        let s0 = &ui::SCUS_STRING_POOLS[0];
        assert_eq!(
            pool_index(&Key::ScusStr { va: s0.va_start }),
            Some(ui::UI_STRING_POOLS.len())
        );
        assert_eq!(pool_index(&Key::ScusStr { va: 0x8000_0000 }), None);
    }
}
