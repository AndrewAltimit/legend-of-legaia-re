use super::*;

/// An attached escape sprite lays out at the pen, `y_offset` down, and
/// advances the pen by its own width; a numeric escape (no sprite) and a
/// font with nothing attached lay out as nothing.
#[test]
fn a_sprite_escape_lays_out_at_the_pen() {
    use escape_icons::{EscapeIcon, EscapeIcons};
    let icon = EscapeIcon {
        index: 1,
        w: 16,
        h: 16,
        y_offset: -2,
        advance: 18,
        rgba: vec![0xAB; 16 * 16 * 4],
    };
    let icons = EscapeIcons {
        icons: vec![None, Some(icon)],
    };
    let base = synthetic_for_tests();
    let (_, h0) = base.atlas_dimensions();
    assert!(base.layout(&[b'A', 0xCE, 1, b'B']).glyphs.len() == 2);
    let f = base.with_escape_icons(&icons);
    let (w, h) = f.atlas_dimensions();
    assert_eq!(h, h0 + 16, "the sprite rows sit below the glyph cells");
    let l = f.layout(&[b'A', 0xCE, 1, b'B', 0xCE, 0, b'C']);
    assert_eq!(l.glyphs.len(), 4);
    let g = l.glyphs[1];
    let a = f.advance_of(b'A') as i32;
    assert_eq!((g.byte, g.dst_x, g.dst_y), (ESCAPE_GLYPH_BYTE, a, -2));
    assert_eq!((g.atlas_x, g.atlas_y, g.width, g.height), (0, h0, 16, 16));
    assert_eq!(l.glyphs[2].dst_x, a + 18);
    let px = ((g.atlas_y * w) * 4) as usize;
    assert_eq!(f.atlas_rgba()[px], 0xAB, "the sprite's own texels");
}

#[test]
fn glyph_origin_first_char() {
    let (x, y) = Font::glyph_origin(0x20).unwrap();
    assert_eq!((x, y), (0, 0));
}

#[test]
fn glyph_origin_a() {
    // 'A' = 0x41. Column 1, row 2.
    let (x, y) = Font::glyph_origin(b'A').unwrap();
    assert_eq!((x, y), (GLYPH_W, GLYPH_H * 2));
}

#[test]
fn glyph_origin_below_first_char_returns_none() {
    assert!(Font::glyph_origin(0x10).is_none());
    assert!(Font::glyph_origin(0x1F).is_none());
}

#[test]
fn glyph_origin_last_char() {
    // 0xFF = column 0xF, row 0xD.
    let (x, y) = Font::glyph_origin(0xFF).unwrap();
    assert_eq!((x, y), (15 * GLYPH_W, 13 * GLYPH_H));
}

#[test]
fn layout_ascii_hands_a_latin1_char_back_as_its_byte() {
    // A disc string reader returns an accent cell `0x82` as `U+0082`; the
    // font must lay out the one byte, not its two UTF-8 bytes.
    let f = synthetic_for_tests();
    let via_str = f.layout_ascii("@B\u{82}g");
    let via_bytes = f.layout(b"@B\x82g");
    assert_eq!(via_str.glyphs.len(), via_bytes.glyphs.len());
    assert_eq!(via_str.advance_x, via_bytes.advance_x);
}

#[test]
fn synthetic_font_layout_advances_per_char() {
    let f = synthetic_for_tests();
    // Three chars with the synthetic width formula; advances accumulate.
    let layout = f.layout(b"AAA");
    assert_eq!(layout.glyphs.len(), 3);
    let advance0 = f.advance_of(b'A');
    assert_eq!(layout.glyphs[0].dst_x, 0);
    assert_eq!(layout.glyphs[1].dst_x, advance0 as i32);
    assert_eq!(layout.glyphs[2].dst_x, (advance0 * 2) as i32);
    assert!(layout.advance_x >= advance0 * 3);
}

#[test]
fn newline_advances_y_resets_x() {
    let f = synthetic_for_tests();
    let layout = f.layout(&[b'A', b'B', NEWLINE, b'C', b'D']);
    // Two glyphs on line 0, two on line 1.
    assert_eq!(layout.glyphs.len(), 4);
    assert_eq!(layout.glyphs[2].dst_x, 0);
    assert_eq!(layout.glyphs[2].dst_y, LINE_HEIGHT as i32);
    assert_eq!(layout.glyphs[3].dst_y, LINE_HEIGHT as i32);
}

