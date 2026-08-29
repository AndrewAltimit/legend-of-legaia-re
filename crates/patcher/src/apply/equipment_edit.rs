//! Manual equipment editor: per-(character, weapon) **swing cost** and
//! per-item **equip owner** edits.
//!
//! Two disc tables, both documented in
//! [`docs/subsystems/arts-command-gauge.md`](../../../../docs/subsystems/arts-command-gauge.md):
//!
//! - **Swing cost** - the AP a direction command charges on the Arts gauge,
//!   which is also the pennant width (`cost - 6` px). It is an authored byte
//!   inside each weapon's LZS section of that character's player battle file
//!   (`section[+0x04]` swing record `+0x74`), so the same weapon carries a
//!   different cost per character (favored `0x1E`, off-class `0x2A`, far
//!   `0x36`; the Astral Sword is Vahn's one `0x36`). An edit decompresses the
//!   section, rewrites the byte, and recompresses in place - the same path the
//!   [`crate::weapon_specialty`] randomizer takes, but to a value the modder
//!   chooses.
//! - **Equip owner** - the `+6` character mask of the item's row in the SCUS
//!   equipment stat-bonus table (`DAT_80074F68`; bit `1` Vahn, `2` Noa,
//!   `4` Gala). The equip screen gates on it (`FUN_8003fb10`), so this changes
//!   who may equip the item. It does **not** add a battle model or swing record
//!   to a file that lacks one: a character whose player file has no section
//!   for the item falls through to the section default at battle load
//!   (default appearance, the default record's own cost - retail `0x1E`).
//!   The report names those combinations so the page can say so, and the
//!   default record itself is addressable as item id `0` (`Vahn:default=30`)
//!   so the fall-through price can be set - shared by every unlisted weapon
//!   that character equips, and by the unarmed swing.
//!
//! Both edits are same-size, in-place, and idempotent.

use anyhow::{Context, Result, bail};
use legaia_asset::battle_data_pack;
use legaia_asset::equip_stats::{BONUS_STRIDE, EquipSlot, EquipStatTable, bonus_table_file_offset};

use crate::disc::DiscPatcher;
use crate::weapon_specialty::{self, PLAYERS, arm_cost_offset};

use super::SCUS_NAME;

/// Smallest swing cost the gauge can draw: the pennant body is `cost - 6`
/// pixels wide, so anything lower has no body.
pub const MIN_SWING_COST: u8 = 7;

/// Pseudo item id naming a character's weapon-section **default** record: the
/// bare-hand section the battle loader splices when the equipped weapon has no
/// section of its own (an unlisted weapon, or nothing equipped).
pub const DEFAULT_WEAPON: u8 = 0;

/// Equip-owner mask bits (`+6` byte of a bonus row).
pub const MASK_VAHN: u8 = 1;
/// Equip-owner mask bit for Noa.
pub const MASK_NOA: u8 = 2;
/// Equip-owner mask bit for Gala.
pub const MASK_GALA: u8 = 4;

/// One swing-cost edit: `character` indexes [`PLAYERS`] (0 Vahn, 1 Noa, 2 Gala).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwingCostEdit {
    /// Index into [`PLAYERS`].
    pub character: usize,
    /// Equippable weapon id (the descriptor key of its section), or
    /// [`DEFAULT_WEAPON`] for the character's weapon-section default record.
    pub item_id: u8,
    /// New `+0x74` value (`>= MIN_SWING_COST`).
    pub cost: u8,
}

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

