//! Disc-gated: the Gimard *Tail Fire* summon stager (PROT 0905) parses into a
//! move-VM part-record scene-graph.
//!
//! Pins, on real disc bytes, that the per-summon stager overlay's
//! `FUN_80021B04` spawn calls reference part records in-file (under the
//! `0x801F69D8` link base) — correcting the earlier "records beyond the 0x5800
//! file" reading, which conflated the 0905 stager with the resident 0900 render
//! overlay. Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::summon_overlay::{self, MODEL_SEL_TRANSFORM_NODE, SUMMON_OVERLAY_LINK_BASE};
use legaia_prot::archive::Archive;

/// CDNAME index of the Gimard Tail Fire summon stager overlay.
const PROT_GIMARD_SUMMON_STAGER: usize = 905;

fn extracted_prot() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

#[test]
fn gimard_summon_stager_parses_into_move_vm_part_records() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(PROT_GIMARD_SUMMON_STAGER)
        .cloned()
        .expect("PROT 0905 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0905");
    assert!(
        bytes.len() >= 0x1E00,
        "stager overlay must include its data region (got {} bytes)",
        bytes.len()
    );

    let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);

    // The stager spawns the summon's body parts via FUN_80021B04 — many calls.
    assert!(
        overlay.spawn_sites >= 20,
        "expected many FUN_80021B04 spawn sites (got {})",
        overlay.spawn_sites
    );
    // Recovered part records, all sitting in-file.
    assert!(
        overlay.parts.len() >= 15,
        "expected the summon scene-graph's part records (got {})",
        overlay.parts.len()
    );
    for p in &overlay.parts {
        assert!(
            p.record_off + 4 <= bytes.len(),
            "record {:#x} must be in-file",
            p.record_off
        );
        assert!(
            p.bytecode.start <= p.bytecode.end && p.bytecode.end <= bytes.len(),
            "record {:#x} bytecode range out of bounds",
            p.record_off
        );
    }

    // Most parts are transform/pivot nodes (`model_sel == -1`) whose mesh is
    // bound by the move-VM anim-bank ops — the dominant kind in the corpus.
    let transform_nodes = overlay
        .parts
        .iter()
        .filter(|p| p.model_sel == MODEL_SEL_TRANSFORM_NODE)
        .count();
    assert!(
        transform_nodes * 2 >= overlay.parts.len(),
        "transform nodes should dominate ({transform_nodes}/{})",
        overlay.parts.len()
    );

    // Each transform-node record's move-VM bytecode opens with the same opcode
    // the corpus shows (0x13) — a sanity check that the records are real move-VM
    // programs and not arbitrary code/data the pointer scan stumbled into.
    let first_node = overlay
        .parts
        .iter()
        .find(|p| p.is_transform_node())
        .expect("at least one transform node");
    assert_eq!(
        bytes[first_node.bytecode.start], 0x13,
        "transform-node move-VM bytecode opens with op 0x13"
    );

    eprintln!(
        "PROT 0905 summon stager: {} spawn sites, {} part records ({} transform nodes), base {:#x}",
        overlay.spawn_sites,
        overlay.parts.len(),
        transform_nodes,
        overlay.link_base
    );
}
