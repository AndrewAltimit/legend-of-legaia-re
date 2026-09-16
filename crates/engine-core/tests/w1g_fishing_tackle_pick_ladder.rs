//! The fishing venue's two **picker screens** - the five-row main menu
//! (`overlay_fishing_801D0474`) and the rod / lure select
//! (`FUN_801D0F5C`) - driven by pad, with the picks they produce carried
//! into the pond.
//!
//! `w1f1_fishing_pond_ladder` reaches four of the six fishing session kernels
//! and cannot reach these two, because it **sets** the lure and rod as pond
//! constructor arguments. That is a precondition, not a screen: nothing in it
//! moves a cursor, refuses an unowned row, or skips an unowned rod slot. This
//! ladder is denominated in the **player's picks** instead - every value the
//! pond opens on here came out of a pad edge on one of the two screens - so
//! the band-4 preconditions the pond ladder seeds (venue 0, Normal lure, third
//! rod) arrive the way a player earns them.
//!
//! What it does **not** establish, and the reason these rows are host-owed
//! rather than fixture-owed: no host owns either screen. The native window and
//! the browser play page enter the pond directly and take the rod from a dev
//! constant; the standalone minigames page takes rod and lure as
//! `fishing_pond_start` arguments and models no tackle inventory at all. Both
//! kernels say so in their own doc blocks. This ladder measures reach; the
//! wiring gap stands until a host owns the five rows and the tackle list.
//!
//! Disc-free: both kernels are pure pad-in / decision-out, and the pond's
//! species, spawn page and gesture templates are synthetic in the same shape
//! the disc carries.

use legaia_asset::fishing_species::{CadenceStep, CadenceTemplate, FishingSpecies, SPAWN_BANDS};
use legaia_engine_core::fishing::{
    CAST_POWER_MAX, FISHING_MENU_ROW_STATES, FISHING_MENU_ROW_X, FISHING_MENU_ROW_Y0, FishingMenu,
    FishingRecord, PondEvent, PondInput, PondPhase, PondSession, RodLureSelect, lure_item_id,
    rod_item_id, spawn_species,
};

/// Pad bits the two screens read, as retail masks them.
const PAD_UP: u32 = 0x1000;
const PAD_DOWN: u32 = 0x4000;
/// Accept: Cross `0x40` or L1 `0x04`.
const PAD_ACCEPT: u32 = 0x40;
/// Cancel: Circle `0x20` or L2 `0x01`.
const PAD_CANCEL: u32 = 0x20;
/// Reel-A (Cross) held bit of the retail `_DAT_8007b850` word.
const REEL_A: u32 = 0x40;

/// The lure row the player picks. Venue 0's band-4 arm wants lure `1`.
const PICKED_LURE_ROW: u32 = 1;
/// The rod slot the player's pick must land on: the player owns rod slots
/// `0` and `2`, so the *second* visible rod row is slot `2` - which is the
/// rod the band-4 arm wants, reached by the kernel's own skip of slot `1`.
const PICKED_ROD_SLOT: i32 = 2;
/// Species index the picked lure's spawn row names.
const PICKED_LURE_SPECIES: usize = 3;

/// The player's tackle bag: lure `0x9E` (row 1) and rods `0xA0` / `0xA2`.
/// Lure row `0` is deliberately absent, so the refuse arm has a row to
/// refuse.
fn bag() -> impl FnMut(u32) -> i32 {
    move |id| {
        let owned = [
            lure_item_id(PICKED_LURE_ROW),
            rod_item_id(0),
            rod_item_id(2),
        ];
        owned.contains(&id) as i32
    }
}

/// A species record tuned to land on a straight reel-in.
fn species(index: usize, score_value: i32, pull_factor: i32) -> FishingSpecies {
    FishingSpecies {
        index,
        name_ptr_va: 0,
        score_value,
        pull_factor,
        dart_factor: 60,
        sink_factor: 4,
        depth_gate: 4096,
        roll_cutoff_a: 200,
        roll_cutoff_b: 512,
        roll_cutoff_c: 90,
        strike_gate: 100,
    }
}

/// A venue page whose every lure row names a **different** species, so the
/// fish that hooks names the row the player's pick selected.
fn spawn_page() -> Vec<[u32; SPAWN_BANDS]> {
    (0..8u32)
        .map(|row| {
            let id = if row == PICKED_LURE_ROW {
                PICKED_LURE_SPECIES as u32
            } else {
                (row as usize % 10) as u32
            };
            [id; SPAWN_BANDS]
        })
        .collect()
}

const HOLD: i32 = 6;

fn templates() -> Vec<CadenceTemplate> {
    vec![CadenceTemplate {
        history_window: HOLD * 2,
        steps: vec![
            CadenceStep {
                duration: HOLD,
                button: 1,
            },
            CadenceStep {
                duration: HOLD,
                button: 0,
            },
        ],
    }]
}

// ---------------------------------------------------------------------------
// Rung 1 - the rod / lure select, by pad, over a real tackle bag.
// ---------------------------------------------------------------------------

