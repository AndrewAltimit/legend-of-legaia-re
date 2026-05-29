//! Disc-gated regression for [`legaia_asset::baka_fighter_pack`].
//!
//! Pins the on-disc layout of PROT 1204 (`other5`): five battle-form character
//! TMD chunks at the streaming offsets the doc page lists + seven 256x256 4bpp
//! TIM atlases at fixed `0x8224` stride. Skips when `LEGAIA_DISC_BIN` is
//! unset so CI works without redistributing Sony data.

use legaia_asset::baka_fighter_pack::{
    ATLAS_CLUT_ROWS, ATLAS_COUNT, ATLAS_STRIDE_BYTES, BATTLE_TMD_CHUNK_TYPE, FIRST_ATLAS_OFFSET,
    PROT_ENTRY_INDEX, SLOT_COUNT, parse, slot_label,
};
use std::path::PathBuf;

/// Pinned on-disc TMD body byte sizes (5 slots) — matches the streaming
/// chunk sizes `0x82EC`, `0x8364`, `0x60CC`, `0x699C`, `0x823C`.
const EXPECTED_BODY_SIZES: [usize; SLOT_COUNT] = [33516, 33636, 24780, 27036, 33340];

/// Pinned on-disc `nobj` (TMD header `+0x08`) per slot.
const EXPECTED_NOBJ: [u32; SLOT_COUNT] = [15, 16, 15, 20, 15];

/// Pinned absolute file offsets of each TMD body inside PROT 1204.
const EXPECTED_BODY_OFFSETS: [usize; SLOT_COUNT] = [0x4, 0x82F4, 0x1065C, 0x1672C, 0x1D0CC];

fn extracted_root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let prot = repo.join("extracted").join("PROT");
    prot.is_dir().then_some(prot)
}

fn locate_prot_1204() -> Option<PathBuf> {
    let prot_dir = extracted_root()?;
    let entries = std::fs::read_dir(&prot_dir).ok()?;
    for e in entries.flatten() {
        let name = e.file_name();
        let s = name.to_string_lossy();
        if s.starts_with(&format!("{PROT_ENTRY_INDEX:04}_")) && s.ends_with(".BIN") {
            return Some(e.path());
        }
    }
    None
}

#[test]
fn real_pack_layout() {
    let Some(path) = locate_prot_1204() else {
        eprintln!("LEGAIA_DISC_BIN or extracted/PROT not available; skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read PROT 1204");
    let pack = parse(&bytes).expect("parse PROT 1204 as battle character pack");

    // Slot pin: count + sizes + nobj + file offsets.
    assert_eq!(pack.slots.len(), SLOT_COUNT);
    for (i, slot) in pack.slots.iter().enumerate() {
        assert_eq!(slot.slot, i, "slot index round-trip");
        assert_eq!(
            slot.disc_nobj,
            EXPECTED_NOBJ[i],
            "disc nobj for slot {i} ({})",
            slot_label(i)
        );
        assert_eq!(
            slot.tmd_bytes.len(),
            EXPECTED_BODY_SIZES[i],
            "TMD body size for slot {i} ({})",
            slot_label(i)
        );
        assert_eq!(
            slot.file_offset,
            EXPECTED_BODY_OFFSETS[i],
            "file offset for slot {i} ({})",
            slot_label(i)
        );
        // Each TMD parses cleanly with the canonical Legaia TMD walker.
        let parsed =
            legaia_tmd::parse(&slot.tmd_bytes).expect("Legaia TMD parse for battle character");
        assert_eq!(parsed.objects.len(), EXPECTED_NOBJ[i] as usize);
    }

    // Atlas pin: 7 TIMs at stride 0x8224 starting at 0x25804, CLUTs at
    // y=490..495,497.
    assert_eq!(pack.atlases.len(), ATLAS_COUNT);
    for (i, atlas) in pack.atlases.iter().enumerate() {
        assert_eq!(atlas.atlas_index, i);
        assert_eq!(
            atlas.file_offset,
            FIRST_ATLAS_OFFSET + i * ATLAS_STRIDE_BYTES,
            "atlas {i} file offset"
        );
        assert_eq!(atlas.clut_fb_y, ATLAS_CLUT_ROWS[i], "atlas {i} CLUT row");
        // TIM magic check (parse byte 0..4 inline; legaia_tim parses too but
        // its full image-block validation isn't needed here).
        assert_eq!(
            &atlas.tim_bytes[..4],
            [0x10, 0, 0, 0],
            "atlas {i} TIM magic"
        );
    }

    // Sanity: streaming chunks are type 0x09 (the dispatcher tag for
    // battle-form character TMDs).
    assert_eq!(BATTLE_TMD_CHUNK_TYPE, 0x09);
}
