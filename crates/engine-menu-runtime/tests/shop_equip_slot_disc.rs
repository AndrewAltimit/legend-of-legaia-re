//! Disc-gated: a shop's **buy-and-equip** lands a piece in the same record
//! byte the pause menu's Equip screen puts it in, for every piece of
//! equipment every gold shop on the disc sells and every character who can
//! wear it.
//!
//! Two paths write a character's `+0x196` equip bytes from a player's
//! choice: the shop's recipient picker (`FUN_801DB380`, the "<member> will
//! equip" rows) and the Equip screen's candidate confirm (`FUN_801E5A08`).
//! Retail resolves both through the item's equipment-record `+7` class, so
//! they cannot disagree. Here they are two pieces of Rust, and this sweep
//! drives each one through its own pad surface and compares where the piece
//! ended up: body armour on byte `0`, head gear on `1`, footwear on `4`, the
//! weapon on the character's own byte (`2` / `3` / `2`).
//!
//! The anchor is the Biron Monastery counter (`bylon`), whose Savior Clothes
//! Vahn buys and wears as body armour, not as a weapon.
//!
//! No Sony bytes are asserted - only where a bought id lands. Skips + passes
//! when `LEGAIA_DISC_BIN` is absent.

use legaia_engine_battle::battle_stats::EquipmentTable;
use legaia_engine_battle::spells::SpellCatalog;
use legaia_engine_battle::tactical_arts_editor::ChainLibrary;
use legaia_engine_core::world::World;
use legaia_engine_menu_runtime::field_menu_dispatch::{FieldMenuSubsession, apply_equip_swaps};
use legaia_engine_menu_runtime::menu_runtime::MenuRuntime;
use legaia_engine_menus::equip_session::record_byte_for_category;
use legaia_engine_menus::equipment::DiscEquipInfo;
use legaia_engine_menus::field_menu::FieldMenuRow;
use legaia_engine_menus::menu::{MenuInput, MenuState};
use legaia_engine_menus::options::OptionsState;
use legaia_engine_menus::save_select::{SaveRack, SlotSnapshot};
use legaia_engine_menus::shop::{ShopInventory, ShopItem, ShopSession};
use legaia_engine_menus::shop_catalog::{ShopItemData, scene_shops};
use legaia_engine_scene::vfs::Vfs;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_game_tables::equip_stats::{EquipSlot as Disc, EquipStatTable};
use legaia_save::{CharacterRecord, Party};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Savior Clothes, the piece the Biron Monastery counter sells.
const SAVIOR_CLOTHES: u8 = 0x44;
/// The CDNAME scene of Biron Monastery.
const BIRON: &str = "bylon";

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

/// Three joined members with empty equip bytes and a full purse.
fn party_world() -> World {
    let mut world = World::default();
    world.load_party(Party {
        members: (0..3).map(|_| CharacterRecord::zeroed()).collect(),
    });
    world.party.active_party = vec![0, 1, 2];
    world.party.money = 9_000_000;
    world
}

fn press(set: fn(&mut MenuInput)) -> MenuInput {
    let mut i = MenuInput::default();
    set(&mut i);
    i
}

/// The one record byte holding `item` after a path ran, or the reason there
/// is not exactly one.
fn byte_holding(world: &World, ch: u8, item: u8) -> Result<usize, String> {
    let slots = world.party.roster.members[usize::from(ch)]
        .equipment()
        .slots;
    let hits: Vec<usize> = (0..slots.len()).filter(|&i| slots[i] == item).collect();
    match hits[..] {
        [one] => Ok(one),
        _ => Err(format!(
            "item {item:#04x} sits in bytes {hits:?} of {slots:02x?}"
        )),
    }
}

/// Buy `item` at a counter and take the "<member> will equip" row for
/// character `ch`: the record byte the shop wrote.
fn shop_equip_byte(info: &DiscEquipInfo, item: u8, price: u32, ch: u8) -> Result<usize, String> {
    let mut world = party_world();
    let row = world
        .present_party_list()
        .iter()
        .position(|&c| c == ch)
        .ok_or("character not in the present party")?;
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut rt = MenuRuntime::new(tmp.path().to_path_buf());
    rt.retail_equipment_buy = true;
    rt.install_equip_info(info.clone());
    rt.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: item,
            price,
        }],
    )));
    rt.ctx.state = MenuState::ShopBuy.as_byte();
    rt.tick(&mut world, press(|i| i.cross = true));
    if rt.recipient_session.is_none() {
        return Err("the buy row did not open the recipient picker".into());
    }
    rt.tick(&mut world, MenuInput::default());
    for _ in 0..=row {
        rt.tick(&mut world, press(|i| i.down = true));
    }
    rt.tick(&mut world, press(|i| i.cross = true));
    rt.tick(&mut world, MenuInput::default());
    if rt.recipient_session.is_some() {
        return Err("the recipient picker refused the member".into());
    }
    byte_holding(&world, ch, item)
}

