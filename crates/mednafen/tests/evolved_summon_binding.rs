//! Disc + library gated: the evolved-Seru summon cast block (spell ids `0x8C..`)
//! byte-pins to its stager entry, capture-confirming the static prediction.
//!
//! The base block (`0x81..=0x8B` → 903..913) and high block (`0x99..=0xA0` →
//! 927..934) were each capture-pinned: one mid-cast save per spell holds the
//! battle overlay's loader-B current-id (`0x8007BC4C`) at exactly
//! `spell_id - 0x79`, with the predicted stager (`extraction = loader_id + 895`)
//! byte-resident at the slot-B link base `0x801F69D8`. The evolved-Seru block
//! between them (`summon_overlay::EVOLVED_SUMMON_STAGER_PROT`, 914..923) was
//! only *statically* mapped — the entries parse as stagers, but no mid-cast
//! state had pinned the per-id binding.
//!
//! Eight catalogued mid-cast states pin eight of the ten legs with the same
//! evidential shape — `0x8C..=0x8F` → `914..=917` and `0x92..=0x95` →
//! `920..=923` — confirming the `(id − 0x81) + 903` arithmetic continues
//! through the gap exactly as the bracket-pinned base and high blocks
//! predicted. Both `0x4000` render-mode-node carriers (`0x8E → 916` Aluru,
//! `0x93 → 921` Iota) are among them, confirming those carriers are player
//! casts. Only `0x90 → 918` and `0x91 → 919` stay arithmetic-predicted.
//!
//! Disc + library gated: needs the extracted PROT (stager bytes) and the
//! mednafen save backups (mid-cast RAM). Skip-passes otherwise.

use std::path::PathBuf;

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::scene::ProtIndex;
use legaia_engine_core::summon::summon_stager_prot_entry;
use legaia_mednafen::{SaveState, ScenarioManifest, extract::ram_slice};

/// `(scenario label, loader-B id, extraction entry, spell id)` for the
/// catalogued evolved-Seru player casts. Eight of the ten legs are pinned; only
/// `0x90 → 918` and `0x91 → 919` remain arithmetic-predicted (no mid-cast yet).
const EVOLVED_CASTS: &[(&str, u16, u32, u8)] = &[
    ("gola_gola_summon_mid_cast", 19, 914, 0x8C), // Vahn, fire, "Spinning Flare"
    ("mushura_summon_mid_cast", 20, 915, 0x8D),   // Noa, earth, "Crazy Driver"
    ("aluru_summon_mid_cast", 21, 916, 0x8E),     // Vahn, light, "Final Blast" (0x4000 carrier)
    ("barra_summon_mid_cast", 22, 917, 0x8F),     // Gala, wind, "Hell Dive"
    ("slippery_summon_mid_cast", 25, 920, 0x92),  // Vahn, water, "Deadly Rain"
    ("iota_summon_mid_cast", 26, 921, 0x93),      // Vahn, earth, "Odd Dimension" (0x4000 carrier)
    ("puera_summon_mid_cast", 27, 922, 0x94),     // Noa, dark, "Dream Illusion"
    ("gilium_summon_mid_cast", 28, 923, 0x95),    // Noa, thunder, "Space Cannon"
];

/// Battle overlay loader-B current-id (`*DAT_8007BC4C`, the last stager id the
/// slot-B loader resolved).
const LOADER_B_VA: u32 = 0x8007_BC4C;

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

#[test]
fn evolved_seru_casts_byte_pin_their_predicted_stager() {
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

    let mut pinned = 0usize;

    for &(label, loader_b, extraction, spell_id) in EVOLVED_CASTS {
        let Some(scn) = manifest.scenarios.iter().find(|s| s.label == label) else {
            eprintln!("[skip] scenario {label} absent");
            continue;
        };
        let Some(save_path) = manifest.library_save_path(scn, &lib) else {
            eprintln!("[skip] {label}: no mednafen library backup");
            continue;
        };
        let save = SaveState::from_path(&save_path).expect("parse save");
        let ram = save.main_ram().expect("main RAM");

        // The arithmetic the whole run rides: extraction = loader_b + 895,
        // spell = loader_b + 0x79. Encode it so the constants can't drift.
        assert_eq!(
            extraction,
            loader_b as u32 + 895,
            "{label}: extraction must be loader_b + 895"
        );
        assert_eq!(
            spell_id,
            loader_b as u8 + 0x79,
            "{label}: spell id must be loader_b + 0x79"
        );

        // Loader-B current-id read mid-cast == the predicted leg.
        let lb = ram_slice(ram, LOADER_B_VA, LOADER_B_VA + 2).expect("loader-B window");
        let live = u16::from_le_bytes([lb[0], lb[1]]);
        assert_eq!(
            live, loader_b,
            "{label}: loader-B id {live} (0x{live:x}) != predicted {loader_b}",
        );

        // The engine maps the spell to the same entry.
        assert_eq!(
            summon_stager_prot_entry(spell_id),
            Some(extraction),
            "{label}: engine summon_stager_prot_entry(0x{spell_id:02x}) must map to {extraction}",
        );

        // The predicted stager is byte-resident at slot B (a genuine mid-cast,
        // not a stale loader-B tracker): trim the disc entry to its TOC-gap
        // footprint and compare against the resident slot-B window.
        let bytes = index
            .entry_bytes_lba_footprint(extraction)
            .expect("read stager footprint");
        let resident = ram_slice(
            ram,
            SUMMON_OVERLAY_LINK_BASE,
            SUMMON_OVERLAY_LINK_BASE + bytes.len() as u32,
        )
        .expect("resident stager window");
        let match_pct =
            resident.iter().zip(&bytes).filter(|(a, b)| a == b).count() as f64 / bytes.len() as f64;
        assert!(
            match_pct > 0.99,
            "{label}: predicted stager {extraction} not resident at slot B \
             (only {:.1}% byte-match)",
            match_pct * 100.0,
        );

        // And the entry is a stager (move-VM scene-graph), not some other asset.
        let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
        assert!(
            overlay.spawn_sites >= 4 && overlay.parts.len() >= 3,
            "{label}: PROT {extraction} should parse as a stager (got {} sites, {} parts)",
            overlay.spawn_sites,
            overlay.parts.len(),
        );

        eprintln!(
            "{label}: spell 0x{spell_id:02x} loader-B {live} -> PROT {extraction}; \
             resident {:.1}%, {} parts",
            match_pct * 100.0,
            overlay.parts.len(),
        );
        pinned += 1;
    }

    if pinned == 0 {
        eprintln!("[skip] no evolved-Seru mid-cast states available");
    }
}
