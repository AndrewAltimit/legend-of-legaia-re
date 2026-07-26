//! Disc-gated: the six enemy-boss (final-boss Cort) special-attack stagers
//! (`summon_overlay::ENEMY_BOSS_STAGER_PROT` = extraction PROT 0938 Mystic
//! Circle / 0940 Mystic Shield / 0944 Guilty Cross / 0961 Final Crisis / 0962
//! Ultra Charge / 0966 Evil Seru Magic) parse as summon stagers under the
//! shared slot-B link base `0x801F69D8`, exactly like the player block. This is
//! the disc-only *structural* test; the per-leg RAM loader-B binding against the
//! `cort_*_mid_cast` save states is pinned by `enemy_stager_binding`
//! (disc+library-gated, in `legaia-mednafen`).
//!
//! Two structural facts are pinned here:
//!
//! 1. **Each entry ends at its TOC gap**, and `Archive::read_entry` now
//!    delivers exactly that - so the independent `unique_content_len` trim is
//!    a no-op, which is what the first assertion checks. It used to trim, back
//!    when the reader over-read into the following entries and the tail's
//!    spawn sites belonged to *neighbouring* stagers (their record pointers
//!    are only valid for the neighbour's own load). The live mid-cast captures
//!    pin the boundary: the slot-B resident image matches the file exactly up
//!    to the TOC gap.
//! 2. Every recovered record first word is a `-1` transform node or a small
//!    library-mesh index - no out-of-band "sentinel" values (those were
//!    over-read artifacts). The enemy stagers spawn dominantly through the
//!    `FUN_80050ED4` pool wrapper, which `parse` scans alongside the direct
//!    `FUN_80021B04` calls.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::summon_overlay::{
    self, ENEMY_BOSS_STAGER_PROT, SUMMON_OVERLAY_LINK_BASE, SummonPartKind, unique_content_len,
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
fn enemy_boss_stagers_parse_as_summon_scene_graphs() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");

    for entry_idx in ENEMY_BOSS_STAGER_PROT {
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

        // The reader already stops at the entry's end, so the independent
        // TOC-gap trim has nothing left to remove. Cross-checking it here
        // keeps the two derivations of that boundary pinned to each other.
        let unique = unique_content_len(bytes.len(), entry.start_lba, next.start_lba);
        assert_eq!(
            unique,
            bytes.len(),
            "PROT {entry_idx}: read_entry must already be the entry's own sectors",
        );
        bytes.truncate(unique);

        let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);

        // Every enemy stager stages parts through the two spawn helpers ...
        assert!(
            overlay.spawn_sites >= 10,
            "PROT {entry_idx}: expected spawn sites in the trimmed stager (got {})",
            overlay.spawn_sites,
        );
        // ... and recovers a non-empty part-record table from them.
        assert!(
            overlay.parts.len() >= 8,
            "PROT {entry_idx}: expected a non-trivial part-record table (got {})",
            overlay.parts.len(),
        );

        let mut nodes = 0usize;
        let mut meshes = 0usize;
        for (i, p) in overlay.parts.iter().enumerate() {
            assert!(
                p.record_off + 4 <= bytes.len() && p.bytecode.end <= bytes.len(),
                "PROT {entry_idx} part {i}: record/bytecode out of the trimmed file",
            );
            match p.kind() {
                SummonPartKind::TransformNode => nodes += 1,
                SummonPartKind::LibraryMesh => meshes += 1,
                SummonPartKind::Sentinel => panic!(
                    "PROT {entry_idx} part {i}: unexpected sentinel {:#06x} at {:#x} - \
                     out-of-band first words are the over-read signature and must not \
                     survive trimming",
                    p.model_sel as u16, p.record_off,
                ),
            }
        }
        // Transform nodes dominate every enemy stager (the live captures show
        // the spawned part-actors all carrying `-1` records).
        assert!(
            nodes > meshes,
            "PROT {entry_idx}: transform nodes should dominate ({nodes} nodes / {meshes} meshes)",
        );

        eprintln!(
            "PROT {entry_idx} enemy stager: {unique:#x} bytes, {} spawn sites, \
             {} parts ({nodes} transform nodes, {meshes} library meshes)",
            overlay.spawn_sites,
            overlay.parts.len(),
        );
    }
}