/// Put `item` in the bag and equip it on character `ch` through the pause
/// menu's Equip screen: the record byte that screen wrote.
fn equip_screen_byte(table: &EquipStatTable, cat: Disc, item: u8, ch: u8) -> Result<usize, String> {
    let mut world = party_world();
    world.party.active_party = vec![ch];
    world.set_equip_stats(table.clone());
    world.party.inventory.clear();
    world.party.inventory.insert(item, 1);
    let rack = SaveRack::Blocks((0..3).map(SlotSnapshot::empty).collect());
    let mut sub = FieldMenuSubsession::build(
        FieldMenuRow::Equip,
        &world,
        &OptionsState::default(),
        &rack,
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &EquipmentTable::new(),
    );
    // Character picker -> slot browse; row 0 is Best Equipment, then
    // weapon, helmet, body armour, footwear.
    let browse_row = match cat {
        Disc::Weapon => 1,
        Disc::Head => 2,
        Disc::Body => 3,
        Disc::Footwear => 4,
    };
    sub.tick_pad_edge(PadButton::Cross.mask());
    for _ in 0..browse_row {
        sub.tick_pad_edge(PadButton::Down.mask());
    }
    sub.tick_pad_edge(PadButton::Cross.mask());
    sub.tick_pad_edge(PadButton::Cross.mask());
    let FieldMenuSubsession::Equip {
        session, char_slot, ..
    } = &mut sub
    else {
        return Err("the Equip row built another sub-session".into());
    };
    if *char_slot != ch {
        return Err(format!("the Equip screen opened on member {char_slot}"));
    }
    let slot = *char_slot;
    if !apply_equip_swaps(session, slot, &mut world) {
        return Err("the Equip screen committed nothing".into());
    }
    byte_holding(&world, ch, item)
}

struct Tables {
    equip: EquipStatTable,
    info: DiscEquipInfo,
    data: ShopItemData,
}

fn tables(path: &PathBuf) -> Tables {
    let scus = legaia_engine_scene::vfs::DiscVfs::open(path)
        .expect("open disc vfs")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let equip = EquipStatTable::from_scus(&scus).expect("equipment table");
    Tables {
        info: DiscEquipInfo::from_disc(&equip),
        data: ShopItemData::from_scus(&scus).expect("item table"),
        equip,
    }
}

/// Every `(item id, price)` of equipment a gold shop on the disc sells.
fn sold_equipment(host: &SceneHost, t: &Tables) -> BTreeSet<(u8, u32)> {
    let mut all = BTreeSet::new();
    for idx in 0..host.index.entry_count() as u32 {
        let Ok(bytes) = host.index.entry_bytes_extended(idx) else {
            continue;
        };
        for shop in scene_shops(&bytes, idx as usize, Some(&t.data)) {
            for it in &shop.inventory.items {
                if t.info.entry(it.item_id).is_some() {
                    all.insert((it.item_id, it.price));
                }
            }
        }
    }
    all
}

/// The reported case: Vahn buys Savior Clothes at Biron Monastery and takes
/// the equip-now row. They are body armour, so they land on byte `0`, and
/// his weapon byte `2` keeps what it held.
#[test]
fn savior_clothes_bought_at_biron_equip_to_vahns_body_slot() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let t = tables(&path);
    // The counter's own stock, as entering the scene installs it.
    let mut host = SceneHost::open_disc(&path).expect("open disc");
    host.world.shops.item_shop_data = Some(t.data.clone());
    host.enter_field_scene(BIRON, 0)
        .expect("enter Biron Monastery");
    let price = host
        .world
        .shops
        .scene_shops
        .iter()
        .flat_map(|s| s.inventory.items.iter())
        .find(|i| i.item_id == SAVIOR_CLOTHES)
        .map(|i| i.price)
        .expect("Biron Monastery's counter sells Savior Clothes");
    assert_eq!(t.info.category(SAVIOR_CLOTHES), Some(Disc::Body));
    assert!(t.info.can_equip(SAVIOR_CLOTHES, 0), "Vahn can wear them");
    assert_eq!(
        shop_equip_byte(&t.info, SAVIOR_CLOTHES, price, 0),
        Ok(0),
        "body armour is record byte 0, not Vahn's weapon byte"
    );
    assert_eq!(
        equip_screen_byte(&t.equip, Disc::Body, SAVIOR_CLOTHES, 0),
        Ok(0),
        "the Equip screen agrees"
    );
}

