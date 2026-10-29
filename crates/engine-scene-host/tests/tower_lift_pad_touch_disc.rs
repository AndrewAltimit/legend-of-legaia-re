//! Disc-gated regression: **a walk-touch contact runs the touched object's
//! own record.**
//!
//! Retail's touch post `FUN_801D5B5C` decides nothing about what a contact
//! does: beside the engagement bookkeeping it raises the touched actor's
//! `+0x10 |= 0x100` (`0x801D5BB0..0x801D5BBC`), the bit the dialog SM
//! `FUN_80039B7C` gates that actor's context on, so the object's record
//! resumes from its parked cursor and does whatever it does.
//!
//! - `tower`'s lifts (partition-0 records 2..17) open their activation arm
//!   with the cue `36 00 80 30 00`, flicker the lift's texture with `4C 60`
//!   `MoveImage`s, walk the player in and jump with `A3 F8`.
//! - A `town01` house doorway (record 32, a clip-less door marker) washes the
//!   screen white (`34 01 FF FF FF 30`), waits out the wash, and only then
//!   jumps with its own `A3 F8 19 1B`
//!   (`walk_on_trigger_dispatch_disc::house_door_binds_teleport_on_contact`).
//!
//! The census test asserts every `.MAP` object walk-touch bind on the disc
//! is a bank entry, so its contact takes that path and none applies a
//! decoded summary of its record.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::world::World;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_engine_system::sound_state::SfxRingOp;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("extracted")
        });
    if d.join("PROT.DAT").is_file() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

/// The lift's flicker: `4C 60` copies one of the two lit frames at
/// `(576, 256)` / `(576, 288)` / `(576, 320)` onto the lift's texture at
/// `(512, 448)`.
fn is_flicker(m: &legaia_engine_field::script_vram_move::ScriptVramMove) -> bool {
    m.dst == (512, 448) && m.size == (16, 32) && m.src.0 == 576
}

#[test]
fn tower_lift_contact_fires_its_cue_flicker_and_jump() {
    let Some(ex) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&ex).expect("open SceneHost");
    host.enter_field_scene("tower", 0).expect("enter tower");
    for _ in 0..90 {
        let _ = host.tick();
    }
    let _ = host.world.take_sfx_ring_ops();
    host.world.ambient.script_vram_moves.clear();
    // P0[17]: first floor, east lift, contact (4064, 1952).
    let (cx, cz) = (4064i32, 1952i32);
    let start = (cx, cz - 176);
    {
        let w = &mut host.world;
        let slot = w.player_actor_slot.expect("player slot") as usize;
        let y = w.sample_field_floor_height(start.0, start.1) as i16;
        let ms = &mut w.actors[slot].move_state;
        ms.world_x = start.0 as i16;
        ms.world_z = start.1 as i16;
        ms.world_y = y;
    }
    let mut cue_at = None;
    let mut flicker_at = None;
    let mut jumped_at = None;
    for f in 0..600 {
        host.world
            .set_pad(if f < 40 { PadButton::Up.mask() } else { 0 });
        let _ = host.tick();
        if host
            .world
            .take_sfx_ring_ops()
            .contains(&SfxRingOp::Push(0x30))
        {
            cue_at.get_or_insert(f);
        }
        if host.world.ambient.script_vram_moves.iter().any(is_flicker) {
            flicker_at.get_or_insert(f);
        }
        host.world.ambient.script_vram_moves.clear();
        let w = &host.world;
        let ms = &w.actors[w.player_actor_slot.expect("player slot") as usize].move_state;
        if (i32::from(ms.world_z) - start.1).abs() > 1200 {
            jumped_at.get_or_insert(f);
        }
    }
    eprintln!("[tower] P0[17]: cue {cue_at:?} flicker {flicker_at:?} jump {jumped_at:?}");
    let cue = cue_at.expect("the lift's record fires cue 0x30 on contact");
    let flicker = flicker_at.expect("the lift's record flickers its texture");
    let jump = jumped_at.expect("the lift's record moves the player to another floor");
    assert!(
        cue <= flicker && flicker < jump,
        "the record's order: cue, flicker, then the jump"
    );
}

