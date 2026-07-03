//! Scene random-encounter randomization (per-scene / kingdom / world scope, solo-strong pass).

use super::*;

/// Outcome of randomizing scene encounters.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EncounterApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total formation id bytes changed across all scenes.
    pub ids_changed: usize,
    /// Formation id slots (in written-back scenes) that ended up holding one of
    /// the `unused_enemies` ids - i.e. how many unused enemies the run actually
    /// placed. Always `0` unless `unused_enemies` was non-empty and the mode was
    /// [`DropMode::Random`].
    pub unused_placed: usize,
    /// Random formations forced down to a single enemy by the solo-strong option
    /// (an over-strong monster faced alone instead of in a pack). Always `0`
    /// unless [`randomize_encounters_full`] ran with a [`SoloStrongConfig`].
    pub solo_collapsed: usize,
    /// Scene PROT-entry indices whose recompressed MAN would not fit the
    /// original footprint, so the scene was left untouched.
    pub skipped: Vec<usize>,
}

/// Randomize every scene's random-encounter formations in place. For each scene
/// bundle the monster ids are reassigned from the scene's own id pool, the MAN
/// is recompressed, and - when it fits the original compressed footprint -
/// written back. Scenes whose re-pack overflows are recorded in `skipped` and
/// left unchanged.
///
/// `unused_enemies` is the curated set of monster ids no formation normally
/// references (see [`crate::unused::UNUSED_ENEMY_IDS`]). When non-empty *and*
/// `mode` is [`DropMode::Random`], those ids join each scene's candidate pool so
/// the run can spawn them - the battle loader streams a monster's archive slot
/// on demand by id, so an id outside the scene's own set still loads. Under
/// [`DropMode::Shuffle`] it has no effect (a multiset-preserving permutation
/// can't introduce a new id). Pass an empty slice to keep the prior behaviour;
/// the RNG stream is unchanged when it is empty, so existing results stay
/// byte-identical.
pub fn randomize_encounters(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
    unused_enemies: &[u8],
) -> Result<EncounterApplyReport> {
    let mut report = EncounterApplyReport::default();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(mut scene) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        let changed = scene.randomize_with_extra(seed, mode, unused_enemies);
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
                if !unused_enemies.is_empty() {
                    report.unused_placed += scene.count_ids_in(unused_enemies);
                }
            }
            None => report.skipped.push(idx),
        }
    }
    Ok(report)
}

/// How wide a net the encounter randomizer casts when reassigning a scene's
/// monsters - the *pool* a random encounter is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncounterScope {
    /// Each scene draws only from its own monster ids - the classic per-scene
    /// behaviour ([`randomize_encounters`]). Difficulty stays local; a swap can
    /// never surprise you with a monster the area didn't already host.
    Scene,
    /// "Within a region": a scene draws from the union of every monster across
    /// its **kingdom** (Drake / Sebucus / Karisto, see [`crate::kingdom`]).
    /// Late-game Drake monsters can show up in early Drake, but no monster ever
    /// crosses a kingdom boundary.
    Kingdom,
    /// "Across regions": a scene draws from every monster on the disc, so a
    /// late-game Karisto monster can appear in the opening Drake caves.
    World,
}

/// Tag mixed into the cross-scene shuffle RNG so a scoped shuffle is independent
/// of the per-scene seeding used by [`randomize_encounters`].
const SCOPED_SHUFFLE_NONCE: u64 = 0x5343_4F50_4544_0001; // "SCOPED\0\1"

