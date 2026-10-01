//! Disc-gated: on a scene whose script animates the floor-height ladder, the
//! **drawn** ground and the **walked** ground are one surface, and a player
//! standing on it rides it.
//!
//! `jouina` (Bio Castle A) arms a travelling wave across rungs `2..=13` at
//! scene entry (op `0x4C 0x90` per rung). Retail's ground pass (PROT 0900
//! `FUN_801F6D48`) takes each cell's corner tiers through the live ladder
//! every frame, and its floor sampler `FUN_80019278` reads the same ladder, so
//! the path pulses and the player moves with it. The port used to bake the
//! ground once (a frozen path the player sank into or floated over), and its
//! snap-style footing only re-read the floor on a committed step.
//!
//! Skips without `LEGAIA_DISC_BIN` / `extracted/` (disc-gated convention).

use std::path::PathBuf;

use legaia_engine_core::field_ground;
use legaia_engine_core::scene::SceneHost;

const SCENE: &str = "jouina";

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    eprintln!("[skip] extracted/ missing");
    None
}

#[test]
fn the_drawn_ground_is_the_walked_ground_and_the_player_rides_it() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene(SCENE, 0).expect("enter");
    host.world.locomotion.follow_terrain_height = true;
    let hf = host
        .scene
        .as_ref()
        .unwrap()
        .walk_heightfield(&host.index)
        .unwrap()
        .expect("jouina has a walk ground");
    let baked = field_ground::render_positions(&hf);

    // A walkable tile whose 2x2 corner block is all on one animated rung, so
    // the sampler's bilinear surface is flat there and equals the corner Y.
    let g = host.world.terrain.collision_grid.clone();
    let (col, row) = (1..127usize)
        .flat_map(|r| (1..127usize).map(move |c| (c, r)))
        .find(|&(c, r)| {
            let t = g[r * 128 + c] & 0x0F;
            (2..=13).contains(&t)
                && [
                    g[r * 128 + c],
                    g[r * 128 + c + 1],
                    g[(r + 1) * 128 + c],
                    g[(r + 1) * 128 + c + 1],
                ]
                .iter()
                .all(|&b| b & 0xF0 == 0 && b & 0x0F == t)
        })
        .expect("a walkable cell on an animated rung");
    let (x, z) = ((col * 128 + 128) as i16, (row * 128 + 128) as i16);
    let slot = usize::from(host.world.player_actor_slot.expect("player"));
    host.world.actors[slot].move_state.world_x = x;
    host.world.actors[slot].move_state.world_z = z;
    // Seated standing on the floor there, as a walk onto the tile leaves it.
    host.world.actors[slot].move_state.world_y =
        host.world
            .sample_field_floor_height(i32::from(x), i32::from(z)) as i16;
    // The ground vertex at that tile corner.
    let vi = hf
        .positions
        .iter()
        .position(|p| p[0] == f32::from(x) && p[2] == f32::from(z))
        .expect("a ground vertex at the tile corner");

    let sink = legaia_engine_core::coplanar_draws::GROUND_SINK;
    let mut ground_moved = false;
    let mut player_moved = false;
    let y0 = host.world.actors[slot].move_state.world_y;
    for f in 0..180 {
        let _ = host.world.tick();
        let live = field_ground::live_render_positions(&hf, &host.world.terrain.floor_height_lut);
        ground_moved |= live[vi][1] != baked[vi][1];
        let floor = host
            .world
            .sample_field_floor_height(i32::from(x), i32::from(z));
        assert_eq!(
            live[vi][1] - sink,
            floor as f32,
            "the drawn ground is the sampled floor (f{f})"
        );
        let y = host.world.actors[slot].move_state.world_y;
        player_moved |= y != y0;
        assert_eq!(
            i32::from(y),
            floor,
            "the standing player rides the floor (f{f})"
        );
    }
    assert!(ground_moved, "the path pulses");
    assert!(player_moved, "and carries the player with it");
}
