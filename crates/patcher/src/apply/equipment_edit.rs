//! Manual equipment editor: per-(character, item) **command cost** edits for
//! the four Arts-bar directions, and per-item **equip owner** edits.
//!
//! Two disc tables, both documented in
//! [`docs/subsystems/arts-command-gauge.md`](../../../../docs/subsystems/arts-command-gauge.md):
//!
//! - **Command cost** - the AP one press of a direction command charges on
//!   the Arts gauge, which is also the pennant width (`cost - 6` px). Each of
//!   the four commands is priced by an authored byte inside an equipment
//!   section of that character's player battle file (`record + 0x74` of the
//!   swing record the section's `+0x04` word points at): section 2 fills
//!   Left, section 3 Right, and the footwear section (4) fills Down from its
//!   `+0x04` record and Up from a second record at `+0x08`. Vahn's and Gala's
//!   files carry the weapons in section 2 and the Ra-Seru arm in 3; Noa's file
//!   is the other way round, so her weapon prices Right. The same weapon
//!   carries a different cost per character (favored `0x1E`, off-class
//!   `0x2A`, far `0x36`; the Astral Sword is Vahn's one `0x36`); every
//!   Ra-Seru and footwear record ships at `0x1E`. An edit decompresses the
//!   section, rewrites the byte, and recompresses in place - the same path the
//!   [`crate::weapon_specialty`] randomizer takes, but to a value the modder
//!   chooses, on any of the four commands.
//! - **Equip owner** - the `+6` character mask of the item's row in the SCUS
//!   equipment stat-bonus table (`DAT_80074F68`; bit `1` Vahn, `2` Noa,
//!   `4` Gala). The equip screen gates on it (`FUN_8003fb10`), so this changes
//!   who may equip the item - any slot: weapon, body, head, footwear. It does
//!   **not** add a battle model or swing record to a file that lacks one: a
//!   character whose player file has no section for the item falls through to
//!   that section's default record at battle load (default appearance; for a
//!   weapon or footwear also the default record's own cost). The report names
//!   those combinations so the page can say so, and every section default is
//!   itself addressable ([`CostTarget::Default`]: `Vahn:default=30`,
//!   `Noa:raseru=30`, `Gala:feet:up=30`) so the fall-through price can be
//!   set - shared by every unlisted item of that slot the character equips,
//!   and by the unarmed / barefoot swing.
//!
//! Both edits are same-size, in-place, and idempotent.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use legaia_asset::battle_data_pack;
use legaia_asset::equip_stats::{BONUS_STRIDE, EquipSlot, EquipStatTable, bonus_table_file_offset};
use legaia_asset::equip_transplant::{
    self, Transplant, packed_len, records_with_transplants, transplant_weapon,
};
use legaia_asset::party_swap::playerize::{rebuild_player_file, rebuild_player_file_unbounded};

use crate::disc::DiscPatcher;
use crate::weapon_specialty::{self, PLAYERS, arm_cost_offset, up_cost_offset};

use super::SCUS_NAME;

/// Smallest command cost the editor accepts. The gauge draws each command
/// as a text window `cost - 6` pixels wide and condenses the label
/// (`High` / `Arms` / `RaSeru` / `Low`) to fit it: retail's 30 (24 px) is
/// the tightest fully clean label, 24 (18 px) is still legible, and below
/// that the labels smear into glyph fragments until at 7 only a 1-px
/// sliver and the arrow caps remain (measured on a PCSX-Redux sweep of the
/// gauge build in `FUN_801D388C`; see `docs/subsystems/arts-command-gauge.md`).
pub const MIN_SWING_COST: u8 = 24;
/// Costs below this draw a condensed command label (retail never goes lower).
pub const CLEAN_SWING_COST: u8 = 30;

/// The three equipment sections of a player file that carry swing records,
/// named by what they hold rather than by index (the weapon / Ra-Seru order
/// differs per file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SwingSection {
    /// The weapon section (2 in Vahn's and Gala's files, 3 in Noa's).
    Weapon,
    /// The Ra-Seru arm section (the other of 2 / 3).
    RaSeru,
    /// The footwear section (4): two records, Down and Up.
    Footwear,
}

impl SwingSection {
    fn label(self) -> &'static str {
        match self {
            SwingSection::Weapon => "default",
            SwingSection::RaSeru => "raseru",
            SwingSection::Footwear => "feet",
        }
    }
}

/// Which swing record of a section an edit targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SwingRecord {
    /// The `+0x04` record: the section's own command (Left / Right / Down).
    #[default]
    Primary,
    /// The `+0x08` record: the Up kick. Footwear sections only.
    Up,
}

/// What a cost edit points at: one item's section, or a section's `id = 0`
/// default record (what the loader splices when nothing matches).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostTarget {
    /// The section keyed by this equippable item id.
    Item(u8),
    /// The default record of the named section.
    Default(SwingSection),
}

/// One command-cost edit: `character` indexes [`PLAYERS`] (0 Vahn, 1 Noa, 2 Gala).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwingCostEdit {
    /// Index into [`PLAYERS`].
    pub character: usize,
    /// The section whose record is repriced.
    pub target: CostTarget,
    /// Which of the section's records (`Up` exists only for footwear).
    pub record: SwingRecord,
    /// New `+0x74` value (`>= MIN_SWING_COST`).
    pub cost: u8,
}

impl SwingCostEdit {
    /// The token spelling of the target: `0xBA`, `0x5E:up`, `default`, `feet:up`.
    pub fn label(&self) -> String {
        let t = match self.target {
            CostTarget::Item(id) => format!("0x{id:02X}"),
            CostTarget::Default(s) => s.label().to_string(),
        };
        match self.record {
            SwingRecord::Primary => t,
            SwingRecord::Up => format!("{t}:up"),
        }
    }
}

/// Equip-owner mask bits (`+6` byte of a bonus row).
pub const MASK_VAHN: u8 = 1;
/// Equip-owner mask bit for Noa.
pub const MASK_NOA: u8 = 2;
/// Equip-owner mask bit for Gala.
pub const MASK_GALA: u8 = 4;

/// One equip-owner edit: the low three bits of the item's row `+6` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquipOwnerEdit {
    /// Item id whose bonus row is rewritten.
    pub item_id: u8,
    /// New owner bits (`MASK_VAHN | MASK_NOA | MASK_GALA` subset).
    pub mask: u8,
}

