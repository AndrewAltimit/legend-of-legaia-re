//! Numeral fields on the retail cell grid.

use super::*;

/// Retail's numeral fields, measured in retail's numeral cells.
///
/// Every number on the battle screen is a run of fixed 8-px cells off the
/// menu-glyph strip, right-aligned against a pinned edge - **both** halves of
/// a `cur / max` pair, not just the current. Proportional dialog-font digits
/// are not what retail draws and are wider than the field allows: a
/// four-digit maximum seated as a forward-running font string runs past the
/// roster panel's 102-px plate and into its neighbour.
///
/// Font-independent by construction, so it runs on CI too. Reads the seats
/// out of the packet-pinned `engine-ui::battle_chrome` rather than out of
/// literals, so a re-seated pin fails here rather than passing against a
/// stale copy. The companion
/// [`bar_panel_and_diag_columns_clear_the_retail_font_or_skips`] covers the
/// fields that really are proportional - the names and the diagnostic rows.
#[test]
fn numeral_fields_fit_their_widest_value_in_retail_cells() {
    use legaia_engine_ui::battle_chrome as bc;
    let left_of = |right: i16, digits: u16| bc::digits_left_of(right, digits);
    // The label cells are 16 wide, the `/` separator 8.
    let label_w = 16;
    let sep_w = bc::SEPARATOR.2 as i16;

    // Active-actor bar. Four cells per HP field, three per MP field - the
    // width retail actually gave each one.
    assert!(
        left_of(bc::BAR_HP_CUR_RIGHT, 4) >= bc::BAR_HP_LABEL.0 + label_w,
        "a four-digit HP current overruns the bar's HP label cell"
    );
    assert!(
        left_of(bc::BAR_HP_MAX_RIGHT, 4) >= bc::BAR_HP_SEPARATOR.0 + sep_w,
        "a four-digit HP maximum overruns the bar's `/` separator"
    );
    assert!(
        bc::BAR_HP_MAX_RIGHT <= bc::BAR_MP_LABEL.0,
        "the bar's HP maximum overruns the MP label seat"
    );
    assert!(
        left_of(bc::BAR_MP_CUR_RIGHT, 3) >= bc::BAR_MP_LABEL.0 + label_w,
        "a three-digit MP current overruns the bar's MP label cell"
    );
    assert!(
        left_of(bc::BAR_MP_MAX_RIGHT, 3) >= bc::BAR_MP_SEPARATOR.0 + sep_w,
        "a three-digit MP maximum overruns the bar's `/` separator"
    );
    assert!(
        bc::BAR_MP_MAX_RIGHT <= bc::BAR_X + bc::PLATE_CAP_W as i16 + bc::BAR_INTERIOR_W as i16,
        "the bar's MP maximum overruns the right cap"
    );

    // Roster panel: the same two pairs inside a 102-px plate whose pitch is
    // also 102, so an overrun lands on the next member's panel.
    let plate_w = bc::PANEL_BG.2 as i16;
    assert!(
        left_of(bc::panel::CUR_RIGHT, 4) >= bc::panel::HP_LABEL.0 + label_w,
        "a four-digit HP current overruns the panel's HP label cell"
    );
    assert!(
        left_of(bc::panel::MAX_RIGHT, 4) >= bc::panel::HP_SEPARATOR.0 + sep_w,
        "a four-digit maximum overruns the panel's `/` separator"
    );
    assert!(
        bc::panel::MAX_RIGHT <= plate_w,
        "a four-digit maximum overruns the panel's right edge"
    );
    // Level: two cells, clearing the LV label and the plate.
    assert!(
        left_of(bc::panel::LV_DIGITS_RIGHT, 2) >= bc::panel::LV_LABEL.0 + label_w,
        "a two-digit level overruns the panel's LV label cell"
    );
    assert!(
        bc::panel::LV_DIGITS_RIGHT <= plate_w,
        "the level overruns the panel's right edge"
    );

    // And the `engine-ui` mirror really is drawing at those pins: the
    // builder's own four-digit output, cell for cell.
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 9999, 9999, 999, 999);
    let cells = numeral_cells(&hud_frame_with_chrome(&font, &[slot], None));
    assert_eq!(
        numerals_on_row(&cells, PANEL_Y + 19),
        vec![
            (
                SOLO_PANEL_X + left_of(bc::panel::CUR_RIGHT, 4) as i32,
                "9999".into()
            ),
            (
                SOLO_PANEL_X + left_of(bc::panel::MAX_RIGHT, 4) as i32,
                "9999".into()
            ),
        ],
        "the engine-ui mirror drifted from the battle_chrome pins"
    );
}

