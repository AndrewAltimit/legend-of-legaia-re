use super::*;
use crate::{decode_rgba8, parse_strict};

/// 4bpp 16x4 TIM, four palettes (CLUT 16x4), indices 1..=6 per row.
fn tim4() -> Vec<u8> {
    let mut b = vec![];
    b.extend_from_slice(&0x10u32.to_le_bytes());
    b.extend_from_slice(&0x08u32.to_le_bytes());
    b.extend_from_slice(&(12u32 + 16 * 4 * 2).to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&511u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    for p in 0..4u16 {
        for e in 0..16u16 {
            // Distinct colour per (palette, entry); entry 0 transparent.
            let c = if e == 0 {
                0
            } else {
                (e & 0x1F) | (((p * 7) & 0x1F) << 5) | (((e + p) & 0x1F) << 10)
            };
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    // 16x4 @4bpp: fb_w = 4, h = 4.
    b.extend_from_slice(&(12u32 + 4 * 4 * 2).to_le_bytes());
    b.extend_from_slice(&896u16.to_le_bytes());
    b.extend_from_slice(&256u16.to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    for row in 0..4u8 {
        for pair in 0..8u8 {
            let a = (pair * 2 + row) % 6 + 1;
            let c = (pair * 2 + 1 + row) % 6 + 1;
            b.push(a | (c << 4));
        }
    }
    b
}

fn quadrant_map(tim: &Tim) -> Vec<u16> {
    // Left half palette 1, right half palette 3.
    let w = tim.pixel_width();
    (0..w * tim.pixel_height())
        .map(|i| if i % w < w / 2 { 1 } else { 3 })
        .collect()
}

#[test]
fn composite_round_trip_is_byte_identical() {
    let bytes = tim4();
    let tim = parse_strict(&bytes).unwrap();
    let ctx = PaletteContext {
        external: vec![],
        map: Some(quadrant_map(&tim)),
    };
    let sets = all_sets(&tim, &ctx);
    let (w, h, rgba) = render_composite(&tim, &sets, ctx.map.as_ref().unwrap()).unwrap();
    let png = rgba_png(w, h, &rgba).unwrap();
    let imp = import_png(&tim, &png, &ctx, &EncodeOptions::default()).unwrap();
    assert_eq!(imp.kind, ImportKind::Composite(View::InGame));
    assert_eq!(imp.encoded.bytes, bytes);
    assert_eq!(imp.palette_entries_changed, 0);
}

#[test]
fn strip_edit_changes_only_the_edited_entry() {
    let bytes = tim4();
    let tim = parse_strict(&bytes).unwrap();
    let g = standalone_strip_geom(&tim).unwrap();
    let own = own_palettes(&tim);
    let mut rgba = render_strip(&own, g, g.width());
    // Repaint palette 2, entry 5 to pure red.
    for y in 2 * g.cell_h..3 * g.cell_h {
        for x in 5 * g.cell_w..6 * g.cell_w {
            let o = (y * g.width() + x) * 4;
            rgba[o..o + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    let png = rgba_png(g.width(), g.height(), &rgba).unwrap();
    let imp = import_png(
        &tim,
        &png,
        &PaletteContext::default(),
        &EncodeOptions::default(),
    )
    .unwrap();
    assert_eq!(imp.kind, ImportKind::PaletteStrip);
    assert_eq!(imp.palette_entries_changed, 1);
    let out = parse_strict(&imp.encoded.bytes).unwrap();
    let got = own_palettes(&out);
    for p in 0..4 {
        for e in 0..16 {
            if (p, e) == (2, 5) {
                assert_eq!(got[p][e], 0x001F);
            } else {
                assert_eq!(got[p][e], own[p][e], "palette {p} entry {e} must not move");
            }
        }
    }
    // Indices untouched.
    assert_eq!(indices(&out), indices(&tim));
}

#[test]
fn strip_of_any_integer_scale_is_accepted_and_smoothing_is_refused() {
    let tim = parse_strict(&tim4()).unwrap();
    let own = own_palettes(&tim);
    let g = StripGeom {
        per: 16,
        count: 4,
        cell_w: 3,
        cell_h: 5,
    };
    let mut rgba = render_strip(&own, g, g.width());
    let png = rgba_png(g.width(), g.height(), &rgba).unwrap();
    let imp = import_png(
        &tim,
        &png,
        &PaletteContext::default(),
        &EncodeOptions::default(),
    )
    .unwrap();
    assert_eq!(imp.kind, ImportKind::PaletteStrip);
    assert_eq!(imp.palette_entries_changed, 0);
    // One off-colour pixel inside the entry-1 cell (a smoothed edge).
    rgba[(g.width() + 4) * 4] ^= 0x40;
    let png = rgba_png(g.width(), g.height(), &rgba).unwrap();
    let err = import_png(
        &tim,
        &png,
        &PaletteContext::default(),
        &EncodeOptions::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("not one solid colour"), "{err}");
}

#[test]
fn indexed_png_round_trip_writes_indices_only() {
    let bytes = tim4();
    let tim = parse_strict(&bytes).unwrap();
    let own = own_palettes(&tim);
    let png = indexed_png(&tim, &own[2]).unwrap();
    let imp = import_png(
        &tim,
        &png,
        &PaletteContext::default(),
        &EncodeOptions::default(),
    )
    .unwrap();
    assert_eq!(imp.kind, ImportKind::Indexed(2));
    assert_eq!(imp.encoded.bytes, bytes);

    // Repaint one pixel to index 9 in the indexed domain.
    let (w, h, mut idx, _) = read_indexed_png(&png).unwrap();
    idx[3] = 9;
    let mut edited = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut edited, w as u32, h as u32);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        let plte: Vec<u8> = own[2]
            .iter()
            .flat_map(|&c| bgr555_to_rgba8(c)[..3].to_vec())
            .collect();
        enc.set_palette(plte);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&idx).unwrap();
    }
    let imp = import_png(
        &tim,
        &edited,
        &PaletteContext::default(),
        &EncodeOptions::default(),
    )
    .unwrap();
    let out = parse_strict(&imp.encoded.bytes).unwrap();
    assert_eq!(indices(&out)[3], 9);
    assert_eq!(own_palettes(&out), own, "no palette touched");
}

#[test]
fn image_through_one_palette_is_recognised_and_writes_only_that_palette() {
    let bytes = tim4();
    let tim = parse_strict(&bytes).unwrap();
    let own = own_palettes(&tim);
    let mut rgba = decode_rgba8(&tim, 3).unwrap();
    // New colour on pixel 0: must land in palette 3 only.
    rgba[0..4].copy_from_slice(&[255, 0, 255, 255]);
    let png = rgba_png(16, 4, &rgba).unwrap();
    let imp = import_png(
        &tim,
        &png,
        &PaletteContext::default(),
        &EncodeOptions::default(),
    )
    .unwrap();
    assert_eq!(imp.kind, ImportKind::Image(View::Palette(3)));
    let out = parse_strict(&imp.encoded.bytes).unwrap();
    let got = own_palettes(&out);
    for p in [0, 1, 2] {
        assert_eq!(got[p], own[p], "palette {p} must stay byte-identical");
    }
    assert_eq!(&decode_rgba8(&out, 3).unwrap()[0..4], &[255, 0, 255, 255]);
}

#[test]
fn mapped_image_keeps_each_region_in_its_palette() {
    let bytes = tim4();
    let tim = parse_strict(&bytes).unwrap();
    let map = quadrant_map(&tim);
    let ctx = PaletteContext {
        external: vec![],
        map: Some(map.clone()),
    };
    let sets = all_sets(&tim, &ctx);
    let mut rgba = decode_mapped(&tim, &sets, &map).unwrap();
    // Repaint right-half pixel (15, 0) with palette 3's entry-2 colour:
    // an existing colour of that region's palette, so no palette moves.
    let want = bgr555_to_rgba8(sets[3][2]);
    rgba[15 * 4..16 * 4].copy_from_slice(&want);
    let png = rgba_png(16, 4, &rgba).unwrap();
    let imp = import_png(&tim, &png, &ctx, &EncodeOptions::default()).unwrap();
    assert_eq!(imp.kind, ImportKind::Image(View::InGame));
    assert_eq!(imp.palette_entries_changed, 0);
    let out = parse_strict(&imp.encoded.bytes).unwrap();
    assert_eq!(indices(&out)[15], 2);
    let mut expect = indices(&tim);
    expect[15] = 2;
    assert_eq!(indices(&out), expect);
}

#[test]
fn external_palette_is_read_only() {
    let tim = parse_strict(&tim4()).unwrap();
    let ext: Vec<u16> = (0..16u16)
        .map(|e| if e == 0 { 0 } else { 0x4000 | e })
        .collect();
    // Pixel 0 drawn through the external palette (index 4), rest palette 0.
    let mut map = uniform_map(&tim, 0);
    map[0] = 4;
    let ctx = PaletteContext {
        external: vec![ext],
        map: Some(map.clone()),
    };
    let sets = all_sets(&tim, &ctx);
    let mut rgba = decode_mapped(&tim, &sets, &map).unwrap();
    rgba[0..4].copy_from_slice(&[255, 255, 0, 255]); // not in the external palette
    let png = rgba_png(16, 4, &rgba).unwrap();
    let err = import_png(&tim, &png, &ctx, &EncodeOptions::default()).unwrap_err();
    assert!(
        err.to_string().contains("belongs to another texture"),
        "{err}"
    );
    // Quantize folds it instead.
    let opts = EncodeOptions {
        quantize: true,
        ..Default::default()
    };
    let imp = import_png(&tim, &png, &ctx, &opts).unwrap();
    assert_eq!(imp.encoded.quantized_pixels, 1);
    assert_eq!(imp.palette_entries_changed, 0);
}

#[test]
fn composite_strip_edit_carries_through_untouched_pixels() {
    let bytes = tim4();
    let tim = parse_strict(&bytes).unwrap();
    let ctx = PaletteContext {
        external: vec![],
        map: Some(quadrant_map(&tim)),
    };
    let sets = all_sets(&tim, &ctx);
    let (w, h, mut rgba) = render_composite(&tim, &sets, ctx.map.as_ref().unwrap()).unwrap();
    let g = composite_geom(&tim).unwrap();
    // Recolour palette 1 entry 1 in the strip only.
    let top_h = tim.pixel_height();
    for y in top_h + g.cell_h..top_h + 2 * g.cell_h {
        for x in g.cell_w..2 * g.cell_w {
            let o = (y * w + x) * 4;
            rgba[o..o + 4].copy_from_slice(&[0, 0, 255, 255]);
        }
    }
    let png = rgba_png(w, h, &rgba).unwrap();
    let imp = import_png(&tim, &png, &ctx, &EncodeOptions::default()).unwrap();
    assert_eq!(imp.palette_entries_changed, 1);
    let out = parse_strict(&imp.encoded.bytes).unwrap();
    assert_eq!(indices(&out), indices(&tim), "indices untouched");
    assert_eq!(own_palettes(&out)[1][1], 0x7C00);
}

#[test]
fn unknown_shape_names_the_accepted_ones() {
    let tim = parse_strict(&tim4()).unwrap();
    let png = rgba_png(5, 5, &[0u8; 100]).unwrap();
    let err = import_png(
        &tim,
        &png,
        &PaletteContext::default(),
        &EncodeOptions::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("palette strip"), "{err}");
}
