//! Disc-gated decode of the slot machine's cabinet body: PROT 1200's `TMD`
//! descriptor, the untextured mesh the overlay installs into the model bank.
//!
//! Asserts only the census `docs/subsystems/minigame-slot-machine.md` records
//! (76 prims = 38 tri + 38 quad, so 114 triangles once the quads split) and
//! that the body encloses the glass furniture; no Sony bytes are checked in.
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use std::path::PathBuf;

use legaia_asset::minigame_art::SLOT_ART_PROT_INDEX;
use legaia_asset::minigame_slot_scene as sc;
use legaia_prot::archive::Archive;

fn entry(index: usize) -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let prot = ["extracted/PROT.DAT", "../../extracted/PROT.DAT"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let e = archive
        .entries
        .iter()
        .find(|e| e.index as usize == index)
        .cloned()
        .expect("PROT entry present");
    let mut raw = Vec::new();
    archive.read_entry(&e, &mut raw).expect("read entry");
    Some(raw)
}

#[test]
fn the_cabinet_mesh_decodes_and_encloses_the_machine() {
    let Some(raw) = entry(SLOT_ART_PROT_INDEX) else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let mesh = sc::parse_cabinet(&raw).expect("cabinet TMD decodes");
    assert_eq!(mesh.tris.len(), 38 + 38 * 2, "38 tris + 38 quads");
    let xs = mesh.tris.iter().flat_map(|t| t.pos.iter().map(|p| p.x));
    let (lo, hi) = xs.fold((i16::MAX, i16::MIN), |(a, b), x| (a.min(x), b.max(x)));
    // The reels span x -512..512 and the paylines +-640; the body is wider.
    assert!(lo < -640 && hi > 640, "x span {lo}..{hi}");
    eprintln!("[ran] cabinet: {} triangles, x {lo}..{hi}", mesh.tris.len());
}

/// The landing-line table `FUN_801d2440` indexes by the per-spin `rand % 5`
/// decodes to the five paylines, in the order the engine's constant carries.
/// Also pins the dead developer-label run the byte accounting claims: seven
/// NUL-padded ASCII cells directly below the table.
#[test]
fn slot_landing_lines_match_the_retail_table() {
    let Some(raw) = entry(legaia_asset::slot_payout::SLOT_OVERLAY_PROT_INDEX) else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    assert_eq!(
        sc::parse_landing_lines(&raw),
        Some(sc::LANDING_LINE_BY_JITTER),
        "landing-line table decodes to the pinned payline order"
    );
    let labels = &raw[sc::DEV_LABELS_OFFSET..][..sc::DEV_LABEL_COUNT * sc::DEV_LABEL_STRIDE];
    for cell in labels.chunks(sc::DEV_LABEL_STRIDE) {
        let n = cell.iter().position(|&b| b == 0).expect("NUL-terminated");
        assert!(n > 0 && cell[..n].iter().all(|b| b.is_ascii_graphic()));
        assert!(cell[n..].iter().all(|&b| b == 0));
    }
    eprintln!("[ran] slot landing lines {:?}", sc::LANDING_LINE_BY_JITTER);
}
