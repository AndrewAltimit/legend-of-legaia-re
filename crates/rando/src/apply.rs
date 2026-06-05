//! High-level orchestration: read the current gameplay data off a disc, plan a
//! randomization from a seed, and write the plan back into a [`DiscPatcher`].
//!
//! This is the glue the top-level CLI drives. It keeps the per-module logic
//! (drop planning, slot re-pack, sector write-back) decoupled and testable while
//! giving the binary a single call per feature. It embeds no game bytes — every
//! value it reads comes from the user's own disc image at runtime.

use anyhow::{Context, Result};

use crate::casino::{self, CasinoExchange};
use crate::chest::SceneChests;
use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use crate::door::SceneDoors;
use crate::drops::{CurrentDrop, DropAssignment, DropMode, plan_drops};
use crate::encounter::SceneEncounters;
use crate::equipment::{EquipmentItem, MonsterExp, plan_equipment_drops};
use crate::house_door::SceneHouseDoors;
use crate::rng::SplitMix64;
use crate::shop::SceneShops;

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

/// Read every populated monster's id + base EXP reward out of the `battle_data`
/// archive — the per-enemy input the equipment-drop planner tiers on. Mirrors
/// [`current_drops`] but exposes EXP (`+0x46`) instead of the drop fields.
pub fn current_monster_exp(patcher: &DiscPatcher) -> Result<Vec<MonsterExp>> {
    let entry = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive")?;
    let records =
        legaia_asset::monster_archive::records(&entry).context("decode monster archive records")?;
    Ok(records
        .iter()
        .map(|r| MonsterExp {
            monster_id: r.id,
            exp: r.exp,
        })
        .collect())
}

/// Plan and apply the equipment-drop randomization: turn **every** monster's
/// drop slot into a rare random weapon / armor / accessory, with a chance tiered
/// by the gear's price and the enemy's EXP (see [`crate::equipment`]). `pool` is
/// the equipment pool from [`crate::equipment::equipment_pool`]. Reuses
/// [`apply_drop_plan`], so a slot too tight to re-pack is skipped (recorded in
/// the report) rather than aborting the run.
pub fn randomize_equipment_drops(
    patcher: &mut DiscPatcher,
    pool: &[EquipmentItem],
    seed: u64,
) -> Result<(Vec<DropAssignment>, DropApplyReport)> {
    let monsters = current_monster_exp(patcher)?;
    let plan = plan_equipment_drops(&monsters, pool, seed);
    let report = apply_drop_plan(patcher, &plan)?;
    Ok((plan, report))
}

/// Build the `256`-entry "id names a real item" mask from the disc's SCUS item
/// table, used to validate town-shop records (so a stray `0x49`-prefixed byte
/// run can't be mistaken for a shop). `None` if SCUS / its item table is absent
/// (the shop locator then falls back to structural-only validation).
fn named_item_mask(patcher: &DiscPatcher) -> Option<[bool; 256]> {
    let scus = patcher.read_named_file("SCUS_942.54")?;
    let table = legaia_asset::item_names::ItemNameTable::from_scus(&scus)?;
    let mut mask = [false; 256];
    for (id, slot) in mask.iter_mut().enumerate() {
        *slot = table.name(id as u8).is_some();
    }
    Some(mask)
}

/// Locate a scene's shops, using the SCUS item-name mask when available
/// (strict) and structural-only validation otherwise.
fn locate_shops(entry: &[u8], idx: usize, mask: Option<&[bool; 256]>) -> Option<SceneShops> {
    match mask {
        Some(m) => SceneShops::locate_with_items(entry, idx, m),
        None => SceneShops::locate(entry, idx),
    }
}

/// One town shop's current stock, for the read-only listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopListing {
    /// PROT entry index of the scene bundle holding this shop.
    pub entry_idx: usize,
    /// On-screen shop name (e.g. "Variety Store").
    pub name: String,
    /// Item ids the shop currently sells, in display order.
    pub items: Vec<u8>,
}

/// Read every town-merchant shop on the disc (the randomizable population), in
/// PROT-entry then in-scene order. Mirrors [`current_chests`]: read-only, decodes
/// each scene MAN once via [`SceneShops::locate`].
pub fn current_shops(patcher: &DiscPatcher) -> Result<Vec<ShopListing>> {
    let mask = named_item_mask(patcher);
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(sc) = locate_shops(&entry, idx, mask.as_ref()) else {
            continue;
        };
        for shop in &sc.shops {
            out.push(ShopListing {
                entry_idx: idx,
                name: shop.name.clone(),
                items: shop.id_offsets.iter().map(|&o| sc.decoded[o]).collect(),
            });
        }
    }
    Ok(out)
}

/// Outcome of randomizing town shops.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ShopApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total shop item-id bytes changed.
    pub items_changed: usize,
    /// Total shop item slots found (the randomizable population).
    pub slots_total: usize,
    /// Scene PROT-entry indices whose recompressed MAN would not fit, skipped.
    pub skipped: Vec<usize>,
}

/// Install the chest-found-equipment shop prices (see [`crate::item_price`]) by
/// patching the `SCUS_942.54` item table in place. Returns the number of price
/// fields changed. Idempotent (re-applying writes nothing). Safe no-op if SCUS
/// or the item table is absent.
pub fn apply_item_price_edits(patcher: &mut DiscPatcher) -> Result<usize> {
    let Some(scus) = patcher.read_named_file(crate::steal::SCUS_NAME) else {
        return Ok(0);
    };
    let patches = crate::item_price::price_patches(&scus)?;
    for (off, bytes) in &patches {
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, *off as u64, bytes)
            .with_context(|| format!("write item price at SCUS offset {off}"))?;
    }
    Ok(patches.len())
}

