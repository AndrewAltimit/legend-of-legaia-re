//! Disc-gated: the equip screen's three **Goods** rows offer the retail
//! candidate set.
//!
//! Retail reaches those rows through the content-builder cases `0x1C`..`0x1E`
//! (`FUN_80030628`), whose filter is item-record class **2** plus an
//! item-effect `+3` byte other than `0x41` - a different id space from the
//! four armament rows' class-1 lists, and with no character-mask term. A
//! `DiscEquipInfo` built from the equipment stat table alone knows none of
//! those ids, so before the widening a Goods row browsed an empty list.
//!
//! Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::Vfs;
use legaia_engine_core::battle_stats::{EquipmentTable, StatRecord, StatusModifiers};
use legaia_engine_core::equip_session::EquipSession;
use legaia_engine_core::equipment::{DiscEquipInfo, EquipSlot};
use legaia_engine_core::world::ItemBag;
use std::path::PathBuf;

fn scus() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !path.is_file() {
        return None;
    }
    legaia_engine_core::DiscVfs::open(&path)
        .ok()?
        .read("SCUS_942.54")
        .ok()
}

#[test]
fn goods_index_is_the_class_2_rows_with_a_passive() {
    let Some(scus) = scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or unreadable");
        return;
    };
    let stats = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus).expect("equip table");
    let effects = legaia_asset::item_effect::ItemEffectTable::from_scus(&scus).expect("effects");
    let mut info = DiscEquipInfo::from_disc(&stats);
    assert_eq!(
        info.goods_count(),
        0,
        "the equipment stat table alone indexes no Goods id"
    );
    info.install_goods(&effects);
    assert!(info.goods_count() > 0, "the disc offers Goods candidates");

    // Every indexed id passes the retail filter, and no class-1 equipment id
    // leaks into it (the armament lists own those).
    for id in 0u8..=u8::MAX {
        let listed = info.is_goods_candidate(id);
        let kind = effects.kind(id);
        let marker = effects
            .descriptor(effects.subtype(id))
            .map(|d| d.marker)
            .unwrap_or(legaia_engine_core::menu_list_rows::GOODS_NO_PASSIVE_MARKER);
        assert_eq!(
            listed,
            kind == 2 && marker != legaia_engine_core::menu_list_rows::GOODS_NO_PASSIVE_MARKER,
            "id {id:#04x}: kind {kind}, marker {marker:#04x}"
        );
        if listed {
            assert!(
                !stats.is_equipment(id),
                "id {id:#04x} is class 1 equipment, not a Goods candidate"
            );
        }
    }
    eprintln!("[ok] Goods candidate ids: {}", info.goods_count());
}

#[test]
fn a_goods_row_lists_accessories_and_an_armament_row_does_not() {
    let Some(scus) = scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or unreadable");
        return;
    };
    let stats = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus).expect("equip table");
    let effects = legaia_asset::item_effect::ItemEffectTable::from_scus(&scus).expect("effects");

    // One Goods candidate and one class-1 helmet, both in the bag.
    let goods_id = (0u8..=u8::MAX)
        .find(|&id| {
            effects.kind(id) == 2
                && effects
                    .descriptor(effects.subtype(id))
                    .is_some_and(|d| d.marker != 0x41)
        })
        .expect("the disc carries a Goods candidate");
    let helmet_id = (0u8..=u8::MAX)
        .find(|&id| {
            stats.bonus(id).is_some_and(|b| {
                b.slot() == legaia_asset::equip_stats::EquipSlot::Head && b.equips_party_slot(0)
            })
        })
        .expect("the disc carries a helmet Vahn can wear");

    let mut bag = ItemBag::default();
    bag.add(goods_id, 1);
    bag.add(helmet_id, 1);

    let build = |info: DiscEquipInfo| {
        EquipSession::new(
            StatRecord::default(),
            bag.clone(),
            EquipmentTable::new(),
            StatusModifiers::default(),
            Vec::new(),
        )
        .with_restrictions(info, 0)
    };

    // Without the Goods index the row is empty - the shape the widening fixes.
    let bare = build(DiscEquipInfo::from_disc(&stats));
    assert!(
        bare.items_for_slot(EquipSlot::Ring1.as_index()).is_empty(),
        "an un-widened table offers no Goods candidate"
    );

    let mut info = DiscEquipInfo::from_disc(&stats);
    info.install_goods(&effects);
    let session = build(info);
    let goods_rows: Vec<u8> = session
        .items_for_slot(EquipSlot::Ring1.as_index())
        .iter()
        .map(|i| i.id)
        .collect();
    assert!(
        goods_rows.contains(&goods_id),
        "the Goods row lists the accessory ({goods_id:#04x}): {goods_rows:?}"
    );
    assert!(
        !goods_rows.contains(&helmet_id),
        "the Goods row does not list class-1 equipment"
    );

    let helmet_rows: Vec<u8> = session
        .items_for_slot(EquipSlot::Helmet.as_index())
        .iter()
        .map(|i| i.id)
        .collect();
    assert!(
        helmet_rows.contains(&helmet_id),
        "the helmet row still lists the helmet: {helmet_rows:?}"
    );
    assert!(
        !helmet_rows.contains(&goods_id),
        "an armament row never lists a Goods id"
    );
    eprintln!("[ok] Goods row {goods_id:#04x}, helmet row {helmet_id:#04x}");
}
