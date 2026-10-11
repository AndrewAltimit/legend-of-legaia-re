//! The space report's data model: the schema version, options, per-string
//! outcome, and the summary / category / code-image / name / pool / scene /
//! carrier rows.

use super::super::import::{IssueKind, WritePath};
use super::super::name_pool::PinReason;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// Schema tag of a serialized [`SpaceReport`].
pub const SPACE_SCHEMA: &str = "legaia-space-v1";

/// Pack sections whose strings are SCUS name-table strings.
pub(super) const NAME_SECTIONS: [&str; 5] = [
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

    pub(super) fn from_issue(kind: IssueKind) -> Self {
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
    /// A `ui_menu` / `system_text` string that can move with its references
    /// ([`super::super::code_strings`]): `room` in place, past it the free bytes of
    /// its image (see `code_images`).
    StringMovable,
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
    /// Every character of the pack that will not draw as typed under its
    /// accent mode on this disc (pack only), with its key and index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub characters: Vec<super::super::accents::KeyedNote>,
    /// Capacity per translatable category - where its strings live, how
    /// they are reached, the room they have and what else wants it.
    #[serde(default)]
    pub categories: Vec<CategorySpace>,
    /// Per code image (the executable and each overlay the `ui_menu` pools
    /// cover): how many strings can move and the room they can move into.
    #[serde(default)]
    pub code_images: Vec<CodeImageSpace>,
    /// Every spare region of the [space ledger](legaia_disc_patch::space_ledger), with
    /// its owner and how much of it is still zero on this disc.
    #[serde(default)]
    pub spare_regions: Vec<SpareSpace>,
}

/// One translatable category's capacity ([`SpaceReport::categories`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySpace {
    /// `items`, `monster_names`, `place_names`, `ui_menu`, `system_text`.
    pub category: String,
    /// The pack sections it covers.
    pub sections: Vec<String>,
    /// Where the strings live.
    pub carrier: String,
    /// How the game reaches a string.
    pub addressing: String,
    /// Strings on the disc.
    pub strings: usize,
    /// Strings that can take more than their own room (move or grow).
    pub growable: usize,
    /// Bytes English uses (each string plus its terminator).
    pub english_bytes: usize,
    /// Bytes of in-place room (each string's span plus its terminator).
    pub room_bytes: usize,
    /// Room a longer string can take beyond its own, with English: the
    /// free runs compaction leaves, or a record's growth headroom.
    pub free_english: usize,
    /// Spare bytes the space ledger reserves for this category.
    pub spare_bytes: usize,
    /// `free_english + spare_bytes` after the pack (pack only).
    pub free_pack: Option<usize>,
    /// What happens to a string longer than its room.
    pub growth: String,
    /// Other writers of the same room.
    pub competing: Vec<String>,
}

/// One code image's movable strings ([`SpaceReport::code_images`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeImageSpace {
    /// `SCUS_942.54` or `PROT nnnn`.
    pub image: String,
    /// The overlay's PROT entry (`None` for the executable).
    pub prot: Option<usize>,
    /// `ui_menu` or `system_text`.
    pub section: String,
    /// Strings its pools hold.
    pub strings: usize,
    /// Strings that can move.
    pub movable: usize,
    /// Pinned strings per [`super::super::code_strings::CodePin`] reason.
    pub pinned: BTreeMap<String, usize>,
    /// Free bytes the compaction of its pools leaves with English.
    pub english_free: usize,
    /// Spare bytes a moved string can take outside its pools: the ledger's
    /// translation region, or (executable) the name pools' free runs.
    pub spare: usize,
    /// Free bytes (compaction + spare) left after the pack (pack only).
    pub pack_free: Option<usize>,
}

/// One space-ledger region ([`SpaceReport::spare_regions`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpareSpace {
    /// `SCUS_942.54` or `PROT nnnn`.
    pub image: String,
    /// First VA.
    pub start_va: u32,
    /// One past the last VA.
    pub end_va: u32,
    /// `translation` or `mods`.
    pub owner: String,
    /// The `randomize` flags that write it (mod regions).
    pub mods: Vec<String>,
    /// Bytes still zero on this disc (4-aligned runs).
    pub zero_on_disc: usize,
    /// What it is.
    pub why: String,
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

impl SpaceReport {
    /// Keep only the rows of pack section `section`: the per-area lists are
    /// narrowed to the areas that section uses, and the summary's per-key
    /// counts (`entries`, `filled`, `outcomes`) are recounted over it (the
    /// byte totals stay whole-disc).
    pub fn retain_section(&mut self, section: &str) {
        self.entries.retain(|e| e.section == section);
        self.characters.retain(|n| n.section == section);
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
        self.categories
            .retain(|c| c.sections.iter().any(|x| x == section));
        self.code_images.retain(|c| c.section == section);
    }

    /// The row for `key`.
    pub fn entry(&self, key: &str) -> Option<&EntrySpace> {
        self.entries.iter().find(|e| e.key == key)
    }
}