/// Randomize town-merchant stock (field-VM shop op `0x49`; see [`crate::shop`]).
/// Shop item ids are global inventory ids, so this is a **global** reassignment
/// across every town shop on the disc: `Shuffle` redistributes the existing
/// shop-item multiset, `Random` draws each slot from the **sellable pool** —
/// items the game prices `> 0` (see [`crate::item_price::sellable_pool`]), which
/// excludes every quest / key / story item (all price `0`) so a shop can never
/// stock one. As a prerequisite this first prices the chest-found equipment
/// ([`apply_item_price_edits`]) so that gear is non-free and joins the sellable
/// pool. Only the item-id bytes are rewritten; each touched scene MAN is
/// recompressed and a scene whose MAN overflows is skipped.
pub fn randomize_shops(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<ShopApplyReport> {
    // Price the chest-found equipment so it is sellable (and not free) before we
    // read the sellable pool / stock it.
    apply_item_price_edits(patcher)?;

    // `Random` fill draws from the sellable pool (priced items only); `Shuffle`
    // redistributes the existing shop entries and ignores the pool.
    let item_pool: Vec<u8> = if mode == DropMode::Random {
        match patcher.read_named_file(crate::steal::SCUS_NAME) {
            Some(scus) => crate::item_price::sellable_pool(&scus)?,
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let item_pool = item_pool.as_slice();

    // Pass 1: collect every scene's shops (decoded MAN held for pass 2).
    let mask = named_item_mask(patcher);
    let mut scenes: Vec<SceneShops> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(sc) = locate_shops(&entry, idx, mask.as_ref()) {
            scenes.push(sc);
        }
    }

    // Per-scene ordered item-id slot offsets + originals.
    let offsets: Vec<Vec<usize>> = scenes.iter().map(|s| s.id_offsets()).collect();
    let originals: Vec<Vec<u8>> = scenes
        .iter()
        .zip(&offsets)
        .map(|(s, offs)| offs.iter().map(|&o| s.decoded[o]).collect())
        .collect();

    let mut report = ShopApplyReport {
        slots_total: offsets.iter().map(|o| o.len()).sum(),
        ..Default::default()
    };
    if report.slots_total == 0 {
        return Ok(report);
    }

    let mut skipped: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut streams: Vec<(usize, u64, Vec<u8>)> = Vec::new();

    match mode {
        DropMode::Shuffle => {
            // Iteratively converge on a writable set: shuffle the originals of
            // the not-yet-skipped scenes among those same slots, repack, and fold
            // any fresh overflow into `skipped` (which only shrinks the pool, so
            // it converges and the multiset over written slots stays preserved).
            loop {
                for (i, sc) in scenes.iter_mut().enumerate() {
                    for (k, &o) in offsets[i].iter().enumerate() {
                        sc.set_id(o, originals[i][k]);
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
                    for &o in &offsets[i] {
                        sc.set_id(o, pool[cur]);
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
        }
        DropMode::Random => {
            if item_pool.is_empty() {
                return Ok(report);
            }
            let mut rng = SplitMix64::new(seed);
            for (i, sc) in scenes.iter_mut().enumerate() {
                for &o in &offsets[i] {
                    let v = item_pool[rng.below(item_pool.len())];
                    sc.set_id(o, v);
                }
                match sc.repack() {
                    Some(stream) => streams.push((sc.entry_idx, sc.man_offset as u64, stream)),
                    None => {
                        skipped.insert(i);
                    }
                }
            }
        }
    }

    // Tally changes (over non-skipped scenes) and write the streams back.
    for (i, sc) in scenes.iter().enumerate() {
        if skipped.contains(&i) {
            continue;
        }
        let changed = offsets[i]
            .iter()
            .enumerate()
            .filter(|&(k, &o)| sc.decoded[o] != originals[i][k])
            .count();
        if changed > 0 {
            report.scenes_changed += 1;
            report.items_changed += changed;
        }
    }
    for (entry_idx, man_offset, stream) in streams {
        patcher
            .patch_prot_entry(entry_idx, man_offset, &stream)
            .with_context(|| format!("write scene {entry_idx} shop MAN"))?;
    }
    report.skipped = skipped.into_iter().map(|i| scenes[i].entry_idx).collect();
    Ok(report)
}

/// Read the casino prize-exchange table (PROT 0899), for the read-only listing.
/// Returns `None` if the entry / table can't be parsed.
pub fn current_casino(patcher: &DiscPatcher) -> Result<Option<CasinoExchange>> {
    let entry = patcher
        .read_entry(casino::CASINO_ENTRY)
        .context("read casino overlay entry 0899")?;
    Ok(CasinoExchange::parse(
        &entry,
        casino::CASINO_TABLE_OFFSET,
        casino::CASINO_BLOCK_COUNT,
    ))
}

/// Randomize the casino prize-exchange table (see [`crate::casino`]). A
/// same-size raw edit of PROT entry 0899 (no LZS), so it never overflows.
/// Returns the number of prize slots that changed.
pub fn randomize_casino(patcher: &mut DiscPatcher, seed: u64, mode: DropMode) -> Result<usize> {
    let mut entry = patcher
        .read_entry(casino::CASINO_ENTRY)
        .context("read casino overlay entry 0899")?;
    let Some(mut ex) = CasinoExchange::parse(
        &entry,
        casino::CASINO_TABLE_OFFSET,
        casino::CASINO_BLOCK_COUNT,
    ) else {
        return Ok(0);
    };
    let base = casino::CASINO_TABLE_OFFSET;
    let span = casino::CASINO_BLOCK_COUNT * casino::BLOCK_SIZE;
    let before = entry[base..base + span].to_vec();
    ex.randomize(seed, mode);
    ex.write_back(&mut entry);
    let after = &entry[base..base + span];
    let changed = before
        .chunks(casino::RECORD_SIZE)
        .zip(after.chunks(casino::RECORD_SIZE))
        .filter(|(a, b)| a != b)
        .count();
    if after != before.as_slice() {
        patcher
            .patch_prot_entry(casino::CASINO_ENTRY, base as u64, after)
            .context("write casino prize table")?;
    }
    Ok(changed)
}

/// Outcome of randomizing scene encounters.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EncounterApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total formation id bytes changed across all scenes.
    pub ids_changed: usize,
    /// Formation id slots (in written-back scenes) that ended up holding one of
    /// the `unused_enemies` ids — i.e. how many unused enemies the run actually
    /// placed. Always `0` unless `unused_enemies` was non-empty and the mode was
    /// [`DropMode::Random`].
    pub unused_placed: usize,
    /// Scene PROT-entry indices whose recompressed MAN would not fit the
    /// original footprint, so the scene was left untouched.
    pub skipped: Vec<usize>,
}

/// Randomize every scene's random-encounter formations in place. For each scene
/// bundle the monster ids are reassigned from the scene's own id pool, the MAN
/// is recompressed, and — when it fits the original compressed footprint —
/// written back. Scenes whose re-pack overflows are recorded in `skipped` and
/// left unchanged.
///
/// `unused_enemies` is the curated set of monster ids no formation normally
/// references (see [`crate::unused::UNUSED_ENEMY_IDS`]). When non-empty *and*
/// `mode` is [`DropMode::Random`], those ids join each scene's candidate pool so
/// the run can spawn them — the battle loader streams a monster's archive slot
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

/// A complete scene-transition destination descriptor (everything a `0x3F` op
/// carries). The atomic unit the door randomizer moves between sites: moving the
/// whole descriptor keeps the destination scene, its entry tile, and facing
/// internally consistent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dest {
    index: i16,
    name: Vec<u8>,
    entry_x: u8,
    entry_z: u8,
    dir: u8,
}

/// How scene-transition doors are reconnected.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DoorCoupling {
    /// Bidirectional: re-pair doors into two-way connections so walking through
    /// a door and turning around returns you the way you came. Doors that have
    /// no reverse partner (dead-end / one-way story warps) fall back to the
    /// decoupled assignment and are counted in [`DoorApplyReport::unpaired`].
    Coupled,
    /// One-way: every door's destination is reassigned independently, so going
    /// back through the destination's own doors is not guaranteed to return you.
    Decoupled,
}

/// Outcome of randomizing scene-transition doors.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DoorApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total door sites whose destination changed.
    pub sites_changed: usize,
    /// Total door sites found (the randomizable population).
    pub sites_total: usize,
    /// Coupled mode only: door sites with no reverse partner (dead-end / one-way
    /// story warps, or doors orphaned by an unequal-direction connection), left
    /// at their original destination because they can't be made two-way.
    pub unpaired: usize,
    /// Coupled mode only: matched connections left at their original
    /// destination because one endpoint's scene couldn't be rebuilt (e.g. an
    /// overflowing overworld hub). Both ends are reverted so the connection
    /// stays genuinely two-way rather than half-applied (a one-way warp).
    pub coupled_kept_original: usize,
    /// Scene PROT-entry indices whose rebuilt MAN overflowed the compressed
    /// footprint or failed validation, so the scene kept its original doors.
    pub skipped: Vec<usize>,
}

/// Build a random involution over the door sites and assign each its new
/// destination so the connection is **bidirectional**: for matched sites `A` and
/// `B`, `A` is sent to where `B` is reached from (`dest(partner_orig(B))`) and
/// vice versa, so walking `A → B`'s doorstep lands you on `B`, whose new
/// destination is `A`'s doorstep. `partner_orig(X)` is the reverse door (same
/// scene-pair, opposite direction); sites without one — or the odd site left
/// over by an odd matching — are **left at their original destination**
/// (untouched) and returned as the `unpaired` count: a door with no clean reverse
/// can't be made two-way, so coupled mode leaves it vanilla rather than giving it
/// a one-way reassignment. `homes[i]` is the CDNAME label of site `i`'s home
/// scene.
///
/// `plan_doors_coupled`'s return: `(dest_new, unpaired_count, matched_pairs,
/// original_partner)`. See its doc for what each element means.
type CoupledPlan = (Vec<Dest>, usize, Vec<(usize, usize)>, Vec<Option<usize>>);

/// Also returns the matched `(a, b)` pairs (the new involution) and the original
/// `partner` array (each site's reverse door, if any). [`randomize_doors`] needs
/// both: a connection is only truly bidirectional if every site it touches gets
/// written, and a site's edit depends on **both** its matched partner and its
/// original reverse. When a scene can't be rebuilt (e.g. an overflowing overworld
/// hub), the revert must propagate transitively along both involutions, or a
/// one-way warp masquerading as coupled survives.
fn plan_doors_coupled(origs: &[Dest], homes: &[String], rng: &mut SplitMix64) -> CoupledPlan {
    use std::collections::HashMap;
    let n = origs.len();
    let name_of = |d: &Dest| String::from_utf8_lossy(&d.name).into_owned();

    // Group site indices by (home_scene, dest_scene) — both directions of a
    // connection live in mirror groups (a,b) / (b,a).
    let mut groups: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (i, d) in origs.iter().enumerate() {
        groups
            .entry((homes[i].clone(), name_of(d)))
            .or_default()
            .push(i);
    }
    let mut partner = vec![None; n];
    let mut done: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let keys: Vec<(String, String)> = groups.keys().cloned().collect();
    for key in keys {
        if done.contains(&key) {
            continue;
        }
        let (h, d) = key.clone();
        if h == d {
            // Self-scene connection: pair consecutive sites within the group.
            let g = &groups[&key];
            let mut it = g.iter();
            while let (Some(&a), Some(&b)) = (it.next(), it.next()) {
                partner[a] = Some(b);
                partner[b] = Some(a);
            }
            done.insert(key);
            continue;
        }
        let rev = (d.clone(), h.clone());
        done.insert(key.clone());
        done.insert(rev.clone());
        if let (Some(g1), Some(g2)) = (groups.get(&key), groups.get(&rev)) {
            // Only couple a connection whose two directions have the SAME number
            // of doors. If they differ, pairing `min(len)` of them would leave
            // the majority direction's excess doors at their original
            // destination while their lone reverse gets matched away — producing
            // a dangling one-way edge (`HA→HB` survives, `HB→HA` vanishes). The
            // safe choice is to leave the whole unbalanced connection static so
            // both directions stay intact (it's reported in `unpaired`).
            if g1.len() == g2.len() {
                for i in 0..g1.len() {
                    partner[g1[i]] = Some(g2[i]);
                    partner[g2[i]] = Some(g1[i]);
                }
            }
        }
    }

    let mut dest_new = origs.to_vec();

    // Match partnered doors into the new involution, constrained to
    // **length-preserving** swaps. When `a` matches `b`, `a` receives the
    // descriptor `origs[partner[b]]` (its name is `b`'s home-scene label) and
    // vice versa. The rewrite keeps the MAN's decompressed size unchanged — so no
    // scene grows and none can overflow on recompress — exactly when the name
    // lengths line up: `len(home[b]) == len(dest[a])` and `len(home[a]) ==
    // len(dest[b])`. Bucketing each door by `key = (len(home), len(dest))` and
    // matching a `(p, q)` bucket against the mirror `(q, p)` bucket guarantees
    // both. This is what lets coupled mode randomize the overworld hubs (which
    // can't be grown in place) while staying two-way; the variable-length
    // relocation path is reserved for decoupled mode.
    let key = |i: usize| (homes[i].len(), origs[i].name.len());
    let mut buckets: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, p) in partner.iter().enumerate() {
        if p.is_some() {
            buckets.entry(key(i)).or_default().push(i);
        }
    }
    let mut matched_pairs: Vec<(usize, usize)> = Vec::new();
    let mut handled: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    let mut bkeys: Vec<(usize, usize)> = buckets.keys().copied().collect();
    bkeys.sort_unstable(); // deterministic iteration order
    for k in bkeys {
        if handled.contains(&k) {
            continue;
        }
        let (p, q) = k;
        if p == q {
            handled.insert(k);
            let mut g = buckets[&k].clone();
            rng.shuffle(&mut g);
            let mut i = 0;
            while i + 1 < g.len() {
                matched_pairs.push((g[i], g[i + 1]));
                i += 2;
            }
        } else {
            let rk = (q, p);
            handled.insert(k);
            handled.insert(rk);
            let mut ga = buckets.get(&k).cloned().unwrap_or_default();
            let mut gb = buckets.get(&rk).cloned().unwrap_or_default();
            rng.shuffle(&mut ga);
            rng.shuffle(&mut gb);
            for i in 0..ga.len().min(gb.len()) {
                matched_pairs.push((ga[i], gb[i]));
            }
        }
    }
    for &(a, b) in &matched_pairs {
        dest_new[a] = origs[partner[b].unwrap()].clone();
        dest_new[b] = origs[partner[a].unwrap()].clone();
    }

    // Unpaired: every door not placed in a matched pair — no reverse partner
    // (dead-end / one-way story warp, or a door orphaned by an unequal-direction
    // connection) or no length-compatible partner to swap with. These keep their
    // ORIGINAL destination (coupled mode never gives a door a one-way
    // reassignment), so `dest_new` already holds the right bytes — only the count
    // is reported.
    let matched: std::collections::HashSet<usize> =
        matched_pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    let unpaired = (0..n).filter(|i| !matched.contains(i)).count();
    (dest_new, unpaired, matched_pairs, partner)
}

/// Randomize scene-transition doors, one-way ([`DoorCoupling::Decoupled`]) or
/// bidirectional ([`DoorCoupling::Coupled`]). Each door's destination descriptor
/// (scene + entry tile + facing) is the atomic unit moved between sites.
///
/// - **Decoupled**: `Shuffle` permutes the existing destinations across all
///   doors (every scene stays reachable as some door's target); `Random` draws
///   each door's destination from the global pool.
/// - **Coupled**: re-pairs doors into two-way connections (the `mode` is treated
///   as the matching's randomness; doors with no reverse partner get the
///   decoupled fallback — see [`plan_doors_coupled`]).
///
/// Because the destination name is variable length, this uses the
/// [`crate::door::SceneDoors::rebuild`] relocation path (decompress → resize the
/// `0x3F` ops → fix the partition tables / section offset / intra-record jumps →
/// recompress → rewrite the descriptor size word). A scene whose rebuilt MAN
/// overflows its compressed footprint or fails validation keeps its original
/// doors and is recorded in `skipped`.
pub fn randomize_doors(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
    coupling: DoorCoupling,
) -> Result<DoorApplyReport> {
    use legaia_asset::man_edit::DestEdit;
    use legaia_asset::scene_asset_table::encode_size_word;

    let cd = cdname_map(patcher);

    // Pass 1: collect every scene's doors (decoded MAN held for pass 2).
    let mut scenes: Vec<SceneDoors> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(d) = SceneDoors::locate(&entry, idx) {
            scenes.push(d);
        }
    }

    // Flatten to a global ordered list of original destinations + home labels.
    let mut origs: Vec<Dest> = Vec::new();
    let mut homes: Vec<String> = Vec::new();
    for s in &scenes {
        let home = legaia_prot::cdname::block_for(&cd, s.entry_idx as u32)
            .unwrap_or("?")
            .to_string();
        for site in &s.sites {
            origs.push(Dest {
                index: site.index,
                name: site.name.clone().into_bytes(),
                entry_x: site.entry_x,
                entry_z: site.entry_z,
                dir: site.dir,
            });
            homes.push(home.clone());
        }
    }

    let mut report = DoorApplyReport {
        sites_total: origs.len(),
        ..Default::default()
    };
    if origs.is_empty() {
        return Ok(report);
    }

    // Plan the new destination for each global site index. `match_of` is the new
    // (coupled) involution; `partner_of` is each site's original reverse door.
    let n = origs.len();
    let mut rng = SplitMix64::new(seed);
    let mut match_of: Vec<Option<usize>> = vec![None; n];
    let mut partner_of: Vec<Option<usize>> = vec![None; n];
    let new_descs: Vec<Dest> = match coupling {
        DoorCoupling::Coupled => {
            let (descs, unpaired, pairs, partner) = plan_doors_coupled(&origs, &homes, &mut rng);
            report.unpaired = unpaired;
            for (a, b) in pairs {
                match_of[a] = Some(b);
                match_of[b] = Some(a);
            }
            partner_of = partner;
            descs
        }
        DoorCoupling::Decoupled => match mode {
            DropMode::Shuffle => {
                let mut v = origs.clone();
                rng.shuffle(&mut v);
                v
            }
            DropMode::Random => (0..origs.len())
                .map(|_| origs[rng.below(origs.len())].clone())
                .collect(),
        },
    };

    // Map each global site index to its scene (for the coupled revert pass).
    let mut site_scene = vec![0usize; origs.len()];
    {
        let mut g = 0usize;
        for (si, s) in scenes.iter().enumerate() {
            for _ in 0..s.sites.len() {
                site_scene[g] = si;
                g += 1;
            }
        }
    }

    // Pass 2: rebuild each scene from the planned destinations and collect the
    // streams to write. `forced` is the set of global site indices pinned to
    // their ORIGINAL destination (a forced site contributes no edit).
    //
    // The coupled revert is a transitive closure. If a site can't be written —
    // its scene overflows — it keeps its original destination, which only stays a
    // valid two-way connection if its matched partner *and* its original reverse
    // also keep theirs. So a forced site forces both `match_of[X]` (the new
    // partner, which was sending players to X) and `partner_of[X]` (the original
    // reverse, which X's now-reverted destination points back at). Propagating
    // along both involutions reverts whole alternating cycles, never leaving a
    // dangling one-way edge. Forcing only removes edits (shrinks scenes), so the
    // skipped set only shrinks and the loop converges (decoupled mode has empty
    // involutions, so it runs once and reverts nothing).
    let mut forced: std::collections::HashSet<usize> = std::collections::HashSet::new();
    // Per scene: (scene index, rebuilt stream, new decompressed size, edit count).
    let mut streams: Vec<(usize, Vec<u8>, u32, usize)> = Vec::new();
    let mut skipped_scenes: std::collections::HashSet<usize> = std::collections::HashSet::new();
    loop {
        streams.clear();
        skipped_scenes.clear();
        let mut g = 0usize;
        for (si, scene) in scenes.iter().enumerate() {
            let base = g;
            g += scene.sites.len();
            let mut edits: Vec<DestEdit> = Vec::new();
            for (k, site) in scene.sites.iter().enumerate() {
                if forced.contains(&(base + k)) {
                    continue; // pinned to original — no edit
                }
                let d = &new_descs[base + k];
                let unchanged = d.index == site.index
                    && d.name == site.name.as_bytes()
                    && d.entry_x == site.entry_x
                    && d.entry_z == site.entry_z
                    && d.dir == site.dir;
                if unchanged {
                    continue;
                }
                edits.push(DestEdit {
                    op_pc: site.op_pc,
                    index: d.index,
                    name: d.name.clone(),
                    entry_x: d.entry_x,
                    entry_z: d.entry_z,
                    dir: d.dir,
                });
            }
            if edits.is_empty() {
                continue;
            }
            match scene.rebuild(&edits) {
                Some((stream, new_size)) => streams.push((si, stream, new_size, edits.len())),
                None => {
                    skipped_scenes.insert(si);
                }
            }
        }

        // Seed the closure with every coupled site in an overflowing scene, then
        // propagate along both involutions until no new site is forced.
        let mut stack: Vec<usize> = (0..n)
            .filter(|&i| {
                !forced.contains(&i)
                    && (match_of[i].is_some() || partner_of[i].is_some())
                    && skipped_scenes.contains(&site_scene[i])
            })
            .collect();
        let mut new_force = false;
        while let Some(x) = stack.pop() {
            if !forced.insert(x) {
                continue;
            }
            new_force = true;
            if let Some(y) = match_of[x]
                && !forced.contains(&y)
            {
                stack.push(y);
            }
            if let Some(p) = partner_of[x]
                && !forced.contains(&p)
            {
                stack.push(p);
            }
        }
        if !new_force {
            break;
        }
    }

    // Coupled sites reverted to original (their cycle touched an un-writable
    // scene), reported so the user knows what stayed vanilla to keep returns
    // honest.
    report.coupled_kept_original = (0..n)
        .filter(|i| forced.contains(i) && (match_of[*i].is_some() || partner_of[*i].is_some()))
        .count();

    // Write the converged set.
    for (si, stream, new_size, n_edits) in &streams {
        let scene = &scenes[*si];
        patcher
            .patch_prot_entry(
                scene.entry_idx,
                scene.man_descriptor_off as u64,
                &encode_size_word(0x03, *new_size).to_le_bytes(),
            )
            .with_context(|| format!("write scene {} MAN size word", scene.entry_idx))?;
        patcher
            .patch_prot_entry(scene.entry_idx, scene.man_offset as u64, stream)
            .with_context(|| format!("write scene {} MAN", scene.entry_idx))?;
        report.scenes_changed += 1;
        report.sites_changed += n_edits;
    }
    report.skipped = skipped_scenes
        .iter()
        .map(|&si| scenes[si].entry_idx)
        .collect();
    report.skipped.sort_unstable();
    Ok(report)
}