/// The full edit set the CLI and the web patcher hand to [`apply_equipment_edits`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EquipmentEdits {
    /// Swing-cost rewrites.
    pub costs: Vec<SwingCostEdit>,
    /// Equip-owner rewrites.
    pub owners: Vec<EquipOwnerEdit>,
    /// Leave a newly enabled weapon on the default look instead of carrying
    /// its model over from the character file that has it (see
    /// [`ModelTransplant`]).
    pub skip_model_transplant: bool,
    /// When the transplanted records fit neither the three player files
    /// (re-packed) nor the `DMY.DAT` annex, grow the target entries with a
    /// whole-disc relayout (the image gets longer; a PPF cannot carry it).
    pub allow_relayout: bool,
}

impl EquipmentEdits {
    /// `true` when there is nothing to apply.
    pub fn is_empty(&self) -> bool {
        self.costs.is_empty() && self.owners.is_empty()
    }
}

/// Resolve a character name / initial / index to a [`PLAYERS`] index.
pub fn character_index(s: &str) -> Option<usize> {
    match s.trim().to_ascii_lowercase().as_str() {
        "0" | "v" | "vahn" => Some(0),
        "1" | "n" | "noa" => Some(1),
        "2" | "g" | "gala" => Some(2),
        _ => None,
    }
}

fn parse_u8(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

/// Parse `CHAR:ITEM[:up]=COST` (`Vahn:0xBA=30`, `n:0x2E=0x1E`, `2:51=42`,
/// `g:0x5E:up=42`). `ITEM` may name a section default instead of an item:
/// `default` / `fist` / `unarmed` (weapon section), `raseru` (Ra-Seru arm
/// section), `feet` / `barefoot` (footwear section, `:up` for its Up kick).
pub fn parse_cost_token(tok: &str) -> Result<SwingCostEdit> {
    let (lhs, cost) = tok
        .split_once('=')
        .with_context(|| format!("swing cost `{tok}`: expected CHAR:ITEM[:up]=COST"))?;
    let mut parts = lhs.split(':');
    let ch = parts.next().unwrap_or("");
    let item = parts
        .next()
        .with_context(|| format!("swing cost `{tok}`: expected CHAR:ITEM[:up]=COST"))?;
    let record = match parts.next().map(|r| r.trim().to_ascii_lowercase()) {
        None => SwingRecord::Primary,
        Some(r) if r == "down" || r == "primary" => SwingRecord::Primary,
        Some(r) if r == "up" => SwingRecord::Up,
        Some(r) => bail!("swing cost `{tok}`: unknown record `{r}` (use `up`)"),
    };
    if parts.next().is_some() {
        bail!("swing cost `{tok}`: expected CHAR:ITEM[:up]=COST");
    }
    let character =
        character_index(ch).with_context(|| format!("swing cost `{tok}`: unknown character"))?;
    let target = match item.trim().to_ascii_lowercase().as_str() {
        "default" | "fist" | "unarmed" | "default-weapon" => {
            CostTarget::Default(SwingSection::Weapon)
        }
        "raseru" | "ra-seru" | "default-raseru" => CostTarget::Default(SwingSection::RaSeru),
        "feet" | "barefoot" | "default-feet" | "footwear" => {
            CostTarget::Default(SwingSection::Footwear)
        }
        _ => CostTarget::Item(
            parse_u8(item).with_context(|| format!("swing cost `{tok}`: bad item id"))?,
        ),
    };
    let cost = parse_u8(cost).with_context(|| format!("swing cost `{tok}`: bad cost"))?;
    if cost < MIN_SWING_COST {
        bail!("swing cost `{tok}`: cost must be at least {MIN_SWING_COST}");
    }
    Ok(SwingCostEdit {
        character,
        target,
        record,
        cost,
    })
}

/// Parse an owner mask: `V`/`N`/`G` letters in any order (`VNG`, `ng`),
/// `any`, `none`, or a number `0..=7`.
pub fn parse_owner_mask(s: &str) -> Option<u8> {
    let s = s.trim();
    match s.to_ascii_lowercase().as_str() {
        "any" | "all" => return Some(MASK_VAHN | MASK_NOA | MASK_GALA),
        "none" | "-" => return Some(0),
        _ => {}
    }
    if let Some(n) = parse_u8(s) {
        return (n <= 7).then_some(n);
    }
    let mut m = 0u8;
    for c in s.chars() {
        m |= match c.to_ascii_lowercase() {
            'v' => MASK_VAHN,
            'n' => MASK_NOA,
            'g' => MASK_GALA,
            _ => return None,
        };
    }
    Some(m)
}

/// Parse `ITEM=OWNERS` (`0xBA=VNG`, `0x24=any`, `36=G`).
pub fn parse_owner_token(tok: &str) -> Result<EquipOwnerEdit> {
    let (item, owners) = tok
        .split_once('=')
        .with_context(|| format!("equip owner `{tok}`: expected ITEM=OWNERS"))?;
    let item_id = parse_u8(item).with_context(|| format!("equip owner `{tok}`: bad item id"))?;
    let mask =
        parse_owner_mask(owners).with_context(|| format!("equip owner `{tok}`: bad owner set"))?;
    Ok(EquipOwnerEdit { item_id, mask })
}

/// Parse two token lists (each comma / semicolon / whitespace separated).
pub fn parse_edit_lists(costs: &str, owners: &str) -> Result<EquipmentEdits> {
    let split = |s: &str| -> Vec<String> {
        s.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    };
    let mut out = EquipmentEdits::default();
    for t in split(costs) {
        out.costs.push(parse_cost_token(&t)?);
    }
    for t in split(owners) {
        out.owners.push(parse_owner_token(&t)?);
    }
    Ok(out)
}

/// Render an owner mask as `V`/`N`/`G` letters (`any` for all three, `-` for none).
pub fn owner_label(mask: u8) -> String {
    if mask & 7 == 7 {
        return "any".into();
    }
    let mut s = String::new();
    for (bit, ch) in [(MASK_VAHN, 'V'), (MASK_NOA, 'N'), (MASK_GALA, 'G')] {
        if mask & bit != 0 {
            s.push(ch);
        }
    }
    if s.is_empty() { "-".into() } else { s }
}

/// One equippable item as the editor shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentRow {
    /// Item id.
    pub id: u8,
    /// Display name from the SCUS item table (empty if unnamed).
    pub name: String,
    /// `weapon` / `body` / `head` / `footwear`.
    pub slot: &'static str,
    /// `true` for a Ra-Seru arm level (it sits in a Ra-Seru section, so it
    /// prices the other hand than that character's weapons).
    pub ra_seru_arm: bool,
    /// Bonus-table row index (several items may share one).
    pub row: usize,
    /// Other item ids sharing the same bonus row (an owner edit moves them too).
    pub shares_row_with: Vec<u8>,
    /// Current owner mask (`+6`, low three bits).
    pub mask: u8,
    /// Attack bonus (`+1`).
    pub atk: u8,
    /// Per character: the cost of the item's `+0x04` record - the command the
    /// section fills (`Left` / `Right` for weapons and Ra-Seru arms, `Down` for
    /// footwear). `None` when that character's file has no section for the
    /// item; always `[None; 3]` for body / head.
    pub costs: [Option<u8>; 3],
    /// Per character: the Up kick's cost (`+0x08` record). Footwear only.
    pub up_costs: [Option<u8>; 3],
    /// Per character: the command name the `+0x04` record prices
    /// (`"Left"` / `"Right"` / `"Down"`), where a section exists.
    pub cmds: [Option<&'static str>; 3],
}

/// One character's section default records and their costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SectionDefaults {
    /// Weapon-section default: an unlisted weapon, or unarmed.
    pub weapon: Option<u8>,
    /// Ra-Seru-section default.
    pub ra_seru: Option<u8>,
    /// Footwear-section default, Down kick.
    pub down: Option<u8>,
    /// Footwear-section default, Up kick.
    pub up: Option<u8>,
}