#[test]
fn escape_byte_consumes_one_operand() {
    let f = synthetic_for_tests();
    // A, 0xCE, 0x05 (operand), B → only A and B emit glyphs.
    let layout = f.layout(&[b'A', 0xCE, 0x05, b'B']);
    assert_eq!(layout.glyphs.len(), 2);
    assert_eq!(layout.glyphs[1].byte, b'B');
}

#[test]
fn wrap_breaks_at_word_boundary() {
    let f = synthetic_for_tests();
    // "AA AAAA" - synthetic widths give A,B,...,Z width 6 (b - 0x20 + 1)
    // ... actually let's use a wide letter to force wrap.
    // The synthetic font widths are deterministic but varied; just
    // assert the wrapped output contains a NEWLINE that wasn't in
    // the input when the box width is small enough.
    let input = b"hello world there";
    let wrapped = f.wrap_bytes(input, 50);
    assert!(
        wrapped.contains(&NEWLINE),
        "narrow box width should force a wrap: {:?}",
        wrapped
    );
}

#[test]
fn wrap_passes_through_when_fits() {
    let f = synthetic_for_tests();
    let wrapped = f.wrap_bytes(b"hi", 1000);
    assert!(
        !wrapped.contains(&NEWLINE),
        "wide box shouldn't insert wraps: {:?}",
        wrapped
    );
}

#[test]
fn wrap_preserves_explicit_newlines() {
    let f = synthetic_for_tests();
    let input = &[b'A', NEWLINE, b'B'];
    let wrapped = f.wrap_bytes(input, 1000);
    assert_eq!(wrapped, input);
}

#[test]
fn null_terminator_stops_layout() {
    let f = synthetic_for_tests();
    let layout = f.layout(&[b'A', 0, b'B']);
    assert_eq!(layout.glyphs.len(), 1);
    assert_eq!(layout.glyphs[0].byte, b'A');
}

#[test]
fn unprintable_bytes_are_skipped() {
    let f = synthetic_for_tests();
    let layout = f.layout(&[b'A', 0x05, 0x10, b'B']);
    assert_eq!(layout.glyphs.len(), 2);
    // 0x05 / 0x10 don't advance the pen - B sits right after A.
    assert_eq!(layout.glyphs[1].dst_x, f.advance_of(b'A') as i32);
}

#[test]
fn escape_table_parses_real_metadata_shape() {
    // Synthesize a metadata JSON with two entries and verify both parse.
    let json = r#"{
            "escape_table": {
                "entries": [
                    {"idx": 0, "string_id": 55, "advance_px": 16, "y_offset": -2},
                    {"idx": 1, "string_id": 33, "advance_px": 8, "y_offset": 0}
                ]
            }
        }"#;
    let table = EscapeTable::from_json(json).expect("parse");
    assert_eq!(table.entries.len(), 2);
    assert_eq!(table.entry(0).unwrap().string_id, 55);
    assert_eq!(table.entry(1).unwrap().advance_px, 8);
    assert_eq!(table.entry(0).unwrap().y_offset, -2);
    assert!(table.entry(2).is_none());
}

#[test]
fn escape_table_returns_error_when_missing_field() {
    let json = r#"{"unrelated": 42}"#;
    assert!(EscapeTable::from_json(json).is_err());
}

#[test]
fn escape_table_rejects_non_json_garbage() {
    // Truncated / non-JSON input must Err, not panic.
    assert!(EscapeTable::from_json("").is_err());
    assert!(EscapeTable::from_json("{not json").is_err());
    assert!(EscapeTable::from_json("\u{0}\u{1}\u{2}").is_err());
}

#[test]
fn layout_empty_input_yields_empty_layout() {
    let f = synthetic_for_tests();
    let layout = f.layout(&[]);
    assert!(layout.glyphs.is_empty());
    assert_eq!(layout.advance_x, 0);
    assert_eq!(layout.advance_y, LINE_HEIGHT);
}

#[test]
fn layout_all_bytes_does_not_panic() {
    // Every possible byte value in one string: control bytes, escape
    // opcodes (with and without trailing operand), printables. Must not
    // panic and must produce a bounded layout.
    let f = synthetic_for_tests();
    let all: Vec<u8> = (1u16..=255).map(|b| b as u8).collect(); // skip 0 (terminator)
    let layout = f.layout(&all);
    // Bounded glyph count: at most one per byte.
    assert!(layout.glyphs.len() <= all.len());
}

#[test]
fn layout_trailing_escape_with_no_operand_is_safe() {
    // 0xCE at the very end has no operand byte to consume.
    let f = synthetic_for_tests();
    let layout = f.layout(&[b'A', 0xCE]);
    assert_eq!(layout.glyphs.len(), 1);
}

