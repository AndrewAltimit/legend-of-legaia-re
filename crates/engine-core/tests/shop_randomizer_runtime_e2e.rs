//! Disc-gated runtime oracles for the **shop** randomizers - the buy-side
//! counterparts to the chest / drop / steal oracles.
//!
//! The rando crate's own disc-gated test (`shop_patch_real`) proves a patched
//! shop is *written* faithfully (the item-id byte changes inside the re-packed
//! scene MAN / the casino table; the disc still parses). What it does not prove
//! is that a runtime *reads the patched stock and lets the player buy the new
//! item* rather than serving a stale value. A savestate can't answer that
//! cleanly - the same RAM-cache trap the chest/drop oracles document: the menu
//! overlay's shop record is resident in RAM the moment the shop opens, so a
//! state captured in a shop on a patched disc still offers the original stock; a
//! patched shop is only observed after a fresh scene/overlay load re-reads it.
//!
//! The clean-room engine sidesteps the cache: it decodes the shop stock straight
//! from the patched disc bytes and runs the real purchase-grant kernel
//! ([`World::buy_from_shop`], shared with the menu runtime's `ShopConfirm`
//! commit). So each test here patches one shop slot's item id to a distinct id
//! on a scratch copy of the real disc (the surgical single-byte edit the
//! randomizer makes), re-decodes the patched stock off the patched image (the
//! bytes a fresh load would stream), builds a [`ShopSession`] from that stock,
//! drives a buy through `World::buy_from_shop`, and asserts the runtime sells /
//! grants the **patched** id, never the original.
//!
//! A baseline pass over the *unpatched* stock first confirms the engine grants
//! the original id, so the patched assertion can't pass vacuously.
//!
//! - **Town merchants** (`town_shop_buy_grants_patched_item`): stock is inline
//!   in the scene MAN field-VM script (op `0x49`); patched via
//!   [`legaia_patcher::shop::SceneShops`].
//! - **Casino exchange** (`casino_buy_grants_patched_prize`): stock is the
//!   static overlay table the casino buy UI shares with town shops (same
//!   handlers); patched via [`legaia_patcher::casino::CasinoExchange`].
//!
//! Skips without `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_engine_core::shop::{ShopInventory, ShopItem, ShopSession};
use legaia_engine_core::world::World;
use legaia_patcher::casino::{self, CasinoExchange};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::shop::SceneShops;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// A shop session over `ids` (cursor `i` = `ids[i]`), each at 1 gold so any
/// non-broke world can afford the buy - the assertion is about *which id* the
/// buy grants, not the price.
fn session_over(ids: &[u8]) -> ShopSession {
    let items = ids
        .iter()
        .map(|&id| ShopItem {
            item_id: id,
            price: 1,
        })
        .collect();
    ShopSession::new(ShopInventory::new(0, items))
}

/// Buy the item at buy-list cursor `cursor` and return the granted id, or
/// `None` if the buy didn't commit. Exercises the real engine kernel.
fn buy_cursor(world: &mut World, mut session: ShopSession, cursor: usize) -> Option<u8> {
    session.select_buy_item(cursor);
    world.buy_from_shop(&session).map(|(id, _, _)| id)
}

