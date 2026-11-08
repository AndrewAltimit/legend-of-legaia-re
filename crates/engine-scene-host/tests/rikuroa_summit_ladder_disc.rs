//! Disc-gated: a floor-following placed object rides the ladder a script
//! installs after the scene came up.
//!
//! `rikuroa`'s system script (`P1[0]`) installs a ladder per zone of the
//! mountain with `4C 9E`. The arm the summit takes (system flag `0x19E`,
//! region-type bit 3) lifts rung 8 from the MAN header's `-256` to `-920`, so
//! the cave mouth the party leaves `rikuroa2` through stands `664` units above
//! where the header puts it. The cliff that carries the mouth is a placed
//! object bound to `P0[0]`, whose prologue raises the floor-follow class bit
//! (`31 11`), so the field actor tick `FUN_8003BC08` re-samples its Y every
//! tick (`FUN_80019278`) through the live ladder.
//!
//! The retail capture of that arrival (`rikuroa2_field_door_hop` with the
//! door hop played back out to `rikuroa`, `w34b_scene_rikuroa_v488`) holds
//! the ladder below, the player at `(8384, -1688, 4672)`, the cave-mouth
//! actor at `(8768, -920, 4800)` and the two summit chests at `-1912` /
//! `-2296` on the kind-2 tiles beside it.
//!
//! The port baked a follower at the floor under it once, when the host built
//! its placed layer, before the script installed the ladder: the mouth stood
//! at `-256` while the player and the chests stood on the live floor, which
//! drew both `664` units above the ledge.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_scene::field_env;
use legaia_engine_scene::scene::Scene;
use legaia_engine_scene_host::scene_host::SceneHost;

/// The retail capture's live ladder (scratchpad `0x1F80035C`).
const SUMMIT_LADDER: [i16; 16] = [
    0, -32, -600, -96, -760, -2040, -192, -224, -920, -288, -320, -352, -384, -416, -448, -480,
];

/// Where the arrival from `rikuroa2` lands (`rikuroa2` P2[56], tile
/// `(65, 36)`), in world units.
const ARRIVAL: (i16, i16) = (8384, 4672);

/// The cave-mouth cliff's actor position in the capture.
const CAVE_MOUTH: (i32, i32, i32) = (8768, -920, 4800);

#[test]
fn the_cave_mouth_rides_the_summit_ladder() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    // Past the scene's opening beat (`P1[0]` parks on `44 55` while
    // `0x2FB` is clear), so the system script reaches its zone loop.
    host.world.system_flag_set(0x2FB);
    host.set_entry_seat_tile(65, 36);
    host.enter_field_scene("rikuroa", 0).expect("enter rikuroa");
    assert!(host.debug_seat_standing(ARRIVAL.0, ARRIVAL.1));
    for _ in 0..120 {
        host.tick().expect("tick");
    }

    assert_eq!(
        host.world.terrain.floor_height_lut, SUMMIT_LADDER,
        "P1[0]'s summit arm installs the captured ladder"
    );
    let ms = &host.world.actors[0].move_state;
    assert_eq!((ms.world_x, ms.world_y, ms.world_z), (8384, -1688, 4672));
    for (slot, y) in [(27u8, -1912), (28, -2296)] {
        let (x, z) = host.world.npcs.positions[&slot];
        assert_eq!(
            host.world.field_npc_render_y(slot, x, z),
            y,
            "chest P1[{slot}]"
        );
    }

    // Compose the placed layer the way both play hosts do: bake, re-seat the
    // floor followers, then this frame's displacement.
    let index = host.index.clone();
    let scene = Scene::load(&index, "rikuroa").expect("load rikuroa");
    let lut = scene.field_floor_height_lut(&index).expect("lut");
    let placements = scene
        .field_object_placements(&index)
        .expect("placements")
        .expect("rikuroa has a field map");
    let binds = scene
        .field_object_binds(&index)
        .expect("binds")
        .expect("rikuroa binds its objects");
    let env_tmds: Vec<usize> = (0..=u8::MAX as usize).collect();
    let (mut draws, _) =
        field_env::resolve_placed_env_draws(&env_tmds, &placements, lut, Some(&binds));
    let follow = host.world.object_floor_follow_records();
    field_env::follow_floor_placed_draws(&mut draws, &binds, |r, x, z| {
        host.world.object_floor_follow_y(&follow, r, x, z)
    });
    let moves = host.world.object_draw_displacements();
    let mouth = draws
        .iter()
        .find(|d| (d.world_x, d.world_z) == (CAVE_MOUTH.0, CAVE_MOUTH.2))
        .expect("the cave-mouth cliff is a placed draw");
    let key = field_env::placed_object_key(mouth, Some(&binds)).expect("the cliff is bound");
    assert!(follow.contains(&key), "the cliff follows the floor");
    // The bake is the floor under the MAN header's ladder, whenever it runs.
    assert_eq!(mouth.world_y, -256);
    let dy = moves.get(&key).map_or(0, |m| m[1]);
    assert_eq!(
        mouth.world_y + dy,
        CAVE_MOUTH.1,
        "the cliff stands on the live summit floor"
    );
}