#[test]
fn every_object_walk_touch_bind_resumes_its_record() {
    let Some(ex) = extracted_dir() else {
        return;
    };
    let cdname = legaia_prot::cdname::parse(&ex.join("CDNAME.TXT")).expect("CDNAME");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();
    let mut host = SceneHost::open_extracted(&ex).expect("open SceneHost");
    let mut binds = 0;
    let mut unbanked = Vec::new();
    for n in &names {
        if host.enter_field_scene(n, 0).is_err() {
            continue;
        }
        let w = &host.world;
        for (&slot, &(pos, _)) in &w.props.walk_touch {
            if slot < World::TRIGGER_WALK_TOUCH_SLOT_BASE {
                continue;
            }
            binds += 1;
            let banked = w
                .props
                .walk_touch_records
                .get(&slot)
                .is_some_and(|&r| w.props.bank.props.values().any(|p| p.record == r));
            if !banked {
                unbanked.push((n.clone(), pos));
            }
        }
    }
    eprintln!("[census] {binds} object walk-touch binds, unbanked {unbanked:?}");
    assert!(binds > 0, "the disc carries object walk-touch binds");
    assert!(
        unbanked.is_empty(),
        "object binds whose contact would apply a decoded summary: {unbanked:?}"
    );
}

/// Every clip-less door marker, touched, runs its record to the end and sets
/// the player down where the record's own `A3 F8` seats it, clear of every
/// other bind's contact box - the engine's stand-inside contact test would
/// otherwise re-fire on the first step off the landing, which the summary
/// path's arrival bracket used to paper over.
#[test]
fn door_marker_records_land_clear_of_every_contact_box() {
    let Some(ex) = extracted_dir() else {
        return;
    };
    let cdname = legaia_prot::cdname::parse(&ex.join("CDNAME.TXT")).expect("CDNAME");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();
    let mut host = SceneHost::open_extracted(&ex).expect("open SceneHost");
    let mut rides = 0;
    let mut bad = Vec::new();
    let mut held = Vec::new();
    for n in &names {
        if host.enter_field_scene(n, 0).is_err() {
            continue;
        }
        let markers: Vec<((u8, u8), usize)> = host
            .world
            .props
            .bank
            .props
            .iter()
            .filter(|(_, p)| p.anim.anim_id == 0 && !p.interact_gated())
            .filter(|(_, p)| {
                host.world
                    .props
                    .walk_touch_records
                    .values()
                    .any(|&r| r == p.record)
            })
            .map(|(&a, p)| (a, p.record))
            .collect();
        for (anchor, record) in markers {
            host.enter_field_scene(n, 0).expect("re-enter");
            for _ in 0..30 {
                let _ = host.tick();
            }
            let boxes: Vec<(i16, i16)> = host
                .world
                .props
                .walk_touch
                .values()
                .map(|&(p, _)| p)
                .collect();
            // A scene whose entry beat keeps the player engaged
            // (`town0d`'s waits on a box nobody dismisses here) holds every
            // touch, as retail's engaged flag does; its doors are the
            // Rim Elm doors the other three story states ride.
            if host.world.script_context_engages_player() {
                held.push(format!("{n} P0[{record}]"));
                continue;
            }
            if !host.world.start_prop_interaction(anchor) {
                bad.push(format!("{n} P0[{record}]: touch refused"));
                continue;
            }
            let mut done = false;
            for _ in 0..900 {
                let _ = host.tick();
                if host.world.dialog.inline.is_none() {
                    done = true;
                    break;
                }
            }
            let w = &host.world;
            let ms = &w.actors[w.player_actor_slot.expect("player slot") as usize].move_state;
            let at = (ms.world_x, ms.world_z);
            rides += 1;
            if !done {
                bad.push(format!("{n} P0[{record}]: run never ended (at {at:?})"));
                continue;
            }
            if let Some(b) = boxes.iter().find(|&&(bx, bz)| {
                (i32::from(at.0) - i32::from(bx)).abs() < 80
                    && (i32::from(at.1) - i32::from(bz)).abs() < 80
            }) {
                bad.push(format!(
                    "{n} P0[{record}]: lands at {at:?} inside the box at {b:?}"
                ));
            }
        }
    }
    eprintln!(
        "[markers] {rides} rides, held by an entry beat {held:?}, {} bad: {bad:#?}",
        bad.len()
    );
    assert!(rides > 0, "the disc carries clip-less door markers");
    assert!(bad.is_empty(), "door markers that misbehave: {bad:#?}");
}
