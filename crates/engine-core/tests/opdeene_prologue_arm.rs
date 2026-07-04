//! Disc-gated: pin the opening prologue's `town01` hand-off arm to real disc
//! bytecode.
//!
//! The cutscene scene `opdeene` ends its closing timeline with a field-VM
//! `GFLAG_SET 26` (op `0x2E`, operand `0x1A`) that raises scratchpad bit 26 -
//! the flag retail's per-frame field controller `FUN_801D1344` waits on
//! (with the confirm press) to issue the name-based scene change to Rim Elm.
//! This walks `opdeene`'s MAN cutscene-timeline partition as a real field-VM
//! opcode stream and asserts that write is present, then drives the engine's
//! data-driven arm (`World::arm_prologue_handoff_from_man`) end-to-end.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::man_field_scripts::walk_partition_gflag_sites;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{PROLOGUE_HANDOFF_BIT, PROLOGUE_HANDOFF_FLAG, World};
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

#[test]
fn opdeene_partition2_carries_the_gflag_set_26_handoff_arm() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let cutscene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = Scene::load(&index, cutscene).expect("load opdeene");
    let man = scene
        .field_man_payload(&index)
        .expect("man payload fetch")
        .expect("opdeene has a MAN payload");
    let man_file = legaia_asset::man_section::parse(&man).expect("man parse");

    eprintln!(
        "{cutscene} MAN: {} bytes, partition counts {:?}",
        man.len(),
        man_file.header.partition_counts
    );

    // Walk the cutscene-timeline partition (partition 2) as a real opcode
    // stream and collect every GFLAG site.
    let sites = walk_partition_gflag_sites(&man_file, &man, 2);
    for s in &sites {
        eprintln!(
            "  P2[{}] GFLAG.{} bit={} @ 0x{:05X} (op 0x{:02X})",
            s.record,
            if s.set { "Set" } else { "Clear" },
            s.bit,
            s.abs_pc,
            s.opcode,
        );
    }

    let handoff_sites: Vec<_> = sites
        .iter()
        .filter(|s| s.set && s.bit as u32 == PROLOGUE_HANDOFF_BIT)
        .collect();
    assert!(
        !handoff_sites.is_empty(),
        "opdeene partition-2 must carry a GFLAG_SET {PROLOGUE_HANDOFF_BIT} (the town01 hand-off arm)",
    );
    // The arm is a single timeline event in retail.
    assert_eq!(
        handoff_sites.len(),
        1,
        "exactly one GFLAG_SET {PROLOGUE_HANDOFF_BIT} site in opdeene's cutscene timeline",
    );
    let site = handoff_sites[0];
    assert_eq!(site.opcode, 0x2E, "GFLAG_SET opcode");
    // Pinned offset: the byte after the documented record start 0xA47.
    assert_eq!(
        site.abs_pc, 0x0A5E,
        "GFLAG_SET 26 sits at the pinned cutscene-timeline offset",
    );

    // End-to-end: the engine's data-driven arm raises the bit from this MAN.
    let mut world = World::new();
    world.set_active_scene_label(cutscene);
    // The scene entry marks the opening chain as playing (the skip gate's
    // scope); mirror it here since this test drives World directly.
    world.opening_chain_active = true;
    assert_eq!(
        world.story_flags & PROLOGUE_HANDOFF_FLAG,
        0,
        "bit clear before the arm"
    );
    assert!(
        world.arm_prologue_handoff_from_man(&man_file, &man),
        "arm derives from the real opdeene bytecode"
    );
    assert_ne!(
        world.story_flags & PROLOGUE_HANDOFF_FLAG,
        0,
        "bit set after the data-driven arm"
    );
    // The confirm-press gate then hands off to Rim Elm exactly once.
    assert_eq!(
        world.take_prologue_handoff(true),
        Some(legaia_asset::new_game::OPENING_SCENE),
    );
    assert_eq!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0, "fire-once");
}

#[test]
fn town01_partition2_has_no_prologue_handoff_arm() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man = scene
        .field_man_payload(&index)
        .expect("man payload fetch")
        .expect("town01 has a MAN payload");
    let man_file = legaia_asset::man_section::parse(&man).expect("man parse");

    // The interactive scene must NOT carry the prologue arm - otherwise the
    // data-driven arm would fire a spurious hand-off back into town01.
    let mut world = World::new();
    world.set_active_scene_label("town01");
    assert!(
        !world.arm_prologue_handoff_from_man(&man_file, &man),
        "town01 carries no GFLAG_SET {PROLOGUE_HANDOFF_BIT}; the arm must not fire",
    );
    assert_eq!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);
}
