//! Disc-gated tests for the accent font (`translation::accents`,
//! `legaia_font::accent_font`):
//!
//! - the retail USA font page carries ink in some high cells but gives most
//!   of them a zero advance, and the retail disc does not read as patched;
//! - writing the accent font draws every recipe cell with a non-zero
//!   advance, leaves every ASCII cell and every other advance untouched, and
//!   keeps every touched sector EDC/ECC-valid;
//! - an accented line lays out to the width of its ASCII fold (each accent
//!   cell takes its base letter's advance);
//! - a pack with `accents: font` imports its typed accents as cell bytes and
//!   writes the font once; a second import finds it already there.
//!
//! Only counts, widths and shape statistics are asserted - no glyph bytes.
//! Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_font::accent_font::{AccentFontState, CellDraw, FontPage};
use legaia_font::{Font, MeasureOptions, latin};
use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::translation::accents::{self, AccentMode, DiscFont};
use legaia_patcher::translation::markup::{self, Target};
use legaia_patcher::translation::{export_pack, import_pack};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn preview_font(f: &DiscFont) -> Font {
    Font::from_disc_tim_and_scus(&f.tim, &f.scus).expect("font")
}

#[test]
fn retail_font_is_not_patched_and_its_high_cells_overprint() {
    let Some(image) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let p = DiscPatcher::open(image).unwrap();
    let f = DiscFont::read(&p).unwrap();
    assert_eq!(f.state(), AccentFontState::Absent);
    let inked: Vec<u8> = (0x80..=0xFFu8).filter(|&b| f.page.has_ink(b)).collect();
    let overprint = inked
        .iter()
        .filter(|&&b| f.draw(b) == CellDraw::Overprints)
        .count();
    eprintln!(
        "[ok] retail high cells: {} inked of 128, {} of those with a zero advance",
        inked.len(),
        overprint
    );
    // The retail page draws accented letters in the CP437 cells but the
    // advance table leaves them at zero: e-acute is the canonical case.
    assert!(f.page.has_ink(0x82));
    assert_eq!(f.draw(0x82), CellDraw::Overprints);
    assert!(inked.len() >= 20 && overprint * 2 > inked.len());
}

#[test]
fn accent_font_draws_every_recipe_cell_and_touches_nothing_else() {
    let Some(image) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut p = DiscPatcher::open(image.clone()).unwrap();
    let before = DiscFont::read(&p).unwrap();
    let rep = accents::apply_accent_font(&mut p).unwrap();
    assert!(!rep.already);
    let after = DiscFont::read(&p).unwrap();
    assert_eq!(after.state(), AccentFontState::Applied);

    let recipe: Vec<u8> = latin::LATIN_CELLS
        .iter()
        .filter(|c| c.recipe.is_some())
        .map(|c| c.byte)
        .collect();
    assert_eq!(rep.cells, recipe.len());
    for &b in &recipe {
        assert_eq!(after.draw(b), CellDraw::Draws, "cell {b:02x}");
    }
    for b in 0x20u8..=0x7E {
        assert_eq!(
            before.page.has_ink(b),
            after.page.has_ink(b),
            "ASCII cell {b:02x}"
        );
    }
    let ascii_same = |pg: &FontPage| {
        // Re-pack both pages and compare the ASCII rows (cells 0x20..0x7F sit
        // in page rows 0..0x60).
        let mut tim = before.tim.clone();
        pg.write_into_tim(&mut tim).unwrap();
        tim
    };
    let (tb, ta) = (ascii_same(&before.page), ascii_same(&after.page));
    // 8-byte header + 44-byte CLUT block + 12-byte image header.
    let img = 64;
    assert_eq!(tb[img..img + 0x60 * 128], ta[img..img + 0x60 * 128]);
    for b in 0..=255u8 {
        if !recipe.contains(&b) {
            assert_eq!(
                before.widths[b as usize], after.widths[b as usize],
                "{b:02x}"
            );
        }
    }

    // Every changed sector re-encoded.
    let patched = p.into_image();
    let mut changed = 0;
    for (a, b) in image.chunks(SECTOR_SIZE).zip(patched.chunks(SECTOR_SIZE)) {
        if a != b {
            changed += 1;
            assert!(mode2_form1_sector_is_valid(b));
        }
    }
    eprintln!(
        "[ok] accent font: {} cells, {} bytes written, {changed} sectors",
        rep.cells, rep.bytes_written
    );
    assert!(changed > 0);

    // Idempotent.
    let mut p2 = DiscPatcher::open(patched).unwrap();
    assert!(accents::apply_accent_font(&mut p2).unwrap().already);
}

