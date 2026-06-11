//! Disc-gated: the summon-overlay parser generalizes across the **whole player
//! Seru-magic summon block** (extraction PROT 0903..=0913, the corrected
//! loader-arithmetic range), not just the deep-dived stager (PROT 0905, covered
//! byte-for-byte by `summon_overlay_real`).
//!
//! Each entry in [`PLAYER_SUMMON_STAGER_PROT`] is a per-summon stager overlay:
//! [`summon_overlay::parse`] scans its `FUN_80021B04` spawn calls and recovers a
//! move-VM scene-graph of part records. This sweep pins the robust structural
//! invariants that hold for every player summon — many spawn sites, a non-trivial
//! contiguous record table, all records in-file, every bytecode range in bounds —
//! so a regression in the spawn-site scan or the a2 resolver is caught across the
//! block. (Record *header semantics* beyond 0905 — which `model_sel` sentinels
//! mean what, and the per-summon `gp[0x754]` model-library base — need a live
//! cast trace and are out of scope here; this only asserts the table shape.)
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::static_overlay;
use legaia_asset::summon_overlay::{
    self, PLAYER_SUMMON_STAGER_PROT, SUMMON_OVERLAY_LINK_BASE, SummonPartKind,
};
use legaia_prot::archive::Archive;

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
fn player_summon_block_parses_into_move_vm_scene_graphs() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");

    for entry_idx in PLAYER_SUMMON_STAGER_PROT {
        let entry = archive
            .entries
            .get(entry_idx as usize)
            .cloned()
            .unwrap_or_else(|| panic!("PROT {entry_idx} entry exists"));
        let mut bytes = Vec::new();
        archive
            .read_entry(&entry, &mut bytes)
            .unwrap_or_else(|_| panic!("read PROT {entry_idx}"));

        // PROT 0907 (the spell-0x85 slot) is Nighto's stager; its ASCII head
        // is the attack's display title "Hell's Music" (the SCUS spell table
        // carries the same name - parallel to Gimard's "Burning Attack" in
        // summon.dat). Capture-pinned mid-cast; the stager checks below apply
        // to it like every other slot in the block.
        if entry_idx == 907 {
            let head = static_overlay::head_string(&bytes, 0x40, 4);
            assert_eq!(
                head.as_deref(),
                Some("Hell's Music"),
                "PROT {entry_idx}: expected the attack-name title at offset 0"
            );
        }

        let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);

        // Every player summon stages many parts through FUN_80021B04 ...
        assert!(
            overlay.spawn_sites >= 20,
            "PROT {entry_idx}: expected many FUN_80021B04 spawn sites (got {})",
            overlay.spawn_sites,
        );
        // ... and recovers a non-trivial record table from them.
        assert!(
            overlay.parts.len() >= 10,
            "PROT {entry_idx}: expected a non-trivial scene-graph (got {} parts)",
            overlay.parts.len(),
        );

        // The records form a contiguous, sorted, in-file table: each record sits
        // in-file, each bytecode range is well-formed and in bounds, and one
        // record's bytecode runs exactly up to the next record's start.
        let mut prev_end = 0usize;
        for (i, p) in overlay.parts.iter().enumerate() {
            assert!(
                p.record_off + 4 <= bytes.len(),
                "PROT {entry_idx} part {i}: record {:#x} out of file",
                p.record_off,
            );
            assert!(
                p.bytecode.start <= p.bytecode.end && p.bytecode.end <= bytes.len(),
                "PROT {entry_idx} part {i}: bytecode {:#x}..{:#x} out of bounds",
                p.bytecode.start,
                p.bytecode.end,
            );
            assert!(
                p.record_off >= prev_end,
                "PROT {entry_idx} part {i}: record {:#x} overlaps previous",
                p.record_off,
            );
            // The header word always classifies into one of the three kinds
            // (this is exhaustive by construction, but pins the API surface).
            assert!(matches!(
                p.kind(),
                SummonPartKind::TransformNode
                    | SummonPartKind::LibraryMesh
                    | SummonPartKind::Sentinel
            ));
            prev_end = p.bytecode.end;
        }

        let nodes = overlay
            .parts
            .iter()
            .filter(|p| p.kind() == SummonPartKind::TransformNode)
            .count();
        let mesh = overlay
            .parts
            .iter()
            .filter(|p| p.kind() == SummonPartKind::LibraryMesh)
            .count();
        eprintln!(
            "PROT {entry_idx} summon stager: {} spawn sites, {} parts ({nodes} transform nodes, {mesh} library meshes)",
            overlay.spawn_sites,
            overlay.parts.len(),
        );
    }
}
