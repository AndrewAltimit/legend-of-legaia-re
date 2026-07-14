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
//! Plus the bonus game's two disc-sourced halves: the jackpot symbols' colours,
//! and the `1..=10` **numeral art** a bonus round swaps the reels onto.
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

/// The bonus round's reels are **the disc's own numerals**, not a font.
///
/// A bonus round swaps the reels onto the machine's second strip, whose values
/// are `0x10..=0x19`; `FUN_801d0fa8` takes `value >= 0x10` as the signal to
/// switch texpage (`0x0C` -> `0x0D`) and CLUT base (`0x7A80` -> `0x7AC0`), so
/// each numeral is a 64x64 cell of its own artwork on art-pack page 1, drawn
/// through its own palette column. This asserts that structure off the disc:
///
/// * all ten numerals `1..=10` decode as full 64x64 cells;
/// * each is an **inked white reel band** - fully opaque (that opacity is the
///   reel's white strip) with a dark glyph on it covering a plausible fraction;
/// * the ten glyphs are **differently coloured** - which is the falsifiable part.
///   The per-numeral CLUT column is load-bearing: sample one palette for all ten
///   and every digit on the reels comes out the same colour, which is not what
///   the machine shows;
/// * the grid **stops at ten**: the cells past the last numeral (values
///   `0x1B..=0x1E`) are empty, so the numeral bank is exactly `1..=10` and the
///   `1..=1000` payout bound follows from the art as well as from the code.
#[test]
fn the_bonus_reels_carry_ten_distinct_numerals_on_their_own_art_page() {
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

    // The numeral page is the pack's `0x0D` page - fb (832, 0), CLUT row 491.
    let page = art
        .get(minigame_art::SLOT_BONUS_PAGE)
        .expect("art pack has the bonus-numeral page");
    assert_eq!(
        (page.image.fb_x, page.image.fb_y),
        (832, 0),
        "the bonus page is the texpage 0x0D the reel renderer switches to"
    );

    // A numeral's **fill** colour: the mean of its saturated texels. The glyph is
    // a coloured body inside a dark outline, sitting on the reel's near-white
    // band - so keying on saturation (rather than on "dark") picks the body and
    // skips both the shared outline and the band.
    let mut fills = Vec::new();
    for n in 1..=minigame_art::SLOT_BONUS_NUMBER_COUNT {
        let sp = minigame_art::slot_bonus_number(&art, n).expect("numeral decodes");
        assert_eq!(
            (sp.width, sp.height),
            (64, 64),
            "numeral {n} is a 64x64 cell"
        );

        let (mut fill, mut ink, mut opaque) = (Vec::new(), 0usize, 0usize);
        for px in sp.rgba.chunks_exact(4) {
            if px[3] < 8 {
                continue;
            }
            opaque += 1;
            let (lo, hi) = (
                px[0].min(px[1]).min(px[2]) as i32,
                px[0].max(px[1]).max(px[2]) as i32,
            );
            let lum = (px[0] as u32 + px[1] as u32 + px[2] as u32) / 3;
            if lum < 200 {
                ink += 1; // the glyph: outline + body, against the pale band
            }
            if hi - lo > 40 && (40..235).contains(&lum) {
                fill.push([px[0] as u64, px[1] as u64, px[2] as u64]);
            }
        }
        assert_eq!(
            opaque,
            64 * 64,
            "numeral {n} is a solid reel band (every texel opaque)"
        );
        let frac = ink as f32 / (64.0 * 64.0);
        assert!(
            (0.02..0.60).contains(&frac),
            "numeral {n} has a glyph inked on the band (ink fraction {frac:.3})"
        );
        assert!(
            fill.len() > 64,
            "numeral {n}'s glyph has a coloured body ({} saturated texels)",
            fill.len()
        );
        let k = fill.len() as u64;
        fills.push([
            fill.iter().map(|c| c[0]).sum::<u64>() / k,
            fill.iter().map(|c| c[1]).sum::<u64>() / k,
            fill.iter().map(|c| c[2]).sum::<u64>() / k,
        ]);
    }

    // Each numeral has its own colour: no two fills coincide. (The cells are one
    // artwork set recoloured per palette column - decode them all through a
    // single CLUT and this collapses, which is exactly the bug it guards.)
    for a in 0..fills.len() {
        for b in (a + 1)..fills.len() {
            let d: i64 = (0..3)
                .map(|c| (fills[a][c] as i64 - fills[b][c] as i64).abs())
                .sum();
            assert!(
                d > 12,
                "numerals {} and {} are drawn in different colours (fill distance {d})",
                a + 1,
                b + 1
            );
        }
    }

    // And the bank stops at ten: read the cells past the last numeral **the way
    // the renderer would** - value `v` through CLUT column `v & 0xF` - and they
    // draw nothing at all. So `1..=10` is the whole numeral space the strip can
    // address, and the `1..=1000` payout bound follows from the art too.
    for value in 0x1Bu8..=0x1E {
        let pal = legaia_tim::decode_rgba8(page, (value & 0xF) as usize).expect("page 1 decodes");
        let (u, v) = ((value & 3) as usize * 0x40, (value & 0x0C) as usize * 0x10);
        let opaque = (0..64)
            .flat_map(|row| (0..64).map(move |col| (row, col)))
            .filter(|(row, col)| pal[(((v + row) * page.pixel_width()) + u + col) * 4 + 3] >= 8)
            .count();
        assert_eq!(
            opaque, 0,
            "value {value:#x} - past numeral 10 - draws nothing on its own CLUT"
        );
    }
}
