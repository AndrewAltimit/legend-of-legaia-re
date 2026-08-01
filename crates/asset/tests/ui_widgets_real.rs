//! Disc-gated invariants for the widget-class table
//! (`SCUS_942.54` VA `0x800732A4`, `0x0C` stride).
//!
//! This is the sprite book behind every 2-D UI surface, and the join target of
//! a screen-element record's `+0x0E` kind byte. What the assertions pin:
//!
//! * the run decodes end-to-end and stops exactly where the frame tile-set
//!   pool begins;
//! * the status-element badges `0x18..=0x20` are a two-column block of 48x16
//!   cells at sheet `(0 | 48, 48..=112)`, each on its own row-511 sub-palette;
//! * the eight element badges are 20x12 at a 32-texel pitch from `u = 6` on
//!   `v = 192`, and their palette bytes walk `0x40..=0x47`, which is what puts
//!   each one on its own CLUT in the `(896.., 498..499)` block;
//! * the plate 3-slice the packet walk measured falls out of the table: the
//!   blue body is record `0x01` on sub-palette 4, the carved-gold body record
//!   `0x02` on sub-palette 12, and their cap pairs are `(208 | 216, 0)` and
//!   `(208 | 216, 64)`, 8x20 each;
//! * every screen-element record's kind byte names a real widget record, and
//!   the four surfaces the chrome doc names resolve to the art it describes;
//! * the roster panel and the active-actor bar are chains, and following the
//!   chain reaches the 102x48 panel plate / the bar's `HP` and `MP` labels at
//!   the seats the packets drew.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/` are absent.

use std::path::PathBuf;

use legaia_asset::screen_elements::ScreenElementTable;
use legaia_asset::ui_widgets::{
    ELEMENT_BADGE_COUNT, FRAME_CAP_PAIR_VA, FRAME_TILESET_VA, RECORD_COUNT, RECORD_STRIDE,
    SPRITE_ELEMENT_BADGE_FIRST, SPRITE_LEVEL_MARKER, STATUS_BADGES, TABLE_VA, WidgetTable, clut_fb,
};