#[test]
fn wrap_bytes_on_garbage_does_not_panic() {
    let f = synthetic_for_tests();
    let junk: Vec<u8> = (0..512).map(|i| (i * 7 + 1) as u8).collect();
    // Tiny box width forces many wrap decisions on arbitrary bytes.
    let wrapped = f.wrap_bytes(&junk, 4);
    // Output is bounded (input + inserted newlines, no infinite growth).
    assert!(wrapped.len() <= junk.len() * 2 + 16);
}

#[test]
fn wrap_bytes_zero_box_width_terminates() {
    // A zero-width box is a degenerate input; wrapping must still
    // terminate and not loop forever.
    let f = synthetic_for_tests();
    let wrapped = f.wrap_bytes(b"hello world", 0);
    assert!(!wrapped.is_empty());
}

#[test]
fn parse_widths_csv_rejects_malformed_lines() {
    let dir = std::env::temp_dir().join("legaia-font-fuzz");
    let _ = std::fs::create_dir_all(&dir);

    // A line with no comma after the header row → Err.
    let bad = dir.join("bad_no_comma.csv");
    std::fs::write(&bad, "header\nnocommahere\n").unwrap();
    assert!(parse_widths_csv(&bad).is_err());

    // A line with a non-hex char field → Err.
    let bad2 = dir.join("bad_hex.csv");
    std::fs::write(&bad2, "header\nZZ,1,x,8\n").unwrap();
    assert!(parse_widths_csv(&bad2).is_err());

    // A line with a non-numeric width → Err.
    let bad3 = dir.join("bad_width.csv");
    std::fs::write(&bad3, "header\n0x41,65,A,notanumber\n").unwrap();
    assert!(parse_widths_csv(&bad3).is_err());

    let _ = std::fs::remove_file(&bad);
    let _ = std::fs::remove_file(&bad2);
    let _ = std::fs::remove_file(&bad3);
}

#[test]
fn parse_widths_csv_accepts_well_formed_synthetic() {
    let dir = std::env::temp_dir().join("legaia-font-fuzz");
    let _ = std::fs::create_dir_all(&dir);
    let good = dir.join("good.csv");
    // header row skipped; one valid data row for 'A' (0x41) width 12.
    std::fs::write(
        &good,
        "char_hex,char_dec,char_repr,width_px\n0x41,65,\"A\",12\n",
    )
    .unwrap();
    let widths = parse_widths_csv(&good).unwrap();
    assert_eq!(widths[0x41], 12);
    let _ = std::fs::remove_file(&good);
}

