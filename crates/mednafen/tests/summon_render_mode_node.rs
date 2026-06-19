//! Do the summon-stager `0x4000`/`0x4001` render-mode part records ever become
//! *live* pooled part-actors during a player summon cast?
//!
//! The trimmed stager census finds exactly four render-mode records (first
//! word `0x4000`), all in the Sim-Seru stagers PROT 0928 (Palma) / 0929
//! (Mule) / 0931 (Jedo). `FUN_80021B04` would seat such a record as a
//! part-actor with `actor[+0x5A] = 3` (`0x4000`) or `5` (`0x4001`), storing
//! the slot-B record pointer at `actor[+0x48]`. The Cort enemy-boss capture
//! held *live* transform-node part-actors (their `+0x48` pointing into the
//! slot-B record table) but all carried `-1` records, never `0x4000`.
//!
//! FINDING (this test): in the player Sim-Seru summon mid-cast corpus the
//! stager scene-graph is **not live at all** - zero words anywhere in main RAM
//! point at any of the stager's record starts (or their `record+4` bytecode),
//! even though the stager is byte-resident at slot B. The references that *do*
//! land in the stager window are the overlay's own internal code pointers
//! (identical offsets across all three states). This is the move-VM-vs-creature
//! split the player-summon correction established: a player summon renders as
//! its namesake `battle_data` creature through the monster animation pipeline,
//! so by the on-screen phase the stager part-actors are gone. The Cort enemy
//! path *does* run live stager parts, but holds only `-1` nodes.
//!
//! Net: the `0x4000`/`0x4001` render modes have **no live exerciser in the
//! catalogued corpus**. Closing their draw behaviour needs a frame-stepped
//! capture inside an *enemy* stager-spawn window whose stager carries a
//! `0x4000` record - not reachable from the current states. See
//! `docs/subsystems/battle-action.md` § summon render-mode nodes.
//!
//! Disc + library gated: needs the extracted PROT (stager bytes) and the
//! mednafen save backups (mid-cast RAM). Skip-passes otherwise.

use std::collections::HashSet;
use std::path::PathBuf;

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::scene::ProtIndex;
use legaia_mednafen::{SaveState, ScenarioManifest, extract::ram_slice};

/// `(scenario label, stager extraction PROT entry)` for the three Sim-Seru
/// player casts whose stagers carry `0x4000` render-mode records.
const RENDER_NODE_CASTS: &[(&str, u32)] = &[
    ("palma_summon_mid_cast", 928),
    ("mule_summon_mid_cast", 929),
    ("jedo_summon_mid_cast", 931),
];

fn manifest_path() -> Option<PathBuf> {
    for c in [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn library_dir() -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let p = PathBuf::from(c);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

fn extracted_root() -> Option<PathBuf> {
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").is_file() {
            return Some(d);
        }
    }
    None
}

/// Count words anywhere in `ram` whose value (as a u32 LE) equals any address
/// in `targets`.
fn count_refs(ram: &[u8], targets: &HashSet<u32>) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i + 4 <= ram.len() {
        let w = u32::from_le_bytes([ram[i], ram[i + 1], ram[i + 2], ram[i + 3]]);
        if targets.contains(&w) {
            n += 1;
        }
        i += 4;
    }
    n
}

#[test]
fn render_mode_nodes_have_no_live_exerciser_in_player_casts() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let (Some(mpath), Some(lib), Some(root)) = (manifest_path(), library_dir(), extracted_root())
    else {
        eprintln!("[skip] scenarios.toml / saves/library / extracted missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&mpath).expect("parse manifest");
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");

    let mut scanned = 0usize;

    for (label, prot) in RENDER_NODE_CASTS {
        let Some(scn) = manifest.scenarios.iter().find(|s| &s.label == label) else {
            eprintln!("[skip] scenario {label} absent");
            continue;
        };
        let Some(save_path) = manifest.library_save_path(scn, &lib) else {
            eprintln!("[skip] {label}: no mednafen library backup");
            continue;
        };
        let save = SaveState::from_path(&save_path).expect("parse save");
        let ram = save.main_ram().expect("main RAM");
        scanned += 1;

        // Disc stager, trimmed to its loader footprint, parsed to records.
        let bytes = index
            .entry_bytes_lba_footprint(*prot)
            .expect("read stager footprint");
        let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
        let render_records = overlay
            .parts
            .iter()
            .filter(|p| p.is_render_mode_node())
            .count();
        assert!(
            render_records > 0,
            "{label}: PROT {prot} must carry a 0x4000/0x4001 record"
        );

        // The stager is byte-resident at slot B during its own cast.
        let resident = ram_slice(
            ram,
            SUMMON_OVERLAY_LINK_BASE,
            SUMMON_OVERLAY_LINK_BASE + bytes.len() as u32,
        )
        .expect("resident stager window");
        let match_pct =
            resident.iter().zip(&bytes).filter(|(a, b)| a == b).count() as f64 / bytes.len() as f64;
        assert!(
            match_pct > 0.90,
            "{label}: stager not resident at slot B (only {:.1}% byte-match)",
            match_pct * 100.0
        );

        // A live part-actor stores its record pointer at `+0x48` (`record`) and
        // ticks the move VM at `record+4`. If the scene-graph were live, RAM
        // would hold such pointers. Count references to every record's start
        // and its bytecode entry.
        let starts: HashSet<u32> = overlay
            .parts
            .iter()
            .map(|p| SUMMON_OVERLAY_LINK_BASE + p.record_off as u32)
            .collect();
        let bytecodes: HashSet<u32> = overlay
            .parts
            .iter()
            .map(|p| SUMMON_OVERLAY_LINK_BASE + p.record_off as u32 + 4)
            .collect();
        let ref_starts = count_refs(ram, &starts);
        let ref_bytecodes = count_refs(ram, &bytecodes);
        eprintln!(
            "{label}: {} parts ({} render-mode); resident {:.1}%; \
             {ref_starts} record-start refs, {ref_bytecodes} bytecode refs",
            overlay.parts.len(),
            render_records,
            match_pct * 100.0,
        );

        // The sound finding: no live scene-graph part in this player cast - the
        // summon is the creature pipeline at the captured instant, so the
        // 0x4000 render-mode records are present but never seated. A future
        // capture that flips this (a nonzero ref count) would mean the stager
        // IS live here and the render-mode draw can be pinned - update the doc.
        assert_eq!(
            ref_starts + ref_bytecodes,
            0,
            "{label}: a stager record is referenced by a live actor - the \
             scene-graph IS live here; pin the 0x4000 render mode and update \
             docs/subsystems/battle-action.md"
        );
    }

    if scanned == 0 {
        eprintln!("[skip] no Sim-Seru render-node states available");
    }
}