fn scus() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for dir in ["extracted", "../../extracted"] {
        let f = PathBuf::from(dir).join("SCUS_942.54");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

#[test]
fn widget_table_decodes_and_pins_the_battle_sprites() {
    let Some(scus) = scus() else {
        eprintln!("skip: no LEGAIA_DISC_BIN / extracted/SCUS_942.54");
        return;
    };
    let table = WidgetTable::from_scus(&scus).expect("widget table decodes");
    assert_eq!(table.records().len(), RECORD_COUNT);
    assert_eq!(
        TABLE_VA as usize + RECORD_COUNT * RECORD_STRIDE,
        FRAME_TILESET_VA as usize,
        "the record run ends where the tile-set pool begins"
    );

    // Every record in the run is a plausible sheet rect: the sheet is one
    // 256x256 4bpp page, and a class arm reads (u+w, v+h) straight out of it.
    for (i, w) in table.records().iter().enumerate() {
        let (u, v, rw, rh) = w.rect;
        assert!(rw > 0 && rh > 0, "record {i:#04x} has an empty rect");
        assert!(
            u as u16 + rw as u16 <= 256 && v as u16 + rh as u16 <= 256,
            "record {i:#04x} rect {:?} leaves the page",
            w.rect
        );
        assert!(w.class <= 6, "record {i:#04x} class {} has no arm", w.class);
        assert!(w.bias.0.abs() <= 320 && w.bias.1.abs() <= 240);
    }

    // --- the status-element badge sheet -----------------------------------
    // Nine 48x16 cells in two columns; every one on its own sub-palette.
    let mut seats = Vec::new();
    let mut palettes = Vec::new();
    for (id, _mask, _label) in STATUS_BADGES {
        let w = table.get(id).expect("status badge record");
        assert_eq!(w.rect.2, 48, "badge {id:#04x} width");
        assert_eq!(w.rect.3, 16, "badge {id:#04x} height");
        assert!(
            w.rect.0 == 0 || w.rect.0 == 48,
            "badge {id:#04x} is off the two-column block"
        );
        assert!(
            (48..=112).contains(&w.rect.1) && w.rect.1.is_multiple_of(16),
            "badge {id:#04x} row {}",
            w.rect.1
        );
        assert_eq!(w.chain, 0, "a badge is a single sprite, not a chain");
        assert_eq!(w.clut_fb().1, 511, "badge {id:#04x} takes a row-511 CLUT");
        seats.push((w.rect.0, w.rect.1));
        palettes.push(w.palette);
    }
    seats.sort_unstable();
    seats.dedup();
    assert_eq!(seats.len(), 9, "the nine badges occupy nine distinct cells");
    palettes.sort_unstable();
    palettes.dedup();
    assert_eq!(palettes.len(), 9, "each badge carries its own sub-palette");

    // The level marker the ladder's no-ailment arm draws instead.
    let lv = table.get(SPRITE_LEVEL_MARKER).expect("LV label");
    assert_eq!(lv.rect, (192, 86, 16, 10));

    // --- the element badges ------------------------------------------------
    for i in 0..ELEMENT_BADGE_COUNT {
        let w = table
            .get(SPRITE_ELEMENT_BADGE_FIRST + i as u8)
            .expect("element badge record");
        assert_eq!(w.rect, (6 + 32 * i as u8, 192, 20, 12), "badge {i}");
        // The palette byte walks with the badge index, and the bit-6 decode
        // turns that walk into a 4-wide x 2-tall block of CLUTs.
        assert_eq!(w.palette, 0x40 + i as u8, "badge {i} palette byte");
        assert_eq!(
            w.clut_fb(),
            (896 + (i as u16 % 4) * 16, 498 + i as u16 / 4),
            "badge {i} CLUT"
        );
    }
    // The four pairs a live frame caught, re-derived from the disc alone.
    for (idx, want) in [
        (0, (896, 498)),
        (1, (912, 498)),
        (5, (912, 499)),
        (7, (944, 499)),
    ] {
        let w = table.get(SPRITE_ELEMENT_BADGE_FIRST + idx).unwrap();
        assert_eq!(w.rect.0, 6 + 32 * idx);
        assert_eq!(w.clut_fb(), want);
    }

    // --- the plate 3-slice -------------------------------------------------
    let blue = table.get(0x01).expect("blue plate body");
    let gold = table.get(0x02).expect("gold plate body");
    assert_eq!(blue.rect, (192, 0, 16, 20));
    assert_eq!(gold.rect, (192, 64, 16, 20));
    assert_eq!(blue.clut_fb(), (4 * 16, 511), "blue body sub-palette 4");
    assert_eq!(gold.clut_fb(), (12 * 16, 511), "gold body sub-palette 12");
    assert_eq!(blue.class, 3, "plate runs are the class-3 arm");
    assert_eq!(gold.class, 3);
    assert_eq!(blue.bias, (-8, -4), "the content-to-plate bias");
    assert_eq!(gold.bias, (-8, -4));
    let (bl, br) = table.plate_caps(blue.tileset).expect("blue caps");
    assert_eq!((bl.u, bl.v, bl.w, bl.h), (208, 0, 8, 20));
    assert_eq!((br.u, br.v, br.w, br.h), (216, 0, 8, 20));
    let (gl, gr) = table.plate_caps(gold.tileset).expect("gold caps");
    assert_eq!((gl.u, gl.v, gl.w, gl.h), (208, 64, 8, 20));
    assert_eq!((gr.u, gr.v, gr.w, gr.h), (216, 64, 8, 20));
    assert_eq!(
        FRAME_TILESET_VA as usize + 3 * 0x20,
        FRAME_CAP_PAIR_VA as usize
    );

    // --- the corner-framed window ------------------------------------------
    // Class 0 with tile-set 0: the rectangular gold border, 4-pixel corners
    // and 24-pixel edges out of one 32x32 patch at (160, 0).
    let window = table.get(0x03).expect("framed-window record");
    assert_eq!(window.class, 0);
    assert_eq!(
        window.bias,
        (-8, -8),
        "the frame insets by its border width"
    );
    let set = table.tileset(window.tileset).expect("tile-set 0");
    let quad = |i: usize| (set[i].u, set[i].v, set[i].w, set[i].h);
    assert_eq!(quad(0), (160, 0, 4, 4), "top-left corner");
    assert_eq!(quad(1), (188, 0, 4, 4), "top-right corner");
    assert_eq!(quad(2), (160, 28, 4, 4), "bottom-left corner");
    assert_eq!(quad(3), (188, 28, 4, 4), "bottom-right corner");
    assert_eq!(quad(4), (164, 0, 24, 4), "top edge");
    assert_eq!(quad(5), (164, 28, 24, 4), "bottom edge");
    assert_eq!(quad(6), (160, 4, 4, 24), "left edge");
    assert_eq!(quad(7), (188, 4, 4, 24), "right edge");

    // --- the palette decode itself ----------------------------------------
    assert_eq!(clut_fb(0x00), (0, 511));
    assert_eq!(clut_fb(0x40), (896, 498));
}

#[test]
fn every_screen_element_kind_names_a_widget_record() {
    let Some(scus) = scus() else {
        eprintln!("skip: no LEGAIA_DISC_BIN / extracted/SCUS_942.54");
        return;
    };
    let widgets = WidgetTable::from_scus(&scus).expect("widget table decodes");
    let placements = ScreenElementTable::from_scus(&scus).expect("placement table decodes");

    // The join: the low byte of a placement record's kind pair is a widget id.
    for (i, e) in placements.records().iter().enumerate() {
        let lo = (e.kind & 0xFF) as u8;
        let hi = (e.kind >> 8) as u8;
        assert!(
            widgets.get(lo).is_some(),
            "placement {i} kind low byte {lo:#04x} is not a widget id"
        );
        assert!(
            widgets.get(hi).is_some(),
            "placement {i} kind high byte {hi:#04x} is not a widget id"
        );
    }

    // The actor-name plaque is the carved-gold plate; the command chips the
    // blue one. Both were read off retail's packets before the table was.
    let plaque = placements.get(68).unwrap();
    assert_eq!(plaque.kind & 0xFF, 0x02);
    assert_eq!(widgets.get(0x02).unwrap().rect, (192, 64, 16, 20));
    for chip in [8usize, 9, 10, 11] {
        assert_eq!(
            placements.get(chip).unwrap().kind & 0xFF,
            0x01,
            "chip {chip}"
        );
    }
    assert_eq!(widgets.get(0x01).unwrap().rect, (192, 0, 16, 20));

    // The active-actor bar is a chain, and it ends on the blue plate body
    // after laying out both label / separator pairs at the captured biases.
    let bar = placements.get(7).unwrap();
    assert_eq!(bar.kind & 0xFF, 0x2B);
    let chain: Vec<_> = widgets.chain_from(0x2B);
    assert_eq!(
        chain.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![0x2B, 0x2C, 0x2D, 0x2E, 0x2F]
    );
    let pen = (bar.alt_pen().0, bar.alt_pen().1);
    let seat = |w: &legaia_asset::ui_widgets::Widget| (pen.0 + w.bias.0, pen.1 + w.bias.1);
    assert_eq!(seat(&chain[0].1), (80, 194), "HP label");
    assert_eq!(seat(&chain[1].1), (136, 188), "HP separator");
    assert_eq!(seat(&chain[2].1), (192, 194), "MP label");
    assert_eq!(seat(&chain[3].1), (240, 188), "MP separator");
    assert_eq!(chain[4].1.rect, (192, 0, 16, 20), "the blue plate body");

    // The roster panels are the other chain, and it ends on the 102x48 plate.
    for panel in [6usize, 78, 79] {
        assert_eq!(placements.get(panel).unwrap().kind & 0xFF, 0x07);
    }
    let panel_chain: Vec<_> = widgets.chain_from(0x07);
    assert_eq!(
        panel_chain.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![0x07, 0x08, 0x09]
    );
    let plate = panel_chain[2].1;
    assert_eq!(plate.rect, (0, 0, 102, 48), "the marbled panel plate");
    assert_eq!(plate.bias, (-5, -4), "the panel's five-pixel name inset");
    assert_eq!(panel_chain[0].1.bias, (-1, 17), "panel HP row");
    assert_eq!(panel_chain[1].1.bias, (-1, 32), "panel MP row");
}
