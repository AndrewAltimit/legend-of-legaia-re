//! Disc-gated: `koin3`'s dance-hall door and its Gold Card doorman.
//!
//! The corridor to the dance hall ends in a door object (`.MAP` bind, flat
//! record 3 = `P0[3]`) with a doorman beside it (placement `P1[2]`, script id
//! `0x26`). The door's record opens on a `SysFlag.Test` pair: with `0x428` or
//! `0x42F` set it swings the door (`2C 07`, `2C 01`, `2B 03`, `31 00`) and
//! lets the player through; with both clear it runs the doorman's block -
//! the doorman steps into the doorway (`A2 26 35`), the player is glided back
//! (`C1 F8 00 C1`), the doorman steps back (`A2 26 33`) - and the door never
//! moves. `0x42F` is raised by the doorman's own talk (`P1[2]`), on Yes to
//! his Gold Card question when the bag holds item `0x9C` (`4C DC 9C ..`); the
//! block arm carries no text.
//!
//! The prop runner stepped every cross-context op aimed at a placement on the
//! door's own context, so the doorman's clip words (`AC 26 01` drops the
//! hold, `AC 26 03` the clamp) drove the door's clip: without the card the
//! door swung open and looped its swing for the rest of the visit while the
//! doorman stood still, and each further push into the doorway replayed it.
//!
//! REF: FUN_8003C83C, FUN_800204F8, FUN_801D5B5C
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

/// The door's footprint-anchor tile (the bank key of `P0[3]`'s prop).
const DOOR: (u8, u8) = (47, 95);
/// The doorman, placement 2.
const DOORMAN: u8 = 2;
/// The Gold Card.
const GOLD_CARD: u8 = 0x9C;
/// The flag the doorman's talk raises once he has seen the card.
const CARD_SHOWN: u16 = 0x42F;
/// `kor3`'s walk-on tile whose record (`P2[5]`) leads into the `koin3`
/// corridor at `(47, 89)`.
const KOR3_CORRIDOR_DOOR: (i16, i16) = (15, 31);
/// World Z past the doorway: the dance hall.
const HALL_Z: i16 = 12_400;

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

fn seat(host: &mut SceneHost, x: i16, z: i16) {
    let s = host.world.player_actor_slot.expect("player installed") as usize;
    host.world.actors[s].move_state.world_x = x;
    host.world.actors[s].move_state.world_z = z;
}

fn player_z(host: &SceneHost) -> i16 {
    let s = host.world.player_actor_slot.expect("player installed") as usize;
    host.world.actors[s].move_state.world_z
}

/// Walk from `kor3` into the `koin3` corridor the way the game does: the
/// corridor half's placements (the doorman among them) only stay live for a
/// player who arrives outside the north room's box (`P1[0]`'s
/// `CD F8 26 19 37 2C`).
fn arrive_in_corridor(host: &mut SceneHost) {
    host.world.toggles.use_vm_dialogue = true;
    host.enter_field_scene("kor3", 0).expect("enter kor3");
    for _ in 0..60 {
        host.tick().expect("tick");
    }
    let (tx, tz) = KOR3_CORRIDOR_DOOR;
    seat(host, tx * 128 + 64, tz * 128 + 64);
    for _ in 0..400 {
        host.tick().expect("tick");
        if host.scene.as_ref().is_some_and(|s| s.name == "koin3") {
            break;
        }
    }
    assert_eq!(
        host.scene.as_ref().map(|s| s.name.as_str()),
        Some("koin3"),
        "kor3 P2[5] leads into the koin3 corridor"
    );
    for _ in 0..120 {
        host.tick().expect("tick");
    }
    assert!(
        host.world
            .field_vm
            .channels
            .iter()
            .any(|c| !c.object_bind && c.placement_index == usize::from(DOORMAN)),
        "the doorman is live in the corridor half"
    );
}