#[test]
fn accented_line_measures_like_its_fold() {
    let Some(image) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut p = DiscPatcher::open(image).unwrap();
    accents::apply_accent_font(&mut p).unwrap();
    let font = preview_font(&DiscFont::read(&p).unwrap());
    let opts = MeasureOptions::dialog();
    for line in [
        "\u{c9}p\u{e9}e d'acier, \u{e0} bient\u{f4}t",
        "Gr\u{fc}\u{df}e aus K\u{f6}ln",
        "\u{bf}Ad\u{f3}nde est\u{e1} el ni\u{f1}o?",
        "N\u{e3}o, cora\u{e7}\u{e3}o",
    ] {
        let bytes = accents::encode_as_imported(line, Target::Segment, AccentMode::Font).unwrap();
        assert!(bytes.iter().any(|&b| b >= 0x80), "{line}");
        let (fold, _) = accents::fold_text(line);
        let fold_bytes = markup::encode(&fold, Target::Segment).unwrap();
        let w = font.measure(&bytes, &opts).max_px;
        let wf = font.measure(&fold_bytes, &opts).max_px;
        // One-to-one folds measure identically; a two-letter fold (ss) is
        // wider than the drawn sharp s.
        if fold_bytes.len() == bytes.len() {
            assert_eq!(w, wf, "{line}");
        } else {
            assert!(w < wf && w > 0, "{line}: {w} vs {wf}");
        }
        eprintln!("[ok] {} bytes, {w} px (fold {wf} px)", bytes.len());
    }
}

#[test]
fn font_mode_pack_imports_cells_and_writes_the_font_once() {
    let Some(image) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let src = DiscPatcher::open(image.clone()).unwrap();
    let mut pack = export_pack(&src).unwrap().into_skeleton("fr", Vec::new());
    drop(src);
    // A short item name with room for it (a same-length rename).
    let key = {
        let e = pack
            .sections
            .items
            .iter_mut()
            .find(|e| e.budget >= 6)
            .expect("an item with 6 bytes of room");
        e.translation = "\u{c9}p\u{e9}e".to_string();
        e.key.clone()
    };
    pack.accents = AccentMode::Font.header().to_string();

    let mut p = DiscPatcher::open(image.clone()).unwrap();
    let rep = import_pack(&mut p, &pack).unwrap();
    assert!(rep.issues.is_empty(), "{:?}", rep.issues);
    assert_eq!(rep.accents.cells, 2);
    let font = rep.accent_font.as_ref().expect("font written");
    assert!(!font.already);
    let va = u32::from_str_radix(key.trim_start_matches("scus:str:0x"), 16).unwrap();
    let scus = p.read_named_file("SCUS_942.54").unwrap();
    let off = legaia_asset::item_names::file_offset_for_va(&scus, va).unwrap();
    assert_eq!(&scus[off..off + 5], &[0x90, b'p', 0x82, b'e', 0x00]);

    // Strict mode reports the same line as not encodable instead.
    pack.accents.clear();
    let mut q = DiscPatcher::open(image).unwrap();
    let strict = import_pack(&mut q, &pack).unwrap();
    assert_eq!(strict.issues.len(), 1);
    assert!(strict.issues[0].1.contains("accent font"));

    // Re-import in font mode on the patched disc: the font is already there.
    pack.accents = AccentMode::Font.header().to_string();
    let mut r = DiscPatcher::open(p.into_image()).unwrap();
    let again = import_pack(&mut r, &pack).unwrap();
    assert!(again.accent_font.unwrap().already);
    eprintln!(
        "[ok] font-mode import: {} cells, font written once",
        rep.accents.cells
    );
}
