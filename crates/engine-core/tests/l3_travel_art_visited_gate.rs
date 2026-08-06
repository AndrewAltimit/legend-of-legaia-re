//! Reach conversion: the **travel-art quick-travel** rows
//! (`engine-vm/travel_art_actor.rs` `801ee094` Riremito / `801ee328` Rula),
//! whose reach gate reads "a world-map quick-travel with at least one visited
//! destination".
//!
//! Both retail handlers are one ported machine (`TravelArtActor::tick` carries
//! both `PORT:` tags), and both end in the same resolve-and-warp kernel:
//! scan the visited-map table for the party's current map, then install
//! `(tile << 7) + 0x40` as the destination.
//!
//! ## What the gate really costs, and the defect this file pinned
//!
//! The table is built by `World::tick_world_map_panels`. It once recorded a
//! visit with
//!
//! ```text
//! let map = ctrl.panels.visited.last().map(|v| v.map_id).unwrap_or(0);
//! ctrl.panels.note_visit(map, tx, tz);
//! ```
//!
//! reading the map id back **out of the table it was writing**: a one-record
//! table for the whole session no matter how many overworlds the party
//! crossed. The write now keys on the kingdom the party stands on
//! (`kingdom_index_for_scene_base(ctrl.scene_base)`, falling back to the
//! entry fade's derived index and then the active `mapNN` scene label), and
//! `each_kingdom_crossed_gets_its_own_visited_record` - formerly the
//! `#[ignore]`d repro - asserts the multi-record behaviour live.
//!
//! The same hand-off hard-codes `TravelArt::Riremito`, so `801ee328`'s dwell
//! constants have no production installer at all. The panel host itself
//! supports both, so the Rula arm is driven here through
//! `PanelActorHost::install` - the honest shape: the *machine* is reachable,
//! the *binding* is not.
//!
//! Disc-free.

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::World;
use legaia_engine_core::world_map_panel_host::{
    PanelActorHost, PanelActorKind, PanelFlagStore, VisitedMap, packed_pad,
};
use legaia_engine_vm::travel_art_actor::TravelArt;
use legaia_engine_vm::world_map_panel_actors::BRIGHTNESS_MAX;

/// The three retail overworld scene bases, whose kingdom indices are `0/1/2`
/// (`world_map::kingdom_index_for_scene_base`).
const KINGDOM_BASES: [u16; 3] = [0x55, 0xF4, 0x187];

/// A story-flag bank the panel host can read and write without a `World`.
#[derive(Default)]
struct Flags(std::collections::HashSet<i32>);

impl PanelFlagStore for Flags {
    fn flag_test(&self, id: i32) -> bool {
        self.0.contains(&id)
    }
    fn flag_set(&mut self, id: i32) {
        self.0.insert(id);
    }
    fn flag_clear(&mut self, id: i32) {
        self.0.remove(&id);
    }
}

fn overworld() -> World {
    let mut w = World::default();
    w.enter_world_map();
    w.spawn_actor(0).active = true;
    w.player_actor_slot = Some(0);
    w.seat_player_at_tile(20, 30);
    w.set_pad(0);
    w.tick();
    w
}

fn frame(w: &mut World, pad: u16) {
    w.set_pad(pad);
    w.tick();
}

// ---------------------------------------------------------------------------
// The gate: what the visited table can hold
// ---------------------------------------------------------------------------

/// Walk the party across all three kingdom overworlds, letting the idle world
/// tick record a visit on each, and return the table it built.
fn visited_after_three_kingdoms() -> Vec<VisitedMap> {
    let mut w = overworld();
    for (i, base) in KINGDOM_BASES.iter().enumerate() {
        w.world_map_ctrl
            .as_mut()
            .expect("world-map controller")
            .scene_base = *base;
        w.seat_player_at_tile(10 * (i as u8 + 1), 20 * (i as u8 + 1));
        for _ in 0..8 {
            frame(&mut w, 0);
        }
    }
    w.world_map_ctrl
        .as_ref()
        .expect("controller")
        .panels
        .visited
        .clone()
}

