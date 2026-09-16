//! Disc-gated: the **cast's own CD-XA voice** - every slot-B module's head
//! cue, read off the band's bytes, and the clip request the engine raises for
//! it at the arming seam.
//!
//! Two tiers:
//!
//! 1. **Census.** `legaia_engine_vm::battle_cast_cue::module_head_cue` over
//!    all 64 images of PROT `0903..=0966` reproduces the per-module cue
//!    column of `docs/subsystems/cast-module.md` ("Per-module cue census"):
//!    62 literals and the two coin flips (`0936` base `0x1B0`, `0937` base
//!    `0x1B2`, span 2). The table in this file is that doc's column.
//! 2. **Ladder.** A `World` carrying the disc's cast-effect pool and the
//!    `SCUS_942.54` span table (`DAT_800788B8`) arms a Gimard cast (PROT 0903)
//!    and a Vera cast (PROT 0905) and raises, on the `(clip, channel, dur)`
//!    channel both hosts play, exactly the starter triples the two live
//!    captures recorded - `FUN_8003D53C(6, 4, 686)` and `(6, 1, 568)` - while
//!    a cast inside the previous clip's read span raises nothing
//!    (`FUN_8003DE7C(1)`'s decline).
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::cast_effect_pool::{
    CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool,
};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_cast_cue::{ModuleHeadCue, module_head_cue};
use std::path::PathBuf;
use std::sync::Arc;

/// `docs/subsystems/cast-module.md` "Per-module cue census": `(PROT, head cue)`
/// for the 62 literal modules.
const LITERAL_HEAD_CUES: [(u32, u16); 62] = [
    (903, 0x134),
    (904, 0x136),
    (905, 0x131),
    (906, 0x133),
    (907, 0x135),
    (908, 0x132),
    (909, 0x130),
    (910, 0x160),
    (911, 0x161),
    (912, 0x162),
    (913, 0x163),
    (914, 0x164),
    (915, 0x165),
    (916, 0x166),
    (917, 0x168),
    (918, 0x169),
    (919, 0x16a),
    (920, 0x16b),
    (921, 0x16c),
    (922, 0x16d),
    (923, 0x16e),
    (924, 0x189),
    (925, 0x188),
    (926, 0x188),
    (927, 0x177),
    (928, 0x171),
    (929, 0x170),
    (930, 0x173),
    (931, 0x175),
    (932, 0x172),
    (933, 0x174),
    (934, 0x176),
    (935, 0x19c),
    (938, 0x1ac),
    (939, 0x152),
    (940, 0x1b7),
    (941, 0x155),
    (942, 0x1b6),
    (943, 0x1c3),
    (944, 0x1ad),
    (945, 0x19d),
    (946, 0x151),
    (947, 0x19e),
    (948, 0x148),
    (949, 0x145),
    (950, 0x1a8),
    (951, 0x1b5),
    (952, 0x15f),
    (953, 0x15a),
    (954, 0x149),
    (955, 0x157),
    (956, 0x15b),
    (957, 0x15c),
    (958, 0x198),
    (959, 0x199),
    (960, 0x15d),
    (961, 0x1c1),
    (962, 0x1c0),
    (963, 0x1b4),
    (964, 0x1c4),
    (965, 0x1c2),
    (966, 0x1ae),
];

/// The two coin-flip modules: `(PROT, base, span)`.
const RANDOM_HEAD_CUES: [(u32, u16, u8); 2] = [(936, 0x1b0, 2), (937, 0x1b2, 2)];

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

fn band_images(dir: &std::path::Path) -> Vec<(u32, Vec<u8>)> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    (CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST)
        .map(|idx| {
            let entry = archive
                .entries
                .get(idx as usize)
                .cloned()
                .unwrap_or_else(|| panic!("PROT {idx} entry"));
            let mut bytes = Vec::new();
            archive
                .read_entry(&entry, &mut bytes)
                .unwrap_or_else(|e| panic!("read PROT {idx}: {e:#}"));
            (idx, bytes)
        })
        .collect()
}