#[test]
fn town_shop_buy_grants_patched_item() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // Locate the first town shop on the disc and its first buy slot.
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let (entry_idx, orig_ids) = (0..patcher.entry_count())
        .find_map(|idx| {
            let entry = patcher.read_entry(idx).ok()?;
            let sc = SceneShops::locate(&entry, idx)?;
            let shop0 = sc.shops.first()?;
            let ids: Vec<u8> = shop0.id_offsets.iter().map(|&o| sc.decoded[o]).collect();
            (!ids.is_empty()).then_some((idx, ids))
        })
        .expect("a town shop exists on the disc");
    let orig_id = orig_ids[0];

    // Baseline: a buy at cursor 0 of the unpatched shop grants the original id.
    let mut w0 = World {
        money: 1_000_000,
        ..World::default()
    };
    assert_eq!(
        buy_cursor(&mut w0, session_over(&orig_ids), 0),
        Some(orig_id),
        "baseline: unpatched shop sells its original first item"
    );
    assert_eq!(w0.inventory.get(&orig_id).copied(), Some(1));

    // A distinct replacement id (a real consumable, != original).
    let new_id = if orig_id == 0x80 { 0x77 } else { 0x80 };

    // Patch the first shop's first slot in the scene MAN and write it back.
    let entry = patcher.read_entry(entry_idx).unwrap();
    let mut sc = SceneShops::locate(&entry, entry_idx).unwrap();
    let slot_off = sc.shops[0].id_offsets[0];
    sc.set_id(slot_off, new_id);
    let stream = sc.repack().expect("shop MAN re-packs within budget");
    patcher
        .patch_prot_entry(entry_idx, sc.man_offset as u64, &stream)
        .expect("write patched shop MAN");

    // Re-decode the patched stock off the patched image and buy cursor 0.
    let patched_entry = patcher.read_entry(entry_idx).unwrap();
    let sc2 = SceneShops::locate(&patched_entry, entry_idx).expect("patched shop still decodes");
    let patched_ids: Vec<u8> = sc2.shops[0]
        .id_offsets
        .iter()
        .map(|&o| sc2.decoded[o])
        .collect();
    assert_eq!(patched_ids[0], new_id, "patched stock holds the new id");

    let mut w1 = World {
        money: 1_000_000,
        ..World::default()
    };
    let granted = buy_cursor(&mut w1, session_over(&patched_ids), 0);
    assert_eq!(
        granted,
        Some(new_id),
        "runtime sells the PATCHED shop item, not the original"
    );
    assert_eq!(w1.inventory.get(&new_id).copied(), Some(1), "bag got it");
    assert_ne!(new_id, orig_id, "non-vacuous: the id actually changed");
}

#[test]
fn casino_buy_grants_patched_prize() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // Read the casino prize table (block 0) and its first prize.
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let entry = patcher.read_entry(casino::CASINO_ENTRY).unwrap();
    let ex = CasinoExchange::parse(
        &entry,
        casino::CASINO_TABLE_OFFSET,
        casino::CASINO_BLOCK_COUNT,
    )
    .expect("casino table parses");
    let orig_prizes: Vec<u8> = ex.blocks[0].iter().map(|p| p.item_id as u8).collect();
    let orig_id = orig_prizes[0];

    // Baseline: buying prize 0 of the unpatched table grants the original id.
    let mut w0 = World {
        money: 9_999_999,
        ..World::default()
    };
    assert_eq!(
        buy_cursor(&mut w0, session_over(&orig_prizes), 0),
        Some(orig_id),
        "baseline: unpatched casino offers its original first prize"
    );

    let new_id = if orig_id == 0x80 { 0x77 } else { 0x80 };

    // Patch the first prize's item id in the raw overlay table, write it back.
    let mut entry = patcher.read_entry(casino::CASINO_ENTRY).unwrap();
    let mut ex_mut = CasinoExchange::parse(
        &entry,
        casino::CASINO_TABLE_OFFSET,
        casino::CASINO_BLOCK_COUNT,
    )
    .unwrap();
    ex_mut.blocks[0][0].item_id = new_id as u16;
    ex_mut.write_back(&mut entry);
    let base = casino::CASINO_TABLE_OFFSET;
    let span = casino::CASINO_BLOCK_COUNT * casino::BLOCK_SIZE;
    patcher
        .patch_prot_entry(casino::CASINO_ENTRY, base as u64, &entry[base..base + span])
        .expect("write patched casino table");

    // Re-decode off the patched image and buy prize 0.
    let patched = patcher.read_entry(casino::CASINO_ENTRY).unwrap();
    let ex2 = CasinoExchange::parse(
        &patched,
        casino::CASINO_TABLE_OFFSET,
        casino::CASINO_BLOCK_COUNT,
    )
    .unwrap();
    let patched_prizes: Vec<u8> = ex2.blocks[0].iter().map(|p| p.item_id as u8).collect();
    assert_eq!(
        patched_prizes[0], new_id,
        "patched table holds the new prize"
    );

    let mut w1 = World {
        money: 9_999_999,
        ..World::default()
    };
    let granted = buy_cursor(&mut w1, session_over(&patched_prizes), 0);
    assert_eq!(
        granted,
        Some(new_id),
        "runtime grants the PATCHED casino prize, not the original"
    );
    assert_eq!(w1.inventory.get(&new_id).copied(), Some(1), "bag got it");
    assert_ne!(new_id, orig_id, "non-vacuous: the prize actually changed");
}
