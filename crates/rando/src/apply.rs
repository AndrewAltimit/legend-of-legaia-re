//! High-level orchestration: read the current gameplay data off a disc, plan a
//! randomization from a seed, and write the plan back into a [`DiscPatcher`].
//!
//! This is the glue the top-level CLI drives. It keeps the per-module logic
//! (drop planning, slot re-pack, sector write-back) decoupled and testable while
//! giving the binary a single call per feature. It embeds no game bytes — every
//! value it reads comes from the user's own disc image at runtime.

use anyhow::{Context, Result};

use crate::chest::SceneChests;
use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use crate::drops::{CurrentDrop, DropAssignment, DropMode, plan_drops};
use crate::encounter::SceneEncounters;
use crate::rng::SplitMix64;

/// Read every monster's current drop (item id + chance) out of the
/// `battle_data` archive (PROT entry 867). Monsters with no drop are included
/// with `item == 0` so the planner can skip them consistently.
pub fn current_drops(patcher: &DiscPatcher) -> Result<Vec<CurrentDrop>> {
    let entry = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive")?;
    let records =
        legaia_asset::monster_archive::records(&entry).context("decode monster archive records")?;
    Ok(records
        .iter()
        .map(|r| CurrentDrop {
            monster_id: r.id,
            item: r.drop_item,
            chance: r.drop_chance_pct,
        })
        .collect())
}

/// Outcome of applying a drop plan.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DropApplyReport {
    /// Slots actually written.
    pub changed: usize,
    /// Monsters whose re-packed slot would not fit, so the edit was skipped
    /// (the original drop is kept). Our LZS re-packer isn't byte-identical to
    /// Sony's, so a record already near the `0x14000` slot limit can rarely
    /// overflow; skipping keeps the rest of the patch valid. See
    /// [`crate::monster`].
    pub skipped: Vec<u16>,
}

/// Apply a planned drop table to the disc image. Each assignment re-packs that
/// monster's slot in place (decompress -> set drop bytes -> recompress -> sector
/// write-back). A slot whose re-packed stream would overflow is skipped (and
/// recorded in the report) rather than aborting the whole run.
pub fn apply_drop_plan(
    patcher: &mut DiscPatcher,
    plan: &[DropAssignment],
) -> Result<DropApplyReport> {
    let mut report = DropApplyReport::default();
    for a in plan {
        let slot = patcher
            .monster_slot(a.monster_id)
            .with_context(|| format!("read monster {} slot", a.monster_id))?;
        let new_slot = match crate::monster::set_drop(&slot, a.item, a.chance) {
            Ok(s) => s,
            Err(_) => {
                // The only expected failure here is the slot-overflow guard;
                // a malformed slot would have failed in `current_drops` already.
                report.skipped.push(a.monster_id);
                continue;
            }
        };
        if new_slot != slot {
            patcher
                .patch_monster_slot(a.monster_id, &new_slot)
                .with_context(|| format!("write monster {} slot", a.monster_id))?;
            report.changed += 1;
        }
    }
    Ok(report)
}

/// Plan and apply a drop randomization in one call. `item_pool` is only needed
/// for [`DropMode::Random`] (shuffle ignores it); pass an empty slice otherwise.
/// Returns the plan that was generated plus the apply report.
pub fn randomize_drops(
    patcher: &mut DiscPatcher,
    item_pool: &[u8],
    seed: u64,
    mode: DropMode,
) -> Result<(Vec<DropAssignment>, DropApplyReport)> {
    let current = current_drops(patcher)?;
    let plan = plan_drops(&current, item_pool, seed, mode);
    let report = apply_drop_plan(patcher, &plan)?;
    Ok((plan, report))
}

/// Outcome of randomizing scene encounters.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EncounterApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total formation id bytes changed across all scenes.
    pub ids_changed: usize,
    /// Scene PROT-entry indices whose recompressed MAN would not fit the
    /// original footprint, so the scene was left untouched.
    pub skipped: Vec<usize>,
}

/// Randomize every scene's random-encounter formations in place. For each scene
/// bundle the monster ids are reassigned from the scene's own id pool (so every
/// monster stays scene-loaded), the MAN is recompressed, and — when it fits the
/// original compressed footprint — written back. Scenes whose re-pack overflows
/// are recorded in `skipped` and left unchanged.
pub fn randomize_encounters(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<EncounterApplyReport> {
    let mut report = EncounterApplyReport::default();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(mut scene) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        let changed = scene.randomize(seed, mode);
        if changed == 0 {
            continue;
        }
        match scene.repack() {
            Some(stream) => {
                patcher
                    .patch_prot_entry(idx, scene.man_offset as u64, &stream)
                    .with_context(|| format!("write scene {idx} MAN"))?;
                report.scenes_changed += 1;
                report.ids_changed += changed;
            }
            None => report.skipped.push(idx),
        }
    }
    Ok(report)
}

/// Outcome of randomizing treasure-chest contents.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ChestApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total chest item bytes changed.
    pub items_changed: usize,
    /// Total chest give-item sites found (the randomizable population).
    pub sites_total: usize,
    /// Scene PROT-entry indices whose recompressed MAN would not fit, skipped.
    pub skipped: Vec<usize>,
}

