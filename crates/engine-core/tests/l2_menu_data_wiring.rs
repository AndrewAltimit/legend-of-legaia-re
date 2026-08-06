//! Regression ladder for three pause-menu kernels that ran and produced
//! nothing, each scored at its **output** rather than at its call site.
//!
//! A kernel with no caller and a kernel whose caller feeds it an empty table
//! are indistinguishable in a coverage join and in a call-graph audit: both
//! read "live", and both change nothing a player can see. So every test here
//! asserts a value the player would notice - a bag count, a picked weapon id,
//! a list of destination rows - and each keeps a contrasting baseline so it
//! cannot pass vacuously.
//!
//! | # | kernel | what used to happen | what is measured now |
//! |---|---|---|---|
//! | 1 | the pause-menu use commit (`FUN_80042310`) | nothing decremented the bag | the bag count drops by exactly one per use |
//! | 2 | Door of Wind, submenu `0xC` (`FUN_801D8B90`) | the route was filtered out before it could open | the destination list opens, a pick consumes `0x89` and stages the warp |
//! | 3 | the weapon-category check (`FUN_801DD0C0`) | no caller installed its table, so every score was 0 | Best Equipment picks the favoured weapon over the stronger one |
//!
//! Disc-free: every table these drive is either synthesised at the byte
//! level (the menu-overlay category table) or built as typed records.

