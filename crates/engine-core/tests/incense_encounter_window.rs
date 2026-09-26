//! Incense end to end: the pause Items confirm arms the Incense window, the
//! region encounter roll skips while it is open, and the walk tick drains it.
//!
//! Retail's chain, each link read off the disassembly:
//!
//! - the Incense confirm (`FUN_801D8D94`, menu overlay) consumes `0x8A` and
//!   runs the SCUS item applier `FUN_800402F4`, whose class-`0x82` arm
//!   (`0x800421A0`) is one `jal 0x80046870` - `_DAT_8007B600 += 0x40`, capped
//!   at `0x100`;
//! - the field walk tick `FUN_801D0B90` decrements the word once per running
//!   tick (`0x801D0CD4..0x801D0CE8`);
//! - the region encounter roll `FUN_801D9E1C` skips the whole roll while it is
//!   non-zero (`0x801DA174`), after the battle-setup half and before the rate
//!   scale, so the step counter does not drain either;
//! - the Use-list build greys the row while the word is `>= 0xE0`
//!   (`FUN_8003043C` -> validator arm `0x82` -> `FUN_80046898`).
//!
//! Every assertion is paired with a contrast a do-nothing port fails.

use legaia_engine_core::field_menu_dispatch::{
    apply_pause_items_outcome, build_pause_items_session,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::items::ItemCatalog;
use legaia_engine_core::pause_screens::{INCENSE_ITEM_ID, PauseItemsFocus, PauseItemsSession};
use legaia_engine_core::region_encounter::{EncounterRegion, RegionEncounterTable};
use legaia_engine_core::walk_regen::{WALK_REGEN_STEP_COST, tick_walk_regen};
use legaia_engine_core::world::{SceneMode, World};

/// A field world with a seated player standing in one whole-map region whose
/// rate drains any step counter in one step, holding a Healing Leaf and two
/// Incense.
fn field_world() -> World {
    let mut w = World {
        mode: SceneMode::Field,
        ..Default::default()
    };
    w.spawn_actor(0).active = true;
    w.player_actor_slot = Some(0);
    w.party.roster = legaia_save::Party::zeroed(3);
    w.party.inventory.insert(0x77, 3);
    w.party.inventory.insert(INCENSE_ITEM_ID, 2);
    w.party.party_leader_slot = Some(0);
    w.set_item_catalog(ItemCatalog::vanilla());
    let mut t = RegionEncounterTable::new("incense");
    t.regions.push(EncounterRegion {
        tile_x_min: 0,
        tile_z_min: 0,
        tile_x_max: 0xFF,
        tile_z_max: 0xFF,
        rate_increment: 0xFF,
        formation_base: 0,
        formation_count: 1,
        setup: Default::default(),
    });
    w.set_field_regions(Some(t));
    w
}

fn press(s: &mut PauseItemsSession, b: PadButton) {
    s.input_pad_edge(b.mask());
}

/// Command window -> Use -> the Incense row -> Cross. Returns whether the
/// Incense confirm window opened.
fn cross_on_incense(s: &mut PauseItemsSession) -> bool {
    press(s, PadButton::Cross);
    assert_eq!(s.focus, PauseItemsFocus::List);
    let row = s
        .rows
        .iter()
        .position(|r| r.id == INCENSE_ITEM_ID)
        .expect("an Incense row");
    for _ in 0..row {
        press(s, PadButton::Down);
    }
    press(s, PadButton::Cross);
    s.focus == PauseItemsFocus::SpecialRoute
}

/// Use one Incense through the pause Items screen, the way a host does.
fn use_incense(w: &mut World) {
    let mut s = build_pause_items_session(w);
    assert!(cross_on_incense(&mut s), "the confirm window opens");
    press(&mut s, PadButton::Cross); // Yes
    assert_eq!(s.incense_uses(), 1);
    apply_pause_items_outcome(&s, w);
}

/// One walk tick through the kernel the field frame runs.
fn walk_tick(w: &mut World) {
    let mut steps = WALK_REGEN_STEP_COST + 1;
    let mut window = w.locomotion.walk_regen_window;
    tick_walk_regen(&mut steps, &mut [], &mut window);
    w.locomotion.walk_regen_window = window;
}

#[test]
fn a_pause_menu_incense_suppresses_the_next_0x40_walk_ticks_of_rolls() {
    // Contrast: without an Incense the very first step rolls a battle.
    let mut control = field_world();
    control.set_encounter_step_counter(1);
    assert!(control.on_field_step(), "the fixture must roll on step one");

    let mut w = field_world();
    use_incense(&mut w);
    assert_eq!(
        w.locomotion.walk_regen_window, 0x40,
        "one top-up = 0x40 ticks"
    );
    w.set_encounter_step_counter(1);

    for tick in 0..0x40 {
        assert!(
            !w.on_field_step(),
            "tick {tick}: the roll must skip while the window is open"
        );
        assert_eq!(
            w.encounter_step_counter(),
            1,
            "tick {tick}: a skipped roll does not drain the step counter"
        );
        walk_tick(&mut w);
    }
    assert_eq!(w.locomotion.walk_regen_window, 0);
    assert!(
        w.on_field_step(),
        "the first step after the window closes rolls"
    );
}

#[test]
fn incense_top_ups_stack_to_the_0x100_cap_and_the_row_greys_at_0xe0() {
    let mut w = field_world();
    w.party.inventory.insert(INCENSE_ITEM_ID, 9);
    for want in [0x40, 0x80, 0xC0, 0x100] {
        use_incense(&mut w);
        assert_eq!(w.locomotion.walk_regen_window, want);
    }
    // At 0x100 >= 0xE0 the Use-list row is greyed: the confirm never opens.
    let mut s = build_pause_items_session(&w);
    assert!(!cross_on_incense(&mut s), "a greyed Incense row buzzes");
    // Contrast: one tick under the threshold the row is live again.
    w.locomotion.walk_regen_window = 0xDF;
    let mut s = build_pause_items_session(&w);
    assert!(cross_on_incense(&mut s));
}

#[test]
fn backing_out_of_the_incense_confirm_arms_nothing() {
    let mut w = field_world();
    let mut s = build_pause_items_session(&w);
    assert!(cross_on_incense(&mut s));
    press(&mut s, PadButton::Circle);
    assert_eq!(s.incense_uses(), 0);
    apply_pause_items_outcome(&s, &mut w);
    assert_eq!(w.locomotion.walk_regen_window, 0);
}
