//! Disc-gated: a placement's scripted model swap does not outlive its scene.
//!
//! Retail allocates every placement's actor fresh on each map install, with
//! the placement header's model byte (`FUN_8003A1E4` writes it into the
//! spawn template's model word at `0x8003A300` / `0x8003A324` and allocates
//! from the template at `0x8003A398`). A swap a scene's script made
//! (`4C 50`, `CC <id> 50`, motion op `0x0E`) is gone with that scene's actors.
//!
//! The case: `uru`'s beat (`P2[35]`) re-models placements 7..14
//! with `CC 1C..23 50 4B 00`. Entered next, `bubu1` places villagers in
//! those slots; carried across, the swap named `bubu1`'s bank model `0x4B`,
//! a one-object prop, and placement 14 walked its loop drawn as that prop.
//!
//! Skips without `LEGAIA_DISC_BIN` / `extracted/` (disc-gated convention).

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

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
    eprintln!("[skip] extracted/ missing");
    None
}

#[test]
fn uru_p2_35_swaps_do_not_reach_bubu1_villagers() {
    let Some(dir) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&dir).expect("open the host");
    host.world
        .flags
        .system_flags
        .iter_mut()
        .for_each(|b| *b = 0);
    host.enter_field_scene("uru", 0).expect("enter uru");
    for _ in 0..30 {
        let _ = host.world.tick();
    }
    let man = host
        .scene
        .as_ref()
        .and_then(|s| s.field_man_payload(&host.index).ok().flatten())
        .expect("uru's MAN");
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("parse uru's MAN");
    assert!(
        host.world
            .install_cutscene_timeline_record(&man_file, &man, 2, 35, false),
        "uru P2[35] installs"
    );
    for f in 0..3_000 {
        host.world.input.set_pad(if f % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.world.tick();
    }
    host.world.input.set_pad(0);
    for slot in 7..=14u8 {
        assert_eq!(
            host.world.field_npc_live_model(slot),
            Some(0x4B),
            "uru P2[35] re-models placement {slot} (the case is live)"
        );
    }

    host.enter_field_scene("bubu1", 0).expect("enter bubu1");
    for _ in 0..60 {
        let _ = host.world.tick();
    }
    for slot in 7..=14u8 {
        assert_eq!(
            host.world.field_npc_live_model(slot),
            None,
            "bubu1 placement {slot} draws its own header model"
        );
    }
    // bubu1's own spawn prologues still re-stage their party stand-ins
    // (`P1[56]` / `P1[57]` / `P1[59]`, `4C 50 F1|F2|F4 00`).
    assert_eq!(host.world.field_npc_live_model(56), Some(0xF1));
    assert_eq!(host.world.field_npc_live_model(57), Some(0xF2));
    assert_eq!(host.world.field_npc_live_model(59), Some(0xF4));
}