use legaia_engine_core::battle_stats::{EquipmentTable, ItemModifier};
use legaia_engine_core::field_menu::FieldMenuRow;
use legaia_engine_core::field_menu_dispatch::{
    FieldMenuSubsession, apply_inventory_outcome, apply_pause_items_outcome,
    build_pause_items_session, warp_destinations,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::items::ItemCatalog;
use legaia_engine_core::menu_item_category::{CATEGORY_TABLE_OFFSET, CategoryEntry};
use legaia_engine_core::options::OptionsState;
use legaia_engine_core::pause_screens::{
    DOOR_OF_LIGHT_ITEM_ID, DOOR_OF_WIND_ITEM_ID, INCENSE_ITEM_ID, MENU_EXIT_CODE_FIELD_ESCAPE,
    MENU_EXIT_CODE_WORLD_MAP_WARP, PauseItemsSession, StagedWarp,
};
use legaia_engine_core::save_select::SaveRack;
use legaia_engine_core::spells::SpellCatalog;
use legaia_engine_core::tactical_arts_editor::ChainLibrary;
use legaia_engine_core::world::World;

/// Healing Leaf - a plain single-target item the ordinary use flow accepts.
const HEALING_LEAF: u8 = 0x77;

fn press(s: &mut PauseItemsSession, b: PadButton) {
    s.input_pad_edge(b.mask());
}

/// A field world with a three-member roster (each wounded, so a heal is
/// admissible) and the vanilla item catalog.
fn field_world() -> World {
    let mut world = World::new();
    world.roster = legaia_save::Party::zeroed(3);
    for member in &mut world.roster.members {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 50;
        hms.hp_max = 100;
        hms.mp_cur = 10;
        hms.mp_max = 30;
        member.set_hp_mp_sp(hms);
    }
    world.party_leader_slot = Some(0);
    world.set_item_catalog(ItemCatalog::vanilla());
    world
}

/// Walk the hand from the command window onto the Use-list row holding `id`
/// and confirm it.
fn use_list_confirm(s: &mut PauseItemsSession, id: u8) {
    press(s, PadButton::Cross); // command row 0 (Use) -> the list
    let row = s
        .rows
        .iter()
        .position(|r| r.id == id)
        .unwrap_or_else(|| panic!("no bag row for {id:#04X}"));
    for _ in 0..row {
        press(s, PadButton::Down);
    }
    press(s, PadButton::Cross);
}

// ---------------------------------------------------------------------------
// 1 - the use commit reaches the bag
// ---------------------------------------------------------------------------

/// An ordinary pause-menu use takes exactly one copy out of the bag.
///
/// The retail commit is `FUN_80042310(id, 1)`, one decrement per *use* and
/// not per healed ally - so the all-party arm is checked with the same
/// assertion rather than a different one.
#[test]
fn an_ordinary_use_takes_exactly_one_copy_out_of_the_bag() {
    let mut world = field_world();
    world.inventory.insert(HEALING_LEAF, 3);

    let mut s = build_pause_items_session(&world);
    use_list_confirm(&mut s, HEALING_LEAF);
    assert!(s.target_select(), "the confirm opens the target panel");
    press(&mut s, PadButton::Cross); // confirm the first party row
    assert!(s.is_done());

    apply_inventory_outcome(&s.inner, &mut world);
    assert_eq!(
        world.inventory.get(&HEALING_LEAF),
        Some(&2),
        "one copy leaves the bag per completed use"
    );

    // The contrast that keeps this non-vacuous: a session the player backs
    // out of consumes nothing.
    let mut s = build_pause_items_session(&world);
    press(&mut s, PadButton::Circle);
    assert!(s.is_done());
    apply_inventory_outcome(&s.inner, &mut world);
    assert_eq!(world.inventory.get(&HEALING_LEAF), Some(&2));
}

/// The last copy of an item leaves the bag entry entirely, rather than
/// sitting there at count 0 and still listing.
#[test]
fn using_the_last_copy_removes_the_bag_entry() {
    let mut world = field_world();
    world.inventory.insert(HEALING_LEAF, 1);
    let mut s = build_pause_items_session(&world);
    use_list_confirm(&mut s, HEALING_LEAF);
    press(&mut s, PadButton::Cross);
    apply_inventory_outcome(&s.inner, &mut world);
    assert_eq!(world.inventory.get(&HEALING_LEAF), None);
    assert!(
        build_pause_items_session(&world).bag_empty(),
        "an emptied bag greys the command window"
    );
}

/// Each of the three special Use routes commits its own fixed id, and only
/// on a confirm - a cancel leaves the stack alone.
///
/// Door of Light `0x88` (`FUN_801D8A58`) and Incense `0x8A`
/// (`FUN_801D8D94`) both hand `FUN_80042310(id, 1)` on their Yes row.
#[test]
fn each_special_route_commits_its_own_fixed_id() {
    for (id, exit) in [
        (DOOR_OF_LIGHT_ITEM_ID, Some(MENU_EXIT_CODE_FIELD_ESCAPE)),
        (INCENSE_ITEM_ID, None),
    ] {
        let mut world = field_world();
        world.inventory.insert(id, 2);

        // Cancel first: the baseline that proves the confirm is doing it.
        let mut s = build_pause_items_session(&world);
        use_list_confirm(&mut s, id);
        press(&mut s, PadButton::Circle);
        apply_pause_items_outcome(&s, &mut world);
        assert_eq!(
            world.inventory.get(&id),
            Some(&2),
            "{id:#04X}: a cancelled route consumed a copy"
        );

        let mut s = build_pause_items_session(&world);
        use_list_confirm(&mut s, id);
        press(&mut s, PadButton::Cross); // Yes (both routes seed on Yes)
        assert_eq!(apply_pause_items_outcome(&s, &mut world), exit);
        assert_eq!(
            world.inventory.get(&id),
            Some(&1),
            "{id:#04X}: the confirm did not reach the bag"
        );
    }
}

/// A committed Door of Light also raises the field-escape handoff the outer
/// menu SM keys on (`_DAT_8007B43C = 4`); an Incense does not.
#[test]
fn only_the_escape_route_raises_the_field_escape_handoff() {
    let mut world = field_world();
    world.inventory.insert(INCENSE_ITEM_ID, 1);
    let mut s = build_pause_items_session(&world);
    use_list_confirm(&mut s, INCENSE_ITEM_ID);
    press(&mut s, PadButton::Cross);
    apply_pause_items_outcome(&s, &mut world);
    assert!(!world.pending_menu_escape);

    world.inventory.insert(DOOR_OF_LIGHT_ITEM_ID, 1);
    let mut s = build_pause_items_session(&world);
    use_list_confirm(&mut s, DOOR_OF_LIGHT_ITEM_ID);
    press(&mut s, PadButton::Cross);
    apply_pause_items_outcome(&s, &mut world);
    assert!(world.pending_menu_escape);
}

// ---------------------------------------------------------------------------
// 2 - Door of Wind, submenu 0xC
// ---------------------------------------------------------------------------

fn placement(
    index: u32,
    name_idx: u8,
    discovery_flag: u8,
    scene_id: u16,
) -> legaia_asset::worldmap_menu::PlacementRecord {
    legaia_asset::worldmap_menu::PlacementRecord {
        index,
        name_idx,
        discovery_flag,
        scene_id,
        menu_x: 0x10 + index as u8,
        menu_y: 0x20 + index as u8,
    }
}

/// The landmark walk is `FUN_80030628` case `0x19`: the discovery flag
/// gates each record, and a record whose `name_idx` repeats the **last
/// accepted** row's is dropped.
///
/// The dedupe compares against the last *accepted* name, not the last seen
/// one, which is the whole point of the `move s0,s5` sitting inside the
/// accept arm at `0x800318d0`: a locked duplicate must not shadow the
/// unlocked record that follows it.
#[test]
fn the_landmark_walk_gates_on_the_flag_and_dedupes_on_the_last_accepted_name() {
    let mut world = field_world();
    world.worldmap_menu = Some(legaia_asset::worldmap_menu::WorldmapMenu {
        names: vec!["Rim Elm".into(), "Drake Castle".into(), "Sol".into()],
        placements: vec![
            placement(0, 0, 0x10, 0x0055), // Rim Elm, locked below
            placement(1, 0, 0x11, 0x0056), // Rim Elm again, a second entrance
            placement(2, 1, 0x12, 0x0162), // Drake Castle
            placement(3, 2, 0x13, 0x0201), // Sol
        ],
    });

    // Nothing discovered: the list is empty, not a full atlas.
    assert!(warp_destinations(&world).is_empty());

    // Discovering only the *second* Rim Elm record must still list it - the
    // dedupe never saw an accepted "Rim Elm" to compare against.
    world.system_flag_set(0x11 + 0x20);
    let d = warp_destinations(&world);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].name, "Rim Elm");
    assert_eq!(d[0].record_index, 1, "the record ordinal, not the row one");
    assert_eq!(d[0].scene_id, 0x0056);

    // Now discover the first one too: the pair collapses to a single row.
    world.system_flag_set(0x10 + 0x20);
    let d = warp_destinations(&world);
    assert_eq!(
        d.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
        vec!["Rim Elm"]
    );
    assert_eq!(d[0].record_index, 0, "the earlier record wins the dedupe");

    world.system_flag_set(0x12 + 0x20);
    world.system_flag_set(0x13 + 0x20);
    assert_eq!(
        warp_destinations(&world)
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Rim Elm", "Drake Castle", "Sol"]
    );
}

