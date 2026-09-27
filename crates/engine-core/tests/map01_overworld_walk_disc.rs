//! Disc-gated: the player walks on the `map01` overworld under the play
//! hosts' faithful-play arming (terrain follow, leading-edge wall probes,
//! solid NPC bodies) - the play-compose ladder's overworld rung, engine-side.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn pos(host: &SceneHost) -> (i16, i16, i16) {
    let s = host.world.player_actor_slot.expect("player") as usize;
    let ms = &host.world.actors[s].move_state;
    (ms.world_x, ms.world_z, ms.render_26)
}

#[test]
fn map01_player_walks_under_the_play_host_arming() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.locomotion.follow_terrain_height = true;
    host.world.locomotion.leading_edge_wall_probes = true;
    host.world.npcs.solid = true;
    host.world.npcs.animate = true;
    host.enter_world_map_scene("map01").expect("enter map01");
    if let Some(ctrl) = host.world.world_map.ctrl.as_mut() {
        ctrl.debug_enabled = true;
        ctrl.view_mode = 0;
    }
    for _ in 0..5 {
        host.world.set_pad(0);
        host.tick().expect("tick");
    }
    // The entry seat sits half a sub-cell north of a `.MAP` wall: the
    // collision grid refuses `-Z` (the d-pad's Down under the entry camera),
    // so a Down hold turns the player without moving it. Any walk check on
    // this seat must hold an open direction.
    let (x, z, _) = pos(&host);
    assert!(
        host.world.field_tile_is_wall(x, z - 64),
        "the grid's wall sub-cell south of the map01 seat"
    );
    let mut last = pos(&host);
    for (name, pad, moves) in [
        ("down", PadButton::Down.mask(), false),
        ("up", PadButton::Up.mask(), true),
        ("left", PadButton::Left.mask(), true),
        ("right", PadButton::Right.mask(), true),
    ] {
        for _ in 0..60 {
            host.world.set_pad(pad);
            host.tick().expect("tick");
        }
        let now = pos(&host);
        eprintln!("[ok] {name}: {last:?} -> {now:?}");
        assert_eq!(
            (now.0, now.1) != (last.0, last.1),
            moves,
            "{name} from {last:?}"
        );
        last = now;
    }
}
