//! Battle-data table randomizers: move power, element affinity, spell MP cost, equipment stat bonuses.

use super::*;

/// Read the special-attack move-power column (the per-record `+0x00` power
/// halfword) from PROT 0898, for the read-only listing / the randomizer input.
/// Returns `None` if the battle-action overlay entry can't be parsed.
pub fn current_move_powers(patcher: &DiscPatcher) -> Result<Option<Vec<i16>>> {
    let entry = patcher
        .read_entry(legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    Ok(legaia_asset::move_power::parse(&entry)
        .map(|recs| recs.iter().map(|r| r.power_raw).collect()))
}

/// Randomize the special-attack power table (see [`crate::move_power`]). Rewrites
/// only each record's `+0x00` power halfword in PROT 0898 - a same-size raw edit
/// (no LZS). Returns the number of power values that changed.
pub fn randomize_move_powers(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<usize> {
    use legaia_asset::move_power::{
        BATTLE_ACTION_OVERLAY_PROT_INDEX, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
    };
    let mut entry = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    let Some(records) = legaia_asset::move_power::parse(&entry) else {
        return Ok(0);
    };
    // Redistribute powers only among populated records. Empty (all-zero) records
    // - including the index-0 sentinel `parse` keys on - must stay zero, so a
    // move's power is never handed to an unused slot (nor a real move zeroed).
    let populated: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.is_empty())
        .map(|(i, _)| i)
        .collect();
    let current: Vec<i16> = populated.iter().map(|&i| records[i].power_raw).collect();
    let plan = crate::move_power::plan_powers(&current, seed, mode);

    let table = MOVE_POWER_TABLE_FILE_OFFSET;
    let span = records.len() * MOVE_POWER_RECORD_STRIDE;
    let before = entry[table..table + span].to_vec();
    let mut changed = 0usize;
    for (k, &i) in populated.iter().enumerate() {
        if current[k] == plan[k] {
            continue;
        }
        let off = table + i * MOVE_POWER_RECORD_STRIDE;
        entry[off..off + 2].copy_from_slice(&plan[k].to_le_bytes());
        changed += 1;
    }
    let after = &entry[table..table + span];
    if after != before.as_slice() {
        patcher
            .patch_prot_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX, table as u64, after)
            .context("write move-power table")?;
    }
    Ok(changed)
}

