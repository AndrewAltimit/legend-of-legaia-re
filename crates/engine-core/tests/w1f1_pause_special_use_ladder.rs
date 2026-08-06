//! Pad-driven ladder for the pause menu's three **special Use** routes -
//! retail submenus `0xB` / `0xC` / `0xD` behind the Use-list phase-2 dispatch
//! (`FUN_801D7E50`).
//!
//! The route is reached exactly as a host reaches it: build the real Items
//! screen off a `World` whose bag holds the item, then feed edge-triggered pad
//! words to `PauseItemsSession::input_pad_edge` - command window -> Use -> the
//! list -> a Cross on the row. Nothing is constructed by hand, because the
//! question this ladder answers is whether the *pad path* reaches the routes,
//! not whether the sessions compile.
//!
//! ## The seeding helper is shared on purpose
//!
//! No `debug_` helper grants `0x88` / `0x89` / `0x8A`, so the bag is seeded
//! directly ([`world_holding`]). Lane W1-F2 needs the same world for the
//! confirm-window *draw* side (`801d1dac` / `801d1f10` in
//! `engine-ui/src/ui_menu/system_menus.rs`); this file is the reusable copy.
//!
//! ## Door of Wind, and what is still owed on it
//!
//! Door of Wind (`0x89`, `FUN_801D8B90`) **is** reachable now:
//! `special_use_route_for_item` routes all three classes, the session opens in
//! `SpecialUsePhase::PickDestination`, and the landmark rows come off the disc
//! placement table filtered by the live discovery flags
//! (`field_menu_dispatch::warp_destinations`). A pick consumes one `0x89` and
//! hands the menu exit code 5 with the destination triple staged.
//!
//! What it does not do is *enter* the world map at that landmark: the port has
//! no drain for `World::pending_menu_warp`. The last test pins the reachable
//! half and the staged-but-undrained half separately, so neither reads as the
//! other.