/// Confirming a Door of Wind opens the destination list and a pick commits:
/// one `0x89` leaves the bag, the menu takes exit code 5, and the picked
/// record's `+2`/`+4`/`+5` triple is staged.
#[test]
fn a_door_of_wind_pick_consumes_the_item_and_stages_the_destination() {
    let mut world = field_world();
    world.inventory.insert(DOOR_OF_WIND_ITEM_ID, 2);
    world.worldmap_menu = Some(legaia_asset::worldmap_menu::WorldmapMenu {
        names: vec!["Rim Elm".into(), "Drake Castle".into()],
        placements: vec![placement(0, 0, 0x10, 0x0055), placement(1, 1, 0x11, 0x0162)],
    });
    world.system_flag_set(0x10 + 0x20);
    world.system_flag_set(0x11 + 0x20);

    let mut s = build_pause_items_session(&world);
    use_list_confirm(&mut s, DOOR_OF_WIND_ITEM_ID);
    press(&mut s, PadButton::Down); // -> Drake Castle
    press(&mut s, PadButton::Cross);

    assert_eq!(
        apply_pause_items_outcome(&s, &mut world),
        Some(MENU_EXIT_CODE_WORLD_MAP_WARP)
    );
    assert_eq!(world.inventory.get(&DOOR_OF_WIND_ITEM_ID), Some(&1));
    assert_eq!(
        world.pending_menu_warp,
        Some(StagedWarp {
            scene_id: 0x0162,
            menu_x: 0x11,
            menu_y: 0x21,
        })
    );
}

/// Backing out of the destination list returns to the Use list without
/// consuming - the contrast that keeps the test above honest.
#[test]
fn backing_out_of_the_destination_list_consumes_nothing() {
    let mut world = field_world();
    world.inventory.insert(DOOR_OF_WIND_ITEM_ID, 1);
    world.worldmap_menu = Some(legaia_asset::worldmap_menu::WorldmapMenu {
        names: vec!["Rim Elm".into()],
        placements: vec![placement(0, 0, 0x10, 0x0055)],
    });
    world.system_flag_set(0x10 + 0x20);

    let mut s = build_pause_items_session(&world);
    use_list_confirm(&mut s, DOOR_OF_WIND_ITEM_ID);
    press(&mut s, PadButton::Circle);
    assert!(!s.is_done(), "a cancel returns to the Use list");
    assert_eq!(apply_pause_items_outcome(&s, &mut world), None);
    assert_eq!(world.inventory.get(&DOOR_OF_WIND_ITEM_ID), Some(&1));
    assert_eq!(world.pending_menu_warp, None);
}

/// Without the executable the landmark table is absent, and the route then
/// opens an **empty** list rather than an invented one. A pick is
/// impossible, so nothing is consumed.
#[test]
fn no_landmark_table_means_an_empty_list_not_an_invented_one() {
    let mut world = field_world();
    world.inventory.insert(DOOR_OF_WIND_ITEM_ID, 1);
    assert!(world.worldmap_menu.is_none());

    let mut s = build_pause_items_session(&world);
    use_list_confirm(&mut s, DOOR_OF_WIND_ITEM_ID);
    assert!(s.warp_destinations().is_empty());
    press(&mut s, PadButton::Cross);
    assert_eq!(apply_pause_items_outcome(&s, &mut world), None);
    assert_eq!(world.inventory.get(&DOOR_OF_WIND_ITEM_ID), Some(&1));
}