/// Drive the screen to the picks the pond opens on, and return them.
fn pick_tackle() -> (u32, i32) {
    let mut s = RodLureSelect::default();

    // The cursor starts on lure row 0, which the player does not own: accept
    // refuses with the cannot-equip cue and equips nothing.
    let refused = s.tick(PAD_ACCEPT, 0, true, bag());
    assert_eq!(refused.sfx, Some(0x22), "an unowned lure row refuses");
    assert_eq!(refused.equip_lure, None);
    assert_eq!(refused.equip_rod, None);

    // Down one row, by pad, onto the lure the player has.
    let moved = s.tick(0, PAD_DOWN, true, bag());
    assert_eq!(moved.sfx, Some(0x21), "a cursor move plays the move cue");
    assert_eq!(s.cursor, PICKED_LURE_ROW as i32);
    let equipped = s.tick(PAD_ACCEPT, 0, true, bag());
    assert_eq!(equipped.sfx, Some(0x20));
    let lure = equipped.equip_lure;
    assert_eq!(lure, Some(PICKED_LURE_ROW), "the owned lure equips");

    // Rod rows start at cursor 3, and only the OWNED slots are visible rows.
    // The player owns slots 0 and 2, so the second rod row must resolve to
    // slot 2 - the kernel walking past the unowned slot 1 is the whole point.
    for _ in 0..3 {
        s.tick(0, PAD_DOWN, true, bag());
    }
    assert_eq!(s.cursor, 4, "three steps down from lure row 1 is rod row 1");
    let rod_pick = s.tick(PAD_ACCEPT, 0, true, bag());
    assert_eq!(rod_pick.sfx, Some(0x20));
    let rod = rod_pick.equip_rod;
    assert_eq!(
        rod,
        Some(PICKED_ROD_SLOT),
        "the second visible rod row is the second OWNED slot, not slot 1"
    );

    // The snap-wrap is bounded by the owned rods: two owned rods plus three
    // lure rows is five rows, so one more step down wraps to 0.
    s.tick(0, PAD_DOWN, true, bag());
    assert_eq!(s.cursor, 0, "the cursor wraps at owned_rods + 2");
    s.tick(0, PAD_UP, true, bag());
    assert_eq!(s.cursor, 4, "and wraps back the other way");

    // Cancel leaves the screen; the caller jumps the fishing SM to 100.
    let leave = s.tick(PAD_CANCEL, 0, true, bag());
    assert!(leave.leave);
    assert_eq!(leave.sfx, Some(0x37));

    (lure.unwrap(), rod.unwrap())
}

// ---------------------------------------------------------------------------
// Rung 2 - the venue's five-row main menu, by pad.
// ---------------------------------------------------------------------------

/// Walk the menu's rows by pad and confirm the pond row, returning the SM
/// state the confirm asks for.
fn pick_pond_row() -> u32 {
    let mut m = FishingMenu::default();

    // Cursor geometry: row 0 sits at the panel's first row, and each step
    // down advances by the row pitch.
    assert_eq!(m.cursor_pos().1, FISHING_MENU_ROW_Y0);
    assert_eq!(
        m.cursor_pos().0,
        0x5B,
        "the cursor icon sits left of the rows"
    );
    assert!(
        m.cursor_pos().0 < FISHING_MENU_ROW_X,
        "the icon column is left of the row text column"
    );

    // Up from row 0 snaps to the last row (retail's bgez / slti pair, not a
    // modulo), and down from there snaps back to 0.
    m.tick(PAD_UP as u16, true);
    assert_eq!(m.cursor, 4);
    m.tick(PAD_DOWN as u16, true);
    assert_eq!(m.cursor, 0);

    // Row 4 is the venue exit: confirming it arms the exit latch rather than
    // a sub-screen. Take it on a throwaway menu so the walk can continue.
    let mut exit_menu = FishingMenu { cursor: 4 };
    let exit = exit_menu.tick(PAD_ACCEPT as u16, true);
    assert!(exit.leave_venue, "row 4 arms the venue exit");
    assert_eq!(exit.next_state, Some(FISHING_MENU_ROW_STATES[4]));

    // Rows 2 and 3 snapshot the points bank on confirm; the other rows do not.
    for (row, want) in FISHING_MENU_ROW_STATES.iter().enumerate() {
        let mut probe = FishingMenu { cursor: row as i32 };
        let t = probe.tick(PAD_ACCEPT as u16, true);
        assert_eq!(t.snapshot_points, row == 2 || row == 3, "row {row}");
        assert_eq!(t.next_state, Some(*want), "row {row}");
    }

    // The player picks row 1 - the pond - by stepping down once and
    // confirming.
    m.tick(PAD_DOWN as u16, true);
    assert_eq!(m.cursor, 1);
    let go = m.tick(PAD_ACCEPT as u16, true);
    assert_eq!(go.sfx, Some(0x20));
    assert!(!go.leave_venue);
    assert!(!go.snapshot_points);
    go.next_state.expect("the pond row names an SM state")
}