/// Outcome of randomizing intra-town (house / interior) doors.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HouseDoorApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total MOVE_TO target tiles whose operand changed.
    pub sites_changed: usize,
    /// Total shuffleable (non-sentinel) MOVE_TO sites found.
    pub sites_total: usize,
    /// Scene PROT-entry indices whose recompressed MAN overflowed (kept original).
    pub skipped: Vec<usize>,
}

/// Read every shuffleable intra-town MOVE_TO site on the disc (the house-door
/// population), in PROT-entry order. Purely read-only audit surface; see
/// [`crate::house_door`] for the caveat that this set includes some NPC /
/// cutscene movement, not only house-door warps.
pub fn current_house_doors(patcher: &DiscPatcher) -> Result<Vec<(usize, u8, u8)>> {
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(sd) = SceneHouseDoors::locate(&entry, idx) else {
            continue;
        };
        for (xb, zb) in sd.current_targets() {
            out.push((idx, xb & 0x7F, zb & 0x7F));
        }
    }
    Ok(out)
}

/// Randomize intra-town (house / interior) doors by a **per-scene,
/// multiset-preserving shuffle** of the `0x23 MOVE_TO` target tiles (see
/// [`crate::house_door`]). Each scene's MAN is recompressed (same-size operand
/// edits) and written back when it fits; a scene that overflows keeps its
/// original tiles. Only [`DropMode::Shuffle`] is meaningful (a random draw would
/// place actors off-map), so a non-shuffle mode is a no-op.
pub fn randomize_house_doors(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<HouseDoorApplyReport> {
    let mut report = HouseDoorApplyReport::default();
    if !crate::house_door::supported_mode(mode) {
        return Ok(report);
    }
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(mut scene) = SceneHouseDoors::locate(&entry, idx) else {
            continue;
        };
        report.sites_total += scene.sites.len();
        let changed = scene.shuffle(seed);
        if changed == 0 {
            continue;
        }
        match scene.repack() {
            Some(stream) => {
                patcher
                    .patch_prot_entry(idx, scene.man_offset as u64, &stream)
                    .with_context(|| format!("write scene {idx} MAN"))?;
                report.scenes_changed += 1;
                report.sites_changed += changed;
            }
            None => report.skipped.push(idx),
        }
    }
    Ok(report)
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

/// One art's current button combo, for the read-only `arts` listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtSite {
    pub character: legaia_art::queue::Character,
    pub index: u8,
    pub ap: u8,
    /// Decoded combo (separator marker stripped).
    pub commands: Vec<legaia_art::queue::Command>,
    pub is_miracle: bool,
}

