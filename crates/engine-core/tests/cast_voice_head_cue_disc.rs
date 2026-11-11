//! Disc-gated: the **cast's own CD-XA voice** - every slot-B module's head
//! cue, read off the band's bytes, and the clip request the engine raises for
//! it at the arming seam.
//!
//! Two tiers:
//!
//! 1. **Census.** `legaia_engine_battle_vm::battle_cast_cue::module_head_cue` over
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

use legaia_engine_battle_vm::battle_cast_cue::{ModuleHeadCue, module_head_cue};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_overlay_images::cast_effect_pool::{
    CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool,
};
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

/// The arm each module's head cue call sits on, against retail: the phase byte
/// on the frame the clip starter ran (`capture`, the caller log for 0904 and 0927,
/// the mednafen state series for the rest), and the disassembly for 0946, whose
/// arm 0 makes the call as it stores phase `4`.
#[test]
fn the_head_cue_sits_on_the_arm_retail_calls_it_from() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let images = band_images(&dir);
    for (entry, arm) in [
        (904, 3),
        (906, 4),
        (908, 4),
        (911, 2),
        (916, 3),
        (920, 2),
        (923, 2),
        (927, 2),
        (928, 2),
        (930, 3),
        (946, 0),
    ] {
        let (_, bytes) = images.iter().find(|(idx, _)| *idx == entry).unwrap();
        assert_eq!(
            legaia_engine_battle_vm::battle_cast_cue::module_head_cue_arm(bytes),
            Some(arm),
            "PROT {entry}"
        );
    }
    // A multi-spell module's bodies each make their own call.
    use legaia_engine_battle_vm::battle_cast_cue::body_head_cue;
    use legaia_engine_cast::cast_module_ticks::capture_tick_body;
    for (entry, action, cue) in [(964, 0xAF, 0x1C7), (955, 0x60, 0x196), (955, 0x6F, 0x143)] {
        let (_, bytes) = images.iter().find(|(idx, _)| *idx == entry).unwrap();
        let body = capture_tick_body(entry, action).expect("a trampoline arm");
        assert_eq!(
            body_head_cue(bytes, body),
            Some(ModuleHeadCue::Literal(cue)),
            "PROT {entry} action {action:#x}"
        );
    }
    for (entry, slot) in [(904, 7), (908, 7), (909, 6), (911, 12), (927, 14)] {
        let (_, bytes) = images.iter().find(|(idx, _)| *idx == entry).unwrap();
        assert_eq!(
            legaia_engine_battle_vm::battle_cast_cue::module_head_seek(bytes),
            Some(slot),
            "PROT {entry}"
        );
    }
}

/// A battle world with the disc's pool and span table, three party seats +
/// three monster seats.
fn battle_world(dir: &std::path::Path) -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
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
    world.audio.xa_cue_durations = Some(
        legaia_game_tables::xa_cue_table::xa_cue_durations_from_scus(&scus).expect("span table"),
    );
    world
}

/// Past every module's cue arm.
const MAX_ARM: u8 = 0xFE;

#[test]
fn arming_a_seru_cast_raises_the_captured_starter_triple() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut w = battle_world(&dir);
    // Gimard is action id 0x81 -> PROT 0903 -> cue 0x134 -> (6, 4, 686).
    w.arm_summon_stager(0, 0x81);
    assert!(
        w.drain_battle_xa_cues().is_empty(),
        "no voice at the arming seam"
    );
    w.raise_cast_voice_on_arm(MAX_ARM);
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

    // A second cast with a data stream read in flight: `FUN_8003DE7C(1) != 0`
    // declines it. A sounding clip alone does not (the poll reads the stream
    // queue, `0x8003DF70`).
    w.arm_summon_stager(0, 0x83);
    w.casting.sideband_read_left = Some(8);
    w.raise_cast_voice_on_arm(MAX_ARM);
    assert!(
        w.drain_battle_xa_cues().is_empty(),
        "a cast while a stream read is in flight plays no voice"
    );
    w.casting.sideband_read_left = None;
    w.audio.battle_xa_busy_frames = 0;
    // Vera is action id 0x83 -> PROT 0905 -> cue 0x131 -> (6, 1, 568).
    w.arm_summon_stager(0, 0x83);
    w.raise_cast_voice_on_arm(MAX_ARM);
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