/// Read the 8×8 element-affinity matrix (PROT 0898), flattened row-major
/// (`attacker * 8 + defender`), for the read-only listing / randomizer input.
/// `None` if the entry can't parse.
pub fn current_affinity_matrix(
    patcher: &DiscPatcher,
) -> Result<Option<[u8; crate::element_affinity::MATRIX_CELLS]>> {
    let entry = patcher
        .read_entry(legaia_asset::element_affinity::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    let Some(aff) = legaia_asset::element_affinity::ElementAffinity::parse(&entry) else {
        return Ok(None);
    };
    let ec = legaia_asset::element_affinity::ELEMENT_COUNT;
    let mut flat = [0u8; crate::element_affinity::MATRIX_CELLS];
    for (atk, row) in aff.matrix.iter().enumerate() {
        flat[atk * ec..atk * ec + row.len()].copy_from_slice(row);
    }
    Ok(Some(flat))
}

/// Randomize the element-affinity matrix (see [`crate::element_affinity`]).
/// Rewrites the 64 matrix bytes in PROT 0898 - a same-size raw edit (no LZS).
/// Returns the number of cells that changed.
pub fn randomize_element_affinity(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<usize> {
    use legaia_asset::element_affinity::{
        AFFINITY_MATRIX_FILE_OFFSET, BATTLE_ACTION_OVERLAY_PROT_INDEX,
    };
    let Some(current) = current_affinity_matrix(patcher)? else {
        return Ok(0);
    };
    let plan = crate::element_affinity::plan_matrix(&current, seed, mode);
    let changed = current.iter().zip(&plan).filter(|(a, b)| a != b).count();
    if plan != current {
        patcher
            .patch_prot_entry(
                BATTLE_ACTION_OVERLAY_PROT_INDEX,
                AFFINITY_MATRIX_FILE_OFFSET as u64,
                &plan,
            )
            .context("write element-affinity matrix")?;
    }
    Ok(changed)
}

/// One spell's current MP cost, for the read-only listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellCost {
    /// Spell id (index into the spell table).
    pub id: u8,
    /// Display name.
    pub name: String,
    /// MP cost (`stats +3`).
    pub mp: u8,
}

/// Read every named, costed spell's id + name + MP cost from the SCUS spell
/// table - the population the MP-cost randomizer redistributes. Empty / unnamed
/// internal-tier slots and zero-cost spells are excluded. `None` if SCUS / its
/// spell table can't be parsed.
pub fn current_spell_costs(patcher: &DiscPatcher) -> Result<Option<Vec<SpellCost>>> {
    let Some(scus) = patcher.read_named_file(SCUS_NAME) else {
        return Ok(None);
    };
    let Some(table) = legaia_asset::spell_names::SpellNameTable::from_scus(&scus) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for id in 0..=u8::MAX {
        let Some(entry) = table.entry(id) else { break };
        if let Some(name) = entry.name.as_deref().filter(|_| entry.mp > 0) {
            out.push(SpellCost {
                id,
                name: name.to_string(),
                mp: entry.mp,
            });
        }
    }
    Ok(Some(out))
}

/// Randomize spell MP costs in the SCUS spell table (see [`crate::spell_cost`]).
/// Rewrites only the `+3` cost byte of each named, costed spell - a same-size
/// in-place SCUS patch. Returns the number of spells whose cost changed.
pub fn randomize_spell_costs(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<usize> {
    use legaia_asset::spell_names::{RECORD_STRIDE, SPELL_COUNT};
    let Some(scus) = patcher.read_named_file(SCUS_NAME) else {
        return Ok(0);
    };
    let Some(table_off) = legaia_asset::spell_names::stats_file_offset(&scus) else {
        return Ok(0);
    };
    let span = SPELL_COUNT * RECORD_STRIDE;
    let Some(mut table) = scus.get(table_off..table_off + span).map(<[u8]>::to_vec) else {
        return Ok(0);
    };

    // Randomizable population: named spells with a non-zero MP cost.
    let names = legaia_asset::spell_names::SpellNameTable::from_scus(&scus);
    let ids: Vec<usize> = (0..SPELL_COUNT)
        .filter(|&id| {
            let cost = table[id * RECORD_STRIDE + 3];
            let named = names
                .as_ref()
                .and_then(|t| t.name(id as u8))
                .is_some_and(|n| !n.is_empty());
            cost > 0 && named
        })
        .collect();
    let current: Vec<u8> = ids
        .iter()
        .map(|&id| table[id * RECORD_STRIDE + 3])
        .collect();
    let plan = crate::spell_cost::plan_costs(&current, seed, mode);

    let mut changed = 0usize;
    for (k, &id) in ids.iter().enumerate() {
        let off = id * RECORD_STRIDE + 3;
        if table[off] != plan[k] {
            table[off] = plan[k];
            changed += 1;
        }
    }
    if changed > 0 {
        patcher
            .patch_named_file(SCUS_NAME, table_off as u64, &table)
            .context("write spell MP-cost table")?;
    }
    Ok(changed)
}

/// One equipment stat-bonus row, for the read-only listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipBonusRow {
    /// Bonus-table row index (the `bonus_index` an item resolves to).
    pub row: usize,
    /// Slot category the row belongs to (`body` / `head` / `weapon` / `footwear`).
    pub slot: &'static str,
    /// The five stat bonuses `[INT, ATK, UDF, LDF, SPD]` (`+0..+4`).
    pub stats: [u8; 5],
    /// 1-based item ids that resolve to this row (a row can be shared).
    pub items: Vec<u8>,
}

/// Map an [`legaia_asset::equip_stats::EquipSlot`] to its CLI label.
fn equip_slot_name(slot: legaia_asset::equip_stats::EquipSlot) -> &'static str {
    use legaia_asset::equip_stats::EquipSlot;
    match slot {
        EquipSlot::Body => "body",
        EquipSlot::Head => "head",
        EquipSlot::Weapon => "weapon",
        EquipSlot::Footwear => "footwear",
    }
}

