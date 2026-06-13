//! Build [`EncounterTable`]s straight from a scene's on-disc MAN bytes.
//!
//! Bridges [`legaia_asset::man_section`] (the multi-section MAN walker that
//! mirrors `FUN_8003AEB0` / `FUN_8003A110`) into the runtime encounter
//! types. The retail engine doesn't synthesize encounter rates the way the
//! pattern-fallback [`EncounterRegistry`](crate::encounter_registry) does;
//! it reads them out of the MAN's encounter section. This module is the
//! glue that lets the boot path pull real disc-resident tables in place of
//! the synthetic fallbacks.
//!
//! ## Mapping
//!
//! - Each `formation_record` in the MAN encounter section becomes one
//!   [`EncounterEntry`]. The entry's `formation_id` is the formation row's
//!   index (matching the random-encounter trigger reader's
//!   `formation_range_base + roll` indexing at `FUN_801D9E1C`); engines
//!   that need an integer monster-formation id register the matching
//!   [`crate::monster_catalog::FormationDef`] separately.
//! - Each formation's weight is the count of `region_record`s whose
//!   `[formation_range_base, formation_range_base + formation_range_count)`
//!   half-open range covers that formation row. Formations no region
//!   covers get weight 1 so they remain rollable when the engine selects
//!   them directly (scripted encounters, debug menus).
//! - The table's `trigger_rate_q8` is the mean of the per-region
//!   `rate_increment` bytes, clamped to `1..=255`. Retail computes per-
//!   region rates on the fly (the active region's `pbVar9[4]` increments
//!   `_DAT_8007B5FC`); using the mean here is a reasonable summary that
//!   keeps engines that don't yet route the player position through the
//!   region AABB lookup behaving in-character. Engines that DO route the
//!   player position should instead build the region-keyed table with
//!   [`crate::region_encounter::region_encounter_table_from_man`] and roll
//!   against the active region via
//!   [`crate::region_encounter::RegionEncounterTracker`] — the per-region
//!   rate + formation-range model the world-map path already uses (see
//!   [`crate::world::World::set_world_map_regions`]).
//!
//! See [`docs/formats/encounter.md`](../../../docs/formats/encounter.md)
//! for the byte-level MAN layout.

use crate::encounter::{EncounterEntry, EncounterTable};
use crate::monster_catalog::{FormationDef, FormationSlot};
use legaia_asset::man_section;

/// Decode a MAN buffer and synthesize an [`EncounterTable`] for the scene.
///
/// Returns `None` when:
/// - The MAN buffer is too short or has an invalid multi-section header.
/// - The encounter section's interior fails to parse.
/// - The encounter section declares zero formations (no random encounters
///   possible).
///
/// Returns `Some(table)` with a populated entries vec and a trigger rate
/// derived from the per-region rate increments.
pub fn encounter_table_from_man(scene_label: &str, man_bytes: &[u8]) -> Option<EncounterTable> {
    let man = man_section::parse(man_bytes).ok()?;
    let body = man.encounter_section_body(man_bytes)?;
    let es = man_section::parse_encounter_section(body).ok()?;
    if es.formation_count == 0 {
        return None;
    }

    // Aggregate per-formation weights from the region table.
    let mut weights = vec![0u32; es.formation_count as usize];
    let mut rate_sum: u32 = 0;
    let mut rate_n: u32 = 0;
    for region in man_section::region_records(body, &es).flatten() {
        let base = region.formation_range_base as usize;
        let count = region.formation_range_count as usize;
        // Skip degenerate / out-of-bounds region ranges silently - the
        // retail reader bounds-checks at roll time via the modulo on the
        // weighted pick; a malformed region in the corpus shouldn't kill
        // the whole table.
        for idx in base..base.saturating_add(count) {
            if let Some(slot) = weights.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        }
        rate_sum = rate_sum.saturating_add(region.rate_increment as u32);
        rate_n += 1;
    }

    // Mean rate, clamped to a non-zero u8. When there are zero regions,
    // fall back to the retail "moderate" base of 8/256.
    let rate_q8 = rate_sum
        .checked_div(rate_n)
        .map_or(8, |mean| mean.clamp(1, 255) as u8);

    let mut table = EncounterTable::new(scene_label);
    table.set_trigger_rate(rate_q8);
    for (i, f) in man_section::formation_records(body, &es).enumerate() {
        let Some(f) = f else {
            continue;
        };
        if f.monster_count == 0 {
            // A zero-monster formation never spawns anything; the retail
            // reader clears the cell and yields. Skip it from the table
            // so the weighted pick never selects it.
            continue;
        }
        let weight = weights.get(i).copied().unwrap_or(1).max(1);
        // The runtime formation-id slot is the row index (NOT the
        // synthetic-from-ids hash). Engines that route through the
        // formation table by id can swap to the synthetic id by
        // converting the row to a [`crate::encounter_record::EncounterRecord`]
        // first.
        let weight_u16 = weight.min(u16::MAX as u32) as u16;
        table.push(EncounterEntry::new(i as u16, weight_u16));
    }

    if table.entries.is_empty() {
        return None;
    }
    Some(table)
}