/// Parse `CHAR:ITEM=COST` (`Vahn:0xBA=30`, `n:0x2E=0x1E`, `2:51=42`); `ITEM`
/// may be `default` (or `0`) for the character's default weapon record.
pub fn parse_cost_token(tok: &str) -> Result<SwingCostEdit> {
    let (lhs, cost) = tok
        .split_once('=')
        .with_context(|| format!("swing cost `{tok}`: expected CHAR:ITEM=COST"))?;
    let (ch, item) = lhs
        .split_once(':')
        .with_context(|| format!("swing cost `{tok}`: expected CHAR:ITEM=COST"))?;
    let character =
        character_index(ch).with_context(|| format!("swing cost `{tok}`: unknown character"))?;
    let item_id = match item.trim().to_ascii_lowercase().as_str() {
        "default" | "fist" | "unarmed" => DEFAULT_WEAPON,
        _ => parse_u8(item).with_context(|| format!("swing cost `{tok}`: bad item id"))?,
    };
    let cost = parse_u8(cost).with_context(|| format!("swing cost `{tok}`: bad cost"))?;
    if cost < MIN_SWING_COST {
        bail!("swing cost `{tok}`: cost must be at least {MIN_SWING_COST}");
    }
    Ok(SwingCostEdit {
        character,
        item_id,
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
    /// Bonus-table row index (several items may share one).
    pub row: usize,
    /// Other item ids sharing the same bonus row (an owner edit moves them too).
    pub shares_row_with: Vec<u8>,
    /// Current owner mask (`+6`, low three bits).
    pub mask: u8,
    /// Attack bonus (`+1`).
    pub atk: u8,
    /// Per-character swing cost for weapons: `Some(cost)` when that character's
    /// player file has a section for the item, `None` otherwise. Always
    /// `[None; 3]` for non-weapons.
    pub costs: [Option<u8>; 3],
}

fn slot_name(s: EquipSlot) -> &'static str {
    match s {
        EquipSlot::Body => "body",
        EquipSlot::Head => "head",
        EquipSlot::Weapon => "weapon",
        EquipSlot::Footwear => "footwear",
    }
}

/// Per character, `item id -> current swing cost` for every section that
/// carries a swing record.
/// Descriptor index of the weapon section's `id = 0` default record: the
/// terminator of the id group that carries weapon ids (section 2 in Vahn's and
/// Gala's files, section 3 in Noa's).
fn weapon_section_default_index(pack: &battle_data_pack::BattleDataPack) -> Option<usize> {
    let mut group_has_weapon = false;
    for (idx, rec) in pack.records.iter().enumerate() {
        if rec.id == 0 {
            if group_has_weapon {
                return Some(idx);
            }
            group_has_weapon = false;
        } else if rec.id <= 0xFF && weapon_specialty::weapon_family(rec.id as u8).is_some() {
            group_has_weapon = true;
        }
    }
    None
}

/// Descriptor index of `item`'s section in `pack`, with [`DEFAULT_WEAPON`]
/// resolving to the weapon section's default record.
fn section_index(pack: &battle_data_pack::BattleDataPack, item: u8) -> Option<usize> {
    if item == DEFAULT_WEAPON {
        weapon_section_default_index(pack)
    } else {
        pack.records.iter().position(|r| r.id == item as u32)
    }
}

/// Per character: item id -> swing cost, with key [`DEFAULT_WEAPON`] carrying
/// the weapon-section default record's cost.
fn read_swing_costs(patcher: &DiscPatcher) -> [std::collections::BTreeMap<u8, u8>; 3] {
    let mut out: [std::collections::BTreeMap<u8, u8>; 3] = Default::default();
    for (ci, player) in PLAYERS.iter().enumerate() {
        let Ok(buf) = patcher.read_entry(player.entry) else {
            continue;
        };
        let Some(pack) = battle_data_pack::detect(&buf) else {
            continue;
        };
        let default_idx = weapon_section_default_index(&pack);
        for (idx, rec) in pack.records.iter().enumerate() {
            if rec.id > 0xFF || (rec.id == 0 && Some(idx) != default_idx) {
                continue;
            }
            let Ok(dec) = battle_data_pack::decode_record(&buf, &pack, idx) else {
                continue;
            };
            let Some(off) = arm_cost_offset(&dec.bytes) else {
                continue;
            };
            out[ci].insert(rec.id as u8, dec.bytes[off]);
        }
    }
    out
}

/// The table the editor displays: every equippable item, plus each
/// character's default weapon-record cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentTable {
    /// One row per equippable item, in id order.
    pub rows: Vec<EquipmentRow>,
    /// Per character ([`PLAYERS`] order): the swing cost of the weapon-section
    /// default record - what an unlisted weapon (or no weapon) costs.
    pub default_costs: [Option<u8>; 3],
}

/// Read every equippable item with its owner mask and, for weapons, its
/// per-character swing cost - the table the editor displays. `None` when the
/// SCUS equipment table can't be parsed.
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
    let costs = read_swing_costs(patcher);
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
        let per_char = if slot == EquipSlot::Weapon {
            std::array::from_fn(|ci| costs[ci].get(&id).copied())
        } else {
            [None; 3]
        };
        out.push(EquipmentRow {
            id,
            name,
            slot: slot_name(slot),
            row,
            shares_row_with: items_for_rows[row]
                .iter()
                .copied()
                .filter(|&o| o != id)
                .collect(),
            mask: b.equip_mask() & 7,
            atk: b.attack(),
            costs: per_char,
        });
    }
    Ok(Some(EquipmentTable {
        rows: out,
        default_costs: std::array::from_fn(|ci| costs[ci].get(&DEFAULT_WEAPON).copied()),
    }))
}

