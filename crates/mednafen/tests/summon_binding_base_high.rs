//! Disc + library gated: the **base** player-summon block (spell ids
//! `0x82..=0x8B`) and the **high** evil-Seru block (`0x99..=0xA0`) byte-pin to
//! their predicted stager entries, the same evidential shape the evolved-Seru
//! block carries (`evolved_summon_binding`).
//!
//! These two blocks were historically described as "capture-pinned" but the
//! committed regression coverage only fixed the evolved-Seru gap in the middle.
//! This oracle closes that: it asserts, for every catalogued mid-cast state, the
//! battle overlay's loader-B current-id (`0x8007BC4C`) equals the predicted leg,
//! the predicted stager is byte-resident at the slot-B link base
//! `0x801F69D8` (a genuine mid-cast, not a stale loader-B tracker), and the
//! entry parses as a move-VM stager. Any layout drift in the stager arithmetic,
//! the `ProtIndex` TOC-gap footprint, or the spell→entry map then surfaces here
//! against real RAM rather than going silently wrong in-game.
//!
//! The unified arithmetic across all three blocks is `loader_b = spell − 0x79`,
//! `extraction = loader_b + 895` (retail `FUN_8003EC70(id − 0x79)` →
//! `(id − 0x79) + 0x381` raw-TOC, −2 to extraction space). The high block's
//! `0x4000` render-mode-node carriers (Palma 0928 / Mule 0929 / Jedo 0931) are
//! exercised by the stager-parse assertion here too.
//!
//! Gimard `0x81` (the base block's first leg) is catalogued only as a
//! PCSX-Redux state, so it is covered by the PCSX-side summon tests, not this
//! mednafen oracle. Skip-passes when the disc / library / extracted tree is
//! absent.

use std::path::PathBuf;

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::scene::ProtIndex;
use legaia_engine_core::summon::summon_stager_prot_entry;
use legaia_mednafen::{SaveState, ScenarioManifest, extract::ram_slice};

/// `(scenario label, loader-B id, extraction entry, spell id, creature)` for the
/// base player-summon casts `0x82..=0x8B` (Gimard `0x81` is PCSX-only).
const BASE_CASTS: &[(&str, u16, u32, u8, &str)] = &[
    ("theeder_summon_mid_cast", 9, 904, 0x82, "Theeder"),
    ("vera_summon_mid_cast", 10, 905, 0x83, "Vera"),
    ("gizam_summon_mid_cast", 11, 906, 0x84, "Gizam"),
    ("nighto_summon_mid_cast", 12, 907, 0x85, "Nighto"),
    ("zenoir_summon_mid_cast", 13, 908, 0x86, "Zenoir"),
    ("viguro_summon_mid_cast", 14, 909, 0x87, "Viguro"),
    ("swordie_summon_mid_cast", 15, 910, 0x88, "Swordie"),
    ("orb_summon_mid_cast", 16, 911, 0x89, "Orb"),
    ("freed_summon_mid_cast", 17, 912, 0x8A, "Freed"),
    ("nova_summon_mid_cast", 18, 913, 0x8B, "Nova"),
];

/// `(scenario label, loader-B id, extraction entry, spell id, creature)` for the
/// high evil-Seru casts `0x99..=0xA0`. Palma / Mule / Jedo carry the `0x4000`
/// render-mode nodes.
const HIGH_CASTS: &[(&str, u16, u32, u8, &str)] = &[
    ("juggernaut_summon_mid_cast", 32, 927, 0x99, "Juggernaut"),
    ("palma_summon_mid_cast", 33, 928, 0x9A, "Palma"),
    ("mule_summon_mid_cast", 34, 929, 0x9B, "Mule"),
    ("horn_summon_mid_cast", 35, 930, 0x9C, "Horn"),
    ("jedo_summon_mid_cast", 36, 931, 0x9D, "Jedo"),
    ("meta_summon_mid_cast", 37, 932, 0x9E, "Meta"),
    ("terra_summon_mid_cast", 38, 933, 0x9F, "Terra"),
    ("ozma_summon_mid_cast", 39, 934, 0xA0, "Ozma"),
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

/// Run the byte-pin assertions over one block's cast table; returns how many
/// legs were actually pinned (states present). Asserts hard on any mismatch.
fn pin_block(block: &str, casts: &[(&str, u16, u32, u8, &str)]) -> usize {
    let (Some(mpath), Some(lib), Some(root)) = (manifest_path(), library_dir(), extracted_root())
    else {
        eprintln!("[skip] scenarios.toml / saves/library / extracted missing");
        return 0;
    };
    let manifest = ScenarioManifest::from_path(&mpath).expect("parse manifest");
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");

    let mut pinned = 0usize;
    for &(label, loader_b, extraction, spell_id, creature) in casts {
        let Some(scn) = manifest.scenarios.iter().find(|s| s.label == label) else {
            eprintln!("[skip] {block}: scenario {label} absent");
            continue;
        };
        let Some(save_path) = manifest.library_save_path(scn, &lib) else {
            eprintln!("[skip] {block}: {label} has no mednafen library backup");
            continue;
        };
        let save = SaveState::from_path(&save_path).expect("parse save");
        let ram = save.main_ram().expect("main RAM");

        // The unified arithmetic the whole summon run rides — encoded so the
        // table constants can't silently drift apart.
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

        // The predicted stager is byte-resident at slot B: trim the disc entry
        // to its TOC-gap footprint and compare against the resident window.
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
            "{block}/{creature}: spell 0x{spell_id:02x} loader-B {live} -> PROT {extraction}; \
             resident {:.1}%, {} parts",
            match_pct * 100.0,
            overlay.parts.len(),
        );
        pinned += 1;
    }
    pinned
}

#[test]
fn base_summon_casts_byte_pin_their_predicted_stager() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    if pin_block("base", BASE_CASTS) == 0 {
        eprintln!("[skip] no base-block mid-cast states available");
    }
}

#[test]
fn high_summon_casts_byte_pin_their_predicted_stager() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    if pin_block("high", HIGH_CASTS) == 0 {
        eprintln!("[skip] no high-block mid-cast states available");
    }
}