fn slot_name(s: EquipSlot) -> &'static str {
    match s {
        EquipSlot::Body => "body",
        EquipSlot::Head => "head",
        EquipSlot::Weapon => "weapon",
        EquipSlot::Footwear => "footwear",
    }
}

/// Section index (`0..5`, counted by `id = 0` terminators) of every
/// descriptor, plus which of sections 2 / 3 holds the weapons: the id group
/// containing a weapon-family id.
fn section_layout(pack: &battle_data_pack::BattleDataPack) -> (Vec<usize>, usize) {
    let mut sections = Vec::with_capacity(pack.records.len());
    let mut sec = 0usize;
    let mut weapon_section = 2usize;
    for rec in &pack.records {
        sections.push(sec);
        if rec.id != 0
            && rec.id <= 0xFF
            && weapon_specialty::weapon_family(rec.id as u8).is_some()
            && (2..=3).contains(&sec)
        {
            weapon_section = sec;
        }
        if rec.id == 0 {
            sec += 1;
        }
    }
    (sections, weapon_section)
}

fn section_of(kind: SwingSection, weapon_section: usize) -> usize {
    match kind {
        SwingSection::Weapon => weapon_section,
        SwingSection::RaSeru => 5 - weapon_section,
        SwingSection::Footwear => 4,
    }
}

/// The command name section `sec` of a file prices with its `+0x04` record.
fn command_name(sec: usize) -> Option<&'static str> {
    match sec {
        2 => Some("Left"),
        3 => Some("Right"),
        4 => Some("Down"),
        _ => None,
    }
}

/// Descriptor index of `target`'s section in `pack`.
fn section_index(pack: &battle_data_pack::BattleDataPack, target: CostTarget) -> Option<usize> {
    match target {
        CostTarget::Item(item) => pack.records.iter().position(|r| r.id == item as u32),
        CostTarget::Default(kind) => {
            let (sections, weapon_section) = section_layout(pack);
            let want = section_of(kind, weapon_section);
            pack.records
                .iter()
                .zip(&sections)
                .position(|(r, &s)| r.id == 0 && s == want)
        }
    }
}

fn cost_offset(decoded: &[u8], record: SwingRecord) -> Option<usize> {
    match record {
        SwingRecord::Primary => arm_cost_offset(decoded),
        SwingRecord::Up => up_cost_offset(decoded),
    }
}