/// Read every Tactical Art's current button combo out of the static
/// `SCUS_942.54` arts-name table (`DAT_80075EC4`). Purely read-only — the audit
/// surface for what an arts-combo randomization would change. Includes the
/// per-character Miracle Art rows (flagged `is_miracle`), which the randomizer
/// leaves untouched.
pub fn current_arts(patcher: &DiscPatcher) -> Result<Vec<ArtSite>> {
    let edits = crate::arts::ArtsEdits::locate(patcher.image())
        .context("locate SCUS_942.54 arts-name table")?;
    Ok(edits
        .current()
        .into_iter()
        .map(|c| ArtSite {
            character: c.character,
            index: c.index,
            ap: c.ap,
            commands: c.commands,
            is_miracle: c.is_miracle,
        })
        .collect())
}

/// Outcome of randomizing Tactical-Arts button combos.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ArtsApplyReport {
    /// `+8` command pointers actually rewritten (no-op reassignments skipped).
    pub combos_changed: usize,
    /// Regular (non-Miracle) arts considered for reassignment.
    pub arts: usize,
}

/// Randomize each art's button combo by rewriting its directional **glyph
/// bytes in place** (same-size 2-byte SCUS edits — no re-pack, nothing
/// skipped). The bytes are the single copy both the Arts-menu display and the
/// in-battle input matcher read, so the trigger and the display stay in sync.
/// Input counts are preserved and each character's combos stay unique; the
/// per-character Miracle Art strings are left untouched. `Shuffle` permutes the
/// existing combos among same-length strings; `Random` writes fresh same-length
/// combos. Returns the plan plus the apply report.
pub fn randomize_arts(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: crate::arts::ArtsMode,
) -> Result<(Vec<crate::arts::ComboEdit>, ArtsApplyReport)> {
    let edits = crate::arts::ArtsEdits::locate(patcher.image())
        .context("locate SCUS_942.54 arts-name table")?;
    let plan = edits.plan(seed, mode);
    let report = ArtsApplyReport {
        combos_changed: edits.arts_changed(&plan),
        arts: edits.regular_art_count(),
    };
    // 1. The in-battle/menu input MATCHER: rewrite the 1-4 combo in each
    //    character's player-data record0 (the bytes the trigger actually reads).
    for ch in legaia_art::queue::Character::all() {
        let char_edits = edits.player_edits(&plan, ch);
        if char_edits.is_empty() {
            continue;
        }
        let index = crate::arts::player_entry_index(ch);
        let entry = patcher
            .read_entry(index)
            .with_context(|| format!("read player file PROT {index}"))?;
        if let Some((lzs_off, recompressed)) =
            crate::arts::patch_player_record0(&entry, &char_edits)
        {
            patcher
                .patch_prot_entry(index, lzs_off as u64, &recompressed)
                .with_context(|| format!("write player file PROT {index} record0"))?;
        }
    }
    // 2. The Arts-menu DISPLAY: rewrite the SCUS glyph string to the same combo
    //    so the shown arrows match the (now-patched) trigger.
    for (off, bytes) in edits.glyph_patches(&plan) {
        patcher
            .patch_named_file(crate::arts::SCUS_NAME, off, &bytes)
            .with_context(|| format!("write art combo glyph at SCUS offset {off:#x}"))?;
    }
    Ok((plan, report))
}

