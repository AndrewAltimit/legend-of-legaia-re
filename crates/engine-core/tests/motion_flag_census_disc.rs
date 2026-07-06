//! Disc-gated: the disc-wide **motion-VM** flag census over every scene MAN.
//!
//! `FUN_80038158` (the per-actor motion / bytecode VM, dispatched by
//! `FUN_8003BC08` when actor `+0x10 & 0x80`) is the second bytecode VM that
//! writes the `DAT_80085758` system story-flag bank: its op `0x07` SETs and
//! op `0x08` CLEARs the flag in its u16 operand. Its scripts are
//! disc-resident - MAN tail-**section 1**, installed on actors at scene
//! entry by `FUN_8003A9D4` (see `legaia_asset::man_motion`) - so
//! [`motion_flag_census`] can sweep them without a runtime capture, exactly
//! like the field-VM `system_flag_census` sibling.
//!
//! Anchors (verified against a hand decode of the retail corpus):
//! - `map02` motion record 0 (bound to overworld actor `0x24`) carries a
//!   variant gated on flag `0x5A4` that SETs `0x467` (1127) - part of the
//!   Sebucus overworld walking-band flag choreography.
//! - `town0b` motion record 1 (actor `0x49`) CLEARs `0x23F` (575) under a
//!   gate on the same flag - a one-shot "consume the moved marker" stream.
//!
//! Pinned negatives: the spine flags `0x142` (dolk clear), `0x482` (Drake
//! mist walls), `0x1BE`, and `0x225` (549, the town01 opening one-shot) have
//! **no** motion-VM site anywhere on the disc - the motion-VM-writer
//! hypothesis for those flags is falsified; their setters are direct
//! `FUN_8003CE08`-style code paths, not disc bytecode in this carrier.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use legaia_engine_core::man_field_scripts::{MotionCensusSite, motion_flag_census};
use legaia_engine_core::scene::ProtIndex;
use std::collections::BTreeMap;
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

fn run_census() -> Option<BTreeMap<u16, Vec<MotionCensusSite>>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scenes = index.cdname_scene_names();
    eprintln!("[motion census] scanning {} CDNAME scenes", scenes.len());
    Some(motion_flag_census(&index, &scenes))
}

#[test]
fn motion_flag_census_finds_the_known_anchor_sites() {
    use legaia_asset::man_motion::MotionFlagKind;
    let Some(census) = run_census() else { return };

    let total: usize = census.values().map(Vec::len).sum();
    eprintln!(
        "[motion census] {} distinct flags, {} total sites",
        census.len(),
        total,
    );
    assert!(
        !census.is_empty(),
        "the disc-wide motion-VM flag census must surface at least one site",
    );

    // Anchor 1: map02 record 0 (actor 0x24) SETs 0x467 under a 0x5A4 gate.
    let hits_1127 = census
        .get(&0x467)
        .expect("flag 0x467 (1127) must have motion-VM sites");
    assert!(
        hits_1127.iter().any(|h| {
            h.scene_name == "map02"
                && h.site.kind == MotionFlagKind::Set
                && h.site.record == 0
                && h.site.gate == Some(0x5A4)
                && h.bindings.iter().any(|b| b.actor_id == 0x24)
        }),
        "map02 rec0 (actor 0x24, gate 0x5A4) must SET flag 0x467; got {hits_1127:?}",
    );

    // Anchor 2: town0b record 1 (actor 0x49) CLEARs 0x23F under its own gate.
    let hits_575 = census
        .get(&0x23F)
        .expect("flag 0x23F (575) must have a motion-VM site");
    assert!(
        hits_575.iter().any(|h| {
            h.scene_name == "town0b"
                && h.site.kind == MotionFlagKind::Clear
                && h.site.gate == Some(0x23F)
                && h.bindings.iter().any(|b| b.actor_id == 0x49)
        }),
        "town0b (actor 0x49, gate 0x23F) must CLEAR flag 0x23F; got {hits_575:?}",
    );
}

#[test]
fn motion_flag_census_pins_the_spine_flag_negatives() {
    let Some(census) = run_census() else { return };

    // The still-open spine writers are NOT motion-VM ops anywhere on the
    // disc. This is the load-bearing negative: it closes the "second
    // bytecode VM sets 0x142/0x482" hypothesis without a capture, and pins
    // that 549's writer (the town01 opening one-shot) is not disc-resident
    // motion bytecode either.
    for target in [0x142u16, 0x482, 0x1BE, 0x225] {
        assert!(
            !census.contains_key(&target),
            "flag 0x{target:04X} unexpectedly has motion-VM sites: {:?}",
            census.get(&target),
        );
    }
}
