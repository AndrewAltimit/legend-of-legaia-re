//! The hub entry list's equipment sub-panel, painting **live world data**.
//!
//! Its sibling `hub_entry_sub_panel_world` proves the sub-draw is on the frame
//! loop's path. This one asserts what the sub-draw prints: every number comes
//! from `World::roster` and the two static tables a boot installs on the world
//! (`World::item_effects` + `World::equipment_table`), reached through
//! `World::tick` -> `tick_handler_actors` -> `tick_submode_screen` ->
//! `HubPainter::EntryList` -> `entry_list` -> `equip_stat_panel`.
//!
//! The tables are built the way both hosts build them - `ItemEffectTable` /
//! `EquipStatTable` parsed out of a PS-X EXE - over a synthetic executable, so
//! the whole chain runs without disc data and no Sony bytes are needed. The
//! disc-gated sibling `hub_entry_sub_panel_disc` runs the identical assertions
//! against the real `SCUS_942.54`.

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::equipment::{DiscEquipInfo, equip_modifier_table_from_disc};
use legaia_engine_core::field_submode_screen::RETAIL_WEAPON_SLOTS;
use legaia_engine_core::world::World;
use legaia_engine_vm::baka_hub_actors::{HubDraw, slot};
use legaia_engine_vm::world_map_overlay::{
    EQUIP_GLYPH_HIGHER, EQUIP_GLYPH_LOWER, EQUIP_INK_HIGHER, EQUIP_INK_LOWER, EQUIP_INK_NORMAL,
    EQUIP_INK_REJECT, EQUIP_MODE_BLANK, EQUIP_MODE_DIRECT, EQUIP_MODE_INVENTORY, EQUIP_REJECT_VA,
    EquipPanelDraw,
};

/// Panel-window record the entry list paints. Taken from `baka_hub_actors`
/// rather than spelled here: the record table's base was off by four records,
/// and a literal at every call site is what hid it.
const ENTRY_LIST_WINDOW: usize = legaia_engine_vm::baka_hub_actors::window::ENTRY_LIST;

// --- the fixture executable ------------------------------------------------
//
// Item ids and the equipment rows they name. Ids `0x10` / `0x11` are the
// starting loadout, `0x12` a Vahn-only upgrade, `0x20` a consumable (`kind 2`),
// and id `0` is the empty-slot sentinel the disc also gives `kind 0` - the one
// kind that draws nothing at all.

const ID_SWORD: u8 = 0x10;
const ID_ARMOR: u8 = 0x11;
const ID_VAHN_BLADE: u8 = 0x12;
const ID_POTION: u8 = 0x20;

const ROW_SWORD: u8 = 1;
const ROW_ARMOR: u8 = 2;
const ROW_BLADE: u8 = 3;
/// The row the disc's item id `0` names, and which no equippable id reaches.
const ROW_EMPTY_SENTINEL: u8 = 0x6A;

/// `[INT, ATK, UDF, LDF, SPD]` per row.
const SWORD_BONUS: [u8; 5] = [0, 20, 0, 0, 0];
const ARMOR_BONUS: [u8; 5] = [0, 0, 8, 6, 0];
const BLADE_BONUS: [u8; 5] = [0, 40, 0, 0, 0];

const BASE_ATK: u16 = 31;
const BASE_UDF: u16 = 17;
const BASE_LDF: u16 = 13;

