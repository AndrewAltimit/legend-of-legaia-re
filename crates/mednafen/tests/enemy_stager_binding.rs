//! Disc + library gated: ordinary (non-final-boss) enemy special attacks ride
//! the same loader-B stager mechanism as the player summons and the final-boss
//! Cort specials.
//!
//! The enemy-cast stager path was pinned for Cort (`enemy_stager_real`,
//! `ENEMY_BOSS_STAGER_PROT`). These mid-cast states extend it to two ordinary
//! bosses — the Delilas brothers and Zeto — confirming the mechanism is not
//! Cort-specific. For each, the loader-B current-id (`0x8007BC4C`) read mid-cast
//! resolves the stager entry on the universal `extraction = id + 895`
//! arithmetic, and that entry is byte-resident at the slot-B link base
//! `0x801F69D8` and parses as a move-VM stager:
//!
//! ```text
//!   Gi Delilas  / Blazing Slash  id 0x3F -> 0958
//!   Che Delilas / Megaton Press  id 0x40 -> 0959
//!   Lu Delilas  / Plasma Strike  id 0x41 -> 0960
//!   Zeto        / Call Wave       id 0x33 -> 0946
//!   Zeto        / Big Wave        id 0x33 -> 0946   (same stager)
//! ```
//!
//! Zeto's Call Wave and Big Wave are one logical attack spread over two turns
//! (Call Wave summons the wave one turn, Big Wave unleashes it the next), so
//! they legitimately share the single stager 0946 — the matching loader-B id
//! across both states is the move's identity, not a stale tracker.
//!
//! None of these stagers carries a `0x4000` render-mode node, and at the
//! captured instants the part-actor pool `DAT_801C90F0` is empty — so they do
//! not seat a live render-mode part either (logged, not asserted: pool
//! occupancy is instant-specific). The render-mode draw still needs an enemy
//! whose stager carries a `0x4000` record with a live part seated.
//!
//! Disc + library gated: needs the extracted PROT and the mednafen save
//! backups. Skip-passes otherwise.

use std::path::PathBuf;

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::scene::ProtIndex;
use legaia_mednafen::{SaveState, ScenarioManifest, extract::ram_slice};

/// `(scenario label, loader-B id, extraction entry)` for the catalogued
/// non-final-boss enemy special-attack mid-casts.
const ENEMY_CASTS: &[(&str, u16, u32)] = &[
    ("gi_delilas_blazing_slash_mid_cast", 0x3F, 958),
    ("che_delilas_megaton_press_mid_cast", 0x40, 959),
    ("lu_delilas_plasma_strike_mid_cast", 0x41, 960),
    ("zeto_call_wave_mid_cast", 0x33, 946),
    ("zeto_big_wave_mid_cast", 0x33, 946),
];

/// Battle overlay loader-B current-id (`*DAT_8007BC4C`).
const LOADER_B_VA: u32 = 0x8007_BC4C;
/// Part-actor pool base (`DAT_801C90F0`, 0x60 u32 slots).
const PART_POOL_VA: u32 = 0x801C_90F0;
const PART_POOL_SLOTS: u32 = 0x60;

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
fn enemy_specials_byte_pin_their_stager() {
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

    for &(label, loader_b, extraction) in ENEMY_CASTS {
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

        // The universal enemy arithmetic: extraction = loader_b + 895.
        assert_eq!(
            extraction,
            loader_b as u32 + 895,
            "{label}: extraction must be loader_b + 895"
        );

        // Loader-B current-id read mid-cast == the predicted entry.
        let lb = ram_slice(ram, LOADER_B_VA, LOADER_B_VA + 2).expect("loader-B window");
        let live = u16::from_le_bytes([lb[0], lb[1]]);
        assert_eq!(
            live, loader_b,
            "{label}: loader-B id 0x{live:x} != predicted 0x{loader_b:x}",
        );

        // The predicted stager is byte-resident at slot B (a genuine mid-cast).
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
            "{label}: stager {extraction} not resident at slot B ({:.1}% byte-match)",
            match_pct * 100.0,
        );

        // It parses as a move-VM stager, and carries no 0x4000 render-mode node.
        let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
        assert!(
            overlay.spawn_sites >= 4 && overlay.parts.len() >= 3,
            "{label}: PROT {extraction} should parse as a stager (got {} sites, {} parts)",
            overlay.spawn_sites,
            overlay.parts.len(),
        );
        let render_nodes = overlay
            .parts
            .iter()
            .filter(|p| p.is_render_mode_node())
            .count();
        assert_eq!(
            render_nodes, 0,
            "{label}: PROT {extraction} unexpectedly carries 0x4000 render-mode records",
        );

        // Pool occupancy is instant-specific — log it, don't assert it.
        let pool = ram_slice(ram, PART_POOL_VA, PART_POOL_VA + PART_POOL_SLOTS * 4)
            .expect("part pool window");
        let live_parts = (0..PART_POOL_SLOTS as usize)
            .filter(|&k| u32::from_le_bytes(pool[k * 4..k * 4 + 4].try_into().unwrap()) != 0)
            .count();

        eprintln!(
            "{label}: loader-B 0x{live:x} -> PROT {extraction}; resident {:.1}%, \
             {} parts, {render_nodes} render-mode, {live_parts} live pool slots",
            match_pct * 100.0,
            overlay.parts.len(),
        );
        pinned += 1;
    }

    if pinned == 0 {
        eprintln!("[skip] no enemy special-attack mid-cast states available");
    }
}
