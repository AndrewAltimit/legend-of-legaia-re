//! Disc-gated: a **talk proxy** placement is what the action button reaches.
//!
//! `concnow`'s gate guards P1[12] / P1[13] stand at (11520, 15232), inside
//! the wall line of the gate (collision rows 118..121, cols 88..91, which
//! their own talk paints open with `4C 70 59 73 5A 78`). Retail's facing probe
//! (`FUN_801D01B0` -> `FUN_801CF9F4`: a point 64 ahead from `DAT_801F2254`,
//! box `0x40 + 0x20 - 0x18` = 72 around a moving-class actor) cannot reach
//! them from either side of the wall: the player stops at z 15038 north of it
//! and z 15424 south of it.
//!
//! What it reaches is P1[26], an undrawn placement (`4C 40` scale `0`) at
//! (11520, 15104) whose interaction is `B1 3D 08`: the touched mark on actor
//! `0x3D` (flat index 61 = partition-1 record 13). Retail's context runner
//! then steps the guard's record, which raises `0x5F8` and opens the gate.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (disc-gated).

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("extracted");
    if root.join("PROT.DAT").exists() {
        Some(root)
    } else {
        eprintln!("[skip] extracted/ missing");
        None
    }
}

#[test]
fn concnow_gate_guard_is_talked_to_through_its_proxy() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("concnow", 0).expect("enter concnow");
    for _ in 0..120 {
        let _ = host.tick();
    }
    assert_eq!(
        host.world.npcs.talk_proxies.get(&26),
        Some(&(13, (11520, 15104))),
        "P1[26] hands its interaction to guard P1[13]"
    );
    if host.world.player_actor_slot.is_none() {
        host.world.install_field_player(0);
    }
    let s = host.world.player_actor_slot.expect("player slot") as usize;
    // Where the wall stops the player north of the gate, facing it (Z+).
    host.world.actors[s].move_state.world_x = 11584;
    host.world.actors[s].move_state.world_z = 15038;
    host.world.actors[s].move_state.render_26 = 0;
    let hit = host.world.field_interact_probe_slot();
    eprintln!("[ran] concnow gate probe hits {hit:?}");
    assert_eq!(hit, Some(13), "the facing probe reaches the guard's proxy");
    // Without the proxy the guard is out of reach: the probe point lands
    // at z 15102, 130 short of the guard against a 72-unit box.
    host.world.npcs.talk_proxies.clear();
    assert_eq!(host.world.field_interact_probe_slot(), None);
}
