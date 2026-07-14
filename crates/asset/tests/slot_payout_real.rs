//! Disc-gated reproducibility for the slot-machine payout table.
//!
//! Re-extract the slot-machine overlay (PROT 0975) from the user's `PROT.DAT`,
//! decode the payout table, and assert the structural invariants that bound it
//! to 10 entries (no Sony bytes asserted - the payout values stay on the disc):
//!
//! * 10 payout bytes, every one positive (a winning line always pays);
//! * the table is bounded: the 6 bytes after entry 9 are zero padding, and a
//!   printable-ASCII region (an unrelated overlay string) begins at `+0x10`, so
//!   the table does not run past symbol 9.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use std::path::PathBuf;

use legaia_asset::minigame_art;
use legaia_asset::slot_payout::{self, SLOT_SYMBOL_COUNT};
use legaia_asset::static_overlay;
use legaia_prot::archive::Archive;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn slot_overlay() -> Option<Vec<u8>> {
    let prot = prot_dat()?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let rec = static_overlay::overlay_map()
        .by_prot_index(slot_payout::SLOT_OVERLAY_PROT_INDEX as u32)
        .expect("slot overlay in static map");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .expect("PROT entry present");
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    Some(static_overlay::as_loaded(&raw, rec).expect("as-loaded form"))
}

#[test]
fn payout_table_reproduces_and_is_bounded() {
    let Some(overlay) = slot_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let table = slot_payout::parse(&overlay).expect("payout table parses");
    assert_eq!(table.payouts.len(), SLOT_SYMBOL_COUNT);
    for (id, &p) in table.payouts.iter().enumerate() {
        assert!(p > 0, "symbol {id} pays a positive amount");
    }

    // Bound: 6 zero-pad bytes after entry 9, then a printable string at +0x10.
    let off = slot_payout::SLOT_PAYOUT_FILE_OFFSET;
    let pad = &overlay[off + SLOT_SYMBOL_COUNT..off + 0x10];
    assert!(pad.iter().all(|&b| b == 0), "post-table padding is zero");
    let after = &overlay[off + 0x10..off + 0x18];
    assert!(
        after.iter().all(|&b| (0x20..0x7f).contains(&b)),
        "an unrelated string follows the table (table bounded at 10 entries)"
    );
}

/// The bonus-game trigger symbols, pinned to the disc art: symbol 8 is the
/// **blue "kick"** cell (1 bonus round), symbol 9 the **red "punch"** cell (3
/// bonus rounds). Decode the PROT 1200 reel art through each symbol's own CLUT
/// and assert the average opaque hue is blue-dominant for 8, red-dominant for 9
/// - so the player-facing rule maps onto the disc's own colours.
#[test]
fn kick_is_blue_punch_is_red_on_disc() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == minigame_art::SLOT_ART_PROT_INDEX as u32)
        .cloned()
        .expect("PROT 1200 art entry present");
    let mut raw = Vec::new();
    archive
        .read_entry(&entry, &mut raw)
        .expect("read art entry");
    let art = minigame_art::parse_art_pack(&raw).expect("art pack decodes");

    // Average opaque colour of one reel symbol cell, drawn through its own CLUT.
    let avg = |sym: usize| -> (u64, u64, u64) {
        let sp = minigame_art::slot_symbol(&art, sym).expect("symbol decodes");
        let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
        for px in sp.rgba.chunks_exact(4) {
            if px[3] < 8 {
                continue; // ignore transparent texels
            }
            r += px[0] as u64;
            g += px[1] as u64;
            b += px[2] as u64;
            n += 1;
        }
        assert!(n > 0, "symbol {sym} has opaque pixels");
        (r / n, g / n, b / n)
    };

    let (kr, kg, kb) = avg(slot_payout::KICK_SYMBOL_ID as usize);
    assert!(
        kb > kr && kb > kg,
        "the kick symbol ({}) is blue-dominant, avg=({kr},{kg},{kb})",
        slot_payout::KICK_SYMBOL_ID
    );
    let (pr, pg, pb) = avg(slot_payout::PUNCH_SYMBOL_ID as usize);
    assert!(
        pr > pb && pr > pg,
        "the punch symbol ({}) is red-dominant, avg=({pr},{pg},{pb})",
        slot_payout::PUNCH_SYMBOL_ID
    );
}
