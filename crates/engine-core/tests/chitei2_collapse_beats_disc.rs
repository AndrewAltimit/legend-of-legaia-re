//! Disc-gated: `chitei2`'s collapse sequence (Jette's Fortress), beat by
//! beat, through the real `SceneHost`.
//!
//! - **The escape platform is floor.** P2[18] (the collapse) leaves the party
//!   on the platform partition-0 record 31's mesh provides; it has no
//!   walk-visible floor bit, but its collision is open and its stairs lead
//!   down onto the corridor, so the stranded-player rescue must not yank the
//!   player to the cold spawn.
//! - **The boulder falls.** Partition-0 records 28..30 are born 700 units up
//!   at a parking tile (`31 1D` + `4C 42 BC 02`); the boulder beat (P2[17],
//!   global `0x60`) seats them at the foot of the stairs (`A3`) and tweens
//!   `+0x8E` to `0` (`4C 42 0 <ticks>`), which the actor tick's `0x20000000`
//!   height law turns into Y. The placed draws follow the actors.
//! - **The pipe opens.** The rescue beat (P2[16], global `0x5F`) releases the
//!   drain pipe's spawn hold and plays its clip 2 once (`AC 01 01`,
//!   `A2 01 02`): the prop's cursor must advance off frame 0.
//! - **Scripted walks follow the floor.** Every frame of a walk-to-tile leg
//!   resolves the player's Y from the floor under the step.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// The story flags the collapse chain stands on (Cort beaten, the generator
/// down, the fortress coming apart), so the beats' record gates pass.
const COLLAPSE_FLAGS: [u16; 5] = [0x4C4, 0x4C5, 0x4C6, 0x471, 0x6A8];

fn chitei2(root: &std::path::Path) -> SceneHost {
    let mut host = SceneHost::open_extracted(root).expect("open SceneHost");
    host.enter_field_scene("chitei2", 0).expect("enter chitei2");
    for f in COLLAPSE_FLAGS {
        host.world.system_flag_set(f);
    }
    host
}

fn player(host: &SceneHost) -> (i16, i16, i16) {
    let slot = host.world.player_actor_slot.expect("player actor") as usize;
    let ms = &host.world.actors[slot].move_state;
    (ms.world_x, ms.world_y, ms.world_z)
}

#[test]
fn the_escape_platform_is_open_floor_or_skip() {
    let Some(root) = extracted_root() else { return };
    let host = chitei2(&root);
    // Where the collapse beat leaves the party: on the platform, south of the
    // escape stairs.
    let (x, z) = (2240i16, 12800i16);
    assert_eq!(
        host.world.field_walk_component_size(x, z),
        0,
        "the platform carries no walk-visible floor bit (else the rescue never looked)"
    );
    assert!(
        host.world.field_collision_reaches_floor(x, z, 1024),
        "[ran] the platform's open collision leads down the stairs onto the corridor floor"
    );
}

#[test]
fn the_boulder_drops_onto_the_escape_path_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut host = chitei2(&root);
    let at_spawn = host.world.object_draw_displacements();
    for rec in [28usize, 29, 30] {
        assert_eq!(
            at_spawn.get(&rec).map(|d| d[1]),
            Some(-700),
            "boulder piece P0[{rec}] is parked 700 units up at spawn: {at_spawn:?}"
        );
    }
    host.world.field_vm.pending_record_spawns.push(0x60);
    for _ in 0..600 {
        host.tick().expect("tick");
    }
    let moved = host.world.object_draw_displacements();
    eprintln!("[ran] boulder displacements after P2[17]: {moved:?}");
    for rec in [28usize, 29, 30] {
        let d = moved
            .get(&rec)
            .unwrap_or_else(|| panic!("boulder piece P0[{rec}] moved: {moved:?}"));
        assert_eq!(d[1], 0, "P0[{rec}] has dropped to the floor: {d:?}");
        assert!(
            d[0] != 0 && d[2] != 0,
            "P0[{rec}] left its parking tile: {d:?}"
        );
    }
}