/// The class: every piece of equipment any shop sells, for every character
/// who can wear it, lands in one byte whichever screen equips it.
#[test]
fn every_sold_piece_equips_to_the_same_byte_from_the_shop_and_the_equip_screen() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let t = tables(&path);
    let host = SceneHost::open_disc(&path).expect("open disc");
    let all = sold_equipment(&host, &t);
    assert!(
        all.len() > 40,
        "the sweep saw the disc's shops: {}",
        all.len()
    );
    let mut cats = BTreeSet::new();
    let (mut pairs, mut wrong) = (0usize, Vec::new());
    for &(item, price) in &all {
        let cat = t.info.category(item).expect("equipment");
        cats.insert(cat as u8);
        for ch in 0..3u8 {
            if !t.info.can_equip(item, ch) {
                continue;
            }
            pairs += 1;
            let shop = shop_equip_byte(&t.info, item, price, ch);
            let screen = equip_screen_byte(&t.equip, cat, item, ch);
            let rule = Ok(record_byte_for_category(cat, ch));
            if shop != screen || shop != rule {
                wrong.push(format!(
                    "item {item:#04x} ({cat:?}) on character {ch}: shop {shop:?}, \
                     Equip screen {screen:?}, rule {rule:?}"
                ));
            }
        }
    }
    eprintln!(
        "[sweep] {} sold pieces, {pairs} (piece, character) pairs, {} disagree",
        all.len(),
        wrong.len()
    );
    assert_eq!(
        cats.len(),
        4,
        "weapons, head gear, body armour and footwear"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A save made while the shop misfiled the buy - Savior Clothes on Vahn's
/// weapon byte, the armour he already wore still on byte `0`, his weapon in
/// the bag - loads with the misfiled piece back in the bag and the weapon
/// byte free. The same save on a world without the disc table loads as
/// written, so the repair is the table's, not the loader's guess.
#[test]
fn a_save_with_savior_clothes_on_the_weapon_byte_loads_repaired() {
    use legaia_engine_save_state::WorldSaveState;
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let t = tables(&path);
    // Another body armour Vahn can wear, for the occupied body byte.
    let other = (1u8..=255)
        .find(|&id| {
            id != SAVIOR_CLOTHES
                && t.info.category(id) == Some(Disc::Body)
                && t.info.can_equip(id, 0)
                && !t.info.entry(id).is_some_and(|e| e.is_ra_seru)
        })
        .expect("a second body armour for Vahn");
    let weapon = (1u8..=255)
        .find(|&id| t.info.category(id) == Some(Disc::Weapon) && t.info.can_equip(id, 0))
        .expect("a weapon for Vahn");

    let mut bad = party_world();
    let mut slots = [0u8; 8];
    slots[0] = other;
    slots[2] = SAVIOR_CLOTHES;
    bad.party.roster.members[0].set_equipment(legaia_save::EquipmentSlots { slots });
    bad.party.inventory.insert(weapon, 1);
    let save = bad.save_full();

    let mut fixed = World::default();
    fixed.set_equip_stats(t.equip.clone());
    fixed.load_full(save.clone());
    let worn = fixed.party.roster.members[0].equipment().slots;
    assert_eq!(worn[0], other, "the armour he wore stays on");
    assert_eq!(worn[2], 0, "the weapon byte no longer holds body armour");
    assert_eq!(
        fixed.party.inventory.get(&SAVIOR_CLOTHES).copied(),
        Some(1),
        "the misfiled piece is in the bag, not lost"
    );
    assert_eq!(fixed.party.inventory.get(&weapon).copied(), Some(1));

    let mut plain = World::default();
    plain.load_full(save);
    assert_eq!(
        plain.party.roster.members[0].equipment().slots,
        slots,
        "without the disc table the record loads as written"
    );
}
