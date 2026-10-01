//! Disc-gated oracle for `legaia_asset::tim_palette_context`: which palette
//! the game really draws a texture with, read off the disc.
//!
//! Pins:
//! * the system-UI sheet (`PROT.DAT[0x18E0]`) is a boot member whose own
//!   sixteen palettes all survive in VRAM row 511;
//! * the ASCII battle font (`PROT.DAT[0x7F40]`) has its `(0, 510)` strip
//!   overwritten by the menu-glyph atlas (`0x11218`) - its own palette never
//!   reaches VRAM - and the palette the game draws it with, `(208, 510)`, is
//!   one of the row-510 cells VRAM does hold;
//! * the widget table's palette map puts status badge `0x1A` (`Stone`) on the
//!   row-511 extension cell `(256, 511)`, a palette the sheet's own file does
//!   not carry; the button-glyph TIM covers texels `(128, 96)..(191, 127)`;
//!   the Curse badge's texels go to its own sub-palette 13 over the class-4
//!   record that also samples them; and the map claims most of the sheet.
//!
//! The ROM patcher's editor view of the same kernel is pinned in
//! `crates/patcher/tests/tim_multi_palette_real.rs`.
//!
//! Skips + passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_asset::tim_palette_context::{
    BootClutVram, ClutFate, composite_rgba, parse_button_glyph_tim, sheet_palette_regions,
    texel_palettes,
};
use legaia_asset::ui_widgets::{BUTTON_GLYPH_TIM_PROT_OFFSET, WidgetTable};

const SHEET_OFFSET: u64 = 0x18E0;
const BATTLE_FONT_OFFSET: u64 = 0x7F40;
const MENU_GLYPH_OFFSET: u64 = 0x11218;

fn extracted_file(name: &str) -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        dirs.push(PathBuf::from(d));
    }
    dirs.push(PathBuf::from("extracted"));
    dirs.push(PathBuf::from("../../extracted"));
    dirs.into_iter()
        .map(|d| d.join(name))
        .find(|f| f.is_file())
        .and_then(|f| std::fs::read(f).ok())
}

#[test]
fn boot_clut_fates_and_sheet_palette_map() {
    let (Some(prot), Some(scus)) = (extracted_file("PROT.DAT"), extracted_file("SCUS_942.54"))
    else {
        eprintln!("[skip] tim_palette_context_real: no disc / extracted data");
        return;
    };
    eprintln!("[ran] tim_palette_context_real");
    let ctx = BootClutVram::from_prot_dat(&prot).expect("boot bundle");

    let sheet = legaia_tim::parse(&prot[SHEET_OFFSET as usize..]).unwrap();
    assert_eq!(ctx.clut_fate(SHEET_OFFSET, &sheet), ClutFate::Survives);

    let font = legaia_tim::parse(&prot[BATTLE_FONT_OFFSET as usize..]).unwrap();
    match ctx.clut_fate(BATTLE_FONT_OFFSET, &font) {
        ClutFate::Overwritten {
            palettes,
            by_offset,
        } => {
            assert_eq!(palettes, vec![0]);
            assert_eq!(by_offset, Some(MENU_GLYPH_OFFSET));
        }
        other => panic!("battle font CLUT fate: {other:?}"),
    }
    let row510 = ctx.row_palettes(510, 16);
    assert!(
        row510.iter().any(|p| (p.fb_x, p.fb_y) == (208, 510)),
        "row 510 lacks the (208, 510) cell"
    );

    let table = WidgetTable::from_scus(&scus).expect("widget table");
    let regions = sheet_palette_regions(&table);
    let stone = regions
        .iter()
        .find(|r| r.widget == 0x1A)
        .expect("Stone badge region");
    assert_eq!(stone.clut_fb, (256, 511));
    assert_eq!(stone.rect, (48, 80, 48, 16));

    // The one texel -> palette kernel both the viewer composite and the ROM
    // patcher's region map read.
    let cover = parse_button_glyph_tim(&prot[BUTTON_GLYPH_TIM_PROT_OFFSET..]).expect("glyph TIM");
    let texels = texel_palettes(&sheet, &regions, Some(&cover)).unwrap();
    let covered = texels.covered.expect("button glyphs cover the sheet");
    assert_eq!(covered.rect, (128, 96, 64, 32));
    assert_eq!(covered.clut_fb, (304, 511));
    // The Curse badge (class 5, sub-palette 13) wins its texels over the
    // class-4 bar record 0x06 that stretches a 16x16 cut of them through
    // sub-palette 5.
    assert_eq!(texels.clut_at(64, 64), Some((208, 511)));
    // The class-4 bars' cap pair is part of the map.
    assert_eq!(texels.clut_at(193, 25), Some((80, 511)));
    let own = sheet.clut.as_ref().unwrap().entries[..16].to_vec();
    let c = composite_rgba(&sheet, &texels, ctx.vram(), &own, Some(&cover)).unwrap();
    let total = sheet.pixel_width() * sheet.pixel_height();
    eprintln!(
        "sheet texels: {} regions, claimed {}/{} texels, contested {}",
        texels.regions.len(),
        texels.claimed(),
        total,
        texels.contested
    );
    assert!(
        texels.claimed() * 2 > total,
        "the map claims under half the sheet"
    );
    // Local eyeballing only (decoded pixels - never commit the output).
    if let Some(out) = std::env::var_os("LEGAIA_DUMP_SHEET_COMPOSITE") {
        legaia_tim::write_png(
            std::path::Path::new(&out),
            sheet.pixel_width(),
            sheet.pixel_height(),
            &c,
        )
        .unwrap();
    }
}