#[test]
fn the_rescue_beat_opens_the_drain_pipe_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut host = chitei2(&root);
    let pipe = host
        .world
        .props
        .bank
        .props
        .iter()
        .find(|(_, p)| p.record == 1)
        .map(|(&a, _)| a)
        .expect("the drain pipe is a posed prop bound to P0[1]");
    let held = host.world.props.bank.props[&pipe].anim.cursor;
    host.world.system_flag_set(0x4C7);
    host.world.field_vm.pending_record_spawns.push(0x5F);
    let mut max_cursor = held;
    for _ in 0..900 {
        host.tick().expect("tick");
        if let Some(p) = host.world.props.bank.props.get(&pipe) {
            max_cursor = max_cursor.max(p.anim.cursor);
        }
    }
    eprintln!("[ran] pipe cursor: spawn {held}, max {max_cursor}");
    assert_eq!(held, 0, "the pipe is held shut at spawn");
    assert!(
        max_cursor > 16,
        "the pipe's clip played past its first frame"
    );
}

#[test]
fn the_generator_beat_walks_up_the_stairs_on_them_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    host.enter_field_scene("chitei2", 0).expect("enter chitei2");
    // The generator beat (P2[13], global `0x5C`) fires from the walk-on band
    // at the foot of the escape stairs and walks the party up them onto the
    // platform (`C7 F8 11 E2 32`).
    let slot = host.world.player_actor_slot.expect("player actor") as usize;
    let (x0, z0) = (2240i16, 89 * 128 + 64);
    {
        let ms = &mut host.world.actors[slot].move_state;
        ms.world_x = x0;
        ms.world_z = z0;
    }
    let y0 = host
        .world
        .sample_field_floor_height(i32::from(x0), i32::from(z0)) as i16;
    host.world.actors[slot].move_state.world_y = y0;
    host.world.field_vm.pending_record_spawns.push(0x5C);
    let mut worst = 0i32;
    let mut moved = 0;
    let mut climbed = 0;
    let mut prev = player(&host);
    for _ in 0..900 {
        host.tick().expect("tick");
        let p = player(&host);
        if (p.0, p.2) != (prev.0, prev.2) {
            moved += 1;
            let floor = host
                .world
                .sample_field_floor_height(i32::from(p.0), i32::from(p.2));
            if floor != i32::from(y0) {
                climbed += 1;
            }
            worst = worst.max((i32::from(p.1) - floor).abs());
        }
        prev = p;
    }
    eprintln!(
        "[ran] generator-beat walk: {moved} moving frames, {climbed} off the start tier, \
         worst |y - floor| {worst}"
    );
    assert!(climbed > 4, "the beat walks the player up the stair run");
    assert_eq!(worst, 0, "every walked frame stands on the step under it");
}

#[test]
fn the_hologram_panels_go_dark_once_the_generator_is_down_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    host.enter_field_scene("chitei2", 0).expect("enter chitei2");
    // P0[38..39] spawn dark and light only under flag 0x4F0 - a tint
    // that is not the generator's.
    let before = host.world.object_draw_tints();
    eprintln!("[ran] tinted records, generator running: {before:?}");
    assert!(
        (19..=27).all(|r| !before.contains_key(&r)),
        "with the generator running the hologram panels are untinted"
    );
    // Flag 0x4C5 = the generator destroyed. The panels' spawn prologues
    // (P0[19..27]) test it and run `4C 81 00 00 00 00 10 00 00`.
    host.world.system_flag_set(0x4C5);
    host.enter_field_scene("chitei2", 0)
        .expect("re-enter chitei2");
    let tints = host.world.object_draw_tints();
    eprintln!("[ran] tinted records: {tints:?}");
    let mut dark: Vec<usize> = tints
        .iter()
        .filter(|&(_, &t)| t == (0, 0x1000))
        .map(|(&r, _)| r)
        .collect();
    dark.sort_unstable();
    assert!(
        (19..=27).all(|r| dark.contains(&r)),
        "the hologram panels draw black at full blend: {dark:?}"
    );
}