/// Build the per-row [`FormationDef`]s for a scene's MAN encounter section.
///
/// Each formation row becomes one def whose `formation_id` is the row
/// index - matching the [`EncounterEntry::formation_id`] values that
/// [`encounter_table_from_man`] emits, so the installed encounter table's
/// entries resolve straight through `formation_table` at battle-load.
/// Slots are the row's monster ids (the battle max is 4); zero-monster
/// rows are skipped (they never spawn).
///
/// Returns an empty vec when the MAN header / encounter section fails to
/// parse - the caller treats that the same as "no MAN encounters".
pub fn formation_defs_from_man(man_bytes: &[u8]) -> Vec<FormationDef> {
    let Ok(man) = man_section::parse(man_bytes) else {
        return Vec::new();
    };
    let Some(body) = man.encounter_section_body(man_bytes) else {
        return Vec::new();
    };
    let Ok(es) = man_section::parse_encounter_section(body) else {
        return Vec::new();
    };
    let mut defs = Vec::new();
    for (i, f) in man_section::formation_records(body, &es).enumerate() {
        let Some(f) = f else {
            continue;
        };
        if f.monster_count == 0 {
            continue;
        }
        let n = (f.monster_count as usize).min(4);
        let slots: Vec<FormationSlot> = f.monster_ids[..n]
            .iter()
            .map(|&id| FormationSlot::new(id as u16))
            .collect();
        defs.push(FormationDef::new(i as u16, slots));
    }
    defs
}

/// Resolve both the [`EncounterTable`] and its per-row [`FormationDef`]s for
/// a scene in one call - the pair the field scene-entry path installs via
/// [`crate::world::World::install_man_encounter`].
///
/// Returns `None` when [`encounter_table_from_man`] does (invalid MAN, or
/// no rollable formations).
pub fn scene_encounter_from_man(
    scene_label: &str,
    man_bytes: &[u8],
) -> Option<(EncounterTable, Vec<FormationDef>)> {
    let table = encounter_table_from_man(scene_label, man_bytes)?;
    let defs = formation_defs_from_man(man_bytes);
    Some((table, defs))
}