/// What [`apply_equipment_edits`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EquipmentEditReport {
    /// Swing-cost sections rewritten.
    pub costs_changed: usize,
    /// Swing-cost edits that were already at the requested value.
    pub costs_unchanged: usize,
    /// `(character, item)` pairs whose player file has no section for the item.
    pub costs_no_section: Vec<(String, u8)>,
    /// `(character, item)` pairs whose section would not recompress into its slot.
    pub costs_skipped_fit: Vec<(String, u8)>,
    /// Owner rows rewritten.
    pub owners_changed: usize,
    /// Owner edits whose item is not equipment on this disc.
    pub owners_not_equipment: Vec<u8>,
    /// `(item, [other items on the same row])` for owner edits that moved siblings.
    pub owners_shared_rows: Vec<(u8, Vec<u8>)>,
    /// `(character, item, default cost)` for owner edits that newly allow an
    /// item although that character's player file has no section for it: they
    /// equip it but fall through to the default weapon record in battle, at
    /// that record's cost (after any edit to it in this same pass).
    pub owners_without_section: Vec<(String, u8, u8)>,
}

/// Apply the edit set. Swing costs go through the LZS re-pack path (a section
/// that does not fit is reported, not written); owner masks are a same-size
/// SCUS patch. Idempotent.
pub fn apply_equipment_edits(
    patcher: &mut DiscPatcher,
    edits: &EquipmentEdits,
) -> Result<EquipmentEditReport> {
    let mut report = EquipmentEditReport::default();

    // --- swing costs -------------------------------------------------------
    for (ci, player) in PLAYERS.iter().enumerate() {
        let mine: Vec<&SwingCostEdit> = edits.costs.iter().filter(|e| e.character == ci).collect();
        if mine.is_empty() {
            continue;
        }
        let buf = patcher
            .read_entry(player.entry)
            .with_context(|| format!("read {} player file", player.name))?;
        let Some(pack) = battle_data_pack::detect(&buf) else {
            bail!(
                "{} player file (PROT {}) is not a battle data pack",
                player.name,
                player.entry
            );
        };
        for e in mine {
            if e.cost < MIN_SWING_COST {
                bail!(
                    "swing cost {} for {} item {:#04x} is below {MIN_SWING_COST}",
                    e.cost,
                    player.name,
                    e.item_id
                );
            }
            let Some(idx) = section_index(&pack, e.item_id) else {
                report
                    .costs_no_section
                    .push((player.name.to_string(), e.item_id));
                continue;
            };
            let rec = &pack.records[idx];
            let dec = battle_data_pack::decode_record(&buf, &pack, idx)
                .with_context(|| format!("decode {} item {:#04x}", player.name, e.item_id))?;
            let Some(off) = arm_cost_offset(&dec.bytes) else {
                report
                    .costs_no_section
                    .push((player.name.to_string(), e.item_id));
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
                    .push((player.name.to_string(), e.item_id));
                continue;
            }
            let stream_off = rec.file_offset(pack.data_base) + 4;
            patcher
                .patch_prot_entry(player.entry, stream_off as u64, &recompressed)
                .with_context(|| {
                    format!(
                        "write swing cost for {} item {:#04x}",
                        player.name, e.item_id
                    )
                })?;
            report.costs_changed += 1;
        }
    }

    // --- owners ------------------------------------------------------------
    if !edits.owners.is_empty() {
        let scus = patcher
            .read_named_file(SCUS_NAME)
            .context("SCUS_942.54 not found")?;
        let table = EquipStatTable::from_scus(&scus).context("equipment bonus table")?;
        let off = bonus_table_file_offset(&scus).context("equipment bonus table offset")?;
        let items_for_rows = table.items_for_rows();
        let mut rows: Vec<[u8; 8]> = table.rows().iter().map(|b| b.raw).collect();
        let sections = read_swing_costs(patcher);
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
            if table.bonus(e.item_id).map(|b| b.slot()) == Some(EquipSlot::Weapon) {
                for (ci, player) in PLAYERS.iter().enumerate() {
                    let bit = 1u8 << ci;
                    if new & bit != 0 && old & bit == 0 && !sections[ci].contains_key(&e.item_id) {
                        let def = sections[ci]
                            .get(&DEFAULT_WEAPON)
                            .copied()
                            .unwrap_or(weapon_specialty::FAVORED_COST);
                        report.owners_without_section.push((
                            player.name.to_string(),
                            e.item_id,
                            def,
                        ));
                    }
                }
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

    Ok(report)
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
            assert_eq!((e.character, e.item_id, e.cost), (ci, 0xBA, 30));
        }
        let e = parse_cost_token("n:46=0x36").unwrap();
        assert_eq!((e.item_id, e.cost), (46, 0x36));
        let e = parse_cost_token("Gala:default=42").unwrap();
        assert_eq!((e.character, e.item_id, e.cost), (2, DEFAULT_WEAPON, 42));
        assert!(parse_cost_token("x:0xBA=30").is_err());
        assert!(parse_cost_token("v:0xBA").is_err());
        assert!(
            parse_cost_token("v:0xBA=6").is_err(),
            "below the drawable minimum"
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
