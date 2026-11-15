//! Disc-gated: `bylon`'s West Voz door and the monk who guards it.
//!
//! The hall door is placed object 19 (`P0[19]`, anchor `(23, 117)`), and
//! the monk beside it is placement 33 (`P1[33]`, flat id `0x44`). While
//! `0x2A0` is clear a push into the door does not open it: its record
//! engages the monk (`B1 44 08`) and ends (`21`), so the monk's talk is the
//! door's answer. Before Maya's word (`0x29E`, raised by `P1[2]`) he turns
//! the party away. After it his talk tests the door's clip cursor
//! (`CC 13 8C 06 00`) and, the door standing shut on frame 0, skips to the
//! opening: `52 A0`, the door's swing (`AC 13 07`, `AC 13 01`, `AB 13 03`,
//! `B1 13 00`), the fades, and a wait on `P0[15]`'s end latch
//! (`AD 0F 08`). With `0x2A0` up the door's own arm plays the same swing on
//! every later push.
//!
//! The talk runner stepped those object ops on the monk's own context. The
//! cursor test read the monk's idle clip, which sits off frame 0 most of the
//! time, so the talk ended at its `21` without `0x2A0` and every push into
//! the door played the monk's lines again. When the test did read frame 0,
//! the `B1 13 00` dropped the door's collision through the channel poke but
//! the clip words never reached it: the door stood shut, the player walked
//! through it, and the talk parked on a latch nothing raised.
//!
//! REF: FUN_801DE840 (0x801DE894..0x801DE8B4, 0x801E23EC), FUN_8003C83C, FUN_801D5B5C
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_engine_scene::field_env::{ANIM_CLAMP, ANIM_END, PropAnim};
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

/// The hall door's anchor tile (`P0[19]`).
const DOOR: (u8, u8) = (23, 117);
/// The monk, placement 33.
const MONK: u8 = 33;
/// Maya's word: the monk may let the party through.
const PERMISSION: usize = 0x29E;
/// Raised by the monk's opening; the door's own arm takes over.
const DOOR_OPEN: u16 = 0x2A0;
/// The player's Z against the shut door (its footprint box stops the walk).
const AGAINST_DOOR_Z: i16 = 14_898;

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = std::path::PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    SceneHost::open_disc(std::path::PathBuf::from(disc)).ok()
}

fn set_flag(host: &mut SceneHost, f: usize) {
    let flags = &mut host.world.flags.system_flags;
    if flags.len() <= f / 8 {
        flags.resize(f / 8 + 1, 0);
    }
    flags[f / 8] |= 0x80 >> (f % 8);
}

/// Enter `bylon` with Biron's first-visit talks played (`0x297..=0x29D`),
/// and Maya's word too when `permission`, the monk's idle clip on cursor
/// `monk_cursor`.
fn enter(permission: bool, monk_cursor: i16) -> Option<SceneHost> {
    let mut host = open_host()?;
    for f in 0x297..=0x29D {
        set_flag(&mut host, f);
    }
    if permission {
        set_flag(&mut host, PERMISSION);
    }
    host.world.toggles.use_vm_dialogue = true;
    host.enter_field_scene("bylon", 0).expect("enter bylon");
    for _ in 0..90 {
        host.tick().expect("tick");
    }
    assert!(
        host.world.props.bank.props.contains_key(&DOOR),
        "P0[19] is banked at {DOOR:?}"
    );
    assert!(
        host.world.npcs.positions.contains_key(&MONK),
        "the monk stands by the door"
    );
    // A play host binds every NPC's clip and ticks it, so the monk's idle
    // loop is off frame 0 on nearly every frame (and on it now and then);
    // the headless host binds none. Seat the cursor a play host shows.
    let mut idle = PropAnim::cross_context(20, 40);
    idle.cursor = monk_cursor;
    host.world.npcs.clip_cursors.insert(MONK, idle);
    Some(host)
}

struct Push {
    /// Monk talks that opened a box.
    monk_talks: u32,
    /// Whether the door's clip cursor ever moved.
    door_swung: bool,
    /// Ticks the door stood collision-exempt with its clip still on frame 0.
    open_unswung: u32,
    /// The furthest Z the player reached in `bylon`'s hall.
    max_z: i16,
}