/// Stand south of the doorman, face him and page one talk with Cross (the
/// picker opens on Yes), until the box closes again.
fn talk_to_doorman(host: &mut SceneHost) {
    seat(host, 6080, 11900);
    let mut opened = false;
    for f in 0..900u32 {
        let talking = host.world.dialog.inline.is_some();
        opened |= host
            .world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|d| d.npc_slot == Some(DOORMAN));
        if opened && !talking {
            break;
        }
        let pad = if f < 3 {
            PadButton::Up.mask()
        } else if f % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        host.tick().expect("tick");
    }
    assert!(opened, "the doorman talks");
    host.world.set_pad(0);
    for _ in 0..60 {
        host.tick().expect("tick");
    }
    assert!(!host.world.dialogue_owns_input(), "the talk has ended");
}

struct DoorRun {
    /// Distinct door clip-control words seen over the run.
    door_flags: Vec<u16>,
    /// Whether the door's clip cursor ever moved.
    door_moved: bool,
    /// The door's collision-off bit at the end.
    door_open: bool,
    /// Whether the doorman's clip was bound to one of his step clips.
    doorman_stepped: bool,
    /// The furthest Z the player reached.
    max_z: i16,
}

/// Hold Up into the doorway from the corridor, then let go and watch.
fn push_into_door(host: &mut SceneHost) -> DoorRun {
    seat(host, 6176, 11800);
    let mut run = DoorRun {
        door_flags: Vec::new(),
        door_moved: false,
        door_open: false,
        doorman_stepped: false,
        max_z: i16::MIN,
    };
    for f in 0..900u32 {
        host.world
            .set_pad(if f < 600 { PadButton::Up.mask() } else { 0 });
        host.tick().expect("tick");
        let door = &host.world.props.bank.props[&DOOR];
        if !run.door_flags.contains(&door.anim.flags) {
            run.door_flags.push(door.anim.flags);
        }
        run.door_moved |= door.anim.cursor != 0;
        run.door_open = door.cflags & 3 != 0;
        run.doorman_stepped |= host
            .world
            .npcs
            .clip_cursors
            .get(&DOORMAN)
            .is_some_and(|c| matches!(c.anim_id, 51..=53));
        run.max_z = run.max_z.max(player_z(host));
    }
    host.world.set_pad(0);
    run
}

#[test]
fn without_the_card_the_doorman_blocks_and_the_door_stays_shut() {
    let Some(mut host) = open_host() else {
        return;
    };
    arrive_in_corridor(&mut host);
    assert!(!host.world.system_flag_test(CARD_SHOWN));
    let shut_flags = host.world.props.bank.props[&DOOR].anim.flags;
    let run = push_into_door(&mut host);
    eprintln!(
        "[koin3 no card] door flags {:#x?} moved {} open {} doorman stepped {} max z {}",
        run.door_flags, run.door_moved, run.door_open, run.doorman_stepped, run.max_z
    );
    assert_eq!(
        run.door_flags,
        vec![shut_flags],
        "the doorman's clip words never reach the door"
    );
    assert!(!run.door_moved, "the door never swings");
    assert!(!run.door_open, "the door stays solid");
    assert!(run.doorman_stepped, "the doorman plays his step clips");
    assert!(run.max_z < HALL_Z, "the player stays in the corridor");

    // His talk without the card turns the player away and raises nothing.
    talk_to_doorman(&mut host);
    assert!(!host.world.system_flag_test(CARD_SHOWN), "no card, no pass");
}

#[test]
fn with_the_card_the_doorman_lets_the_player_in() {
    let Some(mut host) = open_host() else {
        return;
    };
    arrive_in_corridor(&mut host);
    host.world.party.inventory.insert(GOLD_CARD, 1);
    talk_to_doorman(&mut host);
    assert!(
        host.world.system_flag_test(CARD_SHOWN),
        "Yes with the card in the bag raises 0x42F"
    );
    let run = push_into_door(&mut host);
    eprintln!(
        "[koin3 card] door flags {:#x?} moved {} open {} doorman stepped {} max z {}",
        run.door_flags, run.door_moved, run.door_open, run.doorman_stepped, run.max_z
    );
    assert!(run.door_moved && run.door_open, "the door swings open");
    let door = &host.world.props.bank.props[&DOOR];
    assert!(
        door.anim.flags & legaia_engine_scene::field_env::ANIM_CLAMP != 0,
        "the swing clamps on its last frame rather than looping"
    );
    assert!(run.doorman_stepped, "the doorman steps aside");
    assert!(run.max_z >= HALL_Z, "the player walks into the dance hall");
}
