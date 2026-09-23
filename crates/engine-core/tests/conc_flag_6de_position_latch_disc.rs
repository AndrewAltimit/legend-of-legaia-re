//! Disc-gated: system flag `0x6DE` in `conc` is a **live player-position
//! latch**, not a progress flag - and the engine's field VM has to re-read
//! the player's seat every frame slice for it to behave the way retail does.
//!
//! What retail does, measured with a flag-byte + helper write watch armed
//! from the memory-card load screen across a cold `conc` entry
//! (`scripts/pcsx-redux/autorun_w5a_flag_watch.lua`, watch on
//! `0x80085833` mask `0x02` plus `FUN_8003CE08` / `FUN_8003CE34` filtered
//! on `a0 == 1758`):
//!
//! 1. the save block's own restore seeds the bank while the load screen is
//!    still up (writer `0x8001A8C8`, inside `FUN_8001A8B0`);
//! 2. the scene load clears `0x6DE` **twice** before the field mode word
//!    settles - once from `P1[1]`'s spawn prologue at `+0x0010` and once
//!    from the `P1[0]` entry script at `+0x0018` (both through
//!    `FUN_8003CE34`, both identified by the VM's own bytecode cursor);
//! 3. from the first field frame on, `P1[0]`'s park loop re-evaluates its
//!    `CD F8` player bounding-box test (`+0x0100`, tiles `10..=51` x
//!    `14..=72`) and issues the `56 DE` SET at `+0x010B` on every pass the
//!    player is **outside** that box.
//!
//! Forcing the player inside the box mid-run stops the SET dead (same
//! capture, `LEGAIA_POKE_XZ`), which is what makes this a position test
//! rather than an entry latch. The `conc_field_card_boot` anchor therefore
//! holds the flag set only because its save stands at tile `(17, 97)`.
//!
//! So the oracle here is two-sided: with the player outside the box the
//! engine must re-arm the flag, and with the player inside it must leave it
//! alone. Reading the anchor position once at scene entry passes the first
//! half only by accident of where the entry point is.
//!
//! REF: FUN_8003CE08, FUN_8003CE34 (bank SET / CLEAR helpers)
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_asset::field_disasm::{FlagKind, InsnInfo, LinearWalker};
use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::scene::{Scene, SceneHost};
use std::path::PathBuf;

/// The Conkram door-group latch.
const FLAG_6DE: u16 = 0x6DE;
/// `P1[0]`'s player bounding box, in tiles (`CD F8 0A 0E 33 48 ...`).
const BOX_TILES: (u8, u8, u8, u8) = (10, 14, 51, 72);
/// The `conc_field_card_boot` anchor's player seat - tile `(17, 97)`,
/// outside the box on Z.
const OUTSIDE_XZ: (i16, i16) = (2274, 12506);
/// Tile `(30, 50)`, inside the box on both axes.
const INSIDE_XZ: (i16, i16) = (30 * 128 + 0x40, 50 * 128 + 0x40);

fn open_host() -> Option<SceneHost> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    SceneHost::open_disc(PathBuf::from(disc)).ok()
}

/// Retail's own tile derivation for the `0x4D` bbox compare
/// (`(world - 0x40) >> 7`).
fn tile(v: i16) -> i32 {
    (i32::from(v) - 0x40) >> 7
}

fn outside_box(x: i16, z: i16) -> bool {
    let (x0, z0, x1, z1) = BOX_TILES;
    let (tx, tz) = (tile(x), tile(z));
    !(tx >= i32::from(x0) && tz >= i32::from(z0) && tx <= i32::from(x1) && tz <= i32::from(z1))
}

/// Seat the player actor (both the move and physics copies the field
/// locomotion pass keeps in step).
fn seat_player(host: &mut SceneHost, x: i16, z: i16) {
    let Some(slot) = host.world.player_actor_slot else {
        panic!("conc entry seats no player actor");
    };
    let a = &mut host.world.actors[slot as usize];
    a.move_state.world_x = x;
    a.move_state.world_z = z;
    a.physics.world_x = x;
    a.physics.world_z = z;
}