/// One character's swing-record costs as read from the disc.
#[derive(Debug, Default)]
struct FileCosts {
    /// Item id -> (`+0x04` cost, `+0x08` cost, command the `+0x04` record prices).
    items: std::collections::BTreeMap<u8, (u8, Option<u8>, &'static str)>,
    /// Item ids that sit in the Ra-Seru section.
    ra_seru_arms: std::collections::BTreeSet<u8>,
    /// The section defaults.
    defaults: SectionDefaults,
    /// Command the weapon section prices.
    weapon_hand: Option<&'static str>,
}

fn read_file_costs(patcher: &DiscPatcher) -> [FileCosts; 3] {
    let mut out: [FileCosts; 3] = Default::default();
    for (ci, player) in PLAYERS.iter().enumerate() {
        let Ok(buf) = patcher.read_player_file(player.entry) else {
            continue;
        };
        let Some(pack) = battle_data_pack::detect(&buf) else {
            continue;
        };
        let (sections, weapon_section) = section_layout(&pack);
        let ra_seru_section = 5 - weapon_section;
        out[ci].weapon_hand = command_name(weapon_section);
        for (idx, rec) in pack.records.iter().enumerate() {
            let sec = sections[idx];
            if rec.id > 0xFF || !(2..=4).contains(&sec) {
                continue;
            }
            let Ok(dec) = battle_data_pack::decode_record(&buf, &pack, idx) else {
                continue;
            };
            let Some(off) = arm_cost_offset(&dec.bytes) else {
                continue;
            };
            let primary = dec.bytes[off];
            let up = up_cost_offset(&dec.bytes).map(|o| dec.bytes[o]);
            if rec.id == 0 {
                let d = &mut out[ci].defaults;
                if sec == weapon_section {
                    d.weapon = Some(primary);
                } else if sec == ra_seru_section {
                    d.ra_seru = Some(primary);
                } else {
                    d.down = Some(primary);
                    d.up = up;
                }
                continue;
            }
            let Some(cmd) = command_name(sec) else {
                continue;
            };
            out[ci].items.insert(rec.id as u8, (primary, up, cmd));
            if sec == ra_seru_section {
                out[ci].ra_seru_arms.insert(rec.id as u8);
            }
        }
    }
    out
}

/// The table the editor displays: every equippable item, plus each
/// character's section default records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentTable {
    /// One row per equippable item, in id order.
    pub rows: Vec<EquipmentRow>,
    /// Per character ([`PLAYERS`] order): the default records' costs.
    pub defaults: [SectionDefaults; 3],
    /// Per character: the command the weapon section prices (`"Left"` for
    /// Vahn and Gala, `"Right"` for Noa); the Ra-Seru arm is the other one.
    pub weapon_hand: [Option<&'static str>; 3],
}

/// Read every equippable item with its owner mask and, where a section
/// exists, its per-character command costs - the table the editor displays.
/// `None` when the SCUS equipment table can't be parsed.
pub fn read_equipment_table(patcher: &DiscPatcher) -> Result<Option<EquipmentTable>> {
    let Some(scus) = patcher.read_named_file(SCUS_NAME) else {
        return Ok(None);
    };
    let Some(table) = EquipStatTable::from_scus(&scus) else {
        return Ok(None);
    };
    let names = legaia_asset::item_names::ItemNameTable::from_scus(&scus);
    let items_for_rows = table.items_for_rows();
    let mut row_of = std::collections::BTreeMap::new();
    for (row, items) in items_for_rows.iter().enumerate() {
        for &id in items {
            row_of.insert(id, row);
        }
    }
    let files = read_file_costs(patcher);
    let mut out = Vec::new();
    for id in 1u8..=255 {
        let Some(b) = table.bonus(id) else {
            continue;
        };
        let Some(&row) = row_of.get(&id) else {
            continue;
        };
        let name = names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("")
            .to_string();
        let slot = b.slot();
        let per: [Option<(u8, Option<u8>, &'static str)>; 3] =
            std::array::from_fn(|ci| files[ci].items.get(&id).copied());
        out.push(EquipmentRow {
            id,
            name,
            slot: slot_name(slot),
            ra_seru_arm: files.iter().any(|f| f.ra_seru_arms.contains(&id)),
            row,
            shares_row_with: items_for_rows[row]
                .iter()
                .copied()
                .filter(|&o| o != id)
                .collect(),
            mask: b.equip_mask() & 7,
            atk: b.attack(),
            costs: per.map(|p| p.map(|(c, _, _)| c)),
            up_costs: per.map(|p| p.and_then(|(_, u, _)| u)),
            cmds: per.map(|p| p.map(|(_, _, cmd)| cmd)),
        });
    }
    Ok(Some(EquipmentTable {
        rows: out,
        defaults: std::array::from_fn(|ci| files[ci].defaults),
        weapon_hand: std::array::from_fn(|ci| files[ci].weapon_hand),
    }))
}

/// An owner edit that lets a character equip an item their player file has no
/// section for: they fall through to that section's default record in battle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallThrough {
    /// Character name.
    pub character: String,
    /// The item.
    pub item: u8,
    /// The item's slot (`weapon` / `body` / `head` / `footwear`).
    pub slot: &'static str,
    /// The default record's costs after this pass's edits: `[swing]` for a
    /// weapon, `[down, up]` for footwear, empty for body / head (no cost).
    pub costs: Vec<u8>,
}

impl FallThrough {
    /// One-line human note for reports and the web summary.
    pub fn note(&self) -> String {
        let cost = match (self.slot, self.costs.as_slice()) {
            ("weapon", [c]) => format!(", {c}-AP swing (the weapon Default row)"),
            ("footwear", [d, u]) => {
                format!(", {d}-AP Down / {u}-AP Up kick (the footwear Default row)")
            }
            _ => String::new(),
        };
        format!(
            "{} can now equip 0x{:02X} but has no battle section for it: default look{cost}",
            self.character, self.item
        )
    }
}

/// [`FallThrough::note`] over a report, with the characters that share one
/// item and one cost outcome folded into a single line (`Noa, Gala can now
/// equip 0x34 ...`), in first-seen order.
pub fn fall_through_notes(list: &[FallThrough]) -> Vec<String> {
    let mut groups: Vec<(FallThrough, Vec<String>)> = Vec::new();
    for f in list {
        match groups
            .iter_mut()
            .find(|(g, _)| g.item == f.item && g.slot == f.slot && g.costs == f.costs)
        {
            Some((_, names)) => names.push(f.character.clone()),
            None => groups.push((f.clone(), vec![f.character.clone()])),
        }
    }
    groups
        .into_iter()
        .map(|(g, names)| {
            let joined = FallThrough {
                character: names.join(", "),
                ..g
            };
            joined.note()
        })
        .collect()
}

/// A weapon model carried over from another character's player file so a
/// newly enabled owner holds it in battle: the donor record's weapon
/// primitives and texture tile, seated on the new owner's own bare arm and
/// swing records (`legaia_asset::equip_transplant`). The record keeps the
/// donor weapon's arm cost.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelTransplant {
    /// The new owner.
    pub character: String,
    /// The weapon's item id.
    pub item: u8,
    /// Whose file the model came from.
    pub source: String,
    /// The arm cost the new record carries.
    pub cost: u8,
    /// Degrees the weapon was rotated to sit in the new owner's hand
    /// frame (the hand channel's calibration; see
    /// `legaia_asset::equip_hand_frame`), with the calibration residual.
    pub reseat: Option<(f64, f64)>,
    /// Donor bones whose part of the weapon had no calibration and was
    /// left out.
    pub dropped_channels: Vec<u8>,
}

/// What [`apply_equipment_edits`] did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EquipmentEditReport {
    /// Weapon models carried over into another character's file.
    pub models_transplanted: Vec<ModelTransplant>,
    /// Transplants the three player files had no room for (and no relayout
    /// was allowed): those owners fall through to the default look.
    pub models_no_room: Vec<FallThrough>,
    /// `(character, item, why)` transplants the cut could not build.
    pub models_failed: Vec<(String, u8, String)>,
    /// `(PROT entry, sector delta)` for player-file boundaries that moved
    /// to make room (same total footprint, no relayout).
    pub entries_reassigned: Vec<(usize, i64)>,
    /// Sectors the disc grew by (relayout), 0 when it did not.
    pub relayout_sectors: u32,
    /// `(character, disc LBA, sectors)` player files whose records were
    /// parked in the `DMY.DAT` annex because the PROT pool had no room
    /// (header in place, same-size image).
    pub models_annexed: Vec<(String, u32, u32)>,
    /// Swing-cost sections rewritten.
    pub costs_changed: usize,
    /// Swing-cost edits that were already at the requested value.
    pub costs_unchanged: usize,
    /// `(character, target label)` edits whose player file has no such section
    /// or record (an unlisted item, or `:up` on anything but footwear).
    pub costs_no_section: Vec<(String, String)>,
    /// `(character, target label)` edits whose section would not recompress
    /// into its slot.
    pub costs_skipped_fit: Vec<(String, String)>,
    /// Owner rows rewritten.
    pub owners_changed: usize,
    /// Owner edits whose item is not equipment on this disc.
    pub owners_not_equipment: Vec<u8>,
    /// `(item, [other items on the same row])` for owner edits that moved siblings.
    pub owners_shared_rows: Vec<(u8, Vec<u8>)>,
    /// Owner edits that newly allow an item although that character's player
    /// file has no section for it (see [`FallThrough`]).
    pub owners_without_section: Vec<FallThrough>,
}

