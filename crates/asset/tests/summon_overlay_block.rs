//! Disc-gated: the summon-overlay parser generalizes across the **whole player
//! Seru-magic summon block** (extraction PROT 0903..=0913, the corrected
//! loader-arithmetic range), the **evolved-Seru cast block** (PROT 0914..=0923,
//! the contiguous continuation under the same linear arithmetic) **and the
//! high-summon block** (PROT 0927..=0934), not just the deep-dived stager (PROT
//! 0905, covered byte-for-byte by `summon_overlay_real`).
//!
//! The evolved-Seru block extends the structural pin: those ten entries
//! (`spell_id 0x8C..=0x95`) parse as the same move-VM stagers, and two of them -
//! `0x8E` → 916, `0x93` → 921 - carry `0x4000` render-mode nodes, the only such
//! records outside the Sim-Seru high stagers (0928/0929/0931).
//!
//! Each entry is a per-summon stager overlay: [`summon_overlay::parse`] scans
//! its `FUN_80021B04` + `FUN_80050ED4` spawn calls and recovers a move-VM
//! scene-graph of part records. Each entry is first **trimmed to its TOC-gap
//! unique-content footprint** ([`unique_content_len`]) - the extraction `.BIN`s
//! over-read into the following entries, so the untrimmed tail carries
//! *neighbouring* stagers' spawn sites whose record pointers dereference
//! unrelated bytes here. This sweep pins the post-trim structural invariants
//! across both blocks: spawn sites present, a non-trivial contiguous record
//! table, all records in-file, every bytecode range in bounds, and - the
//! sentinel resolution - every record first word is a `-1` transform node, a
//! small library-mesh index, or the `0x4000` render-mode node (five stagers
//! carry `0x4000` records: the Sim-Seru trio 0928/0929/0931 and the
//! evolved-Seru casts 0916/0921; the historical `0x1000`/`0x8000`-class
//! "sentinels" were over-read artifacts).
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::static_overlay;
use legaia_asset::summon_overlay::{
    self, EVOLVED_SUMMON_STAGER_PROT, HIGH_SUMMON_STAGER_PROT, PLAYER_SUMMON_STAGER_PROT,
    SUMMON_OVERLAY_LINK_BASE, SummonPartKind, unique_content_len,
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

    let mut render_mode_nodes = 0usize;
    let mut render_mode_entries: Vec<u32> = Vec::new();
    for entry_idx in PLAYER_SUMMON_STAGER_PROT
        .chain(EVOLVED_SUMMON_STAGER_PROT)
        .chain(HIGH_SUMMON_STAGER_PROT)
    {
        let entry = archive
            .entries
            .get(entry_idx as usize)
            .cloned()
            .unwrap_or_else(|| panic!("PROT {entry_idx} entry exists"));
        let next = archive
            .entries
            .get(entry_idx as usize + 1)
            .cloned()
            .unwrap_or_else(|| panic!("PROT {} entry exists", entry_idx + 1));
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

        // Trim the over-read window down to the entry's own content.
        let unique = unique_content_len(bytes.len(), entry.start_lba, next.start_lba);
        bytes.truncate(unique);

        let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);

        // Every stager spawns parts through the two spawn helpers ...
        assert!(
            overlay.spawn_sites >= 4,
            "PROT {entry_idx}: expected spawn sites in the trimmed stager (got {})",
            overlay.spawn_sites,
        );
        // ... and recovers a non-empty record table from them.
        assert!(
            overlay.parts.len() >= 3,
            "PROT {entry_idx}: expected a non-empty scene-graph (got {} parts)",
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
            // Post-trim, the only first words in the corpus are transform
            // nodes, library meshes, and the 0x4000 render-mode node. Any
            // other "sentinel" is the over-read signature.
            match p.kind() {
                SummonPartKind::TransformNode | SummonPartKind::LibraryMesh => {}
                SummonPartKind::Sentinel => {
                    assert!(
                        p.is_render_mode_node(),
                        "PROT {entry_idx} part {i}: unexpected sentinel {:#06x} at {:#x}",
                        p.model_sel as u16,
                        p.record_off,
                    );
                    render_mode_nodes += 1;
                    if !render_mode_entries.contains(&entry_idx) {
                        render_mode_entries.push(entry_idx);
                    }
                }
            }
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
            "PROT {entry_idx} summon stager: unique {unique:#x}, {} spawn sites, {} parts \
             ({nodes} transform nodes, {mesh} library meshes)",
            overlay.spawn_sites,
            overlay.parts.len(),
        );
    }

    // The 0x4000 render-mode nodes live in the Sim-Seru high stagers (0928 /
    // 0929 / 0931) AND, as this sweep pins, two evolved-Seru stagers (0916 /
    // 0921); the parse must surface them.
    assert!(
        render_mode_nodes >= 1,
        "expected the stager corpus to carry 0x4000 render-mode node records",
    );
    for expect in [916u32, 921, 928, 929, 931] {
        assert!(
            render_mode_entries.contains(&expect),
            "expected PROT {expect} to carry a 0x4000 render-mode node (got carriers {render_mode_entries:?})",
        );
    }
}
