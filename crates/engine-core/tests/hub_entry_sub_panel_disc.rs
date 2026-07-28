//! Disc-gated: the hub entry list's equipment sub-panel over **real** records
//! and **real** equipment rows.
//!
//! Its disc-free sibling `hub_entry_sub_panel_env` runs the same chain over a
//! synthetic executable. This one takes the character records from the disc's
//! own new-game party template, the item property + equipment stat tables from
//! the real `SCUS_942.54`, and installs them on the world exactly the way a
//! boot does - then drives `World::tick` and reads the printed numbers back.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_core::Vfs;
use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::equipment::{DiscEquipInfo, equip_modifier_table_from_disc};
use legaia_engine_core::field_submode_screen::RETAIL_WEAPON_SLOTS;
use legaia_engine_core::new_game::starting_record;
use legaia_engine_core::world::World;
use legaia_engine_vm::baka_hub_actors::{HubDraw, slot};
use legaia_engine_vm::world_map_overlay::{
    EQUIP_INK_REJECT, EQUIP_MODE_DIRECT, EQUIP_REJECT_VA, EquipPanelDraw,
};
use std::path::PathBuf;

/// Panel-window record the entry list paints.
const ENTRY_LIST_WINDOW: usize = 3;

/// Chaos Breaker: Vahn-only (`+6` mask `1`), weapon slot (`+7 & 0x60 = 0x40`).
const ID_CHAOS_BREAKER: u8 = 0x27;
/// Master Armor: Vahn-only, body-armour slot (`+7 & 0x60 = 0x00`).
const ID_MASTER_ARMOR: u8 = 0x47;

fn disc_scus() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return None;
    }
    Some(
        legaia_engine_core::DiscVfs::open(&path)
            .expect("open disc")
            .read("SCUS_942.54")
            .expect("SCUS_942.54 present"),
    )
}

/// A field world carrying the disc's own tables and two template records.
fn disc_world(scus: &[u8]) -> World {
    let mut w = World::new();
    w.man_load_actor_reset();
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some()
    );

    let equip = legaia_asset::equip_stats::EquipStatTable::from_scus(scus).expect("equip table");
    let effects =
        legaia_asset::item_effect::ItemEffectTable::from_scus(scus).expect("item-effect table");
    w.set_equipment_table(equip_modifier_table_from_disc(&equip));
    w.set_item_effects(effects);
    w.install_hub_equip_restrictions(&DiscEquipInfo::from_disc(&equip));

    // The disc's own new-game party template, two slots of it.
    let template = legaia_asset::new_game::StartingParty::from_scus(scus).expect("party template");
    let members: Vec<_> = (0..2)
        .filter_map(|i| template.member(i))
        .map(starting_record)
        .collect();
    assert_eq!(members.len(), 2, "the template carries four records");
    w.roster = legaia_save::Party { members };
    w.active_party = vec![0];
    w
}

fn sub_panel(w: &World) -> Vec<EquipPanelDraw> {
    w.submode_screen
        .draws()
        .iter()
        .filter_map(|d| match d {
            HubDraw::EntrySubPanel(p) => Some(*p),
            _ => None,
        })
        .collect()
}

fn painted(w: &mut World, mode: u32, cursor: i32) -> Vec<EquipPanelDraw> {
    w.open_field_submode_screen(slot::DRAW_TICK, Some(ENTRY_LIST_WINDOW));
    w.set_hub_equip_mode(mode);
    w.submode_screen.counter.cursor = cursor;
    for _ in 0..16 {
        w.tick();
        let panel = sub_panel(w);
        if !panel.is_empty() {
            return panel;
        }
    }
    Vec::new()
}

fn values(panel: &[EquipPanelDraw]) -> Vec<i32> {
    panel
        .iter()
        .filter_map(|d| match d {
            EquipPanelDraw::Value { value, .. } => Some(*value),
            _ => None,
        })
        .collect()
}

#[test]
fn the_panel_prints_the_template_records_stats_plus_the_disc_bonus_bytes() {
    let Some(scus) = disc_scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = disc_world(&scus);

    let mut eq = w.roster.members[0].equipment();
    eq.slots[0] = ID_CHAOS_BREAKER; // engine weapon slot
    eq.slots[2] = ID_MASTER_ARMOR; // engine body-armour slot
    w.roster.members[0].set_equipment(eq);
    let base = w.roster.members[0].live_stats();

    // The bonus bytes straight off the disc, so the expectation is not a
    // hand-copied number.
    let table = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus).unwrap();
    let weapon = table
        .bonus(ID_CHAOS_BREAKER)
        .expect("Chaos Breaker is equipment");
    let armor = table
        .bonus(ID_MASTER_ARMOR)
        .expect("Master Armor is equipment");
    assert!(
        weapon.attack() > 0 && armor.def_up() > 0,
        "non-vacuous rows"
    );

    let panel = painted(&mut w, 0, 0);
    assert_eq!(
        values(&panel),
        vec![
            i32::from(base.atk) + i32::from(weapon.attack()) + i32::from(armor.attack()),
            i32::from(base.udf) + i32::from(weapon.def_up()) + i32::from(armor.def_up()),
            i32::from(base.ldf) + i32::from(weapon.def_down()) + i32::from(armor.def_down()),
        ],
        "ATK / UDF / LDF = the record's own base stat plus the disc's `+1` / \
         `+2` / `+3` bytes across the equipped slots"
    );
}