/// Read the equipment stat-bonus table (`DAT_80074F68`) off the disc's SCUS, in
/// row order, each with its slot category, decoded stats, and the item ids that
/// reference it. The randomizable population (see [`crate::equip_bonus`]).
/// `None` if SCUS / its bonus table can't be parsed.
pub fn current_equip_bonuses(patcher: &DiscPatcher) -> Result<Option<Vec<EquipBonusRow>>> {
    let Some(scus) = patcher.read_named_file(SCUS_NAME) else {
        return Ok(None);
    };
    let Some(table) = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus) else {
        return Ok(None);
    };
    let items = table.items_for_rows();
    let rows = table
        .rows()
        .iter()
        .enumerate()
        .map(|(i, b)| EquipBonusRow {
            row: i,
            slot: equip_slot_name(b.slot()),
            stats: b.stat_bonus(),
            items: items.get(i).cloned().unwrap_or_default(),
        })
        .collect();
    Ok(Some(rows))
}

/// Randomize the equipment passive stat bonuses (see [`crate::equip_bonus`]).
/// Rewrites only the `+0..+4` stat tuple of each bonus row that at least one
/// equippable item references, reassigning it **within its slot category** - a
/// same-size in-place SCUS patch. The equip-character mask, accessory passive,
/// and slot type stay welded to their row. Returns the number of rows changed.
///
/// Operates on bonus rows (not item ids): several items can share one record,
/// so a per-id rewrite would double-edit a shared record. Rows no equippable
/// item reaches are left untouched, so an unused/garbage row can't hand a real
/// item a junk stat tuple.
pub fn randomize_equip_bonuses(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<usize> {
    use legaia_asset::equip_stats::{BONUS_STRIDE, EquipStatTable, bonus_table_file_offset};
    let Some(scus) = patcher.read_named_file(SCUS_NAME) else {
        return Ok(0);
    };
    let Some(table) = EquipStatTable::from_scus(&scus) else {
        return Ok(0);
    };
    let Some(off) = bonus_table_file_offset(&scus) else {
        return Ok(0);
    };

    let all_rows: Vec<[u8; 8]> = table.rows().iter().map(|b| b.raw).collect();
    let items = table.items_for_rows();
    // Only rows an equippable item actually resolves to participate.
    let participating: Vec<usize> = (0..all_rows.len())
        .filter(|&i| !items[i].is_empty())
        .collect();
    if participating.is_empty() {
        return Ok(0);
    }
    let sub: Vec<[u8; 8]> = participating.iter().map(|&i| all_rows[i]).collect();
    let planned = crate::equip_bonus::plan_bonus_shuffle(&sub, seed, mode);

    let mut new_rows = all_rows.clone();
    let mut changed = 0usize;
    for (k, &i) in participating.iter().enumerate() {
        if planned[k] != all_rows[i] {
            new_rows[i] = planned[k];
            changed += 1;
        }
    }
    if changed == 0 {
        return Ok(0);
    }
    let mut bytes = Vec::with_capacity(all_rows.len() * BONUS_STRIDE);
    for r in &new_rows {
        bytes.extend_from_slice(r);
    }
    patcher
        .patch_named_file(SCUS_NAME, off as u64, &bytes)
        .context("write equipment stat-bonus table")?;
    Ok(changed)
}
