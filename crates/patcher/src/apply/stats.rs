//! Monster combat-stat randomization.

use super::*;

/// Outcome of randomizing monster combat stats.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MonsterStatsReport {
    /// Monster slots actually rewritten.
    pub monsters_changed: usize,
    /// Total stat fields that changed across all rewritten monsters.
    pub fields_changed: usize,
    /// Monster ids whose re-packed slot would overflow the `0x14000` footprint,
    /// so the edit was skipped (the original stats are kept). Our LZS re-packer
    /// isn't byte-identical to Sony's, so a record already near the slot limit
    /// can rarely overflow; skipping keeps the rest of the patch valid (mirrors
    /// the drop randomizer, see [`crate::monster`]).
    pub skipped: Vec<u16>,
}

/// Read every populated monster's id + current combat stats (the
/// [`crate::monster_stats::STAT_FIELDS`] halfwords) out of the `battle_data`
/// archive. This is the population the stat randomizer redistributes.
pub fn current_monster_stats(patcher: &DiscPatcher) -> Result<Vec<monster_stats::StatAssignment>> {
    let entry = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive")?;
    let records =
        legaia_asset::monster_archive::records(&entry).context("decode monster archive records")?;
    Ok(records
        .iter()
        .map(|r| monster_stats::StatAssignment {
            monster_id: r.id,
            stats: [
                r.hp,
                r.mp,
                r.attack(),
                r.defense_high(),
                r.defense_low(),
                r.intelligence(),
                r.speed(),
            ],
        })
        .collect())
}

/// Randomize every monster's combat stats in place (see [`crate::monster_stats`]).
/// Each monster's `0x14000`-byte slot is decompressed, the stat halfwords
/// rewritten, and recompressed back to the same footprint - a same-size,
/// in-place edit. A slot too tight to re-pack is skipped (recorded in the
/// report) rather than aborting the run. Returns the apply report.
pub fn randomize_monster_stats(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<MonsterStatsReport> {
    let current = current_monster_stats(patcher)?;
    let plan = monster_stats::plan_stats(&current, seed, mode);
    let mut report = MonsterStatsReport::default();
    for (cur, new) in current.iter().zip(&plan) {
        if cur.stats == new.stats {
            continue;
        }
        let slot = patcher
            .monster_slot(new.monster_id)
            .with_context(|| format!("read monster {} slot", new.monster_id))?;
        let new_slot = match monster_stats::set_stats(&slot, &new.stats) {
            Ok(s) => s,
            Err(_) => {
                // Expected only on the slot-overflow guard; a malformed slot
                // would have failed in `current_monster_stats` already.
                report.skipped.push(new.monster_id);
                continue;
            }
        };
        if new_slot != slot {
            patcher
                .patch_monster_slot(new.monster_id, &new_slot)
                .with_context(|| format!("write monster {} slot", new.monster_id))?;
            report.monsters_changed += 1;
            report.fields_changed += cur
                .stats
                .iter()
                .zip(&new.stats)
                .filter(|(a, b)| a != b)
                .count();
        }
    }
    Ok(report)
}
