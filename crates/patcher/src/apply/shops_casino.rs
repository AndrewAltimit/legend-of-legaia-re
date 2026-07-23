//! Town-shop stock + casino prize-exchange randomization.

use super::*;

/// Build the `256`-entry "id is sellable" mask from the disc's SCUS item table:
/// an id with a `> 0` price. Used to validate + delimit town-shop records - the
/// record `count` over-counts the purchasable stock by a trailing run of
/// unsellable (price-`0`) template ids, so the sellable mask both rejects stray
/// `0x49` payloads and trims the padding out of the stock (see
/// [`legaia_asset::shop_stock`]). `None` if SCUS / its item table is absent (the
/// shop locator then falls back to structural-only validation).
fn sellable_item_mask(patcher: &DiscPatcher) -> Option<[bool; 256]> {
    let scus = patcher.read_named_file("SCUS_942.54")?;
    let mut mask = [false; 256];
    for (id, slot) in mask.iter_mut().enumerate() {
        *slot = legaia_asset::item_names::price_slot(&scus, id as u8)
            .is_some_and(|(_, price)| price > 0);
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
    let mask = sellable_item_mask(patcher);
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
/// shop-item multiset, `Random` draws each slot from the **sellable pool** -
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
    let mask = sellable_item_mask(patcher);
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

/// Outcome of a fishing-prize price edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FishingPriceReport {
    /// Per edit: `(page, row, item_id, old_price, new_price)`.
    pub edits: Vec<(usize, usize, u32, u32, u32)>,
}

/// Set the fishing-exchange price of every prize row granting `item_id` to
/// `new_price` (across both venue pages). PROT 972 is a raw overlay, so each is
/// a same-size u32 write. No-op (empty report) when the price already matches;
/// errors if no prize grants `item_id`.
pub fn set_fishing_price(
    patcher: &mut DiscPatcher,
    item_id: u32,
    new_price: u32,
) -> Result<FishingPriceReport> {
    let overlay = patcher
        .read_entry(crate::fishing_price::OVERLAY_PROT_INDEX)
        .context("read fishing overlay for price edit")?;
    let edits = crate::fishing_price::plan_set_price(&overlay, item_id, new_price)?;
    let mut report = FishingPriceReport { edits: Vec::new() };
    for e in &edits {
        patcher
            .patch_prot_entry(
                crate::fishing_price::OVERLAY_PROT_INDEX,
                e.offset as u64,
                &e.new_price.to_le_bytes(),
            )
            .with_context(|| {
                format!(
                    "write fishing price for item 0x{:02X} (page {} row {})",
                    e.item_id, e.page, e.row
                )
            })?;
        report
            .edits
            .push((e.page, e.row, e.item_id, e.old_price, e.new_price));
    }
    Ok(report)
}
