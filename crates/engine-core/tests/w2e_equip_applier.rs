//! The per-slot equip applier (`FUN_801E5A08`) as the field menu runs it.
//!
//! Three properties, all read off the disassembly rather than off the port:
//!
//! 1. The destination is the **item's equip class**, not the row the player
//!    confirmed from (`0x801E5A58..0x801E5AE4`), with the class-`3` arm
//!    landing on equip byte `4` because its branch delay slot rewrites `v1`.
//! 2. A commit refunds the byte's prior occupant to the bag
//!    (`FUN_800421D4(old, 1)`) and cues SFX `0x24` (`FUN_80035BD0`).
//! 3. The write lands in the retail record's `+0x196` window, which is what
//!    `legaia_save`'s own accessors read back.

use legaia_engine_core::battle_stats::{EquipmentTable, ItemModifier, StatRecord, StatusModifiers};
use legaia_engine_core::equip_session::{
    EQUIP_SLOTS, EquipEvent, EquipInput, EquipSession, RETAIL_EQUIP_BYTE_TO_ENGINE_SLOT,
    RETAIL_WEAPON_EQUIP_BYTE, retail_slot_row_for_engine_slot,
};
use legaia_engine_core::equipment::{DiscEquipEntry, DiscEquipInfo};
use legaia_engine_vm::dev_equip_commit::{BAG_MISS, EQUIP_SFX_CUE, EquipCommitHost, commit_equip};
use legaia_engine_vm::world_map_overlay::resolve_equip_slot;
use std::collections::HashMap;

use legaia_asset::equip_stats::EquipSlot as DiscSlot;

const HEAD_ITEM: u8 = 0x30;
const BODY_ITEM: u8 = 0x31;
const WEAPON_ITEM: u8 = 0x32;
const FOOT_ITEM: u8 = 0x33;

fn disc_info() -> DiscEquipInfo {
    let mk = |category| DiscEquipEntry {
        mask: 0x07,
        category,
        is_ra_seru: false,
        passive_index: None,
    };
    DiscEquipInfo::from_entries([
        (HEAD_ITEM, mk(DiscSlot::Head)),
        (BODY_ITEM, mk(DiscSlot::Body)),
        (WEAPON_ITEM, mk(DiscSlot::Weapon)),
        (FOOT_ITEM, mk(DiscSlot::Footwear)),
    ])
}

fn record() -> StatRecord {
    StatRecord {
        base_attack: 40,
        base_udf: 20,
        base_ldf: 20,
        base_accuracy: 80,
        base_evasion: 20,
        base_spd: 30,
        base_int: 15,
        equip: [0; 8],
    }
}

fn session(party_slot: u8) -> EquipSession {
    let mut inv: HashMap<u8, u8> = HashMap::new();
    for id in [HEAD_ITEM, BODY_ITEM, WEAPON_ITEM, FOOT_ITEM] {
        inv.insert(id, 1);
    }
    let mut table = EquipmentTable::new();
    for id in [HEAD_ITEM, BODY_ITEM, WEAPON_ITEM, FOOT_ITEM] {
        table.set(
            id,
            ItemModifier {
                atk: 3,
                ..Default::default()
            },
        );
    }
    EquipSession::new_with_restrictions(
        record(),
        inv,
        table,
        StatusModifiers::default(),
        Vec::new(),
        disc_info(),
        party_slot,
    )
}

/// The class routing, expressed against the engine's own slot order.
#[test]
fn the_item_class_picks_the_destination_not_the_browse_row() {
    let s = session(0);
    // Every row answers the same destination for a given item, because the
    // applier re-derives it from the class.
    for row in 0..EQUIP_SLOTS {
        if row == 3 || row >= 5 {
            // Hand Guard is the engine's own row and the Goods rows are
            // retail's verbatim arm; neither consults the class.
            continue;
        }
        assert_eq!(
            s.retail_destination_slot(row, HEAD_ITEM),
            1,
            "head-class item from row {row}"
        );
        assert_eq!(s.retail_destination_slot(row, BODY_ITEM), 2);
        assert_eq!(s.retail_destination_slot(row, WEAPON_ITEM), 0);
        // Class 3 lands on equip byte 4 (footwear), which is engine slot 4.
        assert_eq!(s.retail_destination_slot(row, FOOT_ITEM), 4);
    }
}

/// Noa's weapon lives in equip byte `3`, everyone else's in byte `2`
/// (`DAT_8007B42C`), and both fold onto the engine's single weapon slot.
#[test]
fn the_weapon_byte_is_per_character_and_folds_onto_one_engine_slot() {
    assert_eq!(RETAIL_WEAPON_EQUIP_BYTE, [2, 3, 2]);
    for party_slot in 0u8..3 {
        let s = session(party_slot);
        assert_eq!(s.retail_destination_slot(0, WEAPON_ITEM), 0);
        let byte = resolve_equip_slot(0x40, party_slot as usize, &RETAIL_WEAPON_EQUIP_BYTE);
        assert_eq!(byte, RETAIL_WEAPON_EQUIP_BYTE[party_slot as usize] as usize);
        assert_eq!(RETAIL_EQUIP_BYTE_TO_ENGINE_SLOT[byte], 0);
    }
}