/// The same fields, driven through the builder with a four-digit party, so
/// the seats above are checked against what actually gets drawn rather than
/// against arithmetic alone.
#[test]
fn four_digit_party_numerals_land_on_the_pinned_cells() {
    let font = legaia_font::synthetic_for_tests();
    let mut slot = slot_view("Vahn", true, true, 4955, 4984, 856, 856);
    slot.level = 99;
    let trio = [slot, slot, slot];

    // Resting: three roster panels, each carrying two four-digit HP runs and
    // two three-digit MP runs at the pinned right edges.
    let panels = hud_frame_with_chrome(&font, &trio, None);
    let cells = numeral_cells(&panels);
    for (i, px) in TRIO_PANEL_X.iter().enumerate() {
        assert_eq!(
            numerals_on_row(&cells, PANEL_Y + 19),
            vec![
                (TRIO_PANEL_X[0] + 25, "4955".into()),
                (TRIO_PANEL_X[0] + 65, "4984".into()),
                (TRIO_PANEL_X[1] + 25, "4955".into()),
                (TRIO_PANEL_X[1] + 65, "4984".into()),
                (TRIO_PANEL_X[2] + 25, "4955".into()),
                (TRIO_PANEL_X[2] + 65, "4984".into()),
            ],
            "panel {i} HP row"
        );
        // Level 99 right-aligns to +96, so its two cells are +80 / +88.
        assert!(
            numerals_on_row(&cells, PANEL_Y + 4).contains(&(px + 80, "99".into())),
            "panel {i} level"
        );
    }
    assert_eq!(
        numerals_on_row(&cells, PANEL_Y + 34),
        vec![
            (TRIO_PANEL_X[0] + 33, "856".into()),
            (TRIO_PANEL_X[0] + 73, "856".into()),
            (TRIO_PANEL_X[1] + 33, "856".into()),
            (TRIO_PANEL_X[1] + 73, "856".into()),
            (TRIO_PANEL_X[2] + 33, "856".into()),
            (TRIO_PANEL_X[2] + 73, "856".into()),
        ],
    );
    // Nothing may leave its own panel: cell right edges stay inside the
    // plate the member owns.
    for &(x, _, _) in &cells {
        let px = TRIO_PANEL_X
            .iter()
            .rev()
            .find(|p| x >= **p)
            .expect("a numeral left of every panel");
        assert!(
            x + DIGIT_W <= px + PANEL_W,
            "a numeral at {x} runs past the panel at {px}"
        );
    }

    // Acting: the same member on the full-width bar, at the bar's own edges.
    // These are the seats the retail reference frame shows for `Vahn HP
    // 2318/2318 MP 316/316` - current back from 134 / 238, maximum back
    // from 178 / 274.
    let bar = hud_frame_with_chrome(&font, &trio, Some(0));
    assert_eq!(
        numerals_on_row(&numeral_cells(&bar), BAR_NAME_Y),
        vec![
            (102, "4955".into()),
            (146, "4984".into()),
            (214, "856".into()),
            (250, "856".into()),
        ],
    );
}