/// Give the unnamed accessory (item `0xFD`) the display name **"Seru Bell"** so
/// the `--unused-items` toggle hands out a presentable item instead of a blank.
/// Writes the name into reclaimable `SCUS_942.54` tail space and repoints only
/// `0xFD`'s name pointer (the other ids sharing the empty-string slot keep it).
///
/// Idempotent: if `0xFD` already resolves to a name (e.g. on an
/// already-patched image) it does nothing. Returns the name that was set, or
/// `None` if it was already named or the SCUS layout couldn't be resolved.
pub fn inject_seru_bell_name(patcher: &mut DiscPatcher) -> Result<Option<String>> {
    use crate::item_name::{NameInjection, SERU_BELL_ID, SERU_BELL_NAME};
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    // Skip if it already has a name (don't stack injections on re-runs).
    if let Some(table) = legaia_asset::item_names::ItemNameTable::from_scus(&scus)
        && table.name(SERU_BELL_ID).is_some()
    {
        return Ok(None);
    }
    let Some(plan) = NameInjection::plan(&scus, SERU_BELL_ID, SERU_BELL_NAME) else {
        return Ok(None);
    };
    // Two same-size writes: the string bytes, then the repointed pointer word.
    patcher
        .patch_named_file(
            crate::steal::SCUS_NAME,
            plan.string_file_off as u64,
            &plan.name_bytes,
        )
        .context("write Seru Bell name string")?;
    patcher
        .patch_named_file(
            crate::steal::SCUS_NAME,
            plan.ptr_file_off as u64,
            &plan.string_va.to_le_bytes(),
        )
        .context("repoint accessory 0xFD name pointer")?;
    Ok(Some(SERU_BELL_NAME.to_string()))
}

