//! The Rim Elm opening (`town01`), played from a New Game through the native
//! play window's own frame path (`HeadlessPlayWindow`), with one regression
//! per defect a player of the native build met there:
//!
//! - Mei's front door swings open as she walks out in the Genesis Tree beat
//!   (`P2[4]`) and closes behind her: the record's `CC 01 A0 00 05 00` skips
//!   the `JmpRel` over the close sequence while the door is open, and the
//!   close's `CC 01 8C 18 00` reads the door's clip cursor.
//! - The panoramic map on the blackboard in the Genesis Tree garden
//!   (`P1[51]`) waits on the staged mode switch (`4C 53`, `4C 54`) on its way
//!   into the overview and back out; the talk parks on those polls rather
//!   than ending on them, so the screen comes back.
//! - Mei, seated in Vahn's house for the measuring beat (`P2[5]`), stands on
//!   the floor there, not at the height she had at her own front door.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;

use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;

use super::native_new_game_save_continue::{
    FACING_PROBES, PadLog, free_roam, mask, new_game_to_free_roam,
};

/// Mei's front door: the `.MAP` object whose record is `P0[1]`.
const MEI_DOOR_RECORD: usize = 1;
/// Mei's placement (`P1[34]`).
const MEI_SLOT: u8 = 34;
/// The blackboard carrying the panoramic map (`P1[51]`).
const BLACKBOARD_SLOT: u8 = 51;
/// The `P2[5]` measuring beat.
const MEASURING_RECORD: usize = 5;

fn disc() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)
}

/// A New Game played to free roam in the Genesis Tree garden.
fn new_game(disc: &std::path::Path, tag: &str) -> (HeadlessPlayWindow, PadLog) {
    let save_dir = std::env::temp_dir().join(format!("legaia-rimelm-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&save_dir);
    std::fs::create_dir_all(&save_dir).expect("scratch save dir");
    let mut w = HeadlessPlayWindow::open_boot_ui_in(disc, "town01", None, &save_dir)
        .expect("open the boot-UI window");
    let mut log = PadLog::default();
    new_game_to_free_roam(&mut w, &mut log);
    (w, log)
}

/// Mash Cross through whatever holds the pad until free roam is back, up to
/// `frames` frames. `true` once it is.
fn play_out(w: &mut HeadlessPlayWindow, log: &mut PadLog, frames: u32) -> bool {
    let mut settled = 0;
    for f in 0..frames {
        if free_roam(w) {
            settled += 1;
            if settled > 30 {
                return true;
            }
            log.frame(w, 0);
            continue;
        }
        settled = 0;
        let pad = if f % 8 < 2 { mask(PadButton::Cross) } else { 0 };
        log.frame(w, pad);
    }
    false
}

/// Play the Genesis Tree beat (`P2[4]`): the player stepped onto its walk-on
/// band below the garden gate, the beat played out.
fn play_mei_beat(w: &mut HeadlessPlayWindow, log: &mut PadLog) {
    w.session_mut().host.world.seat_player_at_tile(20, 28);
    log.frame(w, 0);
    w.session_mut().host.world.seat_player_at_tile(20, 26);
    assert!(
        play_out(w, log, 4000),
        "the Genesis Tree beat never handed the pad back"
    );
    assert!(
        w.session().host.world.system_flag_test(0x226),
        "the Genesis Tree beat did not raise 0x226"
    );
}

/// Stand the player facing placement `slot`, close enough that the action
/// button's probe answers with it.
fn seat_facing(w: &mut HeadlessPlayWindow, slot: u8) -> bool {
    let world = &mut w.session_mut().host.world;
    let Some(&(ax, az)) = world.npcs.positions.get(&slot) else {
        return false;
    };
    let Some(player) = world.player_actor_slot.map(usize::from) else {
        return false;
    };
    for q in [8i16, 7, 6, 5, 4] {
        for sector in [4usize, 0, 2, 6, 1, 3, 5, 7] {
            let (dx, dz) = FACING_PROBES[sector];
            let (sx, sz) = (ax - dx * q / 4, az + dz * q / 4);
            // The garden is a small walk component of its own.
            if world.field_walk_component_size(sx, sz) < 4 {
                continue;
            }
            let sy = world.sample_field_floor_height(sx as i32, sz as i32) as i16;
            let m = &mut world.actors[player].move_state;
            m.world_x = sx;
            m.world_y = sy;
            m.world_z = sz;
            m.render_26 = (((sector as i16) << 9) + 0x800) & 0x0FFF;
            if world.field_interact_probe_slot() == Some(slot) {
                return true;
            }
        }
    }
    false
}

/// Mei's door's clip state (`+0x62` control, `+0x68` cursor on retail's
/// actor).
fn door_clip(w: &HeadlessPlayWindow) -> legaia_engine_scene::field_env::PropAnim {
    w.session()
        .host
        .world
        .props
        .bank
        .props
        .values()
        .find(|p| p.record == MEI_DOOR_RECORD)
        .map(|p| p.anim)
        .expect("Mei's door has a prop")
}

#[test]
fn mei_closes_her_door_behind_her() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let (mut w, mut log) = new_game(&disc, "door");
    assert_eq!(door_clip(&w).cursor, 0, "Mei's door starts shut");
    play_mei_beat(&mut w, &mut log);
    // The beat swings the door open (`B1 01 00`, the clip forward to its
    // clamp), Mei walks out, and the close sequence plays the clip back to
    // its first frame: reverse (`AB 01 07`), and the cursor home.
    let door = door_clip(&w);
    assert_eq!(
        door.cursor, 0,
        "the door stands open behind Mei (cursor {}, control {:#x})",
        door.cursor, door.flags
    );
    assert_ne!(
        door.flags & legaia_engine_scene::field_env::ANIM_REVERSE,
        0,
        "the close sequence never ran (control {:#x})",
        door.flags
    );
}

#[test]
fn garden_blackboard_panorama_returns_the_view() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let (mut w, mut log) = new_game(&disc, "board");
    assert!(
        seat_facing(&mut w, BLACKBOARD_SLOT),
        "no seat faces the blackboard"
    );
    // Talk; Cross through the question and its Yes; at the overview's
    // picker, three Downs onto "Forget it" and Cross.
    let mut overview = false;
    let mut picked: Option<u32> = None;
    let mut ended = false;
    for f in 0..3000u32 {
        let (pc, picker) =
            w.session()
                .host
                .world
                .dialog
                .inline
                .as_ref()
                .map_or((None, false), |i| {
                    (
                        Some(i.pc),
                        i.panel.as_ref().is_some_and(|p| p.picker().is_some()),
                    )
                });
        // The overview's question (`+0x1A4`) and its four-way picker.
        if pc.is_some_and(|pc| (0x1A4..0x22F).contains(&pc)) {
            overview = true;
            if picker && picked.is_none() {
                picked = Some(f);
            }
        }
        if f > 4 && pc.is_none() {
            ended = true;
            break;
        }
        let pad = match picked {
            Some(p0) => match f - p0 {
                22 | 62 | 250 => mask(PadButton::Cross),
                150 | 180 | 210 => mask(PadButton::Down),
                _ => 0,
            },
            None if f % 40 == 0 => mask(PadButton::Cross),
            None => 0,
        };
        log.frame(&mut w, pad);
    }
    assert!(
        overview,
        "the talk never reached the overview: it ended at the mode-switch poll"
    );
    assert!(ended, "the talk never ended");
    let world = &w.session().host.world;
    // The way out runs whole: the switch back to the field view (`4C 54`)
    // and the player re-seated in the garden the overview took him from.
    assert_eq!(
        world.dialog.mode_switch.target, 0,
        "the view never switched back from the overview"
    );
    let p = world.player_actor_slot.expect("a player") as usize;
    let m = &world.actors[p].move_state;
    assert_eq!(
        (m.world_x >> 7, m.world_z >> 7),
        (33, 94),
        "the player was not seated back in the garden"
    );
}

