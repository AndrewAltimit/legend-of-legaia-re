use super::*;

#[test]
fn letterbox_scale_pillarbox() {
    let (sx, sy) = letterbox_scale(800, 400, 100, 100);
    assert!((sx - 0.5).abs() < 1e-4, "sx={}", sx);
    assert!((sy - 1.0).abs() < 1e-4, "sy={}", sy);
}

#[test]
fn letterbox_scale_letterbox() {
    let (sx, sy) = letterbox_scale(400, 800, 100, 100);
    assert!((sx - 1.0).abs() < 1e-4, "sx={}", sx);
    assert!((sy - 0.5).abs() < 1e-4, "sy={}", sy);
}

#[test]
fn sprite_draws_translate_world_positions_with_anchor() {
    let reqs = vec![
        SpriteRequest {
            world_x: 5,
            world_y: 7,
            atlas_src: (16, 0, 14, 15),
            color: [1.0, 1.0, 1.0, 1.0],
        },
        SpriteRequest {
            world_x: 0,
            world_y: 0,
            atlas_src: (0, 16, 14, 15),
            color: [1.0, 0.0, 0.0, 1.0],
        },
    ];
    let draws = sprite_draws_for(&reqs, (100, 200));
    assert_eq!(draws.len(), 2);
    assert_eq!(draws[0].dst, (105, 207, 14, 15));
    assert_eq!(draws[0].src, (16, 0, 14, 15));
    assert_eq!(draws[1].dst, (100, 200, 14, 15));
    assert_eq!(draws[1].color, [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn dialog_clut_color_distinct_palette() {
    let white = dialog_clut_color(0);
    let gold = dialog_clut_color(1);
    let red = dialog_clut_color(3);
    assert_eq!(white[0], 1.0);
    assert!(gold[0] > 0.9 && gold[2] < 0.5);
    assert!(red[0] > 0.9 && red[1] < 0.5);
    // Out-of-range index falls through to dim.
    let oob = dialog_clut_color(99);
    assert!(oob[0] < 0.9);
}

#[test]
fn dialog_box_default_layout_origin() {
    let l = DialogBoxLayout::default();
    assert_eq!(l.origin, (8, 168));
    assert_eq!(l.line_h, 14);
}

#[test]
fn dialog_box_draws_emits_one_quad_per_glyph() {
    let font = legaia_font::synthetic_for_tests();
    let glyphs = vec![
        DialogGlyphView {
            byte: b'a',
            clut: 0,
        },
        DialogGlyphView {
            byte: b'b',
            clut: 0,
        },
        DialogGlyphView {
            byte: b'c',
            clut: 1,
        },
    ];
    let layout = DialogBoxLayout::default();
    let draws = dialog_box_draws_for(&font, &glyphs, &layout);
    assert_eq!(draws.len(), 3);
    // Third glyph uses gold tint.
    assert!(draws[2].color[2] < 0.5);
}

#[test]
fn dialog_box_draws_handle_newline() {
    let font = legaia_font::synthetic_for_tests();
    let glyphs = vec![
        DialogGlyphView {
            byte: b'a',
            clut: 0,
        },
        DialogGlyphView {
            byte: b'\n',
            clut: 0,
        },
        DialogGlyphView {
            byte: b'b',
            clut: 0,
        },
    ];
    let layout = DialogBoxLayout::default();
    let draws = dialog_box_draws_for(&font, &glyphs, &layout);
    // Two glyph quads (newline isn't drawn).
    assert_eq!(draws.len(), 2);
    // Second glyph y > first glyph y by at least line_h.
    assert!(draws[1].dst.1 - draws[0].dst.1 >= layout.line_h - 4);
}

#[test]
fn dialog_box_draws_wrap_when_too_wide() {
    let font = legaia_font::synthetic_for_tests();
    // Tiny panel that fits maybe 2-3 glyphs per row.
    let layout = DialogBoxLayout {
        origin: (0, 0),
        size: (40, 60),
        padding: (2, 2),
        line_h: 14,
        cols: 4,
    };
    let glyphs: Vec<_> = (0..12)
        .map(|_| DialogGlyphView {
            byte: b'a',
            clut: 0,
        })
        .collect();
    let draws = dialog_box_draws_for(&font, &glyphs, &layout);
    // Expect more than one row and the y coordinates to vary.
    let rows: std::collections::HashSet<i32> = draws.iter().map(|d| d.dst.1).collect();
    assert!(rows.len() >= 2);
}

#[test]
fn dialog_panel_draws_for_wrapper() {
    let font = legaia_font::synthetic_for_tests();
    let panel: Vec<(u8, u8)> = vec![(b'a', 0), (b'b', 1)];
    let layout = DialogBoxLayout::default();
    let draws = dialog_panel_draws_for(&font, &panel, &layout);
    assert_eq!(draws.len(), 2);
}

#[test]
fn text_draws_translate_layout_to_screen_space() {
    let font = legaia_font::synthetic_for_tests();
    let layout = font.layout(b"Ab");
    let pen = (10, 20);
    let color = [1.0, 0.5, 0.25, 1.0];
    let draws = text_draws_for(&layout, pen, color);
    assert_eq!(draws.len(), layout.glyphs.len());
    let g0 = layout.glyphs[0];
    let d0 = draws[0];
    assert_eq!(d0.dst.0, pen.0 + g0.dst_x);
    assert_eq!(d0.dst.1, pen.1 + g0.dst_y);
    assert_eq!(d0.dst.2, g0.width);
    assert_eq!(d0.src, (g0.atlas_x, g0.atlas_y, g0.width, g0.height));
    assert_eq!(d0.color, color);
}
