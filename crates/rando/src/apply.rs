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
use crate::door::SceneDoors;
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

/// One treasure-chest give-item site: which scene bundle it lives in, the byte
/// offset of its `GIVE_ITEM` (`0x39`) operand inside the decoded MAN, and the
/// item id it currently grants. This is the population the chest randomizer
/// reassigns; listing it lets a user audit which items would change (e.g. to
/// keep quest items static).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChestSite {
    /// PROT entry index of the scene bundle holding this chest.
    pub entry_idx: usize,
    /// Byte offset of the give operand within the scene's decoded MAN. Stable
    /// per disc; identifies the site independent of item id.
    pub man_offset: usize,
    /// The item id the chest currently gives.
    pub item: u8,
}

/// Read every treasure-chest give-item site on the disc (the randomizable
/// population), in PROT-entry order. Mirrors [`current_drops`] for chests:
/// purely read-only, decodes each scene MAN once via [`SceneChests::locate`].
pub fn current_chests(patcher: &DiscPatcher) -> Result<Vec<ChestSite>> {
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(sc) = SceneChests::locate(&entry, idx) else {
            continue;
        };
        let items = sc.current_items();
        for (k, &off) in sc.sites.iter().enumerate() {
            out.push(ChestSite {
                entry_idx: idx,
                man_offset: off,
                item: items[k],
            });
        }
    }
    Ok(out)
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
///
/// `keep_static` is a set of item ids to leave untouched: any chest whose
/// **original** item is in the set keeps that item (it is excluded from the
/// shuffle multiset entirely, so it never moves and no other chest receives it),
/// and the id is dropped from the `Random` fill pool so it can't be duplicated
/// into another chest. This is how quest / key items stay where the player
/// expects them (see [`crate::items::DEFAULT_STATIC_CHEST_ITEMS`]). Pass an empty
/// set to randomize everything.
pub fn randomize_chests(
    patcher: &mut DiscPatcher,
    item_pool: &[u8],
    seed: u64,
    mode: DropMode,
    keep_static: &std::collections::BTreeSet<u8>,
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
                // The shuffle pool is the originals of non-skipped, non-static
                // sites only; static items stay put (already restored above) and
                // never enter the pool, so the multiset over the shuffled sites
                // is preserved and a static item can't land in another chest.
                let mut pool: Vec<u8> = (0..scenes.len())
                    .filter(|i| !skipped.contains(i))
                    .flat_map(|i| originals[i].iter().copied())
                    .filter(|item| !keep_static.contains(item))
                    .collect();
                let mut rng = SplitMix64::new(seed);
                rng.shuffle(&mut pool);
                let mut cur = 0usize;
                for (i, sc) in scenes.iter_mut().enumerate() {
                    if skipped.contains(&i) {
                        continue;
                    }
                    for (k, &orig) in originals[i].iter().enumerate() {
                        if keep_static.contains(&orig) {
                            continue; // static site keeps its restored original
                        }
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
            // Static items are dropped from the fill pool so a random chest can't
            // duplicate a quest / key item elsewhere.
            let fill_pool: Vec<u8> = item_pool
                .iter()
                .copied()
                .filter(|item| !keep_static.contains(item))
                .collect();
            if fill_pool.is_empty() {
                return Ok(report);
            }
            // Each site is independent, so an overflowing scene just reverts
            // (no multiset to preserve under Random).
            let mut rng = SplitMix64::new(seed);
            for (i, sc) in scenes.iter_mut().enumerate() {
                let mut changed = 0;
                for k in 0..sc.sites.len() {
                    // A chest whose original item is static keeps it (and consumes
                    // no rng draw, so the stream past it is unaffected by which
                    // sites are static — only the pool composition is).
                    if keep_static.contains(&sc.decoded[sc.sites[k]]) {
                        continue;
                    }
                    let v = fill_pool[rng.below(fill_pool.len())];
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

/// The CDNAME scene label for a PROT entry, read from the disc's `CDNAME.TXT`
/// (or `"?"` when the map is unavailable). Returns the parsed map so callers can
/// label many entries without re-reading.
pub fn cdname_map(patcher: &DiscPatcher) -> std::collections::BTreeMap<u32, String> {
    patcher
        .read_named_file("CDNAME.TXT")
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|t| legaia_prot::cdname::parse_str(&t).ok())
        .unwrap_or_default()
}

/// One scene-transition ("door / exit") site: where it lives and where it goes.
/// This is the audit surface the door randomizer reassigns. `home_scene` is the
/// CDNAME label of the scene the door is in; `dest_scene` is the inline
/// destination name the `0x3F` op carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoorSite {
    /// PROT entry index of the scene bundle holding this door.
    pub entry_idx: usize,
    /// CDNAME label of the scene the door lives in (e.g. `"town01"`).
    pub home_scene: String,
    /// Byte offset of the `0x3F` op within the scene's decompressed MAN.
    pub op_pc: usize,
    /// Partition of the carrying record (almost always 2).
    pub partition: usize,
    /// Destination-scene `i16` index the op carries.
    pub index: i16,
    /// Destination CDNAME scene label (e.g. `"map01"`).
    pub dest_scene: String,
    /// Destination entry-tile X / Z bytes.
    pub entry_x: u8,
    pub entry_z: u8,
    /// Facing/depth selector byte.
    pub dir: u8,
}

/// Read every scene-transition door on the disc (the randomizable population),
/// in PROT-entry then op-offset order. Purely read-only; decodes each scene MAN
/// once via [`SceneDoors::locate`] and labels the home scene from `CDNAME.TXT`.
pub fn current_doors(patcher: &DiscPatcher) -> Result<Vec<DoorSite>> {
    let cd = cdname_map(patcher);
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(doors) = SceneDoors::locate(&entry, idx) else {
            continue;
        };
        let home = legaia_prot::cdname::block_for(&cd, idx as u32)
            .unwrap_or("?")
            .to_string();
        for s in &doors.sites {
            out.push(DoorSite {
                entry_idx: idx,
                home_scene: home.clone(),
                op_pc: s.op_pc,
                partition: s.partition,
                index: s.index,
                dest_scene: s.name.clone(),
                entry_x: s.entry_x,
                entry_z: s.entry_z,
                dir: s.dir,
            });
        }
    }
    Ok(out)
}

/// One monster's current steal: monster id, item id, and steal chance percent.
/// Mirrors [`CurrentDrop`] for the steal table (see [`crate::steal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StealSite {
    pub monster_id: u16,
    pub item: u8,
    pub chance: u8,
}

/// Read every stealable monster's current steal (item + chance) out of the
/// static `SCUS_942.54` steal table (`DAT_80077828`). Non-stealable monsters
/// (`item == 0` or `chance == 0`) are omitted. Purely read-only — the audit
/// surface for deciding what a steal randomization would change.
pub fn current_steals(patcher: &DiscPatcher) -> Result<Vec<StealSite>> {
    let edits = crate::steal::StealEdits::locate(patcher.image())
        .context("locate SCUS_942.54 steal table")?;
    Ok(edits
        .current()
        .into_iter()
        .map(|c| StealSite {
            monster_id: c.monster_id,
            item: c.item,
            chance: c.chance,
        })
        .collect())
}

/// Outcome of randomizing per-monster steal items.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StealApplyReport {
    /// Steal-item bytes actually written (no-op reassignments are skipped).
    pub items_changed: usize,
    /// Stealable monsters considered for reassignment.
    pub monsters: usize,
}

/// Randomize the per-monster steal items in place. The steal table is a static
/// `SCUS_942.54` table, so each edit is a single same-size byte overwrite of the
/// item (the steal *chance* is preserved) — no re-pack, nothing skipped.
/// `Shuffle` redistributes the existing steal-item multiset among the stealable
/// monsters; `Random` draws each item from `item_pool`. Returns the plan plus
/// the apply report.
pub fn randomize_steals(
    patcher: &mut DiscPatcher,
    item_pool: &[u8],
    seed: u64,
    mode: DropMode,
) -> Result<(Vec<DropAssignment>, StealApplyReport)> {
    let edits = crate::steal::StealEdits::locate(patcher.image())
        .context("locate SCUS_942.54 steal table")?;
    let plan = edits.plan(item_pool, seed, mode);
    let monsters = plan.len();
    let patches = edits.item_patches(&plan);
    let mut report = StealApplyReport {
        items_changed: 0,
        monsters,
    };
    for (off, item) in patches {
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, off, &[item])
            .with_context(|| format!("write steal item at SCUS offset {off:#x}"))?;
        report.items_changed += 1;
    }
    Ok((plan, report))
}