/// Apply the edit set. Owner masks are a same-size SCUS patch; a newly
/// enabled weapon whose owner's file lacks a record for it gets the model
/// **transplanted** from the file that has it (unless
/// `skip_model_transplant`), which rebuilds that player file - room comes
/// from re-packing the three player files with the optimal LZS parse and
/// moving the boundaries between them (`DiscPatcher::reassign_prot_entries`),
/// or from a relayout when allowed. Swing costs go through the LZS re-pack
/// path (a section that does not fit is reported, not written), folded into
/// the rebuild for files that are rebuilt anyway. Idempotent.
pub fn apply_equipment_edits(
    patcher: &mut DiscPatcher,
    edits: &EquipmentEdits,
) -> Result<EquipmentEditReport> {
    let mut report = EquipmentEditReport::default();

    // --- owners ------------------------------------------------------------
    // Target character -> (item, donor character) for the models to carry over.
    let mut plan: BTreeMap<usize, Vec<(u8, usize)>> = BTreeMap::new();
    if !edits.owners.is_empty() {
        let scus = patcher
            .read_named_file(SCUS_NAME)
            .context("SCUS_942.54 not found")?;
        let table = EquipStatTable::from_scus(&scus).context("equipment bonus table")?;
        let off = bonus_table_file_offset(&scus).context("equipment bonus table offset")?;
        let items_for_rows = table.items_for_rows();
        let mut rows: Vec<[u8; 8]> = table.rows().iter().map(|b| b.raw).collect();
        let files = read_file_costs(patcher);
        let mut changed = 0usize;
        for e in &edits.owners {
            let Some(row) = items_for_rows
                .iter()
                .position(|items| items.contains(&e.item_id))
            else {
                report.owners_not_equipment.push(e.item_id);
                continue;
            };
            let old = rows[row][6];
            let new = (old & !7) | (e.mask & 7);
            if new == old {
                continue;
            }
            rows[row][6] = new;
            changed += 1;
            let siblings: Vec<u8> = items_for_rows[row]
                .iter()
                .copied()
                .filter(|&o| o != e.item_id)
                .collect();
            if !siblings.is_empty() {
                report.owners_shared_rows.push((e.item_id, siblings));
            }
            let slot = table.bonus(e.item_id).map(|b| b.slot());
            for (ci, player) in PLAYERS.iter().enumerate() {
                let bit = 1u8 << ci;
                if new & bit == 0 || old & bit != 0 || files[ci].items.contains_key(&e.item_id) {
                    continue;
                }
                if slot == Some(EquipSlot::Weapon)
                    && !edits.skip_model_transplant
                    && let Some(src) = weapon_donor(&files, ci, e.item_id)
                {
                    plan.entry(ci).or_default().push((e.item_id, src));
                    continue;
                }
                report.owners_without_section.push(fall_through(
                    player.name,
                    e.item_id,
                    slot,
                    &files[ci].defaults,
                ));
            }
        }
        if changed > 0 {
            let mut bytes = Vec::with_capacity(rows.len() * BONUS_STRIDE);
            for r in &rows {
                bytes.extend_from_slice(r);
            }
            patcher
                .patch_named_file(SCUS_NAME, off as u64, &bytes)
                .context("write equipment owner masks")?;
        }
        report.owners_changed = changed;
    }

    // --- model transplants (player-file rebuilds) --------------------------
    // Characters whose cost edits were folded into a rebuilt file.
    let mut rebuilt: BTreeSet<usize> = BTreeSet::new();
    if !plan.is_empty() {
        let raw: Vec<Vec<u8>> = PLAYERS
            .iter()
            .map(|p| {
                patcher
                    .read_player_file(p.entry)
                    .with_context(|| format!("read {} player file", p.name))
            })
            .collect::<Result<_>>()?;
        // Per character: the record list to repack (with transplants and
        // this character's cost edits folded in) + what the fold did.
        let mut lists: Vec<Option<RecordPlan>> = vec![None, None, None];
        let mut landed: Vec<Vec<ModelTransplant>> = vec![Vec::new(), Vec::new(), Vec::new()];
        let mut wanted: Vec<Vec<(u8, usize)>> = vec![Vec::new(), Vec::new(), Vec::new()];
        for (&ci, wants) in &plan {
            let mut ts: Vec<Transplant> = Vec::new();
            for &(id, src) in wants {
                match transplant_weapon(&raw[ci], &raw[src], src, ci, id as u32) {
                    Ok(t) => {
                        landed[ci].push(ModelTransplant {
                            character: PLAYERS[ci].name.to_string(),
                            item: id,
                            source: PLAYERS[src].name.to_string(),
                            cost: t.cost,
                            reseat: t.reseated.last().map(|(_, deg, rms)| (*deg, *rms)),
                            dropped_channels: t.dropped_channels.clone(),
                        });
                        wanted[ci].push((id, src));
                        ts.push(t);
                    }
                    Err(e) => report.models_failed.push((
                        PLAYERS[ci].name.to_string(),
                        id,
                        format!("{e:#}"),
                    )),
                }
            }
            if ts.is_empty() {
                continue;
            }
            let (pack, mut records) = records_with_transplants(&raw[ci], &ts)?;
            let fold = fold_costs(&pack, &mut records, ci, &edits.costs)?;
            // The note quotes the cost the record ends up with.
            for t in landed[ci].iter_mut() {
                if let Some((_, dec)) = records.iter().find(|(id, _)| *id == t.item as u32)
                    && let Some(off) = arm_cost_offset(dec)
                {
                    t.cost = dec[off];
                }
            }
            lists[ci] = Some((pack.data_base, records, fold));
        }
        if lists.iter().any(Option::is_some) {
            let sector = 0x800usize;
            let foot: Vec<usize> = PLAYERS
                .iter()
                .map(|p| {
                    patcher
                        .entry_true_footprint_sectors(p.entry)
                        .map(|n| n as usize)
                        .with_context(|| format!("{} footprint", p.name))
                })
                .collect::<Result<_>>()?;
            let total: usize = foot.iter().sum();
            let sized = |data_base: usize, records: &[(u32, Vec<u8>)]| {
                (data_base + packed_len(records)).div_ceil(sector)
            };
            let mut need = foot.clone();
            for ci in 0..PLAYERS.len() {
                if let Some((db, recs, _)) = &lists[ci] {
                    need[ci] = sized(*db, recs);
                }
            }
            if need.iter().sum::<usize>() > total {
                // Re-pack untouched files too - the optimal parse frees a few
                // sectors in each - starting with the one that frees most,
                // and only as many as it takes.
                let mut repacks: Vec<(usize, usize, usize, RecordList)> = Vec::new();
                for ci in 0..PLAYERS.len() {
                    if lists[ci].is_none() {
                        let pack = battle_data_pack::parse(&raw[ci])
                            .with_context(|| format!("parse {} player file", PLAYERS[ci].name))?;
                        let records = equip_transplant::file_records(&raw[ci], &pack)?;
                        let n = sized(pack.data_base, &records);
                        repacks.push((ci, pack.data_base, n, records));
                    }
                }
                repacks.sort_by_key(|(ci, _, n, _)| (foot[*ci] as i64 - *n as i64).wrapping_neg());
                for (ci, db, n, records) in repacks {
                    if need.iter().sum::<usize>() <= total || n >= foot[ci] {
                        break;
                    }
                    need[ci] = n;
                    lists[ci] = Some((db, records, CostFold::default()));
                }
            }
            let table_offset = |ci: usize| -> Result<usize> {
                Ok(battle_data_pack::parse(&raw[ci])?.table_offset)
            };
            if need.iter().sum::<usize>() <= total {
                // Boundaries move; the run keeps its footprint. Leftover
                // sectors go to the last file of the run.
                let leftover = total - need.iter().sum::<usize>();
                *need.last_mut().unwrap() += leftover;
                let mut payloads: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
                for ci in 0..PLAYERS.len() {
                    let entry_len = need[ci] * sector;
                    let file = match lists[ci].take() {
                        Some((db, records, fold)) => {
                            if !fold.is_empty() || !landed[ci].is_empty() {
                                rebuilt.insert(ci);
                            }
                            fold.commit(&mut report);
                            rebuild_player_file(&raw[ci], table_offset(ci)?, db, records, entry_len)
                                .with_context(|| {
                                    format!("rebuild {} player file", PLAYERS[ci].name)
                                })?
                        }
                        None => {
                            let mut f = raw[ci].clone();
                            f.resize(entry_len, 0);
                            f
                        }
                    };
                    if need[ci] != foot[ci] {
                        report
                            .entries_reassigned
                            .push((PLAYERS[ci].entry, need[ci] as i64 - foot[ci] as i64));
                    }
                    payloads.insert(PLAYERS[ci].entry, file);
                }
                patcher
                    .reassign_prot_entries(&payloads)
                    .context("move player-file boundaries")?;
                for l in landed {
                    report.models_transplanted.extend(l);
                }
            } else if let Some(placed) = annex_rebuilt_files(
                patcher,
                &raw,
                &mut lists,
                &landed,
                &table_offset,
                &mut report,
            )? {
                // The PROT pool is short, but DMY.DAT is not: the records of
                // every rebuilt file go there, header in place.
                for ci in placed {
                    rebuilt.insert(ci);
                }
                for l in landed {
                    report.models_transplanted.extend(l);
                }
            } else if edits.allow_relayout {
                let mut payloads: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
                for ci in 0..PLAYERS.len() {
                    if landed[ci].is_empty() {
                        continue;
                    }
                    let Some((db, records, fold)) = lists[ci].take() else {
                        continue;
                    };
                    let sectors = need[ci].max(foot[ci]);
                    fold.commit(&mut report);
                    rebuilt.insert(ci);
                    report.relayout_sectors += (sectors - foot[ci]) as u32;
                    let file = rebuild_player_file(
                        &raw[ci],
                        table_offset(ci)?,
                        db,
                        records,
                        sectors * sector,
                    )
                    .with_context(|| format!("rebuild {} player file", PLAYERS[ci].name))?;
                    payloads.insert(PLAYERS[ci].entry, file);
                }
                patcher
                    .grow_prot_entries(&payloads)
                    .context("grow player files (relayout)")?;
                for l in landed {
                    report.models_transplanted.extend(l);
                }
            } else {
                // No room: the models stay out, those owners fall through.
                let files = read_file_costs(patcher);
                let scus = patcher.read_named_file(SCUS_NAME).context("SCUS_942.54")?;
                let table = EquipStatTable::from_scus(&scus).context("equipment bonus table")?;
                for ci in 0..PLAYERS.len() {
                    for &(id, _) in &wanted[ci] {
                        let slot = table.bonus(id).map(|b| b.slot());
                        report.models_no_room.push(fall_through(
                            PLAYERS[ci].name,
                            id,
                            slot,
                            &files[ci].defaults,
                        ));
                    }
                }
            }
        }
    }

    // The fall-through notes quote the default records' costs as they end
    // up - after the cost edits below - so they are refreshed at the end.

    // --- swing costs, in place ----------------------------------------------
    for (ci, player) in PLAYERS.iter().enumerate() {
        if rebuilt.contains(&ci) {
            continue;
        }
        let mine: Vec<&SwingCostEdit> = edits.costs.iter().filter(|e| e.character == ci).collect();
        if mine.is_empty() {
            continue;
        }
        let buf = patcher
            .read_player_file(player.entry)
            .with_context(|| format!("read {} player file", player.name))?;
        let Some(pack) = battle_data_pack::detect(&buf) else {
            bail!(
                "{} player file (PROT {}) is not a battle data pack",
                player.name,
                player.entry
            );
        };
        for e in mine {
            let label = e.label();
            if e.cost < MIN_SWING_COST {
                bail!(
                    "swing cost {} for {} {label} is below {MIN_SWING_COST}",
                    e.cost,
                    player.name,
                );
            }
            let Some(idx) = section_index(&pack, e.target) else {
                report
                    .costs_no_section
                    .push((player.name.to_string(), label));
                continue;
            };
            let rec = &pack.records[idx];
            let dec = battle_data_pack::decode_record(&buf, &pack, idx)
                .with_context(|| format!("decode {} {label}", player.name))?;
            let Some(off) = cost_offset(&dec.bytes, e.record) else {
                report
                    .costs_no_section
                    .push((player.name.to_string(), label));
                continue;
            };
            if dec.bytes[off] == e.cost {
                report.costs_unchanged += 1;
                continue;
            }
            let mut decoded = dec.bytes.clone();
            decoded[off] = e.cost;
            let recompressed = legaia_lzs::compress(&decoded);
            let avail = (rec.size as usize).saturating_sub(4);
            if recompressed.len() > avail {
                report
                    .costs_skipped_fit
                    .push((player.name.to_string(), label));
                continue;
            }
            let stream_off = rec.file_offset(pack.data_base) + 4;
            patcher
                .patch_player_file(player.entry, stream_off as u64, &recompressed)
                .with_context(|| format!("write swing cost for {} {label}", player.name))?;
            report.costs_changed += 1;
        }
    }

    if !report.owners_without_section.is_empty() || !report.models_no_room.is_empty() {
        let files = read_file_costs(patcher);
        let scus = patcher.read_named_file(SCUS_NAME).context("SCUS_942.54")?;
        let table = EquipStatTable::from_scus(&scus).context("equipment bonus table")?;
        for f in report
            .owners_without_section
            .iter_mut()
            .chain(report.models_no_room.iter_mut())
        {
            let Some(ci) = PLAYERS.iter().position(|p| p.name == f.character) else {
                continue;
            };
            let slot = table.bonus(f.item).map(|b| b.slot());
            f.costs = fall_through(&f.character, f.item, slot, &files[ci].defaults).costs;
        }
    }

    Ok(report)
}

