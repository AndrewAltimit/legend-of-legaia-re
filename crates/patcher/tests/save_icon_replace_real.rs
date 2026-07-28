//! Disc-gated end-to-end test for save-slot portrait replacement: patch one
//! portrait on a scratch copy of the disc, re-read the sheet off the patched
//! image, and confirm:
//!
//! - the replaced slot decodes to exactly the requested pixels;
//! - every other tile - including the blank tile 15 - is byte-identical, so
//!   the scattered 16-run write really is surgical;
//! - re-encoding a slot's own exported pixels is a no-op (zero changed bytes),
//!   which is what makes a shared PPF carry only a user's real edit;
//! - the blank tile is refused as a replacement target;
//! - every touched sector stays EDC/ECC-valid (the patcher re-encodes).
//!
//! No portrait bytes are committed: the test paints synthetic colours and
//! compares against the disc's own bytes read at runtime.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::save_icon::{TILE_COUNT, TILE_SIZE};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::save_icon::{self, SLOT_COUNT};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// A 16x16 RGBA test pattern using 4 distinct colours.
fn pattern() -> Vec<u8> {
    let mut out = Vec::with_capacity(TILE_SIZE * TILE_SIZE * 4);
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let c = match (x / 8, y / 8) {
                (0, 0) => [248, 0, 0, 255],
                (1, 0) => [0, 248, 0, 255],
                (0, 1) => [0, 0, 248, 255],
                _ => [248, 248, 248, 255],
            };
            out.extend_from_slice(&c);
        }
    }
    out
}

#[test]
fn save_icon_replace_is_surgical_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let before_patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let before = save_icon::read_sheet(&before_patcher).expect("sheet parses off retail disc");

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let slot = 5usize;
    let rgba = pattern();
    let outcome = save_icon::replace_slot(&mut patcher, slot, &rgba, false).expect("replace");
    assert_eq!(outcome.slot, slot);
    // One palette write plus one run per strip row.
    assert_eq!(outcome.touched_offsets.len(), TILE_SIZE + 1);
    assert_eq!(outcome.quantized_pixels, 0, "4 colours fit in 16 slots");

    let after = save_icon::read_sheet(&patcher).expect("sheet re-parses after patch");

    // The replaced slot decodes to the requested pixels (through the PSX
    // 15-bit rounding the encoder applies).
    let want: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|p| {
            legaia_tim::bgr555_to_rgba8(legaia_tim::encode::rgba8_to_bgr555(p.try_into().unwrap()))
        })
        .collect();
    assert_eq!(
        save_icon::export_slot(&after, slot).unwrap(),
        want,
        "patched slot decodes to the input image"
    );

    // Nothing else moved - including tile 15, which the write must not touch
    // even though it is adjacent in every strip row.
    for other in 0..TILE_COUNT {
        if other == slot {
            continue;
        }
        assert_eq!(
            before.tile_block_pixels(other).unwrap(),
            after.tile_block_pixels(other).unwrap(),
            "tile {other} pixels unchanged"
        );
        assert_eq!(
            before.tile_clut(other).unwrap(),
            after.tile_clut(other).unwrap(),
            "tile {other} palette unchanged"
        );
    }
    assert!(
        after.tile_is_blank(TILE_COUNT - 1).unwrap(),
        "the blank pad tile stays blank"
    );

    // Every touched sector must still be a valid Mode 2 sector.
    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len(), "no LBA moved");
    let reopened = DiscPatcher::open(patched).expect("patched image re-opens and validates");
    let reread = save_icon::read_sheet(&reopened).expect("sheet parses off the re-opened image");
    assert_eq!(
        reread.tile_block_pixels(slot).unwrap(),
        after.tile_block_pixels(slot).unwrap()
    );
}

#[test]
fn save_icon_reencoding_the_original_changes_nothing() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let sheet = save_icon::read_sheet(&patcher).expect("sheet parses");

    // Feed each slot its own exported pixels back. Because the encoder starts
    // from the tile's existing palette, this must reproduce the disc bytes -
    // otherwise a user's PPF would carry churn they did not author.
    for slot in 0..SLOT_COUNT {
        let rgba = save_icon::export_slot(&sheet, slot).expect("export");
        let outcome = save_icon::replace_slot(&mut patcher, slot, &rgba, false).expect("replace");
        assert_eq!(
            outcome.palette_entries_changed, 0,
            "slot {slot} palette should be untouched by a round-trip"
        );
    }
    let patched = patcher.into_image();
    assert_eq!(
        patched, original,
        "re-encoding every portrait unchanged must leave the image byte-identical"
    );
}

#[test]
fn save_icon_replace_refuses_the_unreachable_tile() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(original).expect("open disc");
    let err = save_icon::replace_slot(&mut patcher, SLOT_COUNT, &pattern(), false)
        .expect_err("tile 15 must be refused");
    assert!(
        err.to_string().contains("never be displayed"),
        "error should explain why: {err}"
    );
}
