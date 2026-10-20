//! Disc-gated: two field props that a spawn prologue seats away from where
//! their raw placement puts them, and that the port drew at the raw seat.
//!
//! - **`rayman`'s plank bridges** (`P1[70..=83]`, models `9` / `61`, row 10)
//!   open `31 1D` + `4C 42 40 01`: actor flag `0x20000000` and `+0x8E = 320`,
//!   so the actor driver's height arm (`FUN_8003BC08`,
//!   `0x8003BC50..0x8003BC64`) holds them at `Y = -320` and the planks' tops
//!   (object-local `-64`) sit level with the ledges they span (`-384`). No
//!   script moves them, so they never entered the port's live-position table,
//!   and the height arm - which walked only that table - snapped them to the
//!   floor sample 64 units too high.
//! - **`conc`'s glass-tube floor** (`P0[17]`, `.MAP` record 365 at cell
//!   `(31, 31)`) opens `37 02 01` in its bind-time prologue: a one-tile compass
//!   walk along `-X` on its own actor. The `conc_field_card_boot` retail state
//!   holds that actor at `(3872, -288, 4032)` and its twin `P0[14]` (record 364)
//!   at `X = 11168`, both one tile west of their `.MAP` seat; the port parked
//!   the walk and never ran it, so the disc stood half outside the tube.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn open_host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    };
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    SceneHost::open_disc(PathBuf::from(disc)).ok()
}

fn enter(scene: &str, frames: usize) -> Option<SceneHost> {
    let mut host = open_host()?;
    host.world.stage_picker_entry(scene, false);
    host.enter_field_scene(scene, 0).expect("enter field scene");
    for _ in 0..frames {
        host.tick().expect("tick");
    }
    Some(host)
}

#[test]
fn rayman_plank_bridges_hold_their_scripted_height_or_skip() {
    let Some(host) = enter("rayman", 30) else {
        return;
    };
    let placements = host
        .scene
        .as_ref()
        .expect("scene")
        .field_actor_placements(&host.index)
        .expect("placements")
        .expect("MAN");
    for slot in 70u8..=83 {
        let p = placements
            .iter()
            .find(|a| a.index == usize::from(slot))
            .expect("bridge placement");
        assert!(
            !host.world.npcs.positions.contains_key(&slot),
            "P1[{slot}] is a never-moved prop"
        );
        let (flags, height) = host.world.field_channel_flags_and_8e(slot);
        assert_ne!(
            flags & 0x2000_0000,
            0,
            "P1[{slot}] raises the held-height bit"
        );
        assert_eq!(height, 320, "P1[{slot}] holds +0x8E at 320");
        let pose = host
            .world
            .field_npc_draw_pose(slot, (p.world_x, p.world_z))
            .expect("drawn");
        assert_eq!(pose.pos[1], -320.0, "P1[{slot}] draws at -(+0x8E)");
    }
    eprintln!("[ran] rayman P1[70..=83] held at Y -320");
}

#[test]
fn conc_tube_floor_walks_one_tile_west_or_skip() {
    let Some(host) = enter("conc", 30) else {
        return;
    };
    let moves = host.world.object_draw_displacements();
    // Retail `conc_field_card_boot`: P0[17] at X 3872 (seat 4000), P0[14]
    // at X 11168 (seat 11296).
    assert_eq!(moves.get(&17), Some(&[-128, 0, 0]), "the tube floor disc");
    assert_eq!(moves.get(&14), Some(&[-128, 0, 0]), "its twin");
    for record in [14u16, 17] {
        let c = host
            .world
            .field_vm
            .channels
            .iter()
            .find(|c| c.object_bind && c.ctx.script_id == record)
            .expect("object channel");
        assert_eq!(c.ctx.flags & 0x400, 0, "P0[{record}]'s walk landed");
    }
    eprintln!("[ran] conc P0[14] / P0[17] walked to their retail seats");
}