/// Read the new game's current starting inventory (`(item_id, count)` slots) by
/// decoding the seed code region in `SCUS_942.54`. Vanilla retail is a single
/// slot — Healing Leaf (`0x77`) ×5. Purely read-only.
pub fn current_starting_items(patcher: &DiscPatcher) -> Result<Vec<(u8, u8)>> {
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    let inv = legaia_asset::new_game::StartingInventory::from_scus(&scus)
        .context("decode starting-inventory seed")?;
    Ok(inv.items().to_vec())
}

/// Read whether the new game currently presets the all-towns Door-of-Wind warp
/// bitmask (the `--all-warps` toggle). Purely read-only.
pub fn current_all_warps(patcher: &DiscPatcher) -> Result<bool> {
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    Ok(legaia_asset::new_game::scus_unlocks_all_warps(&scus).unwrap_or(false))
}

/// Outcome of randomizing the new game's starting seed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StartingItemsApplyReport {
    /// Number of starting-item slots written (`0..=MAX_STARTING_ITEMS`).
    pub items_set: usize,
    /// The seeded `(item_id, count)` slots, for the manifest / CLI summary.
    pub items: Vec<(u8, u8)>,
    /// Whether the all-towns Door-of-Wind warp bitmask was preset.
    pub all_warps: bool,
}