// ---------------------------------------------------------------------------
// Rung 3 - the pond, opened on the picks.
// ---------------------------------------------------------------------------

fn cast(p: &mut PondSession) {
    let press = PondInput {
        cast_edge: true,
        ..Default::default()
    };
    let idle = PondInput::default();
    p.tick(press, 1, 0x80);
    for _ in 0..64 {
        if p.phase() == PondPhase::Power {
            break;
        }
        p.tick(idle, 1, 0x80);
    }
    assert_eq!(p.phase(), PondPhase::Power, "the power meter never opened");
    for _ in 0..64 {
        if p.cast_power() >= CAST_POWER_MAX {
            break;
        }
        p.tick(idle, 1, 0x80);
    }
    p.tick(press, 1, 0x80);
    for _ in 0..64 {
        if p.phase() == PondPhase::Waiting {
            break;
        }
        p.tick(idle, 1, 0x80);
    }
    assert_eq!(p.phase(), PondPhase::Waiting, "the lure never settled");
}

fn work_the_lure(p: &mut PondSession, frames: usize) -> Vec<PondEvent> {
    let mut events = Vec::new();
    let mut f = 0usize;
    while f < frames && p.phase() == PondPhase::Waiting {
        let held = (f as i32 / HOLD) % 2 == 0;
        p.tick(
            PondInput {
                reel_mask: if held { REEL_A } else { 0 },
                cast_edge: false,
                edge_bonus: i32::from(f as i32 % HOLD == 0),
            },
            1,
            0x80,
        );
        events.extend(p.take_events());
        f += 1;
    }
    events
}

fn fight(p: &mut PondSession, frames: usize) -> Vec<PondEvent> {
    let mut events = Vec::new();
    for _ in 0..frames {
        if p.phase() != PondPhase::Hooked {
            break;
        }
        p.tick(
            PondInput {
                reel_mask: REEL_A,
                cast_edge: false,
                edge_bonus: 0,
            },
            1,
            0x80,
        );
        events.extend(p.take_events());
    }
    events
}

#[test]
fn the_picked_lure_and_rod_are_what_the_pond_opens_on() {
    let (lure, rod) = pick_tackle();
    let state = pick_pond_row();
    assert_eq!(
        state, FISHING_MENU_ROW_STATES[1],
        "the pond row names the fishing SM state the venue jumps to"
    );

    // The spawn row the picked lure selects, before a single pond frame runs.
    let page = spawn_page();
    assert_eq!(
        spawn_species(&page, lure, 0),
        Some(PICKED_LURE_SPECIES),
        "the picked lure selects its own spawn row"
    );

    let mut table: Vec<FishingSpecies> = (0..10).map(|i| species(i, 1_000, 250)).collect();
    table[PICKED_LURE_SPECIES] = species(PICKED_LURE_SPECIES, 40_000, 90);
    // Venue 0 with the player's own picks and an even lifetime cast counter
    // past 50: the band-4 arm's preconditions, none of them set by hand.
    let mut p = PondSession::new(
        table,
        page,
        templates(),
        0,
        lure,
        rod,
        100,
        FishingRecord::default(),
        0,
        0x1234_5678,
    );

    let mut events = Vec::new();
    let mut casts = 0;
    while p.phase() != PondPhase::Hooked && casts < 64 {
        cast(&mut p);
        casts += 1;
        events.extend(work_the_lure(&mut p, 4000));
    }
    assert_eq!(p.phase(), PondPhase::Hooked, "no strike in {casts} casts");

    let hooked = events
        .iter()
        .find_map(|e| match e {
            PondEvent::Hooked(id) => Some(*id),
            _ => None,
        })
        .expect("a hook event");
    assert_eq!(
        hooked, PICKED_LURE_SPECIES,
        "the hooked species is the one the PICKED lure's spawn row names"
    );

    let fight_events = fight(&mut p, 8000);
    assert_eq!(p.phase(), PondPhase::Landed, "the weak fish should land");
    let award = fight_events
        .iter()
        .find_map(|e| match e {
            PondEvent::Landed(pts) => Some(*pts),
            _ => None,
        })
        .expect("a landed event");
    assert!(award > 0);
    assert_eq!(p.record.best_fish, PICKED_LURE_SPECIES);
}

#[test]
fn a_non_interactive_frame_moves_neither_picker() {
    // Both screens gate the whole pad block on the interactive flag (retail
    // `a0 != 0` / `param_1 != 0`), and both still run their snap-wrap.
    let mut m = FishingMenu { cursor: 2 };
    let t = m.tick(0xFFFF, false);
    assert_eq!(t.next_state, None);
    assert_eq!(t.sfx, None);
    assert_eq!(m.cursor, 2);

    let mut s = RodLureSelect { cursor: 9 };
    let t = s.tick(0xFFFF_FFFF, 0xFFFF_FFFF, false, bag());
    assert_eq!(t.sfx, None);
    assert_eq!(t.equip_lure, None);
    assert_eq!(t.equip_rod, None);
    assert_eq!(
        s.cursor, 0,
        "the snap-wrap runs even on a non-interactive frame"
    );
}