/// Park every rebuilt file that carries a transplant in the `DMY.DAT`
/// annex (`DiscPatcher::annex_player_file`). Returns the characters placed,
/// or `None` - with nothing written - when the annex cannot take them
/// (no `DMY.DAT`, or it is full), so the caller can fall back. Files in
/// `lists` without a landed transplant (the pool-fill repacks) are left
/// alone; their records are already on the disc.
fn annex_rebuilt_files(
    patcher: &mut DiscPatcher,
    raw: &[Vec<u8>],
    lists: &mut [Option<RecordPlan>],
    landed: &[Vec<ModelTransplant>],
    table_offset: &dyn Fn(usize) -> Result<usize>,
    report: &mut EquipmentEditReport,
) -> Result<Option<Vec<usize>>> {
    let mut files: Vec<(usize, Vec<u8>, CostFold)> = Vec::new();
    for ci in 0..PLAYERS.len() {
        if landed[ci].is_empty() {
            continue;
        }
        let Some((db, records, fold)) = lists[ci].as_ref() else {
            continue;
        };
        let file = rebuild_player_file_unbounded(&raw[ci], table_offset(ci)?, *db, records.clone())
            .with_context(|| format!("rebuild {} player file", PLAYERS[ci].name))?;
        files.push((ci, file, fold.clone()));
    }
    if files.is_empty() {
        return Ok(None);
    }
    let wanted: u32 = files
        .iter()
        .map(|(_, f, _)| {
            (f.len().saturating_sub(crate::disc::PLAYER_FILE_DATA_BASE) / 0x800) as u32
        })
        .sum();
    match patcher.annex_free_sectors() {
        Ok(free) if free >= wanted => {}
        _ => return Ok(None),
    }
    let mut placed = Vec::new();
    for (ci, file, fold) in files {
        let place = patcher
            .annex_player_file(PLAYERS[ci].entry, &file)
            .with_context(|| format!("annex {} player file", PLAYERS[ci].name))?;
        fold.commit(report);
        lists[ci] = None;
        report
            .models_annexed
            .push((PLAYERS[ci].name.to_string(), place.lba, place.sectors));
        placed.push(ci);
    }
    Ok(Some(placed))
}

