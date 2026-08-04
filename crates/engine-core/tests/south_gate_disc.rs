//! Disc-gated: **Rim Elm's south gate is a collision gate, not a script gate.**
//!
//! The first scene exit of the game reads, from the outside, like a walk-on
//! trigger that does not fire. It is not. The `.MAP` kind-1 table gives the
//! gate two gate-1 bands and neither is the mechanism people expect:
//!
//! | Record | Tiles | What its script actually is |
//! |---|---|---|
//! | `P2[10]` | `(24..26, 45)`, `(25, 44)` | five bytes: `Nop; Nop; JmpRel`-to-self. No scene change, no walk, no writes. A resident park. |
//! | `P2[0]` | `(24..26, 46)` | `CFlag.Set`, an `Effect` fade, and the `0x3F` naming `map01` at entry tile `(0x60, 0x19)`. Empty C1/C2. |
//!
//! So the exit record is ungated and the *other* record is inert. What keeps a
//! cold boot inside Rim Elm is the collision grid: `.MAP` row 47 walls
//! `z ∈ [5888, 5951]` across the whole doorway. That row is the gate, and it is
//! opened by the gate object's own script.
//!
//! `town01` `P0[20]` is bound to the object at tile `(23, 43)` by the `.MAP`'s
//! gate-0 kind-1 trigger and runs through the scene-init bind prologue
//! (`FUN_8003A55C`, [`World::seed_object_channels`]). It clears the approach
//! band with three `0x4C` nibble-7 sub-0 paints, then branches on system flags
//! `327` and `321`:
//!
//! - `327` clear -> park. The base map's row-47 wall stands. **Shut.**
//! - `327` set, `321` clear -> re-block rows 44..46, seat the gate at `(24, 44)`.
//!   **Shut, one tile further north.**
//! - both set -> `sub-0` over cols `24..25`, rows `46..47`. **The doorway.**
//!
//! Only the last arm clears the cells the walk hits. These tests pin all three
//! states off the real disc grid and then walk a pad through the open one.
//!
//! Structural assertions only (grid bits, tiles, opcodes) - no Sony bytes.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md convention).

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{SceneHost, SceneTickEvent};
use std::path::PathBuf;

/// The two flags `P0[20]` branches on: `327` = the gate scenery exists,
/// `321` = the gate is open.
const GATE_SCENERY: u16 = 327;
const GATE_OPEN: u16 = 321;

/// World X of the tile-25 centre - the middle of the doorway `P0[20]` cuts.
const DOOR_X: i16 = 25 * 128 + 64;
/// World X of the tile-24 centre - the doorway's other half.
const DOOR_X2: i16 = 24 * 128 + 64;
/// World X of the tile-26 centre - the side wall the open arm re-blocks
/// (`sub-1` over cols `26..27`). Keeps the "gate opened" assertions from
/// passing on a blanket clear.
const SIDE_X: i16 = 26 * 128 + 64;

/// A world Z inside the gate plug (grid row 47, even `z_cell`): the 64-unit
/// band `[5888, 5951]` the doorway paint has to remove.
const PLUG_Z: i16 = 5888;
/// A world Z on the approach, north of the plug - open on a cold boot, blocked
/// by the `327`-only arm.
const APPROACH_Z: i16 = 5824;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Open a host and enter `town01` with `flags` already latched, so the
/// scene-init object-bind prologue sees them. Order matters: `P0[20]` runs
/// once, at scene entry.
fn town01_with_flags(flags: &[u16]) -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    for &f in flags {
        host.world.system_flag_set(f);
    }
    host.enter_field_scene("town01", 0).expect("enter town01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    Some(host)
}

