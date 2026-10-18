//! Disc-gated census: the scripted-fight header byte of a MAN formation row
//! (`record[+0]`, the flag the battle loader's stat-profile pick keys on,
//! `FUN_80054CB0` `0x80055234`) is a minority of the disc's formation rows.
//! The behaviour it gates is asserted in `engine-core`'s
//! `world/tests/seru_side_effect_live_disc.rs`; this walks every CDNAME scene
//! through the scene host.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() && p.join("SCUS_942.54").is_file() {
            return Some(p);
        }
    }
    None
}

/// The discriminator the seed keys on has to actually discriminate: if every
/// MAN formation row carried a non-zero `record[+0]`, every fight would take
/// the boss profile and the change above would be a no-op dressed as a fix.
///
/// So this is a census, not a behaviour test - it walks every CDNAME scene's
/// MAN and counts the rows either way, and requires the scripted rows to be a
/// small minority with both classes present.
#[test]
fn the_scripted_header_byte_is_a_minority_of_the_formation_corpus() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let Ok(mut host) = legaia_engine_scene_host::scene_host::SceneHost::open_extracted(&dir) else {
        eprintln!("[skip] SceneHost would not open");
        return;
    };
    let Ok(cdname) = legaia_prot::cdname::parse(&dir.join("CDNAME.TXT")) else {
        eprintln!("[skip] CDNAME.TXT unreadable");
        return;
    };
    let mut scenes: Vec<String> = cdname.values().cloned().collect();
    scenes.sort();
    scenes.dedup();

    let (mut scripted, mut random, mut scenes_seen) = (0usize, 0usize, 0usize);
    for scene in &scenes {
        if host.load_scene(scene).is_err() {
            continue;
        }
        let Some(sc) = host.scene.as_ref() else {
            continue;
        };
        let Ok(Some(man)) = sc.field_man_payload(&host.index) else {
            continue;
        };
        let defs = legaia_engine_battle::encounter_man::formation_defs_from_man(&man);
        if defs.is_empty() {
            continue;
        }
        scenes_seen += 1;
        for d in &defs {
            if d.per_battle_flags() != 0 {
                scripted += 1;
            } else {
                random += 1;
            }
        }
    }
    let total = scripted + random;
    assert!(
        scenes_seen >= 20 && total >= 100,
        "expected a corpus of MAN formation rows, got {total} rows over {scenes_seen} scenes"
    );
    assert!(scripted > 0, "the disc must carry some scripted rows");
    assert!(
        scripted * 4 < total,
        "scripted rows must be the minority ({scripted} of {total}) - otherwise keying the \
         boost profile on the header byte would make every fight a boss fight"
    );
    eprintln!(
        "[ok] {scripted} scripted / {random} random formation rows over {scenes_seen} scenes"
    );
}
