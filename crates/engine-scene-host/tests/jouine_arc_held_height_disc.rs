//! Disc-gated: `jouine`'s pre-battle beat (`P2[5]`) hops the player twice
//! with the height law up and must leave it standing where the hop lands.
//!
//! Each hop is `B1 F8 1D` (raise the player's `0x20000000`), `C3 F8 00 ..`
//! (a tile-target arc, which never writes `+0x8E`), then `B2 F8 1D` once the
//! arc's watcher releases the record on the landing frame. The player's
//! settle `FUN_801d1ba0` pins `+0x16 = -(+0x8E)` while the bit is up
//! (`0x801D1C14..0x801D1C2C`), so a port that resumes the record a frame
//! after the landing and lets the settle read the bit in between drops the
//! player to `Y = -(+0x8E) = 0`, hundreds of units above the floor.
//!
//! The retail library state `cort_evolved_pre_battle`, taken just after the
//! beat, holds the player at `(2368, 752, 2496)` with the bit clear.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

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

#[test]
fn the_pre_battle_hops_land_the_player_on_the_floor_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    host.enter_field_scene("jouine", 0).expect("enter jouine");
    for _ in 0..4 {
        host.tick().expect("tick");
    }
    // `P2[5]`, global `N0 + N1 + 5` with jouine's counts `[19, 18, 18]`.
    host.world.field_vm.pending_record_spawns.push(19 + 18 + 5);
    let mut hopped = false;
    let mut dropped = Vec::new();
    for i in 0..6000 {
        let pad = if i % 16 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.input.set_pad(pad);
        host.tick().expect("tick");
        let slot = host.world.player_actor_slot.expect("player") as usize;
        let y = host.world.actors[slot].move_state.world_y;
        hopped |= host.world.player_script_arc_live();
        if hopped && y == 0 {
            dropped.push(i);
        }
    }
    let slot = host.world.player_actor_slot.expect("player") as usize;
    let ms = &host.world.actors[slot].move_state;
    eprintln!(
        "[ran] player at ({}, {}, {}), dropped frames {dropped:?}",
        ms.world_x, ms.world_y, ms.world_z
    );
    assert!(hopped, "the beat hops the player");
    assert!(
        dropped.is_empty(),
        "no frame drops the player to Y 0: {dropped:?}"
    );
    assert_eq!(
        (ms.world_x, ms.world_y, ms.world_z),
        (2368, 752, 2496),
        "the player stands where retail's state holds it"
    );
    assert_eq!(ms.flags & 0x2000_0000, 0, "with the height law down");
}