/// Minimal synthetic PS-X EXE carrying an item property table and an equipment
/// stat-bonus table at their retail VAs. No Sony bytes.
fn synthetic_scus() -> Vec<u8> {
    use legaia_asset::equip_stats::{BONUS_STRIDE, BONUS_TABLE_VA, KIND_EQUIPMENT};

    const ITEM_TABLE_VA: u32 = 0x8007_4368;
    const ITEM_STRIDE: usize = 0x0C;
    let t_addr: u32 = 0x8001_0000;
    let t_size: u32 = 0x7_0000;
    let mut scus = vec![0u8; 0x800 + t_size as usize];
    scus[0..8].copy_from_slice(b"PS-X EXE");
    scus[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
    scus[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
    let off = |va: u32| (va - t_addr) as usize + 0x800;

    let mut item = |id: u8, kind: u8, stat_index: u8| {
        let base = off(ITEM_TABLE_VA) + usize::from(id) * ITEM_STRIDE;
        scus[base] = kind;
        scus[base + 1] = stat_index;
    };
    // The disc's own id 0: not equipment, and its `+1` byte names a row that
    // no equippable item reaches.
    item(0, 0, ROW_EMPTY_SENTINEL);
    item(ID_SWORD, KIND_EQUIPMENT, ROW_SWORD);
    item(ID_ARMOR, KIND_EQUIPMENT, ROW_ARMOR);
    item(ID_VAHN_BLADE, KIND_EQUIPMENT, ROW_BLADE);
    item(ID_POTION, 2, 7);

    let mut row = |index: u8, bonus: [u8; 5], mask: u8, slot_bits: u8| {
        let base = off(BONUS_TABLE_VA) + usize::from(index) * BONUS_STRIDE;
        scus[base..base + 5].copy_from_slice(&bonus);
        scus[base + 6] = mask;
        scus[base + 7] = slot_bits;
    };
    // `+7 & 0x60`: `0x40` weapon, `0x00` body armour. `+6`: `7` = any member,
    // `1` = Vahn only.
    row(ROW_SWORD, SWORD_BONUS, 7, 0x40);
    row(ROW_ARMOR, ARMOR_BONUS, 7, 0x00);
    row(ROW_BLADE, BLADE_BONUS, 1, 0x40);
    scus
}

/// A field world with the driver actor up, two party members carrying the
/// starting loadout, and the two tables a boot installs.
fn world_with_tables(restrictions: bool) -> World {
    let mut w = World::new();
    w.man_load_actor_reset();
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some(),
        "every MAN load spawns the op-0x49 submode driver"
    );

    let scus = synthetic_scus();
    let equip = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus)
        .expect("the fixture parses as an equipment stat table");
    let effects = legaia_asset::item_effect::ItemEffectTable::from_scus(&scus)
        .expect("the fixture parses as an item-effect table");
    w.set_equipment_table(equip_modifier_table_from_disc(&equip));
    w.set_item_effects(effects);
    if restrictions {
        w.install_hub_equip_restrictions(&DiscEquipInfo::from_disc(&equip));
    }

    w.roster = legaia_save::Party::zeroed(2);
    for member in w.roster.members.iter_mut() {
        let mut stats = member.live_stats();
        stats.atk = BASE_ATK;
        stats.udf = BASE_UDF;
        stats.ldf = BASE_LDF;
        member.set_live_stats(stats);
        let mut eq = member.equipment();
        // Engine equip-array order: weapon `0`, body armour `2`.
        eq.slots[0] = ID_SWORD;
        eq.slots[2] = ID_ARMOR;
        member.set_equipment(eq);
    }
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

/// Open the entry list and run the frame loop until it paints.
fn painted(w: &mut World) -> Vec<EquipPanelDraw> {
    w.open_field_submode_screen(slot::DRAW_TICK, Some(ENTRY_LIST_WINDOW));
    for _ in 0..16 {
        w.tick();
        let panel = sub_panel(w);
        if !panel.is_empty() {
            return panel;
        }
    }
    Vec::new()
}

/// Same, with the candidate ladder armed: the class word and the cursor the
/// mode ladder reads have to survive the open, which resets the counter.
fn painted_with_candidate(w: &mut World, mode: u32, cursor: i32) -> Vec<EquipPanelDraw> {
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

fn arrows(panel: &[EquipPanelDraw]) -> Vec<i32> {
    panel
        .iter()
        .filter_map(|d| match d {
            EquipPanelDraw::Arrow { glyph, .. } => Some(*glyph),
            _ => None,
        })
        .collect()
}

/// The ink each drawn value carries. Retail stores the arrow's ink *after* the
/// arrow's own `jal`, so a raised / lowered arrow colours the **candidate
/// column beside it**, never the arrow.
fn value_inks(panel: &[EquipPanelDraw]) -> Vec<i32> {
    panel
        .iter()
        .filter_map(|d| match d {
            EquipPanelDraw::Value { ink, .. } => Some(*ink),
            _ => None,
        })
        .collect()
}

/// Every arrow draws under the ink the label left, not under the ink it picks.
fn assert_arrows_keep_the_label_ink(panel: &[EquipPanelDraw]) {
    use legaia_engine_vm::world_map_overlay::EQUIP_INK_NORMAL;
    for d in panel {
        if let EquipPanelDraw::Arrow { ink, .. } = d {
            assert_eq!(*ink, EQUIP_INK_NORMAL, "the arrow keeps the label's ink");
        }
    }
}

// --- the no-candidate arm, which is what a live frame takes -----------------

#[test]
fn the_rows_print_the_characters_own_stats_plus_the_equipped_bonuses() {
    let mut w = world_with_tables(false);
    let panel = painted(&mut w);
    assert!(!panel.is_empty(), "the frame loop never painted the panel");

    // Three rows: ATK / UDF / LDF, each the record's own base stat plus the
    // sum of the five equip slots' `+1` / `+2` / `+3` bytes.
    assert_eq!(
        values(&panel),
        vec![
            i32::from(BASE_ATK) + i32::from(SWORD_BONUS[1]) + i32::from(ARMOR_BONUS[1]),
            i32::from(BASE_UDF) + i32::from(SWORD_BONUS[2]) + i32::from(ARMOR_BONUS[2]),
            i32::from(BASE_LDF) + i32::from(SWORD_BONUS[3]) + i32::from(ARMOR_BONUS[3]),
        ],
        "each row is base + aggregate, not a zero loadout"
    );
    assert!(arrows(&panel).is_empty(), "no candidate, no arrows");
}

#[test]
fn each_entry_gets_its_own_characters_numbers() {
    let mut w = world_with_tables(false);
    // Give the second member a different loadout: armour only.
    let mut eq = w.roster.members[1].equipment();
    eq.slots[0] = 0;
    w.roster.members[1].set_equipment(eq);
    w.active_party = vec![0, 1];

    let panel = painted(&mut w);
    let v = values(&panel);
    assert_eq!(v.len(), 6, "two entries, three rows each");
    assert_eq!(
        v[0] - v[3],
        i32::from(SWORD_BONUS[1]),
        "only the first entry carries the weapon"
    );
    assert_eq!(v[1], v[4], "both carry the same armour");
}

#[test]
fn an_empty_slot_adds_nothing_without_the_port_special_casing_id_zero() {
    let mut w = world_with_tables(false);
    for member in w.roster.members.iter_mut() {
        let mut eq = member.equipment();
        eq.slots = [0; 8];
        member.set_equipment(eq);
    }
    let panel = painted(&mut w);
    assert_eq!(
        values(&panel),
        vec![
            i32::from(BASE_ATK),
            i32::from(BASE_UDF),
            i32::from(BASE_LDF)
        ],
        "id 0 resolves through the tables like any other id and lands on a zero row"
    );
}

// --- the candidate ladder ---------------------------------------------------

#[test]
fn a_kind_one_candidate_the_character_can_equip_draws_the_comparison_columns() {
    let mut w = world_with_tables(true);
    // Mode `0x3000` takes the cursor as the item id itself.
    let panel = painted_with_candidate(&mut w, EQUIP_MODE_DIRECT, i32::from(ID_VAHN_BLADE));
    assert!(!panel.is_empty());

    let v = values(&panel);
    assert_eq!(v.len(), 6, "three rows, current + candidate each");
    let current_atk = i32::from(BASE_ATK) + i32::from(SWORD_BONUS[1]) + i32::from(ARMOR_BONUS[1]);
    assert_eq!(v[0], current_atk);
    // The trial-equip displaces the *weapon*, not the armour: the sword's
    // bonus leaves and the blade's arrives.
    assert_eq!(
        v[1],
        current_atk - i32::from(SWORD_BONUS[1]) + i32::from(BLADE_BONUS[1])
    );
    assert_eq!(
        arrows(&panel),
        vec![EQUIP_GLYPH_HIGHER],
        "one arrow, on the one row that moved, and it raises"
    );
    assert_arrows_keep_the_label_ink(&panel);
    assert_eq!(
        value_inks(&panel)[1],
        EQUIP_INK_HIGHER,
        "the raised ink lands on the candidate column beside the arrow"
    );
}

#[test]
fn a_weaker_candidate_lowers_the_row() {
    let mut w = world_with_tables(true);
    // Swap the loadout to the strong blade so the plain sword is a downgrade.
    let mut eq = w.roster.members[0].equipment();
    eq.slots[0] = ID_VAHN_BLADE;
    w.roster.members[0].set_equipment(eq);

    let panel = painted_with_candidate(&mut w, EQUIP_MODE_DIRECT, i32::from(ID_SWORD));
    assert_eq!(arrows(&panel), vec![EQUIP_GLYPH_LOWER]);
    assert_arrows_keep_the_label_ink(&panel);
    assert_eq!(
        value_inks(&panel)[1],
        EQUIP_INK_LOWER,
        "the candidate column beside the arrow takes the lowered ink"
    );
}

#[test]
fn a_kind_one_candidate_the_character_cannot_equip_replaces_the_panel() {
    let mut w = world_with_tables(true);
    // Entry code 1 is the second roster slot, whose mask bit the Vahn-only
    // blade does not carry.
    w.active_party = vec![1];
    let panel = painted_with_candidate(&mut w, EQUIP_MODE_DIRECT, i32::from(ID_VAHN_BLADE));

    assert_eq!(panel.len(), 1, "the reject arm draws one line and returns");
    match panel[0] {
        EquipPanelDraw::Label { label_va, ink, .. } => {
            assert_eq!(label_va, EQUIP_REJECT_VA);
            assert_eq!(ink, EQUIP_INK_REJECT);
        }
        other => panic!("expected the reject line, got {other:?}"),
    }
}

#[test]
fn a_kind_two_candidate_draws_the_plain_rows() {
    let mut w = world_with_tables(true);
    let panel = painted_with_candidate(&mut w, EQUIP_MODE_DIRECT, i32::from(ID_POTION));
    assert_eq!(values(&panel).len(), 3, "no comparison column");
    assert!(arrows(&panel).is_empty());
    assert_eq!(
        values(&panel)[0],
        i32::from(BASE_ATK) + i32::from(SWORD_BONUS[1]) + i32::from(ARMOR_BONUS[1])
    );
}

#[test]
fn any_other_kind_draws_nothing_at_all() {
    let mut w = world_with_tables(true);
    // Item id `0` is `kind 0` on the disc as well as here - the kind a cursor
    // parked on an empty bag slot resolves to.
    w.open_field_submode_screen(slot::DRAW_TICK, Some(ENTRY_LIST_WINDOW));
    w.set_hub_equip_mode(EQUIP_MODE_DIRECT);
    w.submode_screen.counter.cursor = 0;
    let mut label_drawn = false;
    for _ in 0..16 {
        w.tick();
        // The entry's own label still draws; the sub-panel contributes nothing.
        if w.submode_screen
            .draws()
            .iter()
            .any(|d| matches!(d, HubDraw::Text { .. }))
        {
            label_drawn = true;
            assert!(
                sub_panel(&w).is_empty(),
                "an unhandled kind draws no sub-panel rows"
            );
            break;
        }
    }
    assert!(label_drawn, "the entry list never ran");
}

#[test]
fn the_blank_mode_compares_against_an_empty_loadout() {
    let mut w = world_with_tables(true);
    // `0x4000` with cursor `1` compares against nothing equipped at all.
    let panel = painted_with_candidate(&mut w, EQUIP_MODE_BLANK, 1);
    let v = values(&panel);
    assert_eq!(v.len(), 6);
    assert_eq!(v[1], i32::from(BASE_ATK), "the candidate column is bare");
    assert_eq!(
        arrows(&panel),
        vec![EQUIP_GLYPH_LOWER; 3],
        "every row drops"
    );
    assert_eq!(
        value_inks(&panel),
        vec![
            EQUIP_INK_NORMAL,
            EQUIP_INK_LOWER,
            EQUIP_INK_NORMAL,
            EQUIP_INK_LOWER,
            EQUIP_INK_NORMAL,
            EQUIP_INK_LOWER
        ],
        "the ink is reasserted per row and only the candidate column takes it"
    );
}

#[test]
fn the_bag_modes_index_the_worlds_own_inventory() {
    let mut w = world_with_tables(true);
    w.inventory.insert(ID_VAHN_BLADE, 1);
    w.inventory.insert(ID_POTION, 3);
    // The bag list is id-ordered, so slot 0 is the blade.
    let panel = painted_with_candidate(&mut w, EQUIP_MODE_INVENTORY[0], 0);
    assert_eq!(values(&panel).len(), 6, "slot 0 named the equippable blade");
    assert_eq!(arrows(&panel).len(), 1);

    let mut w = world_with_tables(true);
    w.inventory.insert(ID_VAHN_BLADE, 1);
    w.inventory.insert(ID_POTION, 3);
    let panel = painted_with_candidate(&mut w, EQUIP_MODE_INVENTORY[1], 1);
    assert_eq!(
        values(&panel).len(),
        3,
        "slot 1 named the consumable: plain rows"
    );
}

// --- what stays disclosed ---------------------------------------------------

#[test]
fn a_mode_without_the_restriction_bytes_stays_on_the_no_candidate_arm() {
    let mut w = world_with_tables(false);
    let panel = painted_with_candidate(&mut w, EQUIP_MODE_DIRECT, i32::from(ID_VAHN_BLADE));
    assert_eq!(
        values(&panel).len(),
        3,
        "without the `+6` mask the panel cannot answer the equip gate, so it \
         takes retail's own arm that never asks rather than rejecting everything"
    );
}

#[test]
fn the_weapon_slot_table_decides_which_slot_a_weapon_candidate_displaces() {
    use legaia_engine_core::field_submode_screen::hub_panel_slots;

    let mut equip = [0u8; 8];
    equip[0] = ID_SWORD; // engine weapon slot
    equip[1] = 0x33; // helmet
    equip[2] = ID_ARMOR; // body armour
    equip[4] = 0x44; // boots

    // Retail's `+0x196` order: body `0`, head `1`, weapon at the character's
    // own halfword, footwear `4`.
    let vahn = hub_panel_slots(&equip, 0);
    assert_eq!(vahn[0], ID_ARMOR);
    assert_eq!(vahn[1], 0x33);
    assert_eq!(vahn[RETAIL_WEAPON_SLOTS[0] as usize], ID_SWORD);
    assert_eq!(vahn[4], 0x44);

    let noa = hub_panel_slots(&equip, 1);
    assert_eq!(noa[RETAIL_WEAPON_SLOTS[1] as usize], ID_SWORD);
    assert_ne!(
        RETAIL_WEAPON_SLOTS[0], RETAIL_WEAPON_SLOTS[1],
        "the table is per character, which is why it is a table"
    );
}