/// Randomize treasure-chest contents (field-VM `GIVE_ITEM` op `0x39`). Chest
/// item ids are global inventory ids (any item works anywhere), so this is a
/// **global** reassignment across every chest on the disc: `Shuffle`
/// redistributes the existing chest-item multiset, `Random` draws each from
/// `item_pool`. Only sites reachable by a clean field-VM walk are touched (see
/// [`crate::chest`]). Scenes whose recompressed MAN overflows are skipped.
pub fn randomize_chests(
    patcher: &mut DiscPatcher,
    item_pool: &[u8],
    seed: u64,
    mode: DropMode,
) -> Result<ChestApplyReport> {
    // Pass 1: collect every scene's chest sites + current items (decoded MAN
    // held for pass 2 so we don't decode twice).
    let mut scenes: Vec<SceneChests> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(sc) = SceneChests::locate(&entry, idx) {
            scenes.push(sc);
        }
    }

    let mut report = ChestApplyReport {
        sites_total: scenes.iter().map(|s| s.sites.len()).sum(),
        ..Default::default()
    };
    if report.sites_total == 0 {
        return Ok(report);
    }

    // Original item id at each (scene, site), kept so a skipped scene can be
    // restored and excluded from the shuffle pool.
    let originals: Vec<Vec<u8>> = scenes.iter().map(|s| s.current_items()).collect();
    let entry_indices: Vec<usize> = scenes.iter().map(|s| s.entry_idx).collect();

    // Indices of scenes whose recompressed MAN won't fit; these keep their
    // original items and are excluded from the (multiset-preserving) shuffle
    // pool. Determined iteratively: a fresh overflow shrinks the pool and we
    // re-plan, so the writable set converges (it only ever shrinks).
    let mut skipped: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();

    match mode {
        DropMode::Shuffle => {
            // Each pass: shuffle the original items of the not-yet-skipped scenes
            // among those same sites, repack, and fold any fresh overflow into
            // `skipped` for the next pass. A permutation over the writable set
            // preserves the chest-item multiset both over the written sites and
            // globally (skipped items never enter the pool, so they neither
            // appear nor disappear).
            let mut streams: Vec<(usize, u64, Vec<u8>)> = Vec::new();
            loop {
                // Restore every site to its original, then assign the shuffle.
                for (i, sc) in scenes.iter_mut().enumerate() {
                    for (k, &orig) in originals[i].iter().enumerate() {
                        sc.set_site(k, orig);
                    }
                }
                let mut pool: Vec<u8> = (0..scenes.len())
                    .filter(|i| !skipped.contains(i))
                    .flat_map(|i| originals[i].iter().copied())
                    .collect();
                let mut rng = SplitMix64::new(seed);
                rng.shuffle(&mut pool);
                let mut cur = 0usize;
                for (i, sc) in scenes.iter_mut().enumerate() {
                    if skipped.contains(&i) {
                        continue;
                    }
                    for k in 0..sc.sites.len() {
                        sc.set_site(k, pool[cur]);
                        cur += 1;
                    }
                }

                streams.clear();
                let mut fresh_overflow = false;
                for (i, sc) in scenes.iter().enumerate() {
                    if skipped.contains(&i) {
                        continue;
                    }
                    match sc.repack() {
                        Some(stream) => streams.push((sc.entry_idx, sc.man_offset as u64, stream)),
                        None => {
                            skipped.insert(i);
                            fresh_overflow = true;
                        }
                    }
                }
                if !fresh_overflow {
                    break;
                }
            }

            for (i, sc) in scenes.iter().enumerate() {
                if skipped.contains(&i) {
                    continue;
                }
                let changed = sc
                    .sites
                    .iter()
                    .enumerate()
                    .filter(|&(k, &off)| sc.decoded[off] != originals[i][k])
                    .count();
                if changed > 0 {
                    report.scenes_changed += 1;
                    report.items_changed += changed;
                }
            }
            for (entry_idx, man_offset, stream) in streams {
                patcher
                    .patch_prot_entry(entry_idx, man_offset, &stream)
                    .with_context(|| format!("write scene {entry_idx} MAN"))?;
            }
        }
        DropMode::Random => {
            if item_pool.is_empty() {
                return Ok(report);
            }
            // Each site is independent, so an overflowing scene just reverts
            // (no multiset to preserve under Random).
            let mut rng = SplitMix64::new(seed);
            for (i, sc) in scenes.iter_mut().enumerate() {
                let mut changed = 0;
                for k in 0..sc.sites.len() {
                    let v = item_pool[rng.below(item_pool.len())];
                    if sc.decoded[sc.sites[k]] != v {
                        sc.set_site(k, v);
                        changed += 1;
                    }
                }
                if changed == 0 {
                    continue;
                }
                match sc.repack() {
                    Some(stream) => {
                        patcher
                            .patch_prot_entry(sc.entry_idx, sc.man_offset as u64, &stream)
                            .with_context(|| format!("write scene {} MAN", sc.entry_idx))?;
                        report.scenes_changed += 1;
                        report.items_changed += changed;
                    }
                    None => {
                        skipped.insert(i);
                    }
                }
            }
        }
    }

    report.skipped = skipped.into_iter().map(|i| entry_indices[i]).collect();
    Ok(report)
}