/// Build a minimal 4bpp font TIM (only cell 'A' painted) + a PSX-EXE with a
/// one-entry width table and assert `from_disc_tim_and_scus` places the
/// stencilled glyph + advance where the atlas expects them.
#[test]
fn from_disc_tim_and_scus_stencils_and_reads_widths() {
    // 256x256 4bpp page: 64 halfwords * 2 = 128 bytes per row.
    let stride = 64 * 2;
    let mut page = vec![0u8; stride * 256];
    // 'A' (0x41) cell: U = 1*16 = 16, V = (0x40)-0x20 = 32. Paint the
    // top-left glyph pixel fill (index 15) and the one to its right shadow
    // (index 14).
    let put = |page: &mut [u8], x: usize, y: usize, idx: u8| {
        let b = &mut page[y * stride + x / 2];
        if x & 1 == 0 {
            *b = (*b & 0xF0) | idx;
        } else {
            *b = (*b & 0x0F) | (idx << 4);
        }
    };
    put(&mut page, 16, 32, 15); // fill -> white
    put(&mut page, 17, 32, 14); // shadow -> (32,32,32)

    // Wrap the page in a TIM: header + CLUT block (16 entries) + image block.
    let mut tim = Vec::new();
    tim.extend_from_slice(&0x10u32.to_le_bytes()); // magic
    tim.extend_from_slice(&0x8u32.to_le_bytes()); // flags: 4bpp + CLUT
    let clut_body = 4 + 8 + 16 * 2; // len(4) + dst rect(8) + 16 entries(32) = 44
    tim.extend_from_slice(&(clut_body as u32).to_le_bytes());
    tim.extend_from_slice(&0u16.to_le_bytes()); // clut dx
    tim.extend_from_slice(&510u16.to_le_bytes()); // clut dy
    tim.extend_from_slice(&16u16.to_le_bytes()); // clut w
    tim.extend_from_slice(&1u16.to_le_bytes()); // clut h
    tim.extend_from_slice(&[0u8; 32]); // 16 CLUT entries (ignored)
    let img_len = 4 + 8 + page.len();
    tim.extend_from_slice(&(img_len as u32).to_le_bytes());
    tim.extend_from_slice(&896u16.to_le_bytes()); // img dx
    tim.extend_from_slice(&0u16.to_le_bytes()); // img dy
    tim.extend_from_slice(&64u16.to_le_bytes()); // img w16
    tim.extend_from_slice(&256u16.to_le_bytes()); // img h
    tim.extend_from_slice(&page);

    // Minimal PSX-EXE: magic + t_addr at 0x18, width table at file offset
    // (WIDTH_TABLE_RAM - t_addr) + 0x800.
    let t_addr = SCUS_LOAD_ADDR_FALLBACK;
    let file_off = (WIDTH_TABLE_RAM - t_addr) as usize + PSX_EXE_HEADER as usize;
    let mut scus = vec![0u8; file_off + 256];
    scus[0..8].copy_from_slice(b"PS-X EXE");
    scus[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
    scus[file_off + 0x41] = 12; // width['A'] = 12

    let font = Font::from_disc_tim_and_scus(&tim, &scus).unwrap();
    assert_eq!(font.atlas_dimensions(), (COLS * GLYPH_W, ROWS * GLYPH_H));
    // 'A' advance = width + inter-glyph pad.
    assert_eq!(font.advance_of(b'A'), 12 + INTER_GLYPH_PAD as u32);
    // The painted cell's top-left is fill (white, opaque); the pixel to its
    // right is the dark shadow.
    let (ox, oy) = Font::glyph_origin(b'A').unwrap();
    let aw = (COLS * GLYPH_W) as usize;
    let at = |x: u32, y: u32| -> [u8; 4] {
        let o = ((y as usize) * aw + x as usize) * 4;
        font.atlas_rgba()[o..o + 4].try_into().unwrap()
    };
    assert_eq!(at(ox, oy), [255, 255, 255, 255]);
    assert_eq!(
        at(ox + 1, oy),
        [
            FONT_SHADOW_TEXEL,
            FONT_SHADOW_TEXEL,
            FONT_SHADOW_TEXEL,
            FONT_SHADOW_ALPHA
        ]
    );
}

/// Byte-exact parity oracle: the font decoded straight from the disc
/// (`PROT.DAT` TIM + SCUS width table) must equal the save-state extraction
/// the native engine loads from `extracted/font/`. Skips (passes) when the
/// extracted artifacts aren't present. Proves the WASM site's disc-only
/// font is identical to native's.
#[test]
fn disc_font_matches_extracted_artifacts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    let prot = root.join("PROT.DAT");
    let scus = root.join("SCUS_942.54");
    let atlas_png = root.join("font/dialog_font_atlas.png");
    let widths_csv = root.join("font/dialog_font_widths.csv");
    if !(prot.exists() && scus.exists() && atlas_png.exists() && widths_csv.exists()) {
        eprintln!("[skip] extracted/ font artifacts absent - disc-font parity");
        return;
    }
    let prot_bytes = std::fs::read(&prot).unwrap();
    let scus_bytes = std::fs::read(&scus).unwrap();
    let off = FONT_TIM_PROT_DAT_OFFSET as usize;
    let tim = &prot_bytes[off..off + FONT_TIM_LEN];
    let disc = Font::from_disc_tim_and_scus(tim, &scus_bytes).unwrap();
    let extracted = Font::load_paths(&atlas_png, &widths_csv).unwrap();
    assert_eq!(
        disc.atlas_dimensions(),
        extracted.atlas_dimensions(),
        "atlas dims diverge"
    );
    assert!(
        disc.atlas_rgba() == extracted.atlas_rgba(),
        "disc-decoded font atlas differs from the extracted (save-state) atlas"
    );
    for c in 0u16..256 {
        assert_eq!(
            disc.advance_of(c as u8),
            extracted.advance_of(c as u8),
            "advance for byte 0x{c:02X} diverges"
        );
    }
}

/// Retail's text shadow is a fixed `(33, 33, 33)` and its ink the tier-7
/// grey `206` (`v0_1_tetsu_dialogue_accept`, `menu_status_field`); the shader
/// multiplies the texel by the ink, so the stored shadow must land on `33`
/// under that ink - `32` drew it at `26`.
#[test]
fn shadow_texel_lands_on_retails_shadow_under_the_tier_7_ink() {
    let ink = 206.0f32 / 255.0;
    let drawn = (f32::from(FONT_SHADOW_TEXEL) * ink).round() as u8;
    assert_eq!(drawn, 33);
}
