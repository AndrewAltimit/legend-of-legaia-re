//! Disc + save-library gated: the world-map ocean CLUT cycle is live on
//! **all three kingdoms**.
//!
//! The cycle census (`vram_oracle::WORLD_MAP_CLUT_CYCLE_CELLS`, the
//! script-driven CLUT-cell effect family at rows 506/508/509 sourced from
//! 13-frame strips) was pinned from Drake (`map01`) captures. The resident
//! Sebucus / Karisto captures extend the verification: in each kingdom's
//! live VRAM the ocean-head CLUT row (`(0, 506)`, 16 entries) must hold
//! **one of the 13 animation frames** decoded from that kingdom's own
//! bundle (slot-0 TIM list, `legaia_asset::ocean::find_ocean_assets`) -
//! i.e. the same animator runs against per-kingdom strips, not just on
//! Drake.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` /
//! `scripts/scenarios.toml` / `saves/library` (CI runs without Sony bytes).

use std::path::PathBuf;

use legaia_engine_core::scene::ProtIndex;
use legaia_mednafen::{PsxGpu, SaveState, ScenarioManifest, VRAM_WIDTH};

/// (scenario label, kingdom bundle PROT entry, scene label) per kingdom.
const CAPTURES: &[(&str, u32, &str)] = &[
    ("sebucus_overworld_resident", 244, "map02"),
    ("karisto_overworld_resident", 391, "map03"),
];

/// Ocean head CLUT row target: VRAM `(0, 506)`, 16 BGR555 entries.
const OCEAN_ROW: usize = 506;
const FRAME_BYTES: usize = 32;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn manifest_path() -> Option<PathBuf> {
    [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}

fn library_dir() -> Option<PathBuf> {
    ["saves/library", "../saves/library", "../../saves/library"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.is_dir())
}

#[test]
fn ocean_clut_cycle_live_on_all_kingdoms() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let (Some(manifest_path), Some(library)) = (manifest_path(), library_dir()) else {
        eprintln!("[skip] scenarios manifest / saves library missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");
    let prot = std::fs::read(extracted.join("PROT.DAT")).expect("read PROT.DAT");
    let cdname = std::fs::read_to_string(extracted.join("CDNAME.TXT")).expect("read CDNAME.TXT");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("build ProtIndex");

    let mut checked = 0usize;
    for &(label, bundle_entry, scene) in CAPTURES {
        let Some(scn) = manifest.scenarios.iter().find(|s| s.label == label) else {
            continue;
        };
        let Ok(save_path) = manifest.mednafen_save_path(scn, Some(library.as_path())) else {
            continue;
        };
        if !save_path.exists() {
            continue;
        }
        let state = SaveState::from_path(&save_path).expect("parse save state");
        let gpu = PsxGpu::new(&state);
        let Some(vram) = gpu.vram_bytes() else {
            eprintln!("[skip] {label}: no VRAM section");
            continue;
        };

        // The kingdom bundle's own 13-frame strip, decoded from disc.
        let bundle = index
            .entry_bytes_extended(bundle_entry)
            .expect("read kingdom bundle");
        let slot0 =
            legaia_asset::kingdom_bundle::decode_slot(&bundle, 0).expect("decode bundle slot 0");
        let ocean = legaia_asset::ocean::find_ocean_assets(&slot0)
            .unwrap_or_else(|| panic!("{label}: bundle {bundle_entry} has no ocean assets"));
        let frames = &ocean.animation_frames;
        assert!(
            frames.len() >= FRAME_BYTES && frames.len().is_multiple_of(FRAME_BYTES),
            "{label}: bad animation table ({} bytes)",
            frames.len()
        );

        // Live ocean head row: 16 entries at VRAM (0, 506).
        let off = OCEAN_ROW * VRAM_WIDTH * 2;
        let live = &vram[off..off + FRAME_BYTES];
        let hit = frames.chunks(FRAME_BYTES).position(|frame| frame == live);
        assert!(
            hit.is_some(),
            "{label} ({scene}): live ocean CLUT head {} matches none of the {} bundle frames",
            live.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            frames.len() / FRAME_BYTES
        );
        eprintln!(
            "{label} ({scene}): live ocean head = bundle {bundle_entry} frame {} of {}",
            hit.unwrap(),
            frames.len() / FRAME_BYTES
        );
        checked += 1;
    }
    if checked == 0 {
        eprintln!("[skip] no resident kingdom captures available");
    }
}
