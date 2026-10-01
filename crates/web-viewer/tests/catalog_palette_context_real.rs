//! Disc-gated: the asset viewer's TIM-catalog palette surface
//! (`catalog_palette_context_json` / `catalog_decode_with_choice`) tells the
//! user which palette the game really draws a texture with.
//!
//! * the ASCII battle font's own `(0, 510)` palette is reported overwritten
//!   by the menu-glyph atlas, and the `(208, 510)` cell the game draws it
//!   through is offered and decodes;
//! * the system-UI sheet offers the "as the game draws it" composite, marks
//!   the row-511 cells its sprites use - including the `(256..288, 511)`
//!   extension cells the file itself does not carry - and the composite
//!   decode differs from the palette-0 decode, and shows the button-glyph
//!   TIM where that TIM covers the sheet at runtime.
//!
//! Structural facts only. Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::LegaiaViewer;

fn loaded() -> Option<LegaiaViewer> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut v = LegaiaViewer::new_headless();
    v.load_disc(bytes).ok()?;
    Some(v)
}

fn id_at(v: &LegaiaViewer, abs: u64) -> u32 {
    (0..v.catalog_len())
        .find(|&i| {
            let m: serde_json::Value = serde_json::from_str(&v.catalog_info_json(i)).unwrap();
            m["abs_offset"].as_u64() == Some(abs)
        })
        .unwrap_or_else(|| panic!("no catalog TIM at {abs:#x}"))
}

#[test]
fn catalog_palette_context_names_the_game_palette() {
    let Some(v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    eprintln!("[ran] catalog_palette_context_real");

    let font = id_at(&v, 0x7F40);
    let glyph = id_at(&v, 0x11218);
    let ctx: serde_json::Value =
        serde_json::from_str(&v.catalog_palette_context_json(font)).unwrap();
    assert_eq!(ctx["fate"], "overwritten");
    assert_eq!(ctx["overwritten_by"].as_u64(), Some(glyph as u64));
    let cells = ctx["vram_palettes"].as_array().unwrap();
    assert!(cells.iter().any(|c| c["x"] == 208 && c["y"] == 510));
    let (w, h, rgba) = v.catalog_decode_with_choice(font, "vram:208:510").unwrap();
    assert_eq!(rgba.len(), (w * h * 4) as usize);
    assert_ne!(rgba, v.catalog_decode_with_choice(font, "own:0").unwrap().2);

    let sheet = id_at(&v, 0x18E0);
    let ctx: serde_json::Value =
        serde_json::from_str(&v.catalog_palette_context_json(sheet)).unwrap();
    assert_eq!(ctx["fate"], "survives");
    let comp = &ctx["composite"];
    assert!(comp["covered"].as_u64().unwrap() * 2 > comp["total"].as_u64().unwrap());
    let cells = ctx["vram_palettes"].as_array().unwrap();
    for x in [256, 272, 288] {
        assert!(
            cells
                .iter()
                .any(|c| c["x"] == x && c["y"] == 511 && c["used"] == true),
            "extension cell ({x}, 511) not offered as used"
        );
    }
    let composite = v.catalog_decode_with_choice(sheet, "composite").unwrap().2;
    assert_ne!(
        composite,
        v.catalog_decode_with_choice(sheet, "own:0").unwrap().2
    );
    // The button-glyph rectangle shows the glyph TIM (0x7B00) that covers
    // it at runtime, not the sheet's hidden texels.
    let glyphs = id_at(&v, 0x7B00);
    let (gw, gh, g) = v.catalog_decode_with_choice(glyphs, "own:0").unwrap();
    let (gw, gh) = (gw as usize, gh as usize);
    for (gx, gy) in [(0usize, 0usize), (17, 5), (gw - 1, gh - 1)] {
        let s = ((96 + gy) * 256 + 128 + gx) * 4;
        let o = (gy * gw + gx) * 4;
        assert_eq!(composite[s..s + 4], g[o..o + 4], "glyph texel ({gx}, {gy})");
    }

    // A scene texture is not boot-resident and carries no composite.
    let scene = (0..v.catalog_len())
        .find(|&i| {
            let m: serde_json::Value = serde_json::from_str(&v.catalog_info_json(i)).unwrap();
            m["entry"] != "gap"
        })
        .unwrap();
    let ctx: serde_json::Value =
        serde_json::from_str(&v.catalog_palette_context_json(scene)).unwrap();
    assert_eq!(ctx["fate"], "not_boot");
    assert!(ctx["composite"].is_null());
}
