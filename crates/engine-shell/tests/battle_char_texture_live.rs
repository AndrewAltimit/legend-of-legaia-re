//! Disc + save-library gated: the battle-character **texture-pool VRAM
//! placement** reproduces live battle VRAM byte-for-byte.
//!
//! The placement rule under test (see `docs/formats/battle-data-pack.md`
//! § Texture-pool VRAM placement): per present party member `p` (0-based
//! ordinal), `FUN_80052FA0` uploads seven blocks through the LoadImage
//! helper `FUN_80053B9C` - the two `record[0]` image blocks (inline rects)
//! plus each flagged equipment section's post-TMD pool at the static rect
//! table `0x800775B8`, every rect banded by `x += 0x200 + p*0x80`,
//! `y += 0x100`, with the block-prefix CLUT run STP-set onto row
//! `0x1E1 + p`. The seven rects tile the member's 128x256-halfword band
//! exactly.
//!
//! For each catalogued mid-battle capture, the party ids (`DAT_8007BD10`)
//! and equipped item ids (char record `+0x196`) are read from the live RAM,
//! the same uploads are decoded from the disc player files
//! (`legaia_asset::battle_char_assembly::character_texture_uploads`), and
//! every block's pixel + CLUT bytes are compared against the capture's GPU
//! VRAM at the computed coordinates.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` /
//! `scripts/scenarios.toml` / `saves/library` (CI runs without Sony bytes).

use std::path::PathBuf;

use legaia_asset::battle_char_assembly::character_texture_uploads;
use legaia_engine_core::scene::ProtIndex;
use legaia_mednafen::{PsxGpu, SaveState, ScenarioManifest, VRAM_WIDTH};

/// Catalogued mid-battle captures with the party texture bands resident.
/// (`v0_1_battle_first_frame_tetsu` is deliberately absent: captured on the
/// first battle frame, before the `FUN_80052FA0` upload pass has run, its
/// band still holds field-scene texels - ~20% residual match.)
const CAPTURES: &[&str] = &[
    "party_battle_gobu_gobu",
    "noa_levelup_fight_pre",
    "rim_elm_queen_bee_battle",
    // Noa + Terra party: covers the 4th playable character (char id 4,
    // player file 0866) - the band selector is the present-party ordinal,
    // so Terra bands like any other member.
    "terra_party_battle",
];

/// RAM offset of `DAT_8007BD10` (battle party char ids, 3 slots, 1-based;
/// 0 = empty - the loop bound of `FUN_80052FA0` / `FUN_80052770` case 1).
const PARTY_IDS: usize = 0x7BD10;
/// RAM offset of the live character-record array (`0x80084708`).
const CHAR_RECORDS: usize = 0x84708;
/// Character-record stride.
const CHAR_STRIDE: usize = 0x414;
/// Equipped-item bytes within a character record (`+0x196..+0x19B` - the
/// five ids `FUN_80052770` case 4 matches the descriptor table against).
const EQUIP_OFF: usize = 0x196;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

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
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

/// Count matching bytes between `data` (a `w x h` halfword block) and the
/// VRAM rect at `(fb_x, fb_y)`.
fn rect_match(vram: &[u8], fb_x: u16, fb_y: u16, w: u16, h: u16, data: &[u8]) -> (usize, usize) {
    let mut matched = 0usize;
    let mut total = 0usize;
    let row_bytes = w as usize * 2;
    for row in 0..h as usize {
        let src = &data[row * row_bytes..(row + 1) * row_bytes];
        let off = ((fb_y as usize + row) * VRAM_WIDTH + fb_x as usize) * 2;
        let dst = &vram[off..off + row_bytes];
        matched += src.iter().zip(dst).filter(|(a, b)| a == b).count();
        total += row_bytes;
    }
    (matched, total)
}

#[test]
fn battle_char_texture_placement_matches_live_vram() {
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
    for &label in CAPTURES {
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
        let ram = state.main_ram().expect("main RAM");
        let gpu = PsxGpu::new(&state);
        let Some(vram) = gpu.vram_bytes() else {
            eprintln!("[skip] {label}: no VRAM section");
            continue;
        };

        let party: Vec<u8> = ram[PARTY_IDS..PARTY_IDS + 3]
            .iter()
            .copied()
            .filter(|&c| c != 0)
            .collect();
        assert!(!party.is_empty(), "{label}: empty battle party");

        for (p, &cid) in party.iter().enumerate() {
            assert!((1..=4).contains(&cid), "{label}: bad char id {cid}");
            // Extraction entry = raw TOC (cid + 0x360) - 2 = 862 + cid.
            let file = index
                .entry_bytes_extended(862 + cid as u32)
                .expect("read player battle file");
            let pack = legaia_asset::battle_data_pack::parse(&file).expect("parse player file");
            let rec = CHAR_RECORDS + (cid as usize - 1) * CHAR_STRIDE;
            let equipped: [u8; 5] = ram[rec + EQUIP_OFF..rec + EQUIP_OFF + 5]
                .try_into()
                .unwrap();
            let uploads = character_texture_uploads(&file, &pack, &equipped, p as u8)
                .expect("decode texture uploads");
            // 2 record[0] blocks + one block per *flagged* section (the
            // `u16 @ +0x12` gate - unflagged sections upload nothing).
            assert!(
                (2..=7).contains(&uploads.len()),
                "{label}: char {cid} produced {} uploads (expected 2 + 0..=5)",
                uploads.len()
            );

            let mut matched = 0usize;
            let mut total = 0usize;
            for u in &uploads {
                let (pm, pt) = rect_match(vram, u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                matched += pm;
                total += pt;
                // CLUT run on row 0x1E1 + p.
                let cb = u.clut_bytes();
                if !cb.is_empty() {
                    let (m, t) =
                        rect_match(vram, u.clut_x, u.clut_row(), u.clut.len() as u16, 1, &cb);
                    matched += m;
                    total += t;
                }
                eprintln!(
                    "  [{label}] char {cid} slot {p} rect ({},{}) {}x{}: pixels {:.2}% clut_n {}",
                    u.fb_x(),
                    u.fb_y(),
                    u.rect.w,
                    u.rect.h,
                    100.0 * pm as f64 / pt as f64,
                    u.clut.len(),
                );
            }
            let pct = 100.0 * matched as f64 / total as f64;
            eprintln!("[{label}] char {cid} (slot {p}): band match {pct:.2}% ({matched}/{total})");
            assert!(
                pct >= 99.0,
                "{label}: char {cid} band match {pct:.2}% - placement rule broken?"
            );
        }
        checked += 1;
    }
    if checked == 0 {
        eprintln!("[skip] no catalogued battle captures present");
    } else {
        eprintln!("[ok] validated {checked} battle capture(s)");
    }
}