#[test]
fn mei_stands_on_the_floor_in_vahns_house() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let (mut w, mut log) = new_game(&disc, "mei");
    play_mei_beat(&mut w, &mut log);
    // The measuring beat, as Vahn's front door spawns it (`P0[29]`'s
    // `44 5E` once `0x226` is up).
    {
        let host = &mut w.session_mut().host;
        let man = host
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&host.index).ok().flatten())
            .expect("town01 MAN");
        let mf = legaia_scene_formats::man_section::parse(&man).expect("parse the MAN");
        assert!(
            host.world
                .install_gated_p2_record(&mf, &man, MEASURING_RECORD),
            "the measuring beat did not install"
        );
    }
    let mut checked = 0;
    for f in 0..1200u32 {
        let pad = if f % 30 == 0 {
            mask(PadButton::Cross)
        } else {
            0
        };
        log.frame(&mut w, pad);
        let world = &w.session().host.world;
        let Some(&(x, z)) = world.npcs.positions.get(&MEI_SLOT) else {
            continue;
        };
        // Inside Vahn's house: `CC 46 51 62 8E ..` seats her at (98, 14.5).
        if (x >> 7, z >> 7) != (98, 15) {
            continue;
        }
        let y = world.field_npc_render_y(MEI_SLOT, x, z);
        let floor = world.sample_field_floor_height(i32::from(x), i32::from(z));
        assert_eq!(y, floor, "f{f}: Mei hangs at {y} over the floor at {floor}");
        checked += 1;
    }
    assert!(checked > 0, "Mei never sat down in Vahn's house");
}