/// The static half: the three sites the capture named are in the bytes.
#[test]
fn conc_entry_script_clears_then_rearms_6de_behind_a_player_bbox() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    let scene = Scene::load(&host.index, "conc").expect("load conc");
    let man = scene
        .field_man_payload(&host.index)
        .expect("payload")
        .expect("conc MAN");
    let mf = parse_man(&man).expect("parse conc MAN");

    // P1[0] - the scene-entry system script.
    let (start, pc0) = mf.scene_entry_script(&man).expect("entry script");
    let body = &man[start..];
    let mut clear_at = None;
    let mut set_at = None;
    let mut bbox: Option<(u8, u8, u8, u8, usize)> = None;
    for insn in LinearWalker::new(body, pc0).flatten() {
        match insn.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Clear,
                idx: FLAG_6DE,
                ..
            } if clear_at.is_none() => clear_at = Some(insn.pc),
            InsnInfo::SystemFlag {
                kind: FlagKind::Set,
                idx: FLAG_6DE,
                ..
            } if set_at.is_none() => set_at = Some(insn.pc),
            InsnInfo::BBoxTest {
                x_min,
                z_min,
                x_max,
                z_max,
                skip_target,
                ..
            } if bbox.is_none() && (x_min, z_min, x_max, z_max) == BOX_TILES => {
                bbox = Some((x_min, z_min, x_max, z_max, skip_target));
            }
            _ => {}
        }
    }
    assert_eq!(
        clear_at,
        Some(0x18),
        "conc P1[0] clears 0x6DE in its prologue"
    );
    assert_eq!(set_at, Some(0x10B), "conc P1[0] re-arms 0x6DE at +0x10B");
    let (_, _, _, _, skip_target) = bbox.expect("conc P1[0] carries the player bbox test");
    assert_eq!(
        skip_target,
        set_at.unwrap(),
        "the bbox test's OUTSIDE arm is exactly the 0x6DE set - the flag is a \
         position latch, not a progress latch"
    );

    // P1[1] - the second clear the capture saw, from a spawn prologue.
    let p1_1 = mf
        .actor_placement_record_offset(1, man.len())
        .expect("P1[1] offset");
    let n = man[p1_1] as usize;
    let body1 = &man[p1_1..];
    let pc0_1 = 1 + n * 2 + 4;
    let clear_1 = LinearWalker::new(body1, pc0_1).flatten().find_map(|i| {
        matches!(
            i.info,
            InsnInfo::SystemFlag {
                kind: FlagKind::Clear,
                idx: FLAG_6DE,
                ..
            }
        )
        .then_some(i.pc)
    });
    assert_eq!(
        clear_1,
        Some(0x10),
        "conc P1[1]'s spawn prologue carries the second 0x6DE clear"
    );
}

/// The runtime half: the engine's own field VM has to reproduce the rule.
#[test]
fn engine_conc_entry_tracks_6de_against_the_live_player_seat() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    host.enter_field_scene("conc", 0).expect("enter conc");
    let entry_seat = host
        .world
        .player_actor_slot
        .map(|s| {
            let a = &host.world.actors[s as usize];
            (a.move_state.world_x, a.move_state.world_z)
        })
        .expect("player seated");
    eprintln!(
        "[conc] entry seat ({},{}) tile ({},{}) outside_box={}",
        entry_seat.0,
        entry_seat.1,
        tile(entry_seat.0),
        tile(entry_seat.1),
        outside_box(entry_seat.0, entry_seat.1)
    );

    // Outside the box (the card-boot anchor's own seat): the park loop must
    // re-arm the flag, exactly as the retail capture does every other frame.
    seat_player(&mut host, OUTSIDE_XZ.0, OUTSIDE_XZ.1);
    host.world.system_flag_clear(FLAG_6DE);
    let mut first_set_outside = None;
    for t in 0..240 {
        let _ = host.world.tick();
        if first_set_outside.is_none() && host.world.system_flag_test(FLAG_6DE) {
            first_set_outside = Some(t);
        }
    }
    eprintln!(
        "[conc] outside: first set at tick {first_set_outside:?}; field_ctx=({},{}) pc={:#X}",
        host.world.field_ctx.world_x, host.world.field_ctx.world_z, host.world.field_pc
    );
    assert!(
        outside_box(OUTSIDE_XZ.0, OUTSIDE_XZ.1),
        "fixture seat is the outside-the-box one"
    );
    assert!(
        host.world.system_flag_test(FLAG_6DE),
        "with the player outside tiles {BOX_TILES:?} the entry script's park loop \
         re-arms 0x6DE (retail sets it every other frame from P1[0]+0x10B)"
    );

    // Inside the box: nothing writes the flag at all, so a cleared bank
    // stays cleared.
    seat_player(&mut host, INSIDE_XZ.0, INSIDE_XZ.1);
    host.world.system_flag_clear(FLAG_6DE);
    let mut first_set_inside = None;
    for t in 0..240 {
        let _ = host.world.tick();
        if first_set_inside.is_none() && host.world.system_flag_test(FLAG_6DE) {
            first_set_inside = Some(t);
        }
    }
    eprintln!(
        "[conc] inside: first set at tick {first_set_inside:?}; field_ctx=({},{}) pc={:#X}",
        host.world.field_ctx.world_x, host.world.field_ctx.world_z, host.world.field_pc
    );
    assert!(
        !outside_box(INSIDE_XZ.0, INSIDE_XZ.1),
        "fixture seat is the inside-the-box one"
    );
    assert!(
        !host.world.system_flag_test(FLAG_6DE),
        "with the player inside tiles {BOX_TILES:?} the bbox test takes its \
         fall-through arm and nothing writes 0x6DE"
    );
}