/// Human-readable lines for the model side of a report: what was carried
/// over, what had no room, what the cut refused, and how the disc made
/// room. The CLI prints them, the web patcher puts them in the summary.
pub fn transplant_notes(rep: &EquipmentEditReport) -> Vec<String> {
    let mut out = Vec::new();
    for t in &rep.models_transplanted {
        let seat = match t.reseat {
            Some((deg, _)) => format!(", re-seated {deg:.0} deg into {}'s grip", t.character),
            None => String::new(),
        };
        out.push(format!(
            "{} now holds 0x{:02X} in battle: model carried over from {}'s file ({} AP swing{seat})",
            t.character, t.item, t.source, t.cost
        ));
        if !t.dropped_channels.is_empty() {
            out.push(format!(
                "  part of 0x{:02X} on donor bone(s) {:?} had no grip calibration and was left out",
                t.item, t.dropped_channels
            ));
        }
    }
    for (who, lba, sectors) in &rep.models_annexed {
        out.push(format!(
            "{who}'s battle records now live in DMY.DAT (disc LBA {lba}, {sectors} sectors) - \
             the PROT entry keeps its header, the disc did not grow"
        ));
    }
    for (entry, delta) in &rep.entries_reassigned {
        let who = PLAYERS
            .iter()
            .find(|p| p.entry == *entry)
            .map_or("?", |p| p.name);
        out.push(format!(
            "{who}'s player file (PROT {entry}) {} {} sector(s) - the boundary moved, the disc did not grow",
            if *delta >= 0 { "gained" } else { "gave up" },
            delta.abs()
        ));
    }
    if rep.relayout_sectors > 0 {
        out.push(format!(
            "disc relayout: player files grew by {} sector(s) (image is longer; no PPF)",
            rep.relayout_sectors
        ));
    }
    for n in fall_through_notes(&rep.models_no_room) {
        out.push(format!(
            "no room in the player files or the DMY.DAT annex for the model: {n} (allow a relayout to carry it over)"
        ));
    }
    for (c, id, why) in &rep.models_failed {
        out.push(format!(
            "{c} 0x{id:02X}: model not carried over ({why}); default look"
        ));
    }
    out
}

/// The character whose file carries `item` in its weapon section - the
/// donor for a transplant into `target`'s file. Ra-Seru level forms and
/// Ra-Seru arm records are never donors.
fn weapon_donor(files: &[FileCosts; 3], target: usize, item: u8) -> Option<usize> {
    if item as u32 <= equip_transplant::RA_SERU_MAX_ID {
        return None;
    }
    (0..PLAYERS.len()).find(|&cj| {
        cj != target
            && files[cj].items.contains_key(&item)
            && !files[cj].ra_seru_arms.contains(&item)
    })
}

/// The [`FallThrough`] note for `item` on `name`'s file: the section
/// default's costs for a weapon or footwear, none for the rest.
fn fall_through(name: &str, item: u8, slot: Option<EquipSlot>, d: &SectionDefaults) -> FallThrough {
    let fav = weapon_specialty::FAVORED_COST;
    let costs = match slot {
        Some(EquipSlot::Weapon) => vec![d.weapon.unwrap_or(fav)],
        Some(EquipSlot::Footwear) => vec![d.down.unwrap_or(fav), d.up.unwrap_or(fav)],
        _ => Vec::new(),
    };
    FallThrough {
        character: name.to_string(),
        item,
        slot: slot.map(slot_name).unwrap_or("-"),
        costs,
    }
}

/// A file's decoded records in chain order.
type RecordList = Vec<(u32, Vec<u8>)>;
/// One file's rebuild plan: `(data_base, records, folded cost edits)`.
type RecordPlan = (usize, RecordList, CostFold);