/// Rewrite the new game's starting-seed code in `SCUS_942.54` from
/// [`StartingSeedOptions`].
///
/// Two independent reclaimable regions of `FUN_80034A6C` are patched in place
/// (same-size, no executable growth): the inventory seed region gets the planned
/// `(id, count)` slots, and — only when `all_warps` is set — a separate region
/// gets the visited-towns warp preset (so it never reduces the item capacity).
/// [`plan_seed`] resolves the options into the concrete plan. With inactive
/// options nothing is written (callers guard on
/// [`StartingSeedOptions::is_active`]). Deterministic in `(seed, opts)`.
///
/// [`StartingSeedOptions`]: crate::starting_items::StartingSeedOptions
/// [`plan_seed`]: crate::starting_items::plan_seed
pub fn randomize_starting_items(
    patcher: &mut DiscPatcher,
    seed: u64,
    opts: &crate::starting_items::StartingSeedOptions,
) -> Result<StartingItemsApplyReport> {
    let scus = patcher
        .read_named_file(crate::steal::SCUS_NAME)
        .context("read SCUS_942.54")?;
    let inv_off = legaia_asset::new_game::starting_inv_seed_file_offset(&scus)
        .context("locate starting-inventory seed region in SCUS_942.54")? as u64;
    let plan = crate::starting_items::plan_seed(seed, opts);

    // Inventory seed region: always rewritten when active (this also drops the
    // zero-loop, which the warp preset below relies on).
    let inv_patch = crate::starting_items::build_seed_patch_for(&plan);
    patcher
        .patch_named_file(crate::steal::SCUS_NAME, inv_off, &inv_patch)
        .with_context(|| format!("write starting-item seed at SCUS offset {inv_off:#x}"))?;

    // Warp preset: a separate code region, only touched when enabled.
    if plan.all_warps {
        let warp_off = legaia_asset::new_game::warp_seed_file_offset(&scus)
            .context("locate warp-preset region in SCUS_942.54")? as u64;
        let warp_patch = crate::starting_items::build_warp_patch();
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, warp_off, &warp_patch)
            .with_context(|| format!("write warp preset at SCUS offset {warp_off:#x}"))?;
    }

    Ok(StartingItemsApplyReport {
        items_set: plan.items.len(),
        items: plan.items,
        all_warps: plan.all_warps,
    })
}