/// The three collision states of the gate, read off the grid the locomotion
/// probe actually samples.
#[test]
fn gate_collision_follows_the_two_story_flags() {
    // --- cold boot: the plug stands, the approach is open. ----------------
    let Some(host) = town01_with_flags(&[]) else {
        return;
    };
    for x in [DOOR_X2, DOOR_X, SIDE_X] {
        assert!(
            host.world.field_tile_is_wall(x, PLUG_Z),
            "cold boot: the gate plug must seal x={x} (grid row 47)"
        );
        assert!(
            !host.world.field_tile_is_wall(x, APPROACH_Z),
            "cold boot: the approach at x={x} is cleared by P0[20]'s \
             unconditional sub-0 paints"
        );
    }

    // --- `327` alone: the gate scenery seats and re-blocks the approach. ---
    let Some(host) = town01_with_flags(&[GATE_SCENERY]) else {
        return;
    };
    assert!(
        host.world.field_tile_is_wall(DOOR_X, APPROACH_Z),
        "327 alone takes the closed-gate arm, which blocks the approach band"
    );
    assert!(
        host.world.field_tile_is_wall(DOOR_X, PLUG_Z),
        "327 alone never touches row 47 - the plug still stands"
    );

    // --- both: the doorway is cut, and only the doorway. ------------------
    let Some(host) = town01_with_flags(&[GATE_SCENERY, GATE_OPEN]) else {
        return;
    };
    for x in [DOOR_X2, DOOR_X] {
        assert!(
            !host.world.field_tile_is_wall(x, PLUG_Z),
            "the open-gate arm clears cols 24-25 of row 47 (x={x})"
        );
        assert!(
            !host.world.field_tile_is_wall(x, APPROACH_Z),
            "the approach back into town stays open (x={x})"
        );
    }
    assert!(
        host.world.field_tile_is_wall(SIDE_X, PLUG_Z),
        "the open arm is a doorway, not a blanket clear: col 26 is re-blocked \
         by the same script's sub-1 paint"
    );
}

/// Mirror of the pad inversion in `engine-shell`'s critical-path replay: turn
/// a desired world step into the pad the camera quadrant maps onto it, which
/// is what a player pressing a direction is doing.
fn pad_for_world_step(azimuth: u16, dwx: i16, dwz: i16) -> u16 {
    let quadrant = ((azimuth as u32).wrapping_add(512) / 1024) & 3;
    let (sx, sy) = match quadrant {
        0 => (dwx, dwz),
        1 => (-dwz, dwx),
        2 => (-dwx, -dwz),
        _ => (dwz, -dwx),
    };
    let mut pad = 0u16;
    if sy > 0 {
        pad |= PadButton::Up.mask();
    } else if sy < 0 {
        pad |= PadButton::Down.mask();
    }
    if sx > 0 {
        pad |= PadButton::Right.mask();
    } else if sx < 0 {
        pad |= PadButton::Left.mask();
    }
    pad
}

fn player_z(host: &SceneHost) -> i16 {
    let slot = host.world.player_actor_slot.expect("player actor") as usize;
    host.world.actors[slot].move_state.world_z
}

