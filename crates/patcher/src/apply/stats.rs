//! Monster combat-stat randomization.

use super::*;

/// Outcome of randomizing monster combat stats.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MonsterStatsReport {
    /// Monster slots actually rewritten.
    pub monsters_changed: usize,
    /// How many of [`Self::monsters_changed`] were **bosses** (scripted-only
    /// fights, per [`crate::monster_class`]). Always `0` for the stat randomizer
    /// and for a uniform difficulty scale, which classifies nothing; a split
    /// scale reports it so a run manifest shows the boss half reached something
    /// rather than leaving "did that slider do anything?" to a playthrough.
    pub bosses_changed: usize,
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

/// Scale every monster's combat stats by a difficulty multiplier
/// ([`monster_stats::StatScale`], `0.1x..=5x` per stat field; see
/// [`crate::monster_stats::plan_scale`]). Seedless - the result depends only on
/// the disc and the scale.
///
/// One multiplier for the whole roster and a per-stat scale are the same code
/// path: a uniform scale is simply every field holding the same value, so both
/// share this pass, its clamps and its slot handling.
///
/// Story bosses are scaled too; only the scripted tutorial fight
/// ([`crate::monster_stats::SCALE_PINNED_MONSTER_IDS`]) is pinned. Composes with
/// [`randomize_monster_stats`]: run the randomizer first and this multiplies the
/// values it dealt out, because both read the roster back off the disc. An
/// all-retail scale writes nothing. Slot handling (same-size re-pack,
/// skip-on-overflow) is identical to the randomizer above.
pub fn scale_monster_stats(
    patcher: &mut DiscPatcher,
    scale: monster_stats::StatScale,
) -> Result<MonsterStatsReport> {
    scale_monster_stats_profile(patcher, monster_stats::ScaleProfile::uniform(scale))
}

/// Split every scene's formation table into random encounters and scripted
/// fights, and return the per-monster classification the split difficulty scale
/// selects its multiplier with (see [`crate::monster_class`]).
///
/// Reads every PROT entry once and keeps only the scenes that carry an encounter
/// section - the same walk [`randomize_encounters`](super::randomize_encounters)
/// makes, and the same [`SceneEncounters::locate`] parse, so the two features
/// classify a formation identically by construction. Entries that aren't scene
/// bundles are skipped, not an error.
///
/// Two curated lists bracket the scan, each for a reason the scan can't reach.
/// [`monster_stats::STORY_BOSS_MONSTER_IDS`] is unioned in as a floor: a boss
/// form the game swaps in mid-battle is named by no formation record, so the
/// scan alone would leave it on the trash multiplier.
/// [`monster_stats::TUTORIAL_MONSTER_IDS`] is then forced back to regular,
/// because two of the first three Piura are scripted-only on the disc and a
/// boss slider must not reach a fresh save's opening fights
/// ([`monster_class::MonsterClasses::force_regular`]).
pub fn classify_monsters(patcher: &DiscPatcher) -> Result<monster_class::MonsterClasses> {
    let mut scan = monster_class::ClassScan::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(scene) = SceneEncounters::locate(&entry, idx) {
            scan.observe(&scene);
        }
    }
    let mut classes = scan.finish(monster_stats::STORY_BOSS_MONSTER_IDS);
    classes.force_regular(monster_stats::TUTORIAL_MONSTER_IDS);
    Ok(classes)
}

/// Scale monster combat stats by a **per-class** difficulty profile
/// ([`monster_stats::ScaleProfile`]): one [`monster_stats::StatScale`] for
/// random encounters, another for bosses.
///
/// The general form of [`scale_monster_stats`], which is this with both halves
/// equal. A **uniform** profile skips [`classify_monsters`] entirely - the two
/// halves being equal makes the classification unobservable, so a single-dial
/// run pays nothing for the split existing and writes byte-identical output. A
/// genuine split scans the scene corpus once, then plans and writes exactly as
/// the uniform pass does.
///
/// Story bosses are scaled by the boss half rather than skipped; only the
/// scripted tutorial fight ([`crate::monster_stats::SCALE_PINNED_MONSTER_IDS`])
/// is pinned, in either class. Composes with [`randomize_monster_stats`] the
/// same way: run the randomizer first and this multiplies the values it dealt
/// out. An all-retail profile writes nothing.
pub fn scale_monster_stats_profile(
    patcher: &mut DiscPatcher,
    profile: monster_stats::ScaleProfile,
) -> Result<MonsterStatsReport> {
    let mut report = MonsterStatsReport::default();
    if profile.is_retail() {
        return Ok(report);
    }
    // Both halves equal -> the classification cannot change a byte, so don't
    // pay for the corpus walk.
    let classes = if profile.is_uniform() {
        monster_class::MonsterClasses::all_regular()
    } else {
        classify_monsters(patcher)?
    };
    let current = current_monster_stats(patcher)?;
    let plan = monster_stats::plan_scale_profile(&current, profile, &classes);
    for (cur, new) in current.iter().zip(&plan) {
        if cur.stats == new.stats {
            continue;
        }
        let slot = patcher
            .monster_slot(new.monster_id)
            .with_context(|| format!("read monster {} slot", new.monster_id))?;
        let new_slot = match monster_stats::set_stats(&slot, &new.stats) {
            Ok(s) => s,
            // Only the slot-overflow guard can fire here; a malformed slot
            // would already have failed in `current_monster_stats`.
            Err(_) => {
                report.skipped.push(new.monster_id);
                continue;
            }
        };
        if new_slot != slot {
            patcher
                .patch_monster_slot(new.monster_id, &new_slot)
                .with_context(|| format!("write monster {} slot", new.monster_id))?;
            report.monsters_changed += 1;
            if classes.is_boss(new.monster_id) {
                report.bosses_changed += 1;
            }
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
