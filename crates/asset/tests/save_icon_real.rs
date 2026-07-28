//! Disc-gated regression for [`legaia_asset::save_icon`]: the save-slot
//! portrait sheet in the menu overlay (PROT entry 899).
//!
//! Pins the sheet's location-by-fingerprint, the strip/CLUT rects, the
//! save-block de-interleave, and the two facts a modder needs - that tile
//! 15 is blank and that the boot-resident system-UI bundle carries tiles
//! 0..2 (and only those) already in contiguous tile layout.
//!
//! No portrait or palette bytes are asserted literally; every fixture is
//! derived from the disc at runtime. Skips silently when
//! `LEGAIA_DISC_BIN` is unset or the extraction is missing.

use std::path::PathBuf;

use legaia_asset::{save_icon, system_ui_bundle};

fn extracted(rel: &str) -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let f = PathBuf::from(base).join(rel);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn menu_entry() -> Option<Vec<u8>> {
    let p = extracted("PROT/0899_xxx_dat.BIN")?;
    std::fs::read(p).ok()
}

#[test]
fn save_icon_sheet_parses_from_the_menu_overlay() {
    let Some(entry) = menu_entry() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT/0899_xxx_dat.BIN missing");
        return;
    };

    // The fingerprint scan finds the sheet, and it agrees with the
    // documented constant (which is what the patcher addresses).
    let off = save_icon::find_in_entry(&entry).expect("sheet located by rect fingerprint");
    assert_eq!(
        off,
        save_icon::PROT_ENTRY_OFFSET,
        "sheet offset in entry 899"
    );

    let sheet = save_icon::parse_entry(&entry).expect("sheet parses");
    assert_eq!(sheet.entry_offset, save_icon::PROT_ENTRY_OFFSET);
    assert_eq!(sheet.tim.pixel_width(), 256, "strip is 256 px wide");
    assert_eq!(sheet.tim.pixel_height(), 16, "strip is one tile tall");
    assert_eq!(
        sheet.tim.palette_count(),
        save_icon::TILE_COUNT,
        "one 16-colour palette per tile"
    );
    assert_eq!(
        sheet.tile_clut_offset(0),
        save_icon::PROT_ENTRY_CLUT_DATA_OFFSET
    );
    assert_eq!(
        sheet.tile_pixel_run_offsets(0)[0],
        save_icon::PROT_ENTRY_PIXEL_DATA_OFFSET
    );
}

#[test]
fn save_icon_tile_fifteen_is_the_blank_pad_and_the_rest_are_not() {
    let Some(entry) = menu_entry() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT/0899_xxx_dat.BIN missing");
        return;
    };
    let sheet = save_icon::parse_entry(&entry).expect("sheet parses");

    // Exactly one blank tile, and it is the last one - so the 15 reachable
    // save slots all land on real art.
    for tile in 0..save_icon::USABLE_TILE_COUNT {
        assert!(
            !sheet.tile_is_blank(tile).unwrap(),
            "tile {tile} should carry a portrait"
        );
    }
    assert!(
        sheet.tile_is_blank(save_icon::TILE_COUNT - 1).unwrap(),
        "tile 15 is the width pad: flat pixel index + all-zero palette"
    );

    // Every reachable tile is distinct from every other - the sheet is 15
    // different portraits, not a repeat.
    let mut seen: Vec<[u8; save_icon::TILE_BLOCK_BYTES]> = Vec::new();
    for tile in 0..save_icon::USABLE_TILE_COUNT {
        let px = sheet.tile_block_pixels(tile).unwrap();
        assert!(
            !seen.contains(&px),
            "tile {tile} duplicates an earlier tile"
        );
        seen.push(px);
    }
}

#[test]
fn save_icon_deinterleave_matches_the_boot_resident_contiguous_copies() {
    let Some(entry) = menu_entry() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT/0899_xxx_dat.BIN missing");
        return;
    };
    let Some(prot) = extracted("PROT.DAT") else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let sheet = save_icon::parse_entry(&entry).expect("sheet parses");
    let bundle = system_ui_bundle::read_from_prot_dat(&prot).expect("system-UI bundle parses");

    // Raw TOC entry 0 carries three standalone 16x16 4bpp TIMs uploaded to
    // (976/980/984, 256). Each is one strip tile already in contiguous tile
    // layout, so it is an independent oracle for the de-interleave: if our
    // row-gather were wrong these would not match byte for byte.
    let singles: Vec<_> = bundle
        .tims
        .iter()
        .filter(|m| {
            m.raw_entry == 0
                && m.tim.image.h == 16
                && m.tim.image.fb_w == 4
                && m.tim.image.fb_y == 256
        })
        .collect();
    assert_eq!(
        singles.len(),
        3,
        "three boot-resident single-tile portraits"
    );

    for (tile, m) in singles.iter().enumerate() {
        assert_eq!(
            m.tim.image.fb_x,
            976 + (tile as u16) * 4,
            "boot copy {tile} VRAM x"
        );
        assert_eq!(
            m.tim.image.data,
            sheet.tile_block_pixels(tile).unwrap().to_vec(),
            "boot copy {tile} pixels equal the de-interleaved strip tile"
        );
        let clut = m.tim.clut.as_ref().expect("boot copy has a CLUT");
        assert_eq!(
            clut.entries,
            sheet.tile_clut(tile).unwrap().to_vec(),
            "boot copy {tile} palette equals strip palette {tile}"
        );
        // Its CLUT lives on its own row, not on the strip's row 1.
        assert_eq!((clut.fb_x, clut.fb_y), (976, 304 + tile as u16));
    }
}

#[test]
fn save_icon_slot_rects_stay_inside_the_strip() {
    // A pure-arithmetic guard that needs no disc: the 15 reachable slots
    // must all address halfwords inside the declared image rect.
    let (x0, y0, w, h) = save_icon::IMAGE_RECT;
    for slot in 0..save_icon::USABLE_TILE_COUNT {
        let (x, y, rw, rh) = save_icon::slot_vram_rect(slot);
        assert_eq!(save_icon::tile_for_slot(slot), slot);
        assert!(x >= x0 && x + rw <= x0 + w, "slot {slot} x range");
        assert_eq!((y, rh), (y0, h));
    }
}