/// The round's committed spells list their cast voices ahead of the cast -
/// the page stages them so the cast's own request finds the clip resident.
/// A coin-flip module lists both candidates; the list plays nothing.
#[test]
fn a_rounds_committed_spells_list_their_cast_voices_ahead() {
    use legaia_engine_battle::target_picker::CursorRow;
    use legaia_engine_battle_loop::round_flow::PendingPartyAction;
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut w = battle_world(&dir);
    let spell = |spell_id| {
        Some(PendingPartyAction::Spell {
            spell_id,
            target_row: CursorRow::Enemy,
            target_slot: 3,
        })
    };
    // Gimard (0x81 -> PROT 0903 -> cue 0x134) twice, Vera (0x83 -> 0905).
    w.battle.round_flow.pending = [spell(0x81), spell(0x81), spell(0x83)];
    w.list_round_cast_voices();
    let pre = w.drain_battle_xa_prestage();
    let triples: Vec<_> = pre
        .iter()
        .map(|c| (c.clip, c.channel, c.duration_sectors))
        .collect();
    assert_eq!(
        triples,
        vec![(6, 4, 686), (6, 1, 568)],
        "deduplicated, in commit order"
    );
    assert!(
        w.drain_battle_xa_cues().is_empty(),
        "the list plays nothing"
    );
    assert_eq!(w.audio.battle_xa_busy_frames, 0, "and holds no drive");
    eprintln!("[ok] round prestage lists {triples:?}");
}

/// PROT 0924 (Ultimate Rave, the Lippian flute's `0x96`) plays its cue off
/// arm 0's kill roll: `0x189` on the one-in-128 kill, `0x18A` on a miss
/// (`0x801F6CFC..0x801F6D10`). The roll is drawn at the arming seam and
/// held for the finale arm, so the cue and the kill always agree.
#[test]
fn ultimate_rave_cues_its_kill_roll() {
    use legaia_engine_battle_vm::battle_cast_cue::{
        VoiceCueGates, VoiceCueVerdict, admit_voice_cue,
    };
    use legaia_engine_cast::cast_module_ticks::ultimate_rave_cue;
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut seen = [false; 2];
    for seed in 0u32..4096 {
        let mut w = battle_world(&dir);
        w.rng_state = seed;
        w.arm_summon_stager(0, 0x96);
        let kill = w
            .casting
            .module_cam
            .kill
            .expect("the roll is drawn at arming");
        let id = ultimate_rave_cue(kill);
        let raw = w
            .audio
            .xa_cue_durations
            .as_deref()
            .and_then(|t| t.get(usize::from(id) - 0x100).copied());
        let gates = VoiceCueGates {
            side_band_stage: 0,
            stream_read: false,
        };
        let VoiceCueVerdict::Play(want) = admit_voice_cue(id, gates, raw) else {
            panic!("cue {id:#x} plays");
        };
        w.raise_cast_voice_on_arm(MAX_ARM);
        let xa = w.drain_battle_xa_cues();
        assert_eq!(xa.len(), 1);
        assert_eq!(
            (xa[0].clip, xa[0].channel, xa[0].duration_sectors),
            (want.clip_slot, want.channel, want.duration_sectors),
            "seed {seed}: kill {kill}"
        );
        seen[usize::from(kill)] = true;
        if seen == [true, true] {
            break;
        }
    }
    assert_eq!(seen, [true, true], "both paths reached");
    eprintln!("[ok] PROT 0924 cues 0x18A on a miss, 0x189 on the kill");
}

/// The drive wait ahead of a module's voice, read off its bytes: the arm that
/// seeks the drive to the clip file and the arm that polls the seek
/// (`legaia_engine_battle_vm::battle_cast_cue::module_drive_arms`). Orb's PROT
/// 0911 seeks `0xC` (`XA13`) on arm 1 and polls on arm 2 (`0x801F6B98`,
/// `0x801F6BB8`), which the frame-stepped `orb_summon_mid_cast` shows holding
/// for the seek; Zeto's PROT 0946 calls its head cue with no seek at all.
#[test]
fn a_summon_module_seeks_its_file_then_waits_on_the_drive() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    use legaia_engine_battle_vm::battle_cast_cue::{ModuleDriveArms, module_drive_arms};
    let images = band_images(&dir);
    let arms = |entry: u32| {
        let (_, bytes) = images.iter().find(|(idx, _)| *idx == entry).unwrap();
        module_drive_arms(bytes)
    };
    let mut listed = Vec::new();
    let mut gated = Vec::new();
    for (entry, bytes) in &images {
        if let Some(a) = arms(*entry) {
            listed.push(format!(
                "{entry}:{}/{}/{:#x}",
                a.seek_arm, a.poll_arm, a.slot
            ));
        }
        if legaia_engine_battle_vm::battle_cast_cue::head_cue_waits_arm_gate(bytes, None) {
            gated.push(*entry);
        }
    }
    eprintln!("[ok] head cues behind their arm's own gate: {gated:?}");
    // Gilium's arm 2 drains its countdown before the poll; Orb's polls at once.
    assert!(gated.contains(&923));
    assert!(!gated.contains(&911));
    eprintln!("[ok] drive arms: {}", listed.join(" "));
    assert_eq!(
        arms(911),
        Some(ModuleDriveArms {
            seek_arm: 1,
            poll_arm: 2,
            slot: 0xC
        })
    );
    assert_eq!(arms(946), None);
}