/// Walking the pad south through the opened gate crosses record 0's band and
/// leaves for `map01`. No seat on the trigger tile - the crossing is produced
/// by locomotion, which is the whole point (the spine oracle's `walk_onto_tile`
/// is a teleport pair and cannot see a sealed doorway).
#[test]
fn pad_walk_through_the_open_gate_leaves_for_map01() {
    let Some(mut host) = town01_with_flags(&[GATE_SCENERY, GATE_OPEN]) else {
        return;
    };
    // Start on the approach, inside the gate corridor and north of the plug.
    // The walk - not this seat - has to cross `z = 5888`.
    {
        let slot = host.world.player_actor_slot.expect("player") as usize;
        let ms = &mut host.world.actors[slot].move_state;
        ms.world_x = DOOR_X;
        ms.world_z = APPROACH_Z;
    }
    let start_z = player_z(&host);
    // Deepest Z observed while still in the field - the transition happens
    // inside the tick that crosses, so this is the last frame before it.
    let mut deepest = start_z;
    let mut entered = None;
    for _ in 0..600 {
        let pad = pad_for_world_step(host.world.field_camera_azimuth, 0, 1);
        host.world.set_pad(pad);
        match host.tick().expect("tick") {
            SceneTickEvent::SceneEntered { name } => {
                entered = Some(name);
                break;
            }
            _ => deepest = deepest.max(player_z(&host)),
        }
    }
    host.world.set_pad(0);
    assert!(
        deepest > start_z,
        "the pad moved the player south at all (start z={start_z}, deepest \
         {deepest}) - without this the transition below could be a dispatch \
         off the seat rather than off a walk"
    );
    assert_eq!(
        entered.as_deref(),
        Some("map01"),
        "record 0's band fires its 0x3F and leaves Rim Elm; the seat is in \
         record 10's band (dispatch tile 45), so reaching record 0's band \
         (tile 46, z >= {PLUG_Z}) means the walk crossed the opened plug"
    );
    assert_eq!(
        host.world.mode,
        legaia_engine_core::world::SceneMode::WorldMap,
        "map01 routes through the world-map entry"
    );
}

/// The same walk with the gate shut goes nowhere: the plug stops the player
/// north of record 0's band, and no transition fires. This is the negative
/// control for the test above - without it, a doorway that was never sealed
/// would pass it just as well.
#[test]
fn the_shut_gate_stops_the_same_walk() {
    let Some(mut host) = town01_with_flags(&[]) else {
        return;
    };
    {
        let slot = host.world.player_actor_slot.expect("player") as usize;
        let ms = &mut host.world.actors[slot].move_state;
        ms.world_x = DOOR_X;
        ms.world_z = APPROACH_Z;
    }
    for _ in 0..600 {
        let pad = pad_for_world_step(host.world.field_camera_azimuth, 0, 1);
        host.world.set_pad(pad);
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("the shut gate must not transition (entered {name})");
        }
    }
    host.world.set_pad(0);
    assert!(
        player_z(&host) < PLUG_Z,
        "the plug holds the player north of z={PLUG_Z}; ended at {}",
        player_z(&host)
    );
}

/// Record 10 is not the door and never was: its whole body is
/// `Nop; Nop; JmpRel`-to-self. Pinned against the disc so the "the walk-on
/// record force-walks the player through the wall and then runs the `0x3F`"
/// reading cannot come back - there is no `0x3F` in it, and nothing to walk.
#[test]
fn record_10_is_a_content_free_park() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town01", 0).expect("enter town01");
    let man = host
        .scene
        .as_ref()
        .expect("town01 scene loaded")
        .field_man_payload(&host.index)
        .expect("town01 MAN read")
        .expect("town01 carries a bundle MAN");
    let man_file = legaia_asset::man_section::parse(&man).expect("parse MAN");

    let decode_ops = |record: usize| -> Vec<u8> {
        let (start, pc0, len) = legaia_engine_core::man_field_scripts::partition_record_span(
            &man_file, &man, 2, record,
        )
        .expect("record span");
        let body = &man[start..start + len];
        let mut ops = Vec::new();
        let mut pc = pc0;
        while pc < body.len() {
            let Ok(insn) = legaia_asset::field_disasm::decode(body, pc) else {
                break;
            };
            if insn.size == 0 {
                break;
            }
            ops.push(body[pc] & 0x7F);
            pc += insn.size;
        }
        ops
    };

    // Record 10: NOP, NOP, JMP_REL and nothing else.
    assert_eq!(
        decode_ops(10),
        vec![0x21, 0x21, 0x26],
        "P2[10] is a resident park, not a door script"
    );

    // Record 0, the same walk: the `0x3F` is here. Without this the assertion
    // above would pass on a MAN we simply failed to slice.
    assert!(
        decode_ops(0).contains(&0x3F),
        "P2[0] carries the named scene change - the band that is the exit"
    );
}