/// Retail's third argument is the equip screen's slot **row**; rows `>= 4`
/// are the Goods rows and land at `row + 1`.
#[test]
fn goods_rows_map_onto_themselves() {
    for engine_slot in 5u8..=7 {
        let row = retail_slot_row_for_engine_slot(engine_slot).expect("goods row");
        assert!(row >= 4);
        assert_eq!(row as u8 + 1, engine_slot);
    }
    assert_eq!(
        retail_slot_row_for_engine_slot(3),
        None,
        "Hand Guard is ours"
    );
}

#[derive(Default)]
struct Bag {
    ids: HashMap<u8, u8>,
    sfx: Vec<u8>,
}

impl EquipCommitHost for Bag {
    fn find_in_bag(&self, item_id: u8) -> u16 {
        match self.ids.get(&item_id) {
            Some(q) if *q > 0 => item_id as u16,
            _ => BAG_MISS,
        }
    }
    fn take_from_bag(&mut self, bag_index: u16, qty: u8) {
        if let Some(q) = self.ids.get_mut(&(bag_index as u8)) {
            *q = q.saturating_sub(qty);
        }
    }
    fn give_to_bag(&mut self, item_id: u8, qty: u8) {
        *self.ids.entry(item_id).or_insert(0) += qty;
    }
    fn play_sfx(&mut self, cue: u8) {
        self.sfx.push(cue);
    }
}

/// The write lands where `legaia_save` reads the equip array from.
#[test]
fn the_commit_round_trips_through_the_retail_record_accessors() {
    let mut rec = legaia_save::character::CharacterRecord::zeroed();
    // Seat an old helmet so the refund arm runs.
    let mut slots = rec.equipment();
    slots.slots[1] = 0x77;
    rec.set_equipment(slots);

    let mut bag = Bag::default();
    bag.ids.insert(HEAD_ITEM, 1);

    let out = commit_equip(
        &mut bag,
        &mut rec.raw,
        HEAD_ITEM,
        0,
        0,    // slot row 0 - the applier ignores it for an armament class
        0x20, // `+7 & 0x60` for the head class
        &RETAIL_WEAPON_EQUIP_BYTE,
    )
    .expect("bag holds the item");

    assert_eq!(out.slot, 1, "head class writes equip byte 1");
    assert_eq!(out.equipped, HEAD_ITEM);
    assert_eq!(out.refunded, Some(0x77));
    assert_eq!(bag.sfx, vec![EQUIP_SFX_CUE]);
    assert_eq!(bag.ids.get(&HEAD_ITEM).copied(), Some(0), "one taken");
    assert_eq!(bag.ids.get(&0x77).copied(), Some(1), "old one refunded");

    // The typed accessor sees the same byte the applier wrote.
    assert_eq!(rec.equipment().slots[1], HEAD_ITEM);
    let reparsed = legaia_save::character::CharacterRecord {
        raw: rec.raw.clone(),
    };
    assert_eq!(reparsed.equipment().slots[1], HEAD_ITEM);
}

/// A bag miss is retail's `return 0`: nothing moves and nothing sounds.
#[test]
fn a_bag_miss_changes_nothing() {
    let mut rec = legaia_save::character::CharacterRecord::zeroed();
    let before = rec.raw.clone();
    let mut bag = Bag::default();
    let out = commit_equip(
        &mut bag,
        &mut rec.raw,
        HEAD_ITEM,
        0,
        0,
        0x20,
        &RETAIL_WEAPON_EQUIP_BYTE,
    );
    assert!(out.is_none());
    assert_eq!(rec.raw, before);
    assert!(bag.sfx.is_empty());
}

/// The live path: browse a slot, pick the item, confirm, and the session
/// commits through the applier - cue and all.
#[test]
fn the_equip_screen_confirm_runs_the_applier() {
    let mut s = session(0);
    // Row 0 is Best Equipment, so the Helmet slot (engine index 1) is row 2.
    let cross = EquipInput {
        cross: true,
        ..Default::default()
    };
    let down = EquipInput {
        down: true,
        ..Default::default()
    };
    for _ in 0..2 {
        s.input(down);
    }
    s.input(cross); // open the Helmet slot's candidate list
    s.input(cross); // pick the top candidate -> Yes/No
    s.input(cross); // confirm Yes
    let events = s.drain_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EquipEvent::Committed { slot: 1, .. })),
        "expected a commit into the helmet slot, got {events:?}"
    );
    assert!(
        events.contains(&EquipEvent::ConfirmSfx { cue: EQUIP_SFX_CUE }),
        "the applier's 0x24 cue must reach the host: {events:?}"
    );
    assert_eq!(s.record().equip[1], HEAD_ITEM);
}