use legaia_engine_core::field_menu_dispatch::{
    apply_pause_items_outcome, build_pause_items_session,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::items::ItemCatalog;
use legaia_engine_core::pause_screens::{
    DOOR_OF_LIGHT_ITEM_ID, DOOR_OF_WIND_ITEM_ID, INCENSE_ITEM_ID, MENU_EXIT_CODE_FIELD_ESCAPE,
    MENU_EXIT_CODE_WORLD_MAP_WARP, PauseItemsFocus, PauseItemsSession, SpecialUseOutcome,
    SpecialUsePhase, UseRoute, items_screen_model, special_use_route_for_item,
    use_route_for_effect,
};
use legaia_engine_core::world::World;

/// A field world whose bag holds `ids` (one each) plus a plain Healing Leaf,
/// with a three-member roster the item-use flow can target.
///
/// Reusable: lane W1-F2's confirm-window draw tests need exactly this state.
fn world_holding(ids: &[u8]) -> World {
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
    // 0x77 sorts below every special id, so the special rows are never row 0
    // and the ladder has to actually walk the hand down to them.
    world.inventory.insert(0x77, 3);
    for &id in ids {
        world.inventory.insert(id, 1);
    }
    world.party_leader_slot = Some(0);
    world.set_item_catalog(ItemCatalog::vanilla());
    world
}

fn press(s: &mut PauseItemsSession, b: PadButton) {
    s.input_pad_edge(b.mask());
}

/// Walk the hand from the command window onto the Use-list row holding `id`.
///
/// The hand position is confirmed through the host-facing view model rather
/// than a private cursor: retail restages the info window from the hovered
/// slot every list frame, so the staged item name *is* the hand's readout.
fn open_use_list_on(s: &mut PauseItemsSession, id: u8) {
    assert_eq!(s.focus, PauseItemsFocus::Command, "screens open on Command");
    // Command row 0 is Use; Cross drops into the list (retail scans the bag
    // first and buzzes on an empty one, so a stocked bag is a precondition).
    press(s, PadButton::Cross);
    assert_eq!(s.focus, PauseItemsFocus::List);
    // Rows are id-sorted, one per held id, so the row index is the number of
    // Down steps the hand needs.
    let want = s
        .rows
        .iter()
        .position(|r| r.id == id)
        .unwrap_or_else(|| panic!("no bag row for item {id:#04X}"));
    let name = s.rows[want].name.clone();
    for _ in 0..want {
        press(s, PadButton::Down);
    }
    let staged = items_screen_model(s).info.map(|i| i.name);
    assert_eq!(
        staged.as_deref(),
        Some(name.as_str()),
        "the info window stages {name:?}, so the hand is not on row {want}"
    );
}

#[test]
fn a_use_confirm_on_door_of_light_closes_the_menu_with_the_escape_exit_code() {
    let world = world_holding(&[DOOR_OF_LIGHT_ITEM_ID]);
    let mut s = build_pause_items_session(&world);
    open_use_list_on(&mut s, DOOR_OF_LIGHT_ITEM_ID);

    // Cross on the row opens the route's OWN confirm window rather than the
    // target panel - the phase-2 dispatch keys on the effect class before any
    // target is picked.
    press(&mut s, PadButton::Cross);
    assert_eq!(s.focus, PauseItemsFocus::SpecialRoute);
    let sp = s.special_use().expect("a special route is live");
    assert_eq!(sp.route, UseRoute::DoorOfLight);
    assert_eq!(
        sp.phase,
        SpecialUsePhase::Confirm,
        "Door of Light raises a confirm, not a destination list"
    );
    // Unlike the Throw Out confirm (which seeds on "No"), these two seed on
    // Yes - `801d8ab4` / `801d8df0` zero `DAT_801E46D0`.
    assert_eq!(sp.cursor, 0, "the special confirm seeds on Yes");

    press(&mut s, PadButton::Cross);
    let sp = s
        .special_use()
        .expect("the finished route is still readable");
    assert_eq!(
        sp.phase,
        SpecialUsePhase::Done(SpecialUseOutcome::FieldEscape)
    );
    assert_eq!(sp.exit_code(), Some(MENU_EXIT_CODE_FIELD_ESCAPE));
    assert_eq!(sp.consumed_item_id(), Some(DOOR_OF_LIGHT_ITEM_ID));
    assert!(
        s.is_done(),
        "the escape route closes the whole menu, not just the window"
    );

    // The host is meant to drain the finished route once it has applied it.
    let taken = s.take_special_use().expect("takeable once");
    assert_eq!(taken.exit_code(), Some(MENU_EXIT_CODE_FIELD_ESCAPE));
    assert!(s.special_use().is_none(), "draining leaves nothing behind");
}

#[test]
fn a_use_confirm_on_incense_applies_in_place_and_returns_to_the_use_list() {
    let world = world_holding(&[INCENSE_ITEM_ID]);
    let mut s = build_pause_items_session(&world);
    open_use_list_on(&mut s, INCENSE_ITEM_ID);

    press(&mut s, PadButton::Cross);
    assert_eq!(s.focus, PauseItemsFocus::SpecialRoute);
    assert_eq!(s.special_use().map(|sp| sp.route), Some(UseRoute::Incense));

    press(&mut s, PadButton::Cross);
    let sp = s.special_use().expect("route still readable");
    assert_eq!(
        sp.phase,
        SpecialUsePhase::Done(SpecialUseOutcome::EncounterSuppress)
    );
    // Encounter suppression is an in-place effect: it consumes the item but
    // hands the outer menu SM no exit code, so the menu stays open on the
    // Use list.
    assert_eq!(sp.exit_code(), None, "Incense does not exit the menu");
    assert_eq!(sp.consumed_item_id(), Some(INCENSE_ITEM_ID));
    assert_eq!(s.focus, PauseItemsFocus::List);
    assert!(!s.is_done(), "the menu stays open after an Incense");
}

#[test]
fn backing_out_of_a_special_confirm_consumes_nothing() {
    for (id, back_out) in [
        // Circle is the explicit cancel.
        (DOOR_OF_LIGHT_ITEM_ID, PadButton::Circle),
        (INCENSE_ITEM_ID, PadButton::Circle),
    ] {
        let world = world_holding(&[id]);
        let mut s = build_pause_items_session(&world);
        open_use_list_on(&mut s, id);
        press(&mut s, PadButton::Cross);
        press(&mut s, back_out);
        let sp = s.special_use().expect("route readable");
        assert_eq!(
            sp.phase,
            SpecialUsePhase::Done(SpecialUseOutcome::Cancelled)
        );
        assert_eq!(sp.consumed_item_id(), None, "a cancel consumes nothing");
        assert_eq!(sp.exit_code(), None);
        assert_eq!(s.focus, PauseItemsFocus::List, "back on the Use list");
        assert!(!s.is_done());
    }

    // Cross on "No" is the other way out, and it must not be confused with
    // Cross on "Yes": moving the cursor once flips the two rows.
    for id in [DOOR_OF_LIGHT_ITEM_ID, INCENSE_ITEM_ID] {
        let world = world_holding(&[id]);
        let mut s = build_pause_items_session(&world);
        open_use_list_on(&mut s, id);
        press(&mut s, PadButton::Cross);
        press(&mut s, PadButton::Down);
        assert_eq!(s.special_use().map(|sp| sp.cursor), Some(1), "moved to No");
        press(&mut s, PadButton::Cross);
        let sp = s.special_use().expect("route readable");
        assert_eq!(
            sp.phase,
            SpecialUsePhase::Done(SpecialUseOutcome::Cancelled)
        );
        assert_eq!(sp.consumed_item_id(), None);
    }
}

#[test]
fn an_ordinary_item_never_opens_a_special_route() {
    // The dispatch is keyed on the effect class, so a Healing Leaf confirm
    // must fall through to the target panel and construct no session.
    let world = world_holding(&[]);
    let mut s = build_pause_items_session(&world);
    open_use_list_on(&mut s, 0x77);
    press(&mut s, PadButton::Cross);
    assert!(
        s.special_use().is_none(),
        "an ordinary item opened a special route"
    );
    assert_ne!(s.focus, PauseItemsFocus::SpecialRoute);
}

/// Door of Wind, end to end off a real `World`: the route opens the
/// destination list, the rows are the flag-gated placement records, a pick
/// consumes one `0x89` and stages the world-map warp.
///
/// The undrained half is pinned separately below, so "staged" is never read
/// as "the player is on the world map".
#[test]
fn door_of_wind_opens_the_destination_list_and_stages_the_warp() {
    // The class dispatch names it, and so does the route wrapper - the
    // filter that used to drop it here is what made the whole submenu dead.
    assert_eq!(use_route_for_effect(0x81, 0), UseRoute::DoorOfWind);
    assert_eq!(
        special_use_route_for_item(DOOR_OF_WIND_ITEM_ID),
        Some(UseRoute::DoorOfWind)
    );

    let mut world = world_holding(&[DOOR_OF_WIND_ITEM_ID]);
    // Two landmarks the walk will accept, one it will not: the placement
    // table is disc data, so the flags are what the ladder controls.
    world.worldmap_menu = Some(legaia_asset::worldmap_menu::WorldmapMenu {
        names: vec!["Rim Elm".into(), "Drake Castle".into(), "Sol".into()],
        placements: vec![
            placement(0, 0, 0x10, 0x0055),
            placement(1, 1, 0x11, 0x0162),
            placement(2, 2, 0x12, 0x0201),
        ],
    });
    // Discovery flags live at `record[1] + 0x20` (FUN_8003CE64).
    world.system_flag_set(0x10 + 0x20);
    world.system_flag_set(0x12 + 0x20);

    let mut s = build_pause_items_session(&world);
    assert_eq!(
        s.warp_destinations()
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Rim Elm", "Sol"],
        "an undiscovered landmark must not appear in the list"
    );

    open_use_list_on(&mut s, DOOR_OF_WIND_ITEM_ID);
    press(&mut s, PadButton::Cross);
    assert_eq!(s.focus, PauseItemsFocus::SpecialRoute);
    let sp = s.special_use().expect("the destination list is live");
    assert_eq!(sp.route, UseRoute::DoorOfWind);
    assert_eq!(sp.phase, SpecialUsePhase::PickDestination);

    // The host-facing model is the readout: the list window now shows the
    // landmarks, with no Yes/No prompt and no info window.
    let m = items_screen_model(&s);
    assert_eq!(
        m.page_rows
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        vec!["Rim Elm", "Sol"]
    );
    assert!(m.special_confirm.is_none());
    assert!(m.info.is_none());

    // Pick the second row.
    press(&mut s, PadButton::Down);
    press(&mut s, PadButton::Cross);
    assert!(s.is_done(), "a committed warp closes the whole menu");
    assert_eq!(
        s.special_use().map(|sp| sp.phase.clone()),
        Some(SpecialUsePhase::Done(SpecialUseOutcome::Warp {
            landmark: 1
        }))
    );
    assert_eq!(s.exit_code(), Some(MENU_EXIT_CODE_WORLD_MAP_WARP));
    assert_ne!(MENU_EXIT_CODE_WORLD_MAP_WARP, MENU_EXIT_CODE_FIELD_ESCAPE);

    // The bag is the measurable output: exactly one Door of Wind leaves it.
    apply_pause_items_outcome(&s, &mut world);
    assert_eq!(world.inventory.get(&DOOR_OF_WIND_ITEM_ID), None);
    assert_eq!(
        world.pending_menu_warp,
        Some(legaia_engine_core::pause_screens::StagedWarp {
            scene_id: 0x0201,
            menu_x: 0x40,
            menu_y: 0x50,
        })
    );
}

/// The residual, stated as a test so it cannot rot into a silent claim:
/// nothing in the engine drains `World::pending_menu_warp`, so a committed
/// Door of Wind stages its destination and the player stays put.
#[test]
fn the_staged_warp_has_no_drain_yet() {
    let mut world = world_holding(&[]);
    world.pending_menu_warp = Some(legaia_engine_core::pause_screens::StagedWarp {
        scene_id: 0x0055,
        menu_x: 1,
        menu_y: 2,
    });
    // A scene transition is the channel a warp would have to use, and the
    // menu warp does not feed it.
    assert_eq!(world.pending_scene_transition, None);
    assert!(world.pending_menu_warp.is_some());
}

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
        menu_x: 0x40,
        menu_y: 0x50,
    }
}