/// Without the numeral strip the digits fall back to font glyphs, and they
/// have to sit on the **same** cell grid - the fallback may change
/// letterforms, never layout. Each glyph is centred in its cell, so its
/// centre is what has to land inside; a font whose digits are wider than
/// eight pixels overhangs symmetrically rather than sliding the run.
#[test]
fn font_fallback_numerals_keep_the_retail_cell_grid() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 4955, 4984, 856, 856);
    let bare = hud_frame(&font, &[slot], &[], &[], false);
    assert!(
        numeral_cells(&bare).is_empty(),
        "no chrome atlas, so no digit sprites"
    );
    // The HP row's eight cells: current 25..57 and maximum 65..97, both
    // right-aligned inside the solo panel.
    let cells: Vec<i32> = (0..4)
        .map(|i| SOLO_PANEL_X + 25 + i * DIGIT_W)
        .chain((0..4).map(|i| SOLO_PANEL_X + 65 + i * DIGIT_W))
        .collect();
    let row_y = PANEL_Y + 19;
    let glyphs: Vec<(i32, i32)> = bare
        .text
        .iter()
        .filter(|d| d.src != SOLID && d.dst.1 / STAGE_SCALE == row_y)
        .map(|d| {
            (
                d.dst.0 / STAGE_SCALE,
                (d.dst.0 + d.dst.2 as i32) / STAGE_SCALE,
            )
        })
        .collect();
    // The row also carries the `/` separator at the panel's +57 seat; every
    // other glyph on it is a numeral and has to centre in one cell.
    let (numerals, rest): (Vec<_>, Vec<_>) = glyphs.iter().partition(|(l, r)| {
        cells
            .iter()
            .any(|c| (l + r) / 2 >= *c && (l + r) / 2 < *c + DIGIT_W)
    });
    assert_eq!(
        numerals.len(),
        8,
        "the fallback seated {} of 8 digits on the cell grid; strays: {rest:?}",
        numerals.len()
    );
    for (l, r) in rest {
        let mid = (l + r) / 2;
        assert!(
            (SOLO_PANEL_X + 57..SOLO_PANEL_X + 65).contains(&mid),
            "an extra glyph at {l}..{r} is neither a numeral cell nor the `/`"
        );
    }
}

/// The proportional fields - names and the diagnostic rows - have to clear
/// the retail dialog font's actual advances, or they collide on screen.
/// Skips and passes when `extracted/font/` is absent (same gating as every
/// other artifact-dependent test), so CI does not need redistributed Sony
/// bytes.
///
/// The numeral fields are **not** here: retail draws those in fixed cells,
/// not in this font, and they are covered font-independently by
/// [`numeral_fields_fit_their_widest_value_in_retail_cells`].
#[test]
fn bar_panel_and_diag_columns_clear_the_retail_font_or_skips() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(root) = manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|w| w.join("extracted"))
        .filter(|c| c.join("font").is_dir())
    else {
        eprintln!("extracted/font not present - skipping");
        return;
    };
    let font = legaia_font::Font::load_from_extracted(&root).expect("load extracted font");
    let width = |s: &str| -> i32 {
        font.layout_ascii(s)
            .glyphs
            .last()
            .map(|g| g.dst_x + g.width as i32)
            .unwrap_or(0)
    };

    // The name is the one proportional field on either surface: it must end
    // before the label cell that follows it.
    assert!(
        16 + width("Songi") <= 80,
        "the longest party name overruns the HP label seat at 80"
    );
    assert!(
        5 + width("Songi") <= 64,
        "the longest party name overruns the panel's LV label at +64"
    );

    // Diagnostic monster row: name at 0, HP numerals at 78, K.O. at 150,
    // status strip at 190.
    let columns: [(i32, i32, &str); 3] = [
        (0, width("Juggernaut"), "name"),
        (78, width("250/300"), "HP"),
        (150, width("K.O."), "K.O."),
    ];
    for pair in columns.windows(2) {
        let (x, w, label) = pair[0];
        let (next_x, _, next_label) = pair[1];
        assert!(
            x + w <= next_x,
            "{label} field ends at {} but {next_label} starts at {next_x}",
            x + w
        );
    }
    let last_field_end = columns[2].0 + columns[2].1;
    assert!(
        last_field_end <= 190,
        "row fields end at {last_field_end}, past the status column at 190"
    );

    // Non-vacuous: a full diagnostic monster row really draws its status strip.
    let mut slot = slot_view("Juggernaut", false, false, 250, 300, 0, 0);
    slot.status_sprite = 0x1B;
    let draws = hud_frame(&font, &[slot], &[], &[], true).text;
    assert!(
        draws.iter().any(|d| d.dst.0 >= 190),
        "status column produced no glyph - the fixture is not exercising a full row"
    );
}
