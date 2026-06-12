//! Disc + save-library gated: the battle **facial-animator stamp** is
//! byte-visible in live battle VRAM.
//!
//! The mechanism under test (`FUN_8004C7B4`, `legaia_asset::face_anim`):
//! every frame, the current eye and mouth face frames are `MoveImage`-copied
//! from the member band's face-frame strip onto the band's live face rows.
//! Invariant: in any mid-battle capture, each animated member's live face
//! rows must byte-equal the strip content of SOME frame of the per-character
//! `SCUS_942.54` frame table (whichever frame the animator stamped last) —
//! for the eyes AND the mouth, at the documented destination rects.
//!
//! Terra (char id 4) is excluded: the retail animator skips char index 3
//! and the tables carry no row for her.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` /
//! `scripts/scenarios.toml` / `saves/library` (CI runs without Sony bytes).

use std::path::PathBuf;

use legaia_asset::face_anim::{EYE_FRAME_COUNT, FaceFrameTables, FaceStamp, MOUTH_FRAME_COUNT};
use legaia_mednafen::{PsxGpu, SaveState, ScenarioManifest, VRAM_WIDTH};

/// Catalogued mid-battle captures with the party texture bands resident
/// (the same set the texture-placement oracle uses).
const CAPTURES: &[&str] = &[
    "party_battle_gobu_gobu",
    "noa_levelup_fight_pre",
    "rim_elm_queen_bee_battle",
    "terra_party_battle",
];

/// RAM offset of `DAT_8007BD10` (battle party char ids, 3 slots, 1-based).
const PARTY_IDS: usize = 0x7BD10;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.join("SCUS_942.54").is_file())
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

/// Resolve one eye frame's stamp rect (absolute VRAM coords) from the
/// public table fields, exactly like `FaceFrameTables::stamps` does.
fn eye_stamp(t: &FaceFrameTables, c: usize, p: usize, f: usize) -> FaceStamp {
    let (sx, sy) = t.eye_src[c][f];
    let g = t.eye_geo[c];
    let (dx, dy) = t.slot_delta[p];
    FaceStamp {
        src_x: (sx + dx) as u16,
        src_y: (sy + dy) as u16,
        w: g.w,
        h: g.h,
        dst_x: (g.dest_x + dx) as u16,
        dst_y: (g.dest_y + dy) as u16,
    }
}

/// Resolve one mouth frame's stamp rect.
fn mouth_stamp(t: &FaceFrameTables, c: usize, p: usize, f: usize) -> FaceStamp {
    let (sx, sy) = t.mouth_src[c][f];
    let g = t.mouth_geo[c];
    let (dx, dy) = t.slot_delta[p];
    FaceStamp {
        src_x: (sx + dx) as u16,
        src_y: (sy + dy) as u16,
        w: g.w,
        h: g.h,
        dst_x: (g.dest_x + dx) as u16,
        dst_y: (g.dest_y + dy) as u16,
    }
}

/// Whether the live VRAM bytes at the stamp's destination rect equal the
/// live bytes at its source rect (i.e. the dest holds a copy of that frame).
fn stamp_matches(vram: &[u8], s: &FaceStamp) -> bool {
    for row in 0..s.h as usize {
        let so = ((s.src_y as usize + row) * VRAM_WIDTH + s.src_x as usize) * 2;
        let don = ((s.dst_y as usize + row) * VRAM_WIDTH + s.dst_x as usize) * 2;
        let n = s.w as usize * 2;
        if vram[so..so + n] != vram[don..don + n] {
            return false;
        }
    }
    true
}

#[test]
fn live_face_rows_hold_a_stamped_face_frame() {
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
    let scus = std::fs::read(extracted.join("SCUS_942.54")).expect("read SCUS");
    let tables = FaceFrameTables::from_scus(&scus).expect("parse face tables");

    let mut checked = 0usize;
    for &label in CAPTURES {
        let Some(scn) = manifest.scenarios.iter().find(|s| s.label == label) else {
            continue;
        };
        let Some(save_path) = manifest.library_save_path(scn, library.as_path()) else {
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
        for (p, &cid) in party.iter().enumerate() {
            // The animator covers chars 0..2 (ids 1..=3); Terra is skipped.
            if !(1..=3).contains(&cid) {
                continue;
            }
            let c = cid as usize - 1;
            // The dest face rows must hold a copy of SOME frame of each
            // feature's strip (whichever the animator stamped last).
            let eye_frame =
                (0..EYE_FRAME_COUNT).find(|&f| stamp_matches(vram, &eye_stamp(&tables, c, p, f)));
            let mouth_frame = (0..MOUTH_FRAME_COUNT)
                .find(|&f| stamp_matches(vram, &mouth_stamp(&tables, c, p, f)));
            assert!(
                eye_frame.is_some(),
                "{label}: char {cid} slot {p}: live eye rows match no frame"
            );
            assert!(
                mouth_frame.is_some(),
                "{label}: char {cid} slot {p}: live mouth rows match no frame"
            );
            eprintln!(
                "[{label}] char {cid} slot {p}: eyes = frame {:?}, mouth = frame {:?}",
                eye_frame, mouth_frame
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