/// Randomize scene encounters with a pool [`EncounterScope`] wider than a single
/// scene. `Scene` scope delegates to [`randomize_encounters`] unchanged.
///
/// For `Kingdom` / `World` scope the disc is processed in two passes:
///
/// 1. **Collect** every scene's encounter data and (for `Kingdom`) its kingdom,
///    then build the per-group monster pool - the union of each group's scenes'
///    random-encounter ids (bosses excluded, exactly as the per-scene path).
/// 2. **Reassign**: under [`DropMode::Random`] each scene's random slots are
///    redrawn from its group pool ([`SceneEncounters::fill_random_slots_from_pool`]);
///    under [`DropMode::Shuffle`] the whole group's random ids are pooled,
///    permuted once, and redistributed across the group's scenes
///    ([`SceneEncounters::random_slot_ids`] / [`SceneEncounters::apply_random_slots`]),
///    so the group-wide multiset is preserved but monsters move between scenes
///    (and, for `World`, between kingdoms).
///
/// `Kingdom` scope needs the disc's `CDNAME.TXT` to resolve kingdoms; if it
/// can't be parsed the call errors (every retail disc carries it). `World` scope
/// never touches CDNAME. `unused_enemies` behaves as in [`randomize_encounters`]
/// (it widens the pool under `Random` only). The result is deterministic for a
/// fixed `seed`, independent of patcher iteration order.
pub fn randomize_encounters_scoped(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
    scope: EncounterScope,
    unused_enemies: &[u8],
) -> Result<EncounterApplyReport> {
    use std::collections::BTreeMap;

    if scope == EncounterScope::Scene {
        return randomize_encounters(patcher, seed, mode, unused_enemies);
    }

    // Kingdom scope needs the CDNAME partition; World lumps everything together.
    let kingdom_map = if scope == EncounterScope::Kingdom {
        let cdname = patcher
            .cdname()
            .context("read CDNAME.TXT for kingdom-scoped encounters")?;
        Some(crate::kingdom::KingdomMap::from_cdname(&cdname).context(
            "CDNAME.TXT is missing the map01/map02 kingdom anchors; \
                 use --encounter-scope world instead",
        )?)
    } else {
        None
    };

    // Pass 1: locate every scene and assign it a group key (0 = the single
    // World group; otherwise the kingdom's tag). Scenes are kept so their MAN is
    // only decompressed once.
    struct Located {
        idx: usize,
        group: u64,
        scene: SceneEncounters,
    }
    let mut located: Vec<Located> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(scene) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        let group = match kingdom_map {
            Some(km) => km.kingdom_for_extraction_index(idx).seed_tag(),
            None => 0,
        };
        located.push(Located { idx, group, scene });
    }

    // Per-group monster pool: the union of every member scene's random-encounter
    // ids (boss ids are already excluded by `monster_pool`). Under Random the
    // unused-enemy ids widen each pool.
    let mut pools: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    for l in &located {
        let pool = pools.entry(l.group).or_default();
        for id in l.scene.monster_pool() {
            if !pool.contains(&id) {
                pool.push(id);
            }
        }
    }
    if mode == DropMode::Random {
        for pool in pools.values_mut() {
            for &id in unused_enemies {
                if !pool.contains(&id) {
                    pool.push(id);
                }
            }
            pool.sort_unstable();
        }
    }

    // Pass 2: reassign + write back. The two modes differ enough - Random fills
    // each scene independently; Shuffle moves ids *between* scenes and so must
    // keep the group multiset intact across re-pack failures - that they own
    // their write-back loops.
    let mut report = EncounterApplyReport::default();
    match mode {
        DropMode::Random => {
            for l in &mut located {
                let pool = pools.get(&l.group).map(Vec::as_slice).unwrap_or(&[]);
                let changed = l.scene.fill_random_slots_from_pool(seed, pool);
                if changed == 0 {
                    continue;
                }
                match l.scene.repack() {
                    Some(stream) => {
                        patcher
                            .patch_prot_entry(l.idx, l.scene.man_offset as u64, &stream)
                            .with_context(|| format!("write scene {} MAN", l.idx))?;
                        report.scenes_changed += 1;
                        report.ids_changed += changed;
                        if !unused_enemies.is_empty() {
                            report.unused_placed += l.scene.count_ids_in(unused_enemies);
                        }
                    }
                    None => report.skipped.push(l.idx),
                }
            }
        }
        DropMode::Shuffle => {
            // A cross-scene shuffle conserves the group-wide id multiset only if
            // every shuffled scene is actually written. A scene whose mutated
            // MAN no longer fits its compressed footprint can't be - and if it
            // silently kept its original ids, the ids it was *given* would
            // vanish while the ids it *gave away* would duplicate. So lock any
            // such scene to its original (removing it from the pool) and reshuffle
            // the rest. Each round locks at least one more scene, so this reaches
            // a fixpoint where every still-shuffled scene re-packs cleanly and
            // the conserved multiset is exactly the unlocked scenes' originals.
            let orig_ids: Vec<Vec<u8>> =
                located.iter().map(|l| l.scene.random_slot_ids()).collect();
            let mut locked = vec![false; located.len()];
            let mut streams: Vec<Option<Vec<u8>>> = vec![None; located.len()];
            loop {
                // Restore every scene to its original ids, so a freshly-locked
                // scene reverts and locked scenes stay put.
                for (i, l) in located.iter_mut().enumerate() {
                    l.scene.apply_random_slots(&orig_ids[i]);
                }
                // Group the still-unlocked members, in ascending scene order.
                let mut groups: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
                for (i, l) in located.iter().enumerate() {
                    if !locked[i] {
                        groups.entry(l.group).or_default().push(i);
                    }
                }
                for (gid, members) in &groups {
                    let mut all_ids: Vec<u8> = Vec::new();
                    for &mi in members {
                        all_ids.extend_from_slice(&orig_ids[mi]);
                    }
                    let mut rng = SplitMix64::new(
                        seed ^ gid.wrapping_mul(0x9E3779B97F4A7C15) ^ SCOPED_SHUFFLE_NONCE,
                    );
                    rng.shuffle(&mut all_ids);
                    let mut cursor = 0;
                    for &mi in members {
                        let n = orig_ids[mi].len();
                        located[mi]
                            .scene
                            .apply_random_slots(&all_ids[cursor..cursor + n]);
                        cursor += n;
                    }
                }
                // Try to re-pack every changed, unlocked scene; lock new failures.
                let mut new_failure = false;
                for i in 0..located.len() {
                    if locked[i] {
                        continue;
                    }
                    if located[i].scene.random_slot_ids() == orig_ids[i] {
                        streams[i] = None; // no-op assignment, nothing to write
                        continue;
                    }
                    match located[i].scene.repack() {
                        Some(stream) => streams[i] = Some(stream),
                        None => {
                            locked[i] = true;
                            streams[i] = None;
                            new_failure = true;
                        }
                    }
                }
                if !new_failure {
                    break;
                }
            }
            // Commit: write the scenes that re-packed; the locked ones were
            // restored to their originals, so report them as skipped.
            for i in 0..located.len() {
                if let Some(stream) = &streams[i] {
                    let l = &located[i];
                    patcher
                        .patch_prot_entry(l.idx, l.scene.man_offset as u64, stream)
                        .with_context(|| format!("write scene {} MAN", l.idx))?;
                    report.scenes_changed += 1;
                    report.ids_changed += located[i]
                        .scene
                        .random_slot_ids()
                        .iter()
                        .zip(&orig_ids[i])
                        .filter(|(a, b)| a != b)
                        .count();
                } else if locked[i] {
                    report.skipped.push(located[i].idx);
                }
            }
        }
    }
    Ok(report)
}