// ---------------------------------------------------------------------------
// 3 - the weapon-category check
// ---------------------------------------------------------------------------

/// A menu-overlay-shaped image carrying `entries` at the category table's
/// file offset, zero-terminated the way retail's is.
fn overlay_with_category(entries: &[CategoryEntry]) -> Vec<u8> {
    let mut overlay = vec![0u8; CATEGORY_TABLE_OFFSET + entries.len() * 2 + 2];
    let mut at = CATEGORY_TABLE_OFFSET;
    for e in entries {
        overlay[at] = e.item_id;
        overlay[at + 1] = e.mask;
        at += 2;
    }
    overlay
}

/// Two weapons in the bag: a weak favoured one and a strong unfavoured one.
/// Ids stay below `0x20` so the legacy slot derivation (`id >> 5`) puts both
/// in the weapon slot.
const FAVOURED_WEAPON: u8 = 0x05;
const STRONG_WEAPON: u8 = 0x06;

fn weapon_world() -> (World, EquipmentTable) {
    let mut world = field_world();
    world.inventory.insert(FAVOURED_WEAPON, 1);
    world.inventory.insert(STRONG_WEAPON, 1);
    let mut table = EquipmentTable::new();
    table.set(
        FAVOURED_WEAPON,
        ItemModifier {
            atk: 10,
            ..Default::default()
        },
    );
    table.set(
        STRONG_WEAPON,
        ItemModifier {
            atk: 200,
            ..Default::default()
        },
    );
    (world, table)
}

fn best_weapon_for(world: &mut World, equipment: &EquipmentTable, leader: u8) -> u8 {
    world.party_leader_slot = Some(leader);
    let sub = FieldMenuSubsession::build(
        FieldMenuRow::Equip,
        world,
        &OptionsState::default(),
        &SaveRack::Blocks(Vec::new()),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        equipment,
    );
    let FieldMenuSubsession::Equip { session, char_slot } = sub else {
        panic!("Equip row");
    };
    assert_eq!(char_slot, leader);
    session.best_equipment_now()[0]
}

/// The whole point of the category table: a favoured weapon out-scores a
/// stronger unfavoured one, because `FUN_801DD0C0` adds a flat `1000` no
/// attack byte can reach.
///
/// The baseline is the same world with the table absent - which is retail's
/// own empty-table arm, and the state every build shipped before the parse
/// was wired into `World::install_menu_overlay_tables`.
#[test]
fn the_category_table_makes_best_equipment_prefer_the_favoured_weapon() {
    let (mut world, equipment) = weapon_world();

    // No table: raw ATK decides, so the strong weapon wins for everyone.
    assert!(world.menu_item_category.is_empty());
    for leader in 0..3 {
        assert_eq!(
            best_weapon_for(&mut world, &equipment, leader),
            STRONG_WEAPON,
            "leader {leader} without a table"
        );
    }

    // Install the table with the weak weapon favoured for Noa only (bit
    // `char + group*4`; the chooser calls with group 1, so Noa is bit 5).
    world.install_menu_overlay_tables(&overlay_with_category(&[CategoryEntry {
        item_id: FAVOURED_WEAPON,
        mask: 0x20,
    }]));
    assert_eq!(world.menu_item_category.len(), 1);

    assert_eq!(
        best_weapon_for(&mut world, &equipment, 1),
        FAVOURED_WEAPON,
        "Noa's favoured weapon must beat the stronger one"
    );
    // And the favour is per-character, which is only true because the
    // session carries the real party slot: Vahn and Gala still take ATK.
    assert_eq!(best_weapon_for(&mut world, &equipment, 0), STRONG_WEAPON);
    assert_eq!(best_weapon_for(&mut world, &equipment, 2), STRONG_WEAPON);
}

/// A mask that favours everybody (`0x77`, the class-weapon shape) reverses
/// the pick for all three - the sweep that proves the bit index, not just
/// that *some* score fired.
#[test]
fn an_all_character_mask_favours_every_party_slot() {
    let (mut world, equipment) = weapon_world();
    world.install_menu_overlay_tables(&overlay_with_category(&[CategoryEntry {
        item_id: FAVOURED_WEAPON,
        mask: 0x77,
    }]));
    for leader in 0..3 {
        assert_eq!(
            best_weapon_for(&mut world, &equipment, leader),
            FAVOURED_WEAPON,
            "leader {leader} with an all-character mask"
        );
    }
}