/// Resolve a formation row to its [`crate::encounter_record::EncounterRecord`]
/// shape. Useful for engines that have a roll result and want to install
/// it via [`crate::world::World::install_encounter_from_record`].
pub fn formation_record_for_row(
    man_bytes: &[u8],
    row_index: usize,
) -> Option<crate::encounter_record::EncounterRecord> {
    let man = man_section::parse(man_bytes).ok()?;
    let body = man.encounter_section_body(man_bytes)?;
    let es = man_section::parse_encounter_section(body).ok()?;
    if row_index >= es.formation_count as usize {
        return None;
    }
    let stride = es.formation_stride as usize;
    let (fs, fe) = es.formation_range;
    let p = fs + row_index * stride;
    if p + stride > fe {
        return None;
    }
    let raw = &body[p..p + stride];
    // EncounterRecord::parse reads bytes [0..3] as the opcode header,
    // [3] as count, [4..4+count] as ids - matches the on-disc formation
    // row prefix exactly.
    crate::encounter_record::EncounterRecord::parse(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encounter_record::EncounterRecord;

    /// Hand-build a minimal MAN buffer with one section-0 encounter:
    /// 2 formations, 1 region covering both. Mirrors the on-disc
    /// header math (records partition + u24[0x28] section-0 offset).
    fn build_test_man() -> Vec<u8> {
        let mut buf = Vec::new();
        // Header (43 bytes total to RECORDS_BEGIN_OFFSET = 0x2B)
        buf.extend_from_slice(&[0u8; 0x2B]);
        // No records, so data region starts right at 0x2B and section 0
        // is at data + u24[0x28] = 0.
        buf[0x22] = 0; // N0 = 0
        buf[0x24] = 0; // N1
        buf[0x26] = 0; // N2
        // u24[0x28] = 0
        buf[0x28] = 0;
        buf[0x29] = 0;
        buf[0x2A] = 0;

        // Section 0: header + 2 formations + 0 conditions + 1 region.
        // Body: f_stride=8, c_stride=4, r_stride=12, f_count=2
        let mut section_0_body = vec![8u8, 4, 12, 2];
        // Formation 0: count=1, id=4
        section_0_body.extend_from_slice(&[0, 0, 0, 1, 4, 0, 0, 0]);
        // Formation 1: count=2, id=4,4
        section_0_body.extend_from_slice(&[0, 0, 0, 2, 4, 4, 0, 0]);
        // condition_count = 0
        section_0_body.push(0);
        // region_count = 1
        section_0_body.push(1);
        // Region 0: aabb 0..40 x 0..40, rate 32, range [0..2)
        section_0_body.extend_from_slice(&[0, 0, 40, 40, 32, 0, 0, 2, 0, 0, 0, 0]);

        // Section-0 length prefix + body.
        let s0_len = section_0_body.len() as u32;
        buf.extend_from_slice(&[
            (s0_len & 0xFF) as u8,
            (s0_len >> 8) as u8,
            (s0_len >> 16) as u8,
        ]);
        buf.extend_from_slice(&section_0_body);

        // Sections 1..=4: all empty (length 0).
        for _ in 0..4 {
            buf.extend_from_slice(&[0, 0, 0]);
        }
        // Section 5: zero terminator (already covered by the loop, but
        // the walker treats the 6th slot as terminator; needs to exist).
        buf.extend_from_slice(&[0, 0, 0]);

        buf
    }

    #[test]
    fn synth_man_yields_two_formation_entries_with_region_weight() {
        let man = build_test_man();
        let table = encounter_table_from_man("test", &man).expect("table built");
        assert_eq!(table.scene_label, "test");
        assert_eq!(table.entries.len(), 2);
        // Region covers both formations: each gets weight 1.
        assert_eq!(table.entries[0].weight, 1);
        assert_eq!(table.entries[1].weight, 1);
        // Trigger rate = mean region rate = 32.
        assert_eq!(table.trigger_rate_q8, 32);
        // Formation ids are row indices (NOT the synthetic-from-ids hash).
        assert_eq!(table.entries[0].formation_id, 0);
        assert_eq!(table.entries[1].formation_id, 1);
    }

    #[test]
    fn synth_man_zero_formations_returns_none() {
        // Build a MAN with section-0 declaring zero formations.
        let mut buf = vec![0u8; 0x2B];
        // No records.
        buf[0x22] = 0;
        buf[0x24] = 0;
        buf[0x26] = 0;
        // u24[0x28] = 0
        // Section 0: f_count=0, then condition_count=0, region_count=0.
        let s0_body = vec![8u8, 4, 12, 0, 0, 0];
        let ln = s0_body.len() as u32;
        buf.extend_from_slice(&[(ln & 0xFF) as u8, (ln >> 8) as u8, (ln >> 16) as u8]);
        buf.extend_from_slice(&s0_body);
        for _ in 0..5 {
            buf.extend_from_slice(&[0, 0, 0]);
        }

        assert!(encounter_table_from_man("zero", &buf).is_none());
    }

    #[test]
    fn formation_defs_track_table_entries_by_row_index() {
        let man = build_test_man();
        let (table, defs) = scene_encounter_from_man("test", &man).expect("pair built");
        // One def per rollable formation row; ids are the row indices, so
        // the table's entries resolve straight through.
        assert_eq!(defs.len(), table.entries.len());
        assert_eq!(defs[0].formation_id, 0);
        assert_eq!(defs[1].formation_id, 1);
        // Slots mirror the formation row's monster ids.
        assert_eq!(defs[0].slots.len(), 1);
        assert_eq!(defs[0].slots[0].monster_id, 4);
        assert_eq!(defs[1].slots.len(), 2);
        assert_eq!(defs[1].slots[1].monster_id, 4);
        // Every table entry's formation_id has a matching def.
        for e in &table.entries {
            assert!(defs.iter().any(|d| d.formation_id == e.formation_id));
        }
    }

    #[test]
    fn formation_record_for_row_roundtrips_through_encounter_record() {
        let man = build_test_man();
        // Row 1 is "count=2, ids=4,4". EncounterRecord::parse reads
        // record[+3] as count and record[+4..] as ids.
        let r = formation_record_for_row(&man, 1).expect("row 1");
        assert_eq!(r.count, 2);
        assert_eq!(r.monster_ids, [4, 4, 0, 0]);
        // Out-of-range rows return None.
        assert!(formation_record_for_row(&man, 5).is_none());
    }

    #[test]
    fn formation_record_zero_count_yields_empty_record() {
        // Build a MAN whose only formation has count=0 (clears formation
        // cell).
        let mut buf = vec![0u8; 0x2B];
        // u24[0x28] = 0
        let s0_body = vec![
            8u8, 4, 12, 1, // header: f_count=1
            0, 0, 0, 0, 0, 0, 0, 0, // formation 0: count=0
            0, // condition_count=0
            0, // region_count=0
        ];
        let ln = s0_body.len() as u32;
        buf.extend_from_slice(&[(ln & 0xFF) as u8, (ln >> 8) as u8, (ln >> 16) as u8]);
        buf.extend_from_slice(&s0_body);
        for _ in 0..5 {
            buf.extend_from_slice(&[0, 0, 0]);
        }
        // Table is None because the only formation has count=0 → skipped.
        assert!(encounter_table_from_man("zerofm", &buf).is_none());
        // But the per-row helper still returns an empty record.
        let r = formation_record_for_row(&buf, 0).expect("row 0");
        assert_eq!(r, EncounterRecord::EMPTY);
    }
}