/// Default "strong fight" cut-off for [`SoloStrongConfig`]: a random formation
/// is forced solo when its strongest monster's combat power is at least **twice**
/// (`200`%) the area's native average. Twice the local norm reads as "much
/// stronger than this area expects" without firing on the ordinary spread of a
/// scene's own monsters.
pub const DEFAULT_SOLO_STRONG_THRESHOLD_PCT: u16 = 200;

/// Configuration for the "limit strong fights to a solo enemy" encounter option
/// ([`randomize_encounters_full`]).
#[derive(Debug, Clone, Copy)]
pub struct SoloStrongConfig {
    /// A random formation is collapsed to a single enemy when its strongest
    /// monster's [`crate::monster_stats::combat_power`] is at least this percent
    /// of the scene's native average power. `200` = "twice the area's normal
    /// strength". Values `<= 100` would flag ordinary monsters, so the sensible
    /// range is `> 100`; `0` disables the pass.
    pub threshold_pct: u16,
}

impl Default for SoloStrongConfig {
    fn default() -> Self {
        Self {
            threshold_pct: DEFAULT_SOLO_STRONG_THRESHOLD_PCT,
        }
    }
}

/// Build the per-id combat-power lookup the solo-strong-encounter option compares
/// formations against. Reads the same `battle_data` archive (PROT entry 867) as
/// the stat randomizer and scores each monster by
/// [`crate::monster_stats::combat_power`].
pub fn monster_power_table(patcher: &DiscPatcher) -> Result<crate::encounter::MonsterPowerTable> {
    let stats = current_monster_stats(patcher)?;
    Ok(crate::encounter::MonsterPowerTable::from_powers(
        stats
            .iter()
            .map(|a| (a.monster_id, monster_stats::combat_power(&a.stats))),
    ))
}