#[test]
fn the_empty_slot_sentinel_resolves_to_a_zero_row_on_the_disc() {
    let Some(scus) = disc_scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Retail looks id `0` up like any other id: the item table's `+1` byte for
    // id 0 names a bonus row, and that row is zeroed - which is *why* an empty
    // slot adds nothing, rather than the port special-casing it.
    let effects = legaia_asset::item_effect::ItemEffectTable::from_scus(&scus).unwrap();
    assert_eq!(effects.kind(0), 0, "id 0 is neither equipment nor a good");
    let sentinel_row = effects.subtype(0);
    assert_ne!(sentinel_row, 0, "id 0 names a row past the table's head");

    let mut w = disc_world(&scus);
    let base = w.roster.members[0].live_stats();
    let panel = painted(&mut w, 0, 0);
    assert_eq!(
        values(&panel),
        vec![
            i32::from(base.atk),
            i32::from(base.udf),
            i32::from(base.ldf)
        ],
        "five empty slots contribute nothing"
    );
}

#[test]
fn a_real_vahn_only_candidate_is_accepted_for_vahn_and_rejected_for_noa() {
    let Some(scus) = disc_scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus).unwrap();
    let mask = table
        .bonus(ID_CHAOS_BREAKER)
        .expect("Chaos Breaker is equipment")
        .equip_mask();
    assert_eq!(mask & 1, 1, "Vahn's bit is set");
    assert_eq!(mask & 2, 0, "Noa's is not");

    // Entry code 0 = Vahn: the comparison columns draw.
    let mut w = disc_world(&scus);
    let panel = painted(&mut w, EQUIP_MODE_DIRECT, i32::from(ID_CHAOS_BREAKER));
    assert_eq!(
        values(&panel).len(),
        6,
        "three rows, current + candidate each"
    );

    // Entry code 1 = Noa: the mask misses and one line replaces the panel.
    let mut w = disc_world(&scus);
    w.active_party = vec![1];
    let panel = painted(&mut w, EQUIP_MODE_DIRECT, i32::from(ID_CHAOS_BREAKER));
    assert_eq!(panel.len(), 1);
    match panel[0] {
        EquipPanelDraw::Label { label_va, ink, .. } => {
            assert_eq!(label_va, EQUIP_REJECT_VA);
            assert_eq!(ink, EQUIP_INK_REJECT);
        }
        other => panic!("expected the reject line, got {other:?}"),
    }
}

#[test]
fn a_real_kind_two_candidate_draws_the_plain_rows_and_an_unhandled_kind_draws_nothing() {
    let Some(scus) = disc_scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let effects = legaia_asset::item_effect::ItemEffectTable::from_scus(&scus).unwrap();
    let good = (1u8..=u8::MAX)
        .find(|&id| effects.kind(id) == 2)
        .expect("the disc carries kind-2 items");

    let mut w = disc_world(&scus);
    let panel = painted(&mut w, EQUIP_MODE_DIRECT, i32::from(good));
    assert_eq!(values(&panel).len(), 3, "kind 2 draws the plain block");

    // Every disc id is kind 0, 1 or 2, and the only kind-0 id is `0` - so the
    // "draws nothing" arm is what a cursor parked on an empty bag slot gets.
    assert!(
        (1u8..=u8::MAX).all(|id| matches!(effects.kind(id), 1 | 2)),
        "the disc's kind space is 1 / 2 outside the id-0 sentinel"
    );
    let mut w = disc_world(&scus);
    w.open_field_submode_screen(slot::DRAW_TICK, Some(ENTRY_LIST_WINDOW));
    w.set_hub_equip_mode(EQUIP_MODE_DIRECT);
    w.submode_screen.counter.cursor = 0;
    let mut ran = false;
    for _ in 0..16 {
        w.tick();
        if w.submode_screen
            .draws()
            .iter()
            .any(|d| matches!(d, HubDraw::Text { .. }))
        {
            ran = true;
            assert!(sub_panel(&w).is_empty(), "kind 0 draws no sub-panel rows");
            break;
        }
    }
    assert!(ran, "the entry list never ran");
}

#[test]
fn the_pinned_weapon_slot_table_matches_the_executable() {
    let Some(scus) = disc_scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // `DAT_8007B42C`, a halfword per character.
    let off = legaia_asset::new_game::scus_file_offset(&scus, 0x8007_B42C).expect("in-segment");
    let disc: Vec<i16> = (0..RETAIL_WEAPON_SLOTS.len())
        .map(|i| i16::from_le_bytes([scus[off + i * 2], scus[off + i * 2 + 1]]))
        .collect();
    assert_eq!(disc, RETAIL_WEAPON_SLOTS.to_vec());
    // The fourth halfword is zero, which is also what an index past the pinned
    // three resolves to - so the two agree for every character.
    assert_eq!(
        i16::from_le_bytes([scus[off + 6], scus[off + 7]]),
        0,
        "no fourth weapon slot"
    );
}