#[test]
fn the_recorder_tracks_the_tile_the_party_is_standing_on() {
    // The half that is correct, and the reason the defect below is invisible
    // in play: whatever the record is *keyed* by, its tile follows the party,
    // so a same-map quick travel returns them where they left.
    let visited = visited_after_three_kingdoms();
    let last = visited.last().expect("the idle tick recorded a tile");
    assert_eq!(
        (last.tile_x, last.tile_z),
        (30, 60),
        "the stored tile is the party's last pre-screen position"
    );
    // A distinct map id *was* available on every leg: the resolver
    // `FUN_800196A4` re-derives maps these three scene bases onto 0/1/2. So
    // the ignored repro below is about a discarded input, not a missing one.
    assert_eq!(
        KINGDOM_BASES
            .iter()
            .map(|b| legaia_engine_core::world_map::kingdom_index_for_scene_base(*b))
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1), Some(2)]
    );
}

/// `World::tick_world_map_panels` once recorded a visit with
///
/// ```text
/// let map = ctrl.panels.visited.last().map(|v| v.map_id).unwrap_or(0);
/// ctrl.panels.note_visit(map, tx, tz);
/// ```
///
/// reading the map id back out of the table being written: the first call
/// stored `0` and every later one updated that same record, so crossing
/// three kingdoms left one record. The write now keys on the kingdom the
/// party is standing on (`kingdom_index_for_scene_base(ctrl.scene_base)`,
/// with the entry fade's derived index and the active `mapNN` scene label
/// as fallbacks), so each kingdom gets its own record - and `note_visit`'s
/// own dedupe (asserted by `note_visit_updates_the_stored_tile_in_place`)
/// keeps a revisit updating in place.
#[test]
fn each_kingdom_crossed_gets_its_own_visited_record() {
    let visited = visited_after_three_kingdoms();
    let mut ids: Vec<u32> = visited.iter().map(|v| v.map_id).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![0, 1, 2],
        "one visited record per kingdom the party stood on"
    );
}

#[test]
fn the_resolve_kernel_itself_handles_a_multi_map_table() {
    // The kernel is not what is broken - only its input is. Feed the host a
    // table the world can never build and the scan picks the right record.
    let mut host = PanelActorHost::new();
    host.note_visit(0, 1, 2);
    host.note_visit(1, 30, 40);
    host.note_visit(2, 96, 25);
    assert_eq!(host.visited.len(), 3);

    host.install(PanelActorKind::TravelArt(TravelArt::Riremito), 0x1A);
    let mut flags = Flags::default();
    let mut warp = None;
    for _ in 0..600 {
        let f = host.tick(0, 0, 1, &mut flags);
        if let Some(d) = f.warp {
            warp = Some(d);
            break;
        }
        assert!(!f.travel_unfound, "the scan must not miss a present map");
    }
    let d = warp.expect("the travel art resolved a destination");
    // `current` is the last-recorded map, so record 2 is the hit.
    assert_eq!(d.record_index, 2);
    assert_eq!((d.x, d.y, d.z), ((96 << 7) + 0x40, 0, (25 << 7) + 0x40));
    assert!(!host.is_active(), "the warp retires the actor");
}

// ---------------------------------------------------------------------------
// Both handlers, through the host
// ---------------------------------------------------------------------------

/// Drive one travel art from install to warp and report `(flash frame, warp
/// frame)`.
fn drive(art: TravelArt) -> (usize, usize) {
    let mut host = PanelActorHost::new();
    host.note_visit(7, 12, 34);
    host.install(PanelActorKind::TravelArt(art), 0x1A);
    let mut flags = Flags::default();
    let mut flash_at = None;
    let mut warp_at = None;
    for f in 0..2000 {
        let out = host.tick(0, 0, 1, &mut flags);
        if out.brightness == Some(BRIGHTNESS_MAX) && flash_at.is_none() {
            flash_at = Some(f);
        }
        if out.warp.is_some() {
            warp_at = Some(f);
            break;
        }
    }
    (
        flash_at.expect("the flourish quad never fired"),
        warp_at.expect("the art never warped"),
    )
}

