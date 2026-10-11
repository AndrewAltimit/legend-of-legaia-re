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

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use legaia_disc_patch::disc::DiscPatcher;

use super::export::export_pack;
use super::import::{ImportPhase, ImportReport, Key, WritePath, import_pack_phase, parse_key};
use super::markup::{self, EncodeIssue, Target};
use super::pack::{Entry, LanguagePack};
use super::ui;

mod builder;
mod fitters;
mod report;

use builder::build_report;
pub use fitters::{NameFitter, NamesFit, SceneFit, scene_fit};
pub use report::{
    CarrierSpace, CategorySpace, CodeImageSpace, EntrySpace, MonsterSpace, NameRegion, NameSpace,
    Outcome, PoolSpace, RoomKind, SPACE_SCHEMA, SceneSpace, SpaceOptions, SpaceReport,
    SpaceSummary, SpareSpace,
};

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
    space_report_with_export(patcher, &english, pack, opts)
}

/// [`space_report`] with the disc's export already in hand (`english` must
/// be [`export_pack`] of `patcher`, untouched): a long-lived editor that
/// holds the export skips the second whole-disc walk.
pub fn space_report_with_export(
    patcher: &DiscPatcher,
    english: &LanguagePack,
    pack: Option<&LanguagePack>,
    opts: SpaceOptions,
) -> Result<SpaceReport> {
    let characters = match pack {
        Some(p) => {
            let font = super::accents::DiscFont::read(patcher).ok();
            let mode = super::accents::AccentMode::of(p);
            super::accents::pack_notes(
                p,
                font.as_ref().map(|f| f as &dyn super::accents::DrawLookup),
            )
            .into_iter()
            .filter(|n| super::accents::is_undrawable(&n.note, mode))
            .collect()
        }
        None => Vec::new(),
    };
    let prepared = pack.map(super::accents::prepared);
    let pack = prepared.as_ref().map(|(p, _)| &**p);
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
    let mut report = build_report(patcher, english, pack, dry.as_ref(), opts.relayout)?;
    report.characters = characters;
    Ok(report)
}

#[cfg(test)]
mod tests;