/// What folding one character's cost edits into a record list did - held
/// back until the rebuild is known to land, then committed to the report.
#[derive(Debug, Default, Clone)]
struct CostFold {
    changed: usize,
    unchanged: usize,
    no_section: Vec<(String, String)>,
}

impl CostFold {
    fn is_empty(&self) -> bool {
        self.changed == 0 && self.unchanged == 0 && self.no_section.is_empty()
    }
    fn commit(self, report: &mut EquipmentEditReport) {
        report.costs_changed += self.changed;
        report.costs_unchanged += self.unchanged;
        report.costs_no_section.extend(self.no_section);
    }
}

/// Apply character `ci`'s cost edits to a decoded record list (the file's
/// chain order, transplants included), so a rebuilt file carries them.
fn fold_costs(
    pack: &battle_data_pack::BattleDataPack,
    records: &mut [(u32, Vec<u8>)],
    ci: usize,
    costs: &[SwingCostEdit],
) -> Result<CostFold> {
    let mut fold = CostFold::default();
    let (_, weapon_section) = section_layout(pack);
    // Section of each list entry (`id == 0` closes a section).
    let mut secs = Vec::with_capacity(records.len());
    let mut sec = 0usize;
    for (id, _) in records.iter() {
        secs.push(sec);
        if *id == 0 {
            sec += 1;
        }
    }
    for e in costs.iter().filter(|e| e.character == ci) {
        let label = e.label();
        if e.cost < MIN_SWING_COST {
            bail!(
                "swing cost {} for {} {label} is below {MIN_SWING_COST}",
                e.cost,
                PLAYERS[ci].name
            );
        }
        let idx = match e.target {
            CostTarget::Item(id) => records
                .iter()
                .zip(&secs)
                .position(|((rid, _), s)| *rid == id as u32 && (2..=4).contains(s)),
            CostTarget::Default(kind) => {
                let want = section_of(kind, weapon_section);
                records
                    .iter()
                    .zip(&secs)
                    .position(|((rid, _), s)| *rid == 0 && *s == want)
            }
        };
        let Some(idx) = idx else {
            fold.no_section.push((PLAYERS[ci].name.to_string(), label));
            continue;
        };
        let Some(off) = cost_offset(&records[idx].1, e.record) else {
            fold.no_section.push((PLAYERS[ci].name.to_string(), label));
            continue;
        };
        if records[idx].1[off] == e.cost {
            fold.unchanged += 1;
        } else {
            records[idx].1[off] = e.cost;
            fold.changed += 1;
        }
    }
    Ok(fold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_tokens_parse_every_character_spelling() {
        for (s, ci) in [
            ("Vahn", 0),
            ("v", 0),
            ("NOA", 1),
            ("1", 1),
            ("g", 2),
            ("Gala", 2),
        ] {
            let e = parse_cost_token(&format!("{s}:0xBA=30")).unwrap();
            assert_eq!(
                (e.character, e.target, e.record, e.cost),
                (ci, CostTarget::Item(0xBA), SwingRecord::Primary, 30)
            );
        }
        let e = parse_cost_token("n:46=0x36").unwrap();
        assert_eq!((e.target, e.cost), (CostTarget::Item(46), 0x36));
        let e = parse_cost_token("Gala:default=42").unwrap();
        assert_eq!(
            (e.character, e.target, e.cost),
            (2, CostTarget::Default(SwingSection::Weapon), 42)
        );
        let e = parse_cost_token("v:raseru=42").unwrap();
        assert_eq!(e.target, CostTarget::Default(SwingSection::RaSeru));
        let e = parse_cost_token("g:feet:up=42").unwrap();
        assert_eq!(
            (e.target, e.record, e.label()),
            (
                CostTarget::Default(SwingSection::Footwear),
                SwingRecord::Up,
                "feet:up".to_string()
            )
        );
        let e = parse_cost_token("g:0x5E:UP=42").unwrap();
        assert_eq!(
            (e.record, e.label()),
            (SwingRecord::Up, "0x5E:up".to_string())
        );
        assert!(parse_cost_token("g:0x5E:left=42").is_err());
        assert!(parse_cost_token("g:0x5E:up:x=42").is_err());
        assert!(parse_cost_token("x:0xBA=30").is_err());
        assert!(parse_cost_token("v:0xBA").is_err());
        assert!(
            parse_cost_token("v:0xBA=6").is_err(),
            "below the drawable minimum"
        );
    }

    #[test]
    fn fall_through_notes_fold_characters_per_item() {
        let ft = |c: &str, item: u8, slot: &'static str, costs: Vec<u8>| FallThrough {
            character: c.into(),
            item,
            slot,
            costs,
        };
        let notes = fall_through_notes(&[
            ft("Noa", 0x34, "head", vec![]),
            ft("Gala", 0x34, "head", vec![]),
            ft("Noa", 0xBA, "weapon", vec![54]),
            ft("Gala", 0xBA, "weapon", vec![30]),
            ft("Gala", 0x63, "footwear", vec![30, 44]),
        ]);
        assert_eq!(
            notes,
            vec![
                "Noa, Gala can now equip 0x34 but has no battle section for it: default look",
                "Noa can now equip 0xBA but has no battle section for it: default look, 54-AP swing (the weapon Default row)",
                "Gala can now equip 0xBA but has no battle section for it: default look, 30-AP swing (the weapon Default row)",
                "Gala can now equip 0x63 but has no battle section for it: default look, 30-AP Down / 44-AP Up kick (the footwear Default row)",
            ]
        );
    }

    #[test]
    fn owner_tokens_and_labels_round_trip() {
        assert_eq!(parse_owner_mask("VNG"), Some(7));
        assert_eq!(parse_owner_mask("gn"), Some(6));
        assert_eq!(parse_owner_mask("any"), Some(7));
        assert_eq!(parse_owner_mask("none"), Some(0));
        assert_eq!(parse_owner_mask("5"), Some(5));
        assert_eq!(parse_owner_mask("9"), None);
        assert_eq!(parse_owner_mask("x"), None);
        assert_eq!(owner_label(7), "any");
        assert_eq!(owner_label(5), "VG");
        assert_eq!(owner_label(0), "-");
        let e = parse_owner_token("0xBA=VN").unwrap();
        assert_eq!((e.item_id, e.mask), (0xBA, 3));
    }

    #[test]
    fn lists_split_on_any_separator() {
        let e = parse_edit_lists("v:0xBA=30, n:0x2E=30;g:0x22=30", "0xBA=any 0x24=G").unwrap();
        assert_eq!(e.costs.len(), 3);
        assert_eq!(e.owners.len(), 2);
        assert!(parse_edit_lists("", "").unwrap().is_empty());
    }
}