/// Stand in front of the door, hold Up into it for `frames` ticks and page
/// every box with Cross.
fn push_into_door(host: &mut SceneHost, frames: u32) -> Push {
    let s = usize::from(host.world.player_actor_slot.expect("player"));
    host.world.actors[s].move_state.world_x = 3008;
    host.world.actors[s].move_state.world_z = 14_850;
    let mut run = Push {
        monk_talks: 0,
        door_swung: false,
        open_unswung: 0,
        max_z: i16::MIN,
    };
    let mut was_monk = false;
    for f in 0..frames {
        let pad = if host.world.dialogue_owns_input() {
            if f % 6 == 0 {
                PadButton::Cross.mask()
            } else {
                0
            }
        } else {
            PadButton::Up.mask()
        };
        host.world.set_pad(pad);
        host.tick().expect("tick");
        let monk = host
            .world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|d| d.npc_slot == Some(MONK) && d.panel.is_some());
        if monk && !was_monk {
            run.monk_talks += 1;
        }
        was_monk = monk;
        let door = &host.world.props.bank.props[&DOOR];
        run.door_swung |= door.anim.cursor != 0;
        if door.collision_exempt() && door.anim.cursor == 0 {
            run.open_unswung += 1;
        }
        let z = host.world.actors[s].move_state.world_z;
        // The doorway's far side warps the player to the next room
        // (`P0[32]`, `A3 F8 ..`); count the hall only.
        if (14_000..16_000).contains(&z) {
            run.max_z = run.max_z.max(z);
        }
    }
    host.world.set_pad(0);
    run
}

/// After Maya's word, a push into the door plays the monk's lines once and
/// his talk opens the door: the next pushes find it open, not him. Off
/// frame 0 the monk's clip once ended the talk early (the spam); on it, the
/// door dropped its collision without swinging.
#[test]
fn the_monk_speaks_once_and_opens_the_door() {
    for monk_cursor in [0x90, 0] {
        monk_opens_the_door(monk_cursor);
    }
}

fn monk_opens_the_door(monk_cursor: i16) {
    let Some(mut host) = enter(true, monk_cursor) else {
        return;
    };
    let run = push_into_door(&mut host, 900);
    let door = &host.world.props.bank.props[&DOOR];
    eprintln!(
        "[bylon permission, monk cursor {monk_cursor}] monk talks {} door swung {} cursor {} flags {:#x} open-unswung {} max z {} 0x2A0 {}",
        run.monk_talks,
        run.door_swung,
        door.anim.cursor,
        door.anim.flags,
        run.open_unswung,
        run.max_z,
        host.world.system_flag_test(DOOR_OPEN),
    );
    assert_eq!(
        run.monk_talks, 1,
        "the monk's lines play once: his talk raises 0x2A0, and the door's own arm answers every later push"
    );
    assert!(
        host.world.system_flag_test(DOOR_OPEN),
        "CC 13 8C reads the door on frame 0 and skips to the opening (52 A0)"
    );
    assert!(run.door_swung, "the monk's AB / AC 13 ops swing the door");
    assert!(
        door.anim.flags & ANIM_END != 0 && door.anim.flags & ANIM_CLAMP != 0,
        "the swing clamps on its last frame and latches its end"
    );
    // `B1 13 00` drops the collision in the op's own frame and the swing's
    // first step lands on the next clip tick.
    assert!(
        run.open_unswung <= 2,
        "the door never stands open-but-shut: {} ticks",
        run.open_unswung
    );
    assert!(
        !host.world.dialogue_owns_input(),
        "the talk runs past AD 0F 08 to its 21 and lets the player go"
    );
    assert!(
        run.max_z > AGAINST_DOOR_Z + 64,
        "the player walks through the open doorway (max z {})",
        run.max_z
    );
}

/// Before Maya's word the door stays shut and solid: every push is the
/// monk's refusal, and nothing raises `0x2A0`.
#[test]
fn without_permission_the_door_stays_shut() {
    let Some(mut host) = enter(false, 0x90) else {
        return;
    };
    let run = push_into_door(&mut host, 600);
    let door = &host.world.props.bank.props[&DOOR];
    eprintln!(
        "[bylon no permission] monk talks {} door swung {} cflags {:#x} max z {}",
        run.monk_talks, run.door_swung, door.cflags, run.max_z
    );
    assert!(run.monk_talks >= 1, "the push engages the monk (B1 44 08)");
    assert!(!run.door_swung, "the door never swings");
    assert!(!door.collision_exempt(), "the door stays solid");
    assert!(
        !host.world.system_flag_test(DOOR_OPEN),
        "the refusal raises nothing"
    );
    assert!(
        run.max_z <= AGAINST_DOOR_Z + 8,
        "the player stays on the hall side (max z {})",
        run.max_z
    );
}
