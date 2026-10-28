//! Disc-gated: a placement whose load-frame slice halt-acquires **itself**
//! through the cross-context form turns to the bind it names.
//!
//! `kor3` `P1[11]` (flat 20) runs `CC 14 85 00 00 13` in its spawn section:
//! op `0x4C` nibble-8 sub-5 aimed at its own context (`0x14`), budget `0`,
//! facing bind `0x13` (flat 19, `P1[10]`). The arm stores the op into the
//! target's `+0x94` and raises its `0x400`; the walk kernel's `0x4C` leg
//! (`FUN_8003774C`) snaps the heading on its first visit and its terminal
//! frame clears the halt (`0x80038004`). `kor3_field_route_walk` holds flat
//! 20 at `+0x26 & 0xFFF = 0x52E` with `0x400` clear, facing flat 19 one tile
//! west and half a tile south.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

#[test]
fn kor3_flat_20_faces_flat_19_and_drops_its_halt() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    host.enter_field_scene("kor3", 0).expect("enter kor3");
    for _ in 0..8 {
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
    }
    // Placement 11 = flat 20 (`N0 = 9`).
    assert_eq!(host.world.npcs.positions.get(&11), Some(&(2048, 4416)));
    assert_eq!(host.world.npcs.positions.get(&10), Some(&(1920, 4480)));
    // Retail `0x52E`, engine space `+ 0x800`.
    assert_eq!(
        host.world.npcs.headings.get(&11).map(|h| h & 0xFFF),
        Some((0x52E + 0x800) & 0xFFF),
        "the self-acquire's face leg turns flat 20 toward flat 19"
    );
    let ch = host
        .world
        .field_vm
        .channels
        .iter()
        .find(|c| !c.object_bind && c.placement_index == 11)
        .expect("flat 20's channel");
    assert_eq!(
        ch.ctx.flags & 0x400,
        0,
        "the leg's terminal frame drops the halt"
    );
}