#[cfg(test)]
mod door_plan_tests {
    use super::*;

    fn dest(name: &str, ex: u8) -> Dest {
        Dest {
            index: 0,
            name: name.as_bytes().to_vec(),
            entry_x: ex,
            entry_z: 0,
            dir: 0,
        }
    }

    /// Sorted multiset of (name, entry_x) — the descriptor identity for the
    /// permutation check.
    fn multiset(v: &[Dest]) -> Vec<(Vec<u8>, u8)> {
        let mut m: Vec<_> = v.iter().map(|d| (d.name.clone(), d.entry_x)).collect();
        m.sort();
        m
    }

    #[test]
    fn coupled_preserves_multiset_and_is_deterministic_when_all_paired() {
        // Two clean connections: town<->map and cave<->map.
        //   site 0: home town -> dest map (entry 0x10)   [A]
        //   site 1: home map  -> dest town (entry 0x20)  [B = reverse of A]
        //   site 2: home cave -> dest map (entry 0x30)   [C]
        //   site 3: home map  -> dest cave (entry 0x40)  [D = reverse of C]
        let origs = vec![
            dest("map", 0x10),
            dest("town", 0x20),
            dest("map", 0x30),
            dest("cave", 0x40),
        ];
        let homes = vec![
            "town".to_string(),
            "map".to_string(),
            "cave".to_string(),
            "map".to_string(),
        ];
        let mut rng = SplitMix64::new(0xC0FFEE);
        let (out, unpaired, _pairs, _partner) = plan_doors_coupled(&origs, &homes, &mut rng);
        assert_eq!(unpaired, 0, "all four sites have a reverse partner");
        // Coupling only moves existing descriptors -> multiset preserved.
        assert_eq!(multiset(&out), multiset(&origs));
        // Deterministic for a fixed seed.
        let mut rng2 = SplitMix64::new(0xC0FFEE);
        let (out2, _, _, _) = plan_doors_coupled(&origs, &homes, &mut rng2);
        assert_eq!(out, out2);
        // Scene-level edge multiset stays symmetric: for the new graph, every
        // (home -> dest) edge has a matching (dest -> home) edge.
        let mut edges: Vec<(String, String)> = out
            .iter()
            .zip(&homes)
            .map(|(d, h)| (h.clone(), String::from_utf8_lossy(&d.name).into_owned()))
            .collect();
        edges.sort();
        for (a, b) in &edges {
            assert!(
                edges.iter().any(|(x, y)| x == b && y == a),
                "edge {a}->{b} has no reverse"
            );
        }
    }

    #[test]
    fn coupled_counts_a_dead_end_site_as_unpaired() {
        // A one-way story warp: site 0 home A -> dest B, but no B -> A door.
        // Plus a clean pair (1<->2) so something is matchable.
        let origs = vec![dest("b", 1), dest("y", 2), dest("x", 3)];
        let homes = vec!["a".to_string(), "x".to_string(), "y".to_string()];
        let mut rng = SplitMix64::new(7);
        let (_out, unpaired, _pairs, _partner) = plan_doors_coupled(&origs, &homes, &mut rng);
        // site 0 (a->b) has no reverse; sites 1 (x->y) and 2 (y->x) pair.
        assert_eq!(unpaired, 1);
    }
}