#[test]
fn riremito_and_rula_are_the_same_machine_on_different_dwells() {
    let (riremito_flash, riremito_warp) = drive(TravelArt::Riremito);
    let (rula_flash, rula_warp) = drive(TravelArt::Rula);

    // Riremito flashes leaving phase 1 (dwell 0x50), Rula leaving phase 2
    // (dwell 0x28 then 0x28). Both then resolve on the next frame.
    assert_eq!(
        riremito_flash,
        TravelArt::Riremito.phase1_dwell() as usize,
        "Riremito's quad is on the phase-1 boundary"
    );
    assert_eq!(
        rula_flash,
        (TravelArt::Rula.phase1_dwell() + TravelArt::Rula.phase2_dwell()) as usize,
        "Rula's quad is on the phase-2 boundary"
    );
    assert!(
        riremito_warp > rula_warp,
        "Riremito's longer phase-1 dwell must make it the slower art \
         ({riremito_warp} vs {rula_warp})"
    );
    // Both reach the same kernel on the frame after their second dwell
    // expires, so the total is the sum of the two dwells + 1 either way; only
    // *where the quad lands* inside that span differs.
    for (art, warp) in [
        (TravelArt::Riremito, riremito_warp),
        (TravelArt::Rula, rula_warp),
    ] {
        assert_eq!(
            warp,
            (art.phase1_dwell() + art.phase2_dwell()) as usize + 1,
            "{art:?} resolved off its own dwell pair"
        );
    }
}

#[test]
fn a_travel_art_with_no_visited_map_parks_instead_of_warping() {
    // The miss arm (`PHASE_UNFOUND`, retail's `"UNFIND MAP NUMBER %d"` park).
    // Nothing in the world path reaches it - `tick_world_map_panels` records
    // a visit before it can install the actor - so an empty table is the only
    // way in, and it is a real state: an overworld with no seated player
    // records nothing.
    let mut host = PanelActorHost::new();
    assert!(host.visited.is_empty());
    host.install(PanelActorKind::TravelArt(TravelArt::Riremito), 0x1A);
    let mut flags = Flags::default();
    let mut unfound = false;
    for _ in 0..600 {
        let f = host.tick(0, 0, 1, &mut flags);
        assert!(f.warp.is_none(), "an empty table must resolve nothing");
        if f.travel_unfound {
            unfound = true;
            break;
        }
    }
    assert!(unfound, "the scan miss never parked the actor");
    assert!(!host.is_active(), "and the parked actor leaves the screen");
}

// ---------------------------------------------------------------------------
// The production hand-off
// ---------------------------------------------------------------------------

#[test]
fn the_world_hand_off_installs_riremito_and_warps_to_the_frozen_tile() {
    // The end-to-end path a player takes: the sub-list picker's row-1 confirm
    // is retail's state-3 hand-off, which the port binds to the travel art.
    let mut w = overworld();
    w.world_map_ctrl.as_mut().expect("controller").debug_enabled = true;
    for _ in 0..4 {
        frame(&mut w, 0);
    }
    let frozen: VisitedMap = *w
        .world_map_ctrl
        .as_ref()
        .expect("controller")
        .panels
        .visited
        .last()
        .expect("the idle tick recorded the party's tile");

    frame(&mut w, PadButton::Square.mask());
    frame(&mut w, 0);
    assert_eq!(
        w.world_map_ctrl.as_ref().expect("controller").panels.kind,
        Some(PanelActorKind::SubList),
        "Square opens the sub-list picker"
    );
    frame(&mut w, PadButton::Down.mask());
    frame(&mut w, 0);
    frame(&mut w, PadButton::Cross.mask());
    frame(&mut w, 0);

    // Move the party away so the warp is observable.
    w.seat_player_at_tile(120, 5);
    let mut installed_art = None;
    for _ in 0..900 {
        frame(&mut w, 0);
        if let Some(PanelActorKind::TravelArt(art)) =
            w.world_map_ctrl.as_ref().expect("controller").panels.kind
        {
            installed_art = Some(art);
        }
        if installed_art.is_some()
            && !w
                .world_map_ctrl
                .as_ref()
                .expect("controller")
                .panels
                .is_active()
        {
            break;
        }
    }
    assert_eq!(
        installed_art,
        Some(TravelArt::Riremito),
        "the hand-off hard-codes Riremito - `801ee328`'s dwells have no \
         production installer"
    );
    let slot = w.player_actor_slot.expect("player") as usize;
    let a = &w.actors[slot];
    assert_eq!(
        (a.move_state.world_x, a.move_state.world_z),
        (
            ((frozen.tile_x << 7) + 0x40) as i16,
            ((frozen.tile_z << 7) + 0x40) as i16
        ),
        "the art warps to the tile the screen froze on entry"
    );
    // The packed-pad helper is what the chords above went through; a raw word
    // suppresses nothing, which is the trap the host's own docs name.
    assert_ne!(packed_pad(PadButton::Square.mask()), 0);
}
