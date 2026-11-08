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
//!   than ending on them, so the screen comes back. The tour parks the
//!   villagers around the map in the hide box (`CC <id> 51 7F 7F ..`) and
//!   re-seats them (`CC <id> C3`) on the way out.
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
/// The actors the panorama parks while it tours (`+0x14E..+0x17F`) and
/// re-seats behind it (`+0x409..+0x41E`), by their `+0x50` ids.
const BLACKBOARD_PARKED: [u8; 7] = [0x31, 0x32, 0x41, 0x42, 0x33, 0x34, 0x35];
/// `(127, 127)`'s tile centre: the off-map hide box.
const HIDE_BOX: (i16, i16) = (0x3FC0, 0x3FC0);
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
    let slots: Vec<u8> = {
        let world = &w.session().host.world;
        let view = world.channel_view();
        BLACKBOARD_PARKED
            .iter()
            .filter_map(|&t| legaia_engine_field::field_channels::resolve_target(view, t))
            .filter_map(|ci| u8::try_from(view[ci].placement_index).ok())
            .collect()
    };
    assert!(
        !slots.is_empty(),
        "no villager the panorama parks is placed"
    );
    let parked = |w: &HeadlessPlayWindow| {
        slots
            .iter()
            .filter(|s| w.session().host.world.npcs.positions.get(s) == Some(&HIDE_BOX))
            .count()
    };
    let mut hid_all = false;
    let mut overview = false;
    let mut picked: Option<u32> = None;
    let mut ended = false;
    for f in 0..3000u32 {
        if overview && parked(&w) == slots.len() {
            hid_all = true;
        }
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
    assert!(
        hid_all,
        "the tour never parked the villagers {slots:?} in the hide box"
    );
    assert_eq!(parked(&w), 0, "the tour left villagers in the hide box");
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

/// Tetsu's placement (`P1[10]`).
const TETSU_SLOT: u8 = 10;

/// Talk to Tetsu and take answer `choice` of his four, then play out what it
/// starts - the spar included, fought with the ladder's battle hand - until
/// the pad is back.
fn ask_tetsu(w: &mut HeadlessPlayWindow, log: &mut PadLog, choice: usize) {
    assert!(seat_facing(w, TETSU_SLOT), "no seat faces Tetsu");
    log.frame(w, mask(PadButton::Cross));
    let mut chose = false;
    let mut since = 0u32;
    let mut settled = 0;
    for f in 0..20_000u32 {
        let world = &w.session().host.world;
        if std::env::var_os("TETSU_TRACE").is_some() && f % 100 == 0 {
            eprintln!(
                "[tetsu] f{f} mode {:?} inline {:?} free {} 0x20B {} tl {} {:?}",
                world.mode,
                world
                    .dialog
                    .inline
                    .as_ref()
                    .map(|i| (i.pc, i.panel.is_some())),
                free_roam(w),
                world.system_flag_test(0x20B),
                world.cutscene_timeline_active(),
                world.cutscene.timeline.as_ref().map(|t| (
                    t.pc,
                    t.bytecode.get(t.pc).copied(),
                    t.dialog.is_some(),
                    t.player_wait,
                    t.channel_wait.map(|c| (c.target_id, c.bit))
                ))
            );
        }
        if world.mode == legaia_engine_mode::mode::SceneMode::Battle {
            let pad = super::full_game_ladder::fight_turn::fight_pad(w.session());
            log.frame(w, if f % 4 < 2 { pad } else { 0 });
            continue;
        }
        if free_roam(w) && f > 30 {
            settled += 1;
            if settled > 300 {
                return;
            }
            log.frame(w, 0);
            continue;
        }
        settled = 0;
        // The talk's box, or - after the spar, whose return resumes the talk
        // as a cutscene record - the timeline's.
        let picker = world
            .dialog
            .inline
            .as_ref()
            .and_then(|i| i.panel.as_ref())
            .or_else(|| {
                world
                    .cutscene
                    .timeline
                    .as_ref()
                    .and_then(|t| t.dialog.as_ref())
            })
            .and_then(|p| {
                p.picker_takes_input()
                    .then(|| (p.picker_cursor(), p.picker_labels().len()))
            });
        let pad = match picker {
            Some((cursor, _)) if !chose => {
                since += 1;
                if !since.is_multiple_of(30) {
                    0
                } else if cursor < choice {
                    mask(PadButton::Down)
                } else {
                    chose = true;
                    mask(PadButton::Cross)
                }
            }
            // After the answer, Tetsu's own four-way picker is closed on its
            // last row, "Nothing in particular"; any other on its default.
            Some((cursor, rows)) => {
                since += 1;
                if !since.is_multiple_of(30) {
                    0
                } else if rows == 4 && cursor < 3 {
                    mask(PadButton::Down)
                } else {
                    mask(PadButton::Cross)
                }
            }
            None => {
                since = 0;
                if f % 8 < 2 { mask(PadButton::Cross) } else { 0 }
            }
        };
        log.frame(w, pad);
    }
    panic!("Tetsu's talk never handed the pad back");
}

/// The order a player can take that retail never recovers from: the spar
/// with Tetsu after Mei's beat and before the measuring, then the measuring,
/// then Tetsu's "Nothing in particular". Returns whether the hunters came
/// home (`0x22E`).
fn spar_before_the_measuring(disc: &std::path::Path, fix: bool) -> bool {
    let (mut w, mut log) = new_game(disc, if fix { "spar-fix" } else { "spar-retail" });
    w.session_mut().host.world.toggles.tetsu_spar_fix = fix;
    play_mei_beat(&mut w, &mut log);
    ask_tetsu(&mut w, &mut log, 2);
    let f = |w: &HeadlessPlayWindow, n| w.session().host.world.system_flag_test(n);
    assert!(f(&w, 0x20B), "the spar was not fought");
    assert!(
        !f(&w, 0x20C),
        "a spar before the measuring does not count in retail's bank"
    );
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
                .install_gated_p2_record(&mf, &man, MEASURING_RECORD)
        );
    }
    assert!(
        play_out(&mut w, &mut log, 6000),
        "the measuring never ended"
    );
    assert!(f(&w, 0x227), "the measuring did not raise 0x227");
    // "Nothing in particular" twice: what a stuck player would try.
    ask_tetsu(&mut w, &mut log, 3);
    ask_tetsu(&mut w, &mut log, 3);
    f(&w, 0x22E)
}

/// Retail: a spar fought before the measuring never counts, and the hunters'
/// return can never start. With the Tetsu spar order fix (the default) it
/// starts on Tetsu's next "Nothing in particular".
#[test]
fn tetsu_spar_before_the_measuring() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(
        !spar_before_the_measuring(&disc, false),
        "retail order: the hunters returned without the fix"
    );
    assert!(
        spar_before_the_measuring(&disc, true),
        "with the fix the hunters never returned"
    );
}