#[test]
fn every_band_image_names_the_head_cue_the_doc_census_carries() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let images = band_images(&dir);
    assert_eq!(images.len(), 64);
    let mut literal_hits = 0usize;
    let mut random_hits = 0usize;
    for (idx, bytes) in &images {
        let head = module_head_cue(bytes);
        if let Some(&(_, cue)) = LITERAL_HEAD_CUES.iter().find(|(p, _)| p == idx) {
            assert_eq!(
                head,
                Some(ModuleHeadCue::Literal(cue)),
                "PROT {idx}: head cue must be the census literal {cue:#x}"
            );
            literal_hits += 1;
        } else if let Some(&(_, base, span)) = RANDOM_HEAD_CUES.iter().find(|(p, ..)| p == idx) {
            assert_eq!(
                head,
                Some(ModuleHeadCue::Random { base, span }),
                "PROT {idx}: head cue must be the census coin flip"
            );
            random_hits += 1;
        } else {
            panic!("PROT {idx} is not in the census table");
        }
    }
    eprintln!(
        "[ok] head cues: {literal_hits} literal + {random_hits} random of {} images",
        images.len()
    );
    assert_eq!((literal_hits, random_hits), (62, 2));
}

/// A battle world with the disc's pool and span table, three party seats +
/// three monster seats.
fn battle_world(dir: &std::path::Path) -> World {
    let mut world = World {
        party: legaia_engine_core::world::PartyState {
            party_count: 3,
            ..Default::default()
        },
        ..World::default()
    };
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    for slot in 0usize..6 {
        world.actors[slot].active = true;
        world.actors[slot].battle.max_hp = 4000;
        world.actors[slot].battle.hp = 4000;
    }
    let mut pool = CastEffectPool::new();
    for (idx, bytes) in band_images(dir) {
        assert!(pool.insert(idx, &bytes));
    }
    world.install_cast_effect_pool(Arc::new(pool));
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS");
    world.audio.xa_cue_durations =
        Some(legaia_asset::xa_cue_table::xa_cue_durations_from_scus(&scus).expect("span table"));
    world
}

#[test]
fn arming_a_seru_cast_raises_the_captured_starter_triple() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut w = battle_world(&dir);
    // Gimard is action id 0x81 -> PROT 0903 -> cue 0x134 -> (6, 4, 686).
    w.arm_summon_stager(0, 0x81);
    let xa = w.drain_battle_xa_cues();
    assert_eq!(xa.len(), 1, "one clip request per cast: {xa:?}");
    assert_eq!(
        (xa[0].clip, xa[0].channel, xa[0].duration_sectors),
        (6, 4, 686)
    );
    assert_eq!(
        w.audio.battle_xa_busy_frames, 686,
        "the starter holds the drive for the clip's read span"
    );
    eprintln!("[ok] PROT 0903 -> FUN_8003D53C(6, 4, 686)");

    // A second cast inside that span: `FUN_8003DE7C(1) != 0` declines it.
    w.arm_summon_stager(0, 0x83);
    assert!(
        w.drain_battle_xa_cues().is_empty(),
        "a cast inside the previous clip's read span plays no voice"
    );
    // The span drains (the loop steps it once per tick); the next cast plays.
    w.audio.battle_xa_busy_frames = 0;
    // Vera is action id 0x83 -> PROT 0905 -> cue 0x131 -> (6, 1, 568).
    w.arm_summon_stager(0, 0x83);
    let xa = w.drain_battle_xa_cues();
    assert_eq!(xa.len(), 1);
    assert_eq!(
        (xa[0].clip, xa[0].channel, xa[0].duration_sectors),
        (6, 1, 568)
    );
    eprintln!("[ok] PROT 0905 -> FUN_8003D53C(6, 1, 568)");
}

#[test]
fn without_a_span_table_a_cast_raises_no_clip() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut w = battle_world(&dir);
    w.audio.xa_cue_durations = None;
    w.arm_summon_stager(0, 0x81);
    assert!(w.drain_battle_xa_cues().is_empty());
    assert_eq!(w.audio.battle_xa_busy_frames, 0);
    eprintln!("[ok] disc-free span table -> no request");
}