/// Record each locatable scene's **native** combat-power baseline (mean random
/// monster power), keyed by PROT entry index. Read **before** any encounter edit
/// so the baseline is the area's authored difficulty, not the post-randomization
/// monsters - see [`encounter::SceneEncounters::baseline_power`].
fn solo_strong_baselines(
    patcher: &DiscPatcher,
    table: &crate::encounter::MonsterPowerTable,
) -> Result<std::collections::HashMap<usize, u32>> {
    let mut out = std::collections::HashMap::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(scene) = SceneEncounters::locate(&entry, idx)
            && let Some(base) = scene.baseline_power(table)
        {
            out.insert(idx, base);
        }
    }
    Ok(out)
}

/// Enforce the solo-strong rule across every scene as a post-pass over the
/// **already-randomized** scenes: collapse any random formation whose strongest
/// monster clears `cfg.threshold_pct`% of that scene's pre-saved native baseline
/// down to that lone monster. Decoupled from how the ids were assigned (Scene /
/// Kingdom / World, Shuffle / Random), so it composes with every encounter mode.
/// Returns the number of formations collapsed, id bytes zeroed, and any scene
/// whose collapsed MAN no longer re-packs (left untouched).
fn enforce_solo_strong_encounters(
    patcher: &mut DiscPatcher,
    table: &crate::encounter::MonsterPowerTable,
    baselines: &std::collections::HashMap<usize, u32>,
    cfg: SoloStrongConfig,
) -> Result<(usize, usize, Vec<usize>)> {
    let mut collapsed = 0;
    let mut zeroed = 0;
    let mut skipped = Vec::new();
    for idx in 0..patcher.entry_count() {
        let Some(&baseline) = baselines.get(&idx) else {
            continue;
        };
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(mut scene) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        let (c, z) = scene.enforce_solo_strong(table, baseline, cfg.threshold_pct);
        if c == 0 {
            continue;
        }
        match scene.repack() {
            Some(stream) => {
                patcher
                    .patch_prot_entry(idx, scene.man_offset as u64, &stream)
                    .with_context(|| format!("write scene {idx} MAN (solo-strong)"))?;
                collapsed += c;
                zeroed += z;
            }
            None => skipped.push(idx),
        }
    }
    Ok((collapsed, zeroed, skipped))
}

/// Randomize scene encounters ([`randomize_encounters_scoped`]) and, when `solo`
/// is set, additionally enforce the **solo-strong** rule: any random formation
/// that ends up holding a monster much stronger than the area's natives is forced
/// to that single enemy instead of a pack of 2+ (see
/// [`encounter::SceneEncounters::enforce_solo_strong`]).
///
/// The solo-strong pass is computed against each scene's **native** baseline
/// (captured before randomizing) and applied as a post-pass over the randomized
/// scenes, so it composes with every scope (Scene / Kingdom / World) and mode
/// (Shuffle / Random) without perturbing the multiset bookkeeping of the
/// underlying scoped randomization. `solo == None` reproduces
/// [`randomize_encounters_scoped`] byte-for-byte (the archive is not even read),
/// so existing runs are unchanged.
pub fn randomize_encounters_full(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
    scope: EncounterScope,
    unused_enemies: &[u8],
    solo: Option<SoloStrongConfig>,
) -> Result<EncounterApplyReport> {
    // Capture the native baselines (and the power table) BEFORE any edit, so the
    // "strong" judgement is against the area's authored difficulty.
    let solo_ctx = match solo {
        Some(cfg) => {
            let table = monster_power_table(patcher)?;
            let baselines = solo_strong_baselines(patcher, &table)?;
            Some((cfg, table, baselines))
        }
        None => None,
    };

    let mut report = randomize_encounters_scoped(patcher, seed, mode, scope, unused_enemies)?;

    if let Some((cfg, table, baselines)) = solo_ctx {
        let (collapsed, zeroed, skipped) =
            enforce_solo_strong_encounters(patcher, &table, &baselines, cfg)?;
        report.solo_collapsed += collapsed;
        report.ids_changed += zeroed;
        for idx in skipped {
            if !report.skipped.contains(&idx) {
                report.skipped.push(idx);
            }
        }
    }
    Ok(report)
}
