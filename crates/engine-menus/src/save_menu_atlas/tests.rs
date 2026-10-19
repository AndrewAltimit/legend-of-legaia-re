use super::*;

/// Disc-gated: build the real save-menu atlas from PROT.DAT +
/// PROT 0899 and verify the panel-tile + pill regions contain
/// opaque pixels with the right tonal range.
#[test]
fn builds_real_save_menu_atlas_when_disc_extracted() {
    let prot_dat_path = "../../extracted/PROT.DAT";
    let prot_899_path = "../../extracted/PROT/0899_xxx_dat.BIN";
    let prot_dat = match std::fs::read(prot_dat_path) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("skip: {prot_dat_path} missing");
            return;
        }
    };
    let prot_899 = match std::fs::read(prot_899_path) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("skip: {prot_899_path} missing");
            return;
        }
    };
    // build_atlas now expects a slice that already starts at the
    // system-UI TIM header. Mirror the disc-mode caller's window,
    // which extends through the extension-strip TIMs (ATR icons) and
    // the load-screen portrait / empty-frame TIMs.
    let tim_off = legaia_ui_formats::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET;
    let end = legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
        + legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE;
    let system_ui_slice = &prot_dat[tim_off..end];
    let glyph_off = legaia_ui_formats::menu_glyph_atlas::PROT_DAT_OFFSET as usize;
    let glyph_tim = &prot_dat[glyph_off..glyph_off + legaia_ui_formats::menu_glyph_atlas::TIM_SIZE];
    // The battle HUD's own window starts one TIM earlier, at the row-511
    // CLUT extension, so the three status badges on sub-palettes 16..18
    // come along; `build_atlas` splits that leading TIM back off.
    let hud_slice = &prot_dat[SYSTEM_UI_CLUT_EXT_TIM_OFFSET..end];
    // The menu font's sprite cells: ten numerals, fourteen separator glyphs,
    // the PAGE tag, ten page digits, the slash and two triangles - every one
    // carrying ink.
    let cells = menu_font_cells(hud_slice, Some(glyph_tim));
    assert_eq!(
        cells.len(),
        24 + SEPARATOR_GLYPHS as usize,
        "every menu sprite cell decodes"
    );
    for c in &cells {
        let opaque = c.rgba.chunks(4).filter(|p| p[3] != 0).count();
        assert!(opaque > 3, "menu sprite cell {:#x} decoded empty", c.id);
    }
    let atlas = build_atlas(hud_slice, &prot_899, Some(glyph_tim)).expect("build save-menu atlas");
    assert_eq!(atlas.width, ATLAS_WIDTH);
    assert_eq!(atlas.height, ATLAS_HEIGHT);

    // The dark class-0 skin (widget record 0x43): its fill patch is fully
    // opaque and a near-black neutral marble, with none of the blue skin's
    // blue, and each border cell carries ink.
    let px = |x: u32, y: u32| {
        let o = ((y * ATLAS_WIDTH + x) * 4) as usize;
        [
            atlas.rgba[o],
            atlas.rgba[o + 1],
            atlas.rgba[o + 2],
            atlas.rgba[o + 3],
        ]
    };
    let mean = |(x0, y0, w, h): (u32, u32, u32, u32)| {
        let mut sum = [0u32; 3];
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let p = px(x, y);
                assert_eq!(p[3], 255, "fill texel ({x}, {y}) is opaque");
                for c in 0..3 {
                    sum[c] += u32::from(p[c]);
                }
            }
        }
        sum.map(|s| s / (w * h))
    };
    let dark = atlas.band_class0_dark();
    let dark_fill = mean(dark[8]);
    let blue_fill = mean(atlas.band_panel_filigree());
    let spread = |c: [u32; 3]| c.iter().max().unwrap() - c.iter().min().unwrap();
    assert!(
        spread(dark_fill) < 16 && dark_fill[2] * 2 < blue_fill[2],
        "dark fill {dark_fill:?} is a neutral marble, the blue {blue_fill:?} is not"
    );
    for cell in &dark[..8] {
        let (x0, y0, w, h) = *cell;
        let ink = (y0..y0 + h)
            .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
            .filter(|&(x, y)| px(x, y)[3] != 0)
            .count();
        assert!(ink > 0, "dark border cell {cell:?} decoded empty");
    }
    assert_eq!(atlas.rgba.len(), (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize);

    // The HUD numeral strip: ten 8x12 cells, every one carrying ink,
    // and none of them bleeding into a neighbour's cell. Retail draws
    // every battle HP / MP / level out of this strip.
    assert!(atlas.has_hud_digits);
    let strip = atlas.band_hud_digits().expect("digit strip baked");
    assert_eq!(strip, ATLAS_RECT_HUD_DIGITS);
    let stride = (ATLAS_WIDTH * 4) as usize;
    for d in 0..10u32 {
        let (cx, cy, cw, ch) = hud_digit_cell(strip, d).expect("digit cell");
        let mut ink = 0u32;
        for row in 0..ch {
            for col in 0..cw {
                let off = ((cy + row) as usize) * stride + ((cx + col) as usize) * 4;
                if atlas.rgba[off + 3] != 0 {
                    ink += 1;
                }
            }
        }
        assert!(ink >= 20, "digit {d} cell has only {ink} opaque texels");
    }
    // Cell 9's right edge is the strip's right edge - no eleventh cell.
    assert_eq!(
        hud_digit_cell(strip, 9).unwrap().0 + HUD_DIGIT_W,
        strip.0 + strip.2
    );
    assert_eq!(hud_digit_cell(strip, 10), None);

    // The nine status-element badges, including the three whose
    // sub-palettes (16 / 17 / 18) only exist because the slice reached
    // the row-511 CLUT extension. Each cell is a word tag, so a mostly
    // opaque 48x16 block, and no two decode identically - a wrong
    // palette would collapse several onto one colour.
    let opaque_in = |rect: (u32, u32, u32, u32)| -> u32 {
        let (rx, ry, rw, rh) = rect;
        let mut n = 0;
        for row in 0..rh {
            for col in 0..rw {
                let off = ((ry + row) as usize) * stride + ((rx + col) as usize) * 4;
                if atlas.rgba[off + 3] != 0 {
                    n += 1;
                }
            }
        }
        n
    };
    let mut inks: Vec<Vec<u8>> = Vec::new();
    for (i, cell) in STATUS_BADGE_CELLS.iter().enumerate() {
        let sprite = cell.sprite;
        let rect = atlas
            .band_status_badge(sprite)
            .unwrap_or_else(|| panic!("status badge {sprite:#04x} not baked"));
        assert_eq!(rect, status_badge_atlas_rect(i));
        assert!(
            opaque_in(rect) > 200,
            "status badge {sprite:#04x} decoded nearly empty"
        );
        let (rx, ry, rw, rh) = rect;
        let mut px = Vec::new();
        for row in 0..rh {
            let off = ((ry + row) as usize) * stride + (rx as usize) * 4;
            px.extend_from_slice(&atlas.rgba[off..off + (rw as usize) * 4]);
        }
        inks.push(px);
    }
    for a in 0..inks.len() {
        for b in (a + 1)..inks.len() {
            assert_ne!(
                inks[a], inks[b],
                "status badges {a} and {b} decoded identically - one took the wrong palette"
            );
        }
    }
    // The plain element badges off the extension strip, 20x12 each.
    for i in 0..ELEMENT_BADGE_COUNT {
        let rect = atlas
            .band_element_badge(i)
            .unwrap_or_else(|| panic!("element badge {i} not baked"));
        assert_eq!(rect, element_badge_atlas_rect(i));
        assert!(
            opaque_in(rect) > 40,
            "element badge {i} decoded nearly empty"
        );
    }

    // A sheet-rooted slice cannot reach the CLUT extension, so exactly
    // the three extension-palette badges go unbaked - and the HUD's
    // per-cell fallback is what that shape exists for.
    let narrow = build_atlas(system_ui_slice, &prot_899, Some(glyph_tim))
        .expect("build save-menu atlas without the CLUT extension");
    for cell in STATUS_BADGE_CELLS {
        let (sprite, subpal) = (cell.sprite, cell.subpalette);
        let reachable = subpal < SYSTEM_UI_CLUT_EXT_FIRST_SUBPALETTE;
        assert_eq!(
            narrow.band_status_badge(sprite).is_some(),
            reachable,
            "badge {sprite:#04x} (sub-palette {subpal}) baked against expectation"
        );
    }

    // Built without the glyph TIM, the strip is absent rather than
    // blank - that is what lets the HUD choose its font fallback.
    let bare = build_atlas(system_ui_slice, &prot_899, None).expect("build without glyphs");
    assert!(!bare.has_hud_digits);
    assert_eq!(bare.band_hud_digits(), None);

    // The top-left corner tile must contain opaque gold-bronze
    // pixels (CLUT row 2 entries 7..15).
    let (tlx, tly, tlw, tlh) = atlas.band_panel_tl();
    let stride = (ATLAS_WIDTH * 4) as usize;
    let mut gold_hits = 0u32;
    for row in 0..tlh {
        for col in 0..tlw {
            let off = ((tly + row) as usize) * stride + ((tlx + col) as usize) * 4;
            let (r, g, b, a) = (
                atlas.rgba[off],
                atlas.rgba[off + 1],
                atlas.rgba[off + 2],
                atlas.rgba[off + 3],
            );
            // Gold-bronze tones have r > g > b with r in 60..210.
            if a == 255 && r >= 60 && r > g && g > b {
                gold_hits += 1;
            }
        }
    }
    assert!(
        gold_hits >= 8,
        "panel top-left tile has too few gold-bronze pixels ({gold_hits})"
    );

    // Slot-1 pill band should have a saturated blue tone (CLUT 7).
    let (sx, sy, sw, sh) = atlas.band_slot1();
    let mut blue_hits = 0u32;
    for row in 0..sh {
        for col in 0..sw {
            let off = ((sy + row) as usize) * stride + ((sx + col) as usize) * 4;
            let (r, g, b, a) = (
                atlas.rgba[off],
                atlas.rgba[off + 1],
                atlas.rgba[off + 2],
                atlas.rgba[off + 3],
            );
            if a == 255 && b > 100 && r < 120 && g < 120 {
                blue_hits += 1;
            }
        }
    }
    assert!(
        blue_hits > 30,
        "slot 1 pill has too few blue pixels ({blue_hits}) - CLUT may be off"
    );

    // The AP-gauge digit strip (CLUT row 4) must decode to the red
    // value-digit ink: the retail glyph stroke is (231, 74, 74).
    let (gx, gy, gw, gh) = atlas.band_gauge_digits();
    let mut red_hits = 0u32;
    for row in 0..gh {
        for col in 0..gw {
            let off = ((gy + row) as usize) * stride + ((gx + col) as usize) * 4;
            if atlas.rgba[off + 3] == 255
                && atlas.rgba[off] == 231
                && atlas.rgba[off + 1] == 74
                && atlas.rgba[off + 2] == 74
            {
                red_hits += 1;
            }
        }
    }
    assert!(
        red_hits > 100,
        "gauge digit strip has too few red stroke pixels ({red_hits}) - CLUT row 4 may be off"
    );

    // Every equipment pictogram (CLUT row 8) must carry opaque gold
    // pixels (r > g > b ramp), like the panel chrome check above.
    for (name, rect) in [
        ("weapon", atlas.band_icon_weapon()),
        ("helmet", atlas.band_icon_helmet()),
        ("armor", atlas.band_icon_armor()),
        ("boot", atlas.band_icon_boot()),
        ("goods", atlas.band_icon_goods()),
    ] {
        let (ix, iy, iw, ih) = rect;
        let mut icon_gold = 0u32;
        for row in 0..ih {
            for col in 0..iw {
                let off = ((iy + row) as usize) * stride + ((ix + col) as usize) * 4;
                let (r, g, b, a) = (
                    atlas.rgba[off],
                    atlas.rgba[off + 1],
                    atlas.rgba[off + 2],
                    atlas.rgba[off + 3],
                );
                if a == 255 && r >= 60 && r > g && g > b {
                    icon_gold += 1;
                }
            }
        }
        assert!(
            icon_gold >= 30,
            "{name} pictogram has too few gold pixels ({icon_gold}) - CLUT row 8 may be off"
        );
    }

    // Tab-banner plaque pieces (CLUT row 12) carry the carved brown
    // ramp (r > g > b), like the pictograms.
    for (name, rect) in [
        ("tab cap L", atlas.band_tab_cap_l()),
        ("tab body", atlas.band_tab_body()),
        ("tab cap R", atlas.band_tab_cap_r()),
    ] {
        let (ix, iy, iw, ih) = rect;
        let mut brown = 0u32;
        for row in 0..ih {
            for col in 0..iw {
                let off = ((iy + row) as usize) * stride + ((ix + col) as usize) * 4;
                let (r, g, b, a) = (
                    atlas.rgba[off],
                    atlas.rgba[off + 1],
                    atlas.rgba[off + 2],
                    atlas.rgba[off + 3],
                );
                if a == 255 && r >= 40 && r >= g && g >= b {
                    brown += 1;
                }
            }
        }
        assert!(
            brown >= 40,
            "{name} has too few brown pixels ({brown}) - CLUT row 12 may be off"
        );
    }

    // Pager triangles (CLUT row 7) are light-gray solids with
    // transparent surroundings.
    for (name, rect) in [
        ("pager left", atlas.band_pager_left()),
        ("pager right", atlas.band_pager_right()),
    ] {
        let (ix, iy, iw, ih) = rect;
        let (mut bright, mut clear) = (0u32, 0u32);
        for row in 0..ih {
            for col in 0..iw {
                let off = ((iy + row) as usize) * stride + ((ix + col) as usize) * 4;
                if atlas.rgba[off + 3] == 0 {
                    clear += 1;
                } else if atlas.rgba[off] > 150 {
                    bright += 1;
                }
            }
        }
        assert!(
            bright >= 40 && clear >= 40,
            "{name} triangle looks wrong (bright={bright}, clear={clear})"
        );
    }

    // ATR element icons: Vahn's flame carries saturated orange
    // (r > g > b with high r), and all three have transparent
    // corners (28x12 winged shapes).
    let atr = atlas.band_atr_icons();
    let (vx, vy, vw, vh) = atr[0];
    let mut orange = 0u32;
    for row in 0..vh {
        for col in 0..vw {
            let off = ((vy + row) as usize) * stride + ((vx + col) as usize) * 4;
            let (r, g, b, a) = (
                atlas.rgba[off],
                atlas.rgba[off + 1],
                atlas.rgba[off + 2],
                atlas.rgba[off + 3],
            );
            if a == 255 && r > 180 && r > g && g > b {
                orange += 1;
            }
        }
    }
    assert!(
        orange >= 20,
        "Vahn ATR flame has too few orange pixels ({orange}) - ext-strip CLUT may be off"
    );
    for (i, (ix, iy, _, _)) in atr.iter().enumerate() {
        let off = (*iy as usize) * stride + (*ix as usize) * 4;
        assert_eq!(
            atlas.rgba[off + 3],
            0,
            "ATR icon {i} top-left corner should be transparent"
        );
    }
}
