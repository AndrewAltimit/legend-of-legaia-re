//! Disc-gated: the scripted-motion VM's tint fade (op `0x0C`) reaches the
//! draw-tint tables both play hosts read, through `SceneHost::tick`.
//!
//! Retail's field actor draws hand the actor's `+0x74` tint and `+0x78`
//! draw-mode word to the mesh submit `FUN_80043390` (`0x8001B46C`,
//! `0x8001BC7C`), which stages them as the GTE far colour and `IR0`. The
//! census (`engine-scene`'s `ambient_motion_op_census_disc`) finds the op
//! bound to placed objects in `koin3` / `other7` / `ropeway` / `taiku` and to
//! NPCs in `taiku` / `bubu1` / `bubu2` / `garmel` / `edbubu`. This drives two of
//! those scenes from entry and samples what the hosts sample each frame:
//! `World::object_draw_tints` for placed objects and
//! `World::field_npc_draw_tint` for NPC slots.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    Some(SceneHost::open_extracted(&extracted).expect("open extracted"))
}

#[test]
fn koin3_object_streams_fade_their_records() {
    let Some(mut host) = open() else { return };
    host.enter_field_scene("koin3", 0).expect("enter koin3");
    let mut tinted = std::collections::BTreeSet::new();
    let mut weights = std::collections::BTreeSet::new();
    for _ in 0..600 {
        host.tick().expect("tick");
        for (record, (_, blend)) in host.world.object_draw_tints() {
            tinted.insert(record);
            weights.insert(blend);
        }
    }
    eprintln!(
        "[motion tint] koin3 tinted records {tinted:?}, weights seen {}",
        weights.len()
    );
    assert!(
        tinted.iter().any(|r| (9..=12).contains(r)),
        "no op-0x0C-bound record (9..=12) drew tinted: {tinted:?}"
    );
    assert!(weights.len() > 1, "the fade never ramped: {weights:?}");
}

#[test]
fn taiku_npc_streams_tint_their_slots() {
    let Some(mut host) = open() else { return };
    host.enter_field_scene("taiku", 0).expect("enter taiku");
    // The NPC slots `taiku`'s `0x0C` streams bind (records 1..=3).
    let slots = [11usize, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21];
    let mut tinted = std::collections::BTreeSet::new();
    for _ in 0..600 {
        host.tick().expect("tick");
        for &s in &slots {
            if host.world.field_npc_draw_tint(s).is_some() {
                tinted.insert(s);
            }
        }
    }
    eprintln!("[motion tint] taiku tinted NPC slots {tinted:?}");
    assert!(!tinted.is_empty(), "no taiku NPC drew an op-0x0C tint");
}
