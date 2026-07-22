//! Treasure-chest give-item randomization + audit.

use super::*;

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
                    // sites are static - only the pool composition is).
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
