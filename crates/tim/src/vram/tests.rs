use super::*;
use crate::parse;

/// Build a 4-pixel 16bpp TIM at fb_x=64, fb_y=128 - easiest case to verify
/// (no CLUT, image data goes straight into VRAM as-is).
fn tim_16bpp_at(fb_x: u16, fb_y: u16) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x10u32.to_le_bytes());
    buf.extend_from_slice(&0x02u32.to_le_bytes()); // pmode 2 = 16bpp, no CLUT
    // Image: 4 pixels × 1 row at 16bpp = fb_w=4 words
    // bs_len = 12 + 4 * 1 * 2 = 20
    buf.extend_from_slice(&20u32.to_le_bytes());
    buf.extend_from_slice(&fb_x.to_le_bytes());
    buf.extend_from_slice(&fb_y.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes()); // fb_w
    buf.extend_from_slice(&1u16.to_le_bytes()); // h
    // Four 16-bit pixels: 0xAAAA, 0xBBBB, 0xCCCC, 0xDDDD
    buf.extend_from_slice(&0xAAAAu16.to_le_bytes());
    buf.extend_from_slice(&0xBBBBu16.to_le_bytes());
    buf.extend_from_slice(&0xCCCCu16.to_le_bytes());
    buf.extend_from_slice(&0xDDDDu16.to_le_bytes());
    buf
}

#[test]
fn upload_places_pixels_at_fbxy() {
    let mut vram = Vram::new();
    let buf = tim_16bpp_at(64, 128);
    let tim = parse(&buf).unwrap();
    vram.upload_tim(&tim);
    assert_eq!(vram.pixel(64, 128), 0xAAAA);
    assert_eq!(vram.pixel(65, 128), 0xBBBB);
    assert_eq!(vram.pixel(66, 128), 0xCCCC);
    assert_eq!(vram.pixel(67, 128), 0xDDDD);
    // Untouched cells remain zero.
    assert_eq!(vram.pixel(0, 0), 0);
    assert_eq!(vram.pixel(63, 128), 0);
    assert_eq!(vram.pixel(68, 128), 0);
}

/// 4bpp TIM whose CLUT block declares `w=16, h=2` - two 16-colour
/// palettes - loading at `(clut_x, clut_y)`.
fn tim_4bpp_two_palettes(clut_x: u16, clut_y: u16) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x10u32.to_le_bytes());
    buf.extend_from_slice(&0x08u32.to_le_bytes()); // pmode 0 = 4bpp, has CLUT
    // CLUT block: 12-byte header + 16*2 halfwords.
    buf.extend_from_slice(&(12u32 + 16 * 2 * 2).to_le_bytes());
    buf.extend_from_slice(&clut_x.to_le_bytes());
    buf.extend_from_slice(&clut_y.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes()); // w = 16 colours
    buf.extend_from_slice(&2u16.to_le_bytes()); // h = 2 palettes
    for i in 0..16u16 {
        buf.extend_from_slice(&(0x0100 + i).to_le_bytes()); // palette 0
    }
    for i in 0..16u16 {
        buf.extend_from_slice(&(0x0200 + i).to_le_bytes()); // palette 1
    }
    // Image block: 4 words of 4bpp indices at (0, 0).
    buf.extend_from_slice(&(12u32 + 4 * 2).to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&[0u8; 8]);
    buf
}

/// A multi-palette CLUT block lands as a **linear run** along its row:
/// palette `i` at `(fb_x + 16*i, fb_y)`, NOT stacked at `(fb_x, fb_y + i)`.
///
/// This is what retail does (byte-pinned against the town0c field save
/// state: 69 of 69 non-zero secondary palettes sit where the linear layout
/// puts them, 0 where the rectangle would) and what the scene's prims ask
/// for - their CBAs index those odd 16-colour slots. Uploading the block as
/// a `w x h` rectangle left every odd slot empty, so the 4bpp prims reading
/// them decoded to `0x0000`, got discarded, and punched holes in the town
/// ground.
#[test]
fn multi_palette_clut_loads_linearly_along_the_row() {
    let mut vram = Vram::new();
    vram.upload_tim(&parse(&tim_4bpp_two_palettes(0, 480)).unwrap());

    // Palette 0: slot 0 of row 480.
    assert_eq!(vram.pixel(0, 480), 0x0100);
    assert_eq!(vram.pixel(15, 480), 0x010F);
    // Palette 1: the NEXT SLOT ON THE SAME ROW, not the row below.
    assert_eq!(
        vram.pixel(16, 480),
        0x0200,
        "palette 1 belongs at fb_x + 16"
    );
    assert_eq!(vram.pixel(31, 480), 0x020F);
    // The row below is untouched - a rectangle upload would have put
    // palette 1 there and left slot 1 of row 480 dead.
    assert_eq!(
        vram.pixel(0, 481),
        0,
        "palette 1 must not stack onto fb_y + 1"
    );
    assert_eq!(vram.pixel(15, 481), 0);
}

#[test]
fn last_upload_wins_on_overlap() {
    let mut vram = Vram::new();
    vram.upload_tim(&parse(&tim_16bpp_at(0, 0)).unwrap());
    // Same address, different data
    let mut buf = tim_16bpp_at(0, 0);
    // Patch the first pixel of the image block (offset 8 + 4 = 12 then +12 for image header)
    // Image block starts at offset 8 (after header), pixel data starts at 8+12 = 20.
    buf[20] = 0x11;
    buf[21] = 0x11;
    vram.upload_tim(&parse(&buf).unwrap());
    assert_eq!(vram.pixel(0, 0), 0x1111);
}

#[test]
fn upload_clips_at_vram_edge() {
    // fb_x = 1023 leaves only one column inside VRAM
    let mut vram = Vram::new();
    let buf = tim_16bpp_at(1023, 0);
    vram.upload_tim(&parse(&buf).unwrap());
    assert_eq!(vram.pixel(1023, 0), 0xAAAA);
    // Other cells of the source were clipped, so this row stays zero past 1023.
    // (No way to read beyond VRAM_WIDTH; just verify nothing crashed.)
}

#[test]
fn region_has_data_detects_filled_and_empty_rows() {
    let mut vram = Vram::new();
    // Plant a single non-zero pixel inside (10, 20).
    vram.write_words(10, 20, 1, 1, &[0x1234]);
    assert!(vram.region_has_data(0, 20, 64, 1));
    assert!(vram.region_has_data(10, 20, 1, 1));
    assert!(!vram.region_has_data(0, 19, 64, 1));
    // Out-of-bounds rectangles clip silently and report empty.
    assert!(!vram.region_has_data(2000, 0, 64, 1));
    assert!(!vram.region_has_data(0, 1000, 64, 1));
}

#[test]
fn prim_has_texture_data_drops_empty_pages() {
    // Texture page at tpage_x=64, tpage_y=0 (TSB low nibble = 1, depth 4bpp).
    // CLUT row at cy=64, cx=0 (CBA = 64<<6 = 0x1000).
    let tsb = 0x0001;
    let cba = 64 << 6;
    let uvs = [(0, 0), (16, 16), (0, 16)];
    let mut vram = Vram::new();
    // Empty VRAM -> drop.
    assert!(!vram.prim_has_texture_data(cba, tsb, &uvs));
    // CLUT only -> still drop (page absent, would render flat CLUT[0]).
    vram.write_words(0, 64, 16, 1, &[0x1234; 16]);
    assert!(!vram.prim_has_texture_data(cba, tsb, &uvs));
    // Page only -> also drop (no palette, would render transparent
    // holes anyway and just churn the GPU).
    let mut vram2 = Vram::new();
    vram2.write_words(64, 0, 4, 16, &[0x4567; 64]);
    assert!(!vram2.prim_has_texture_data(cba, tsb, &uvs));
    // Both populated -> keep.
    vram.write_words(64, 0, 4, 16, &[0x4567; 64]);
    assert!(vram.prim_has_texture_data(cba, tsb, &uvs));
    // Both populated, and CLUT row is exactly 256 wide for a 4bpp prim:
    // this is the standard multi-palette layout (16 distinct 16-entry
    // palettes per row, picked by the prim's CBA low 6 bits) so it's
    // KEPT - not a depth mismatch.
    let mut vram_multi = Vram::new();
    vram_multi.write_words(0, 64, 256, 1, &[0x1234; 256]);
    vram_multi.write_words(64, 0, 4, 16, &[0x4567; 64]);
    assert!(vram_multi.prim_has_texture_data(cba, tsb, &uvs));
    // A CLUT row populated past 256 entries is *also* legitimate - the
    // CBA addresses 64 palettes per row, so a densely packed one runs
    // the full width. The old width heuristic dropped these; on the disc
    // that meant losing entire buildings. See
    // `PrimTextureStatus::ClutDepthMismatch` for why width cannot decide
    // this, and keep this prim's own palette (populated above) the only
    // thing that does.
    let mut vram_wide = Vram::new();
    vram_wide.write_words(0, 64, 600, 1, &[0x1234; 600]);
    vram_wide.write_words(64, 0, 4, 16, &[0x4567; 64]);
    assert!(vram_wide.prim_has_texture_data(cba, tsb, &uvs));
}

#[test]
fn prim_texture_status_classifies_each_failure_mode() {
    // Texture page at tpage_x=64, tpage_y=0 (TSB low nibble = 1, depth 4bpp).
    // CLUT row at cy=64, cx=0 (CBA = 64<<6 = 0x1000).
    let tsb = 0x0001;
    let cba = 64 << 6;
    let uvs = [(0, 0), (16, 16), (0, 16)];

    // (1) Empty VRAM -> MissingClut.
    let vram = Vram::new();
    match vram.prim_texture_status(cba, tsb, &uvs) {
        PrimTextureStatus::MissingClut { row } => assert_eq!(row, 64),
        other => panic!("expected MissingClut, got {:?}", other),
    }

    // (2) CLUT only, sized correctly for 4bpp -> MissingTexturePage.
    let mut vram = Vram::new();
    vram.write_words(0, 64, 16, 1, &[0x1234; 16]);
    match vram.prim_texture_status(cba, tsb, &uvs) {
        PrimTextureStatus::MissingTexturePage { .. } => {}
        other => panic!("expected MissingTexturePage, got {:?}", other),
    }

    // (3) Both populated, depth correct -> Ok.
    let mut vram = Vram::new();
    vram.write_words(0, 64, 16, 1, &[0x1234; 16]);
    vram.write_words(64, 0, 4, 16, &[0x4567; 64]);
    assert_eq!(
        vram.prim_texture_status(cba, tsb, &uvs),
        PrimTextureStatus::Ok
    );

    // (4) A widely-populated CLUT row is legitimate at every width, and
    // this is the case the old width heuristic got wrong. The CBA
    // addresses 64 distinct 16-entry palettes per row, so a densely
    // packed row runs the full 1024 - the GPU still reads only the 16
    // entries at this prim's own CBA, which case (3) already populated.
    // `town0b` really ships rows like this, and rejecting them deleted
    // whole houses. See `PrimTextureStatus::ClutDepthMismatch`.
    for width in [256usize, 600, VRAM_WIDTH] {
        let mut vram = Vram::new();
        vram.write_words(0, 64, width as u16, 1, &vec![0x1234; width]);
        vram.write_words(64, 0, 4, 16, &[0x4567; 64]);
        assert_eq!(
            vram.prim_texture_status(cba, tsb, &uvs),
            PrimTextureStatus::Ok,
            "a {width}-wide CLUT row must not disqualify a 4bpp prim"
        );
    }

    // (5) ...and a prim whose own 16-entry window is empty is still
    // rejected, so widening (4) did not disarm the check that matters.
    // Palette 1 (CBA x = 16) is populated; this prim reads palette 0.
    let mut vram = Vram::new();
    vram.write_words(16, 64, 16, 1, &[0x1234; 16]);
    vram.write_words(64, 0, 4, 16, &[0x4567; 64]);
    match vram.prim_texture_status(cba, tsb, &uvs) {
        PrimTextureStatus::MissingClut { row } => assert_eq!(row, 64),
        other => panic!("expected MissingClut, got {other:?}"),
    }
}

#[test]
fn row_populated_width_counts_run_length() {
    let mut vram = Vram::new();
    vram.write_words(0, 32, 16, 1, &[0xAAAA; 16]);
    assert_eq!(vram.row_populated_width(0, 32, 256), 16);
    assert_eq!(vram.row_populated_width(0, 32, 8), 8);
    // No data at this row.
    assert_eq!(vram.row_populated_width(0, 33, 256), 0);
    // Sparse: a single non-zero pixel at column 5.
    let mut vram = Vram::new();
    vram.write_words(5, 100, 1, 1, &[0xFFFF]);
    // Run length is "last non-zero column + 1 - start" = 6.
    assert_eq!(vram.row_populated_width(0, 100, 256), 6);
}

#[test]
fn row_run_width_stops_at_a_large_gap() {
    let mut vram = Vram::new();
    // 240-wide palette band, then an empty gap, then unrelated data at 448.
    vram.write_words(0, 200, 240, 1, &[0x1234; 240]);
    vram.write_words(448, 200, 32, 1, &[0x4567; 32]);
    // Unbounded last-nonzero scan crosses the gap to col 479 -> 480.
    assert_eq!(vram.row_populated_width(0, 200, VRAM_WIDTH), 480);
    // Gap-tolerant run stops at the palette band (gap 240..448 = 208 > 64).
    assert_eq!(vram.row_run_width(0, 200, VRAM_WIDTH, 64), 240);
    // A small internal gap (<= max_gap) is tolerated and spanned.
    let mut vram = Vram::new();
    vram.write_words(0, 10, 16, 1, &[0xAAAA; 16]);
    vram.write_words(24, 10, 16, 1, &[0xBBBB; 16]); // gap of 8 zeros at 16..24
    assert_eq!(vram.row_run_width(0, 10, VRAM_WIDTH, 64), 40);
}

#[test]
fn prim_texture_status_ok_when_clut_shares_scanline_with_a_distant_page() {
    // Reproduce the battle monster VRAM layout: a 4bpp prim whose CLUT row
    // sits inside the texture-page y-band, with the page far to the right
    // on the *same* scanline. The depth-mismatch heuristic must not count
    // the distant page as part of the palette. (Mirrors
    // legaia_actor_formats::monster_archive::battle_render_mesh for slot 2: CLUT row
    // 486, page at x=448, both on row 486.)
    let row = 486u16;
    let cba = row << 6; // palette 0 on the monster CLUT row
    let tsb = 0x0007 | (1 << 4); // tpage_x col 7 (=448px), tpage_y=256, 4bpp
    let uvs = [(62, 38), (41, 0), (79, 0)];

    let mut vram = Vram::new();
    // 15 packed 16-colour palettes (240 cells) at x=0 on the CLUT row.
    vram.write_words(0, row, 240, 1, &[0x1234; 240]);
    // The slot's own 4bpp page: x=448, y=256, 256 rows tall, 32 cells wide
    // (128 texels). Row 486 is inside [256, 512), so the page shares the
    // CLUT's scanline.
    vram.write_words(448, 256, 32, 256, &[0x4567; 32 * 256]);

    assert_eq!(
        vram.prim_texture_status(cba, tsb, &uvs),
        PrimTextureStatus::Ok,
        "monster prim must render despite the CLUT row sharing a scanline with the page"
    );
}

#[test]
fn prim_has_texture_data_15bpp_uses_page_only() {
    // 15bpp direct: depth bits = 2 in TSB. Bit 7..8: (tsb >> 7) & 0x3 = 2.
    let tsb = (2u16 << 7) | 1; // tpage_x = 64, depth = 15bpp
    let cba = 0; // ignored in 15bpp
    let uvs = [(0, 0), (8, 8), (0, 8)];
    let mut vram = Vram::new();
    assert!(!vram.prim_has_texture_data(cba, tsb, &uvs));
    vram.write_words(64, 0, 16, 8, &[0x7FFF; 128]);
    assert!(vram.prim_has_texture_data(cba, tsb, &uvs));
}

#[test]
fn as_bytes_round_trips_le() {
    let mut vram = Vram::new();
    let buf = tim_16bpp_at(0, 0);
    vram.upload_tim(&parse(&buf).unwrap());
    let bytes = vram.as_bytes();
    // First 8 bytes = first 4 pixels = 0xAAAA, 0xBBBB, 0xCCCC, 0xDDDD (LE)
    assert_eq!(
        &bytes[0..8],
        &[0xAA, 0xAA, 0xBB, 0xBB, 0xCC, 0xCC, 0xDD, 0xDD]
    );
}

#[test]
fn write_clut_row_writes_halfwords_at_fbxy() {
    let mut vram = Vram::new();
    // 16 BGR555 halfwords spanning 0x0001..0x0010.
    let mut bytes = [0u8; 32];
    for i in 0..16u16 {
        bytes[(i as usize) * 2..(i as usize) * 2 + 2].copy_from_slice(&(i + 1).to_le_bytes());
    }
    vram.write_clut_row(128, 479, &bytes);
    for i in 0..16u16 {
        assert_eq!(vram.pixel(128 + i as usize, 479), i + 1);
    }
    // Adjacent pixels stay zero.
    assert_eq!(vram.pixel(127, 479), 0);
    assert_eq!(vram.pixel(144, 479), 0);
}

#[test]
fn write_clut_row_skips_empty_input() {
    let mut vram = Vram::new();
    // Sanity: no panic, no writes.
    vram.write_clut_row(0, 0, &[]);
    assert_eq!(vram.pixel(0, 0), 0);
}

#[test]
fn underlay_fills_only_unwritten_words() {
    let mut scene = Vram::new();
    let mut boot = Vram::new();
    // Scene wrote (5, 300) and a zero at (7, 300); boot wrote all three
    // plus (6, 300).
    scene.write_block(5, 300, 1, 1, &0xAAAAu16.to_le_bytes());
    scene.write_block(7, 300, 1, 1, &0u16.to_le_bytes());
    for x in 5..8 {
        boot.write_block(x, 300, 1, 1, &0xBBBBu16.to_le_bytes());
    }
    scene.underlay(&boot);
    // Scene word wins the overlap; boot shows through the unwritten
    // word only - a written zero (a transparent 4bpp run) is content.
    assert_eq!(scene.pixel(5, 300), 0xAAAA);
    assert_eq!(scene.pixel(6, 300), 0xBBBB);
    assert_eq!(scene.pixel(7, 300), 0);
    assert!(scene.is_written(6, 300));
    assert!(!scene.is_written(8, 300));
}

#[test]
fn a_second_underlay_stays_beneath_the_first() {
    let mut scene = Vram::new();
    let mut boot = Vram::new();
    let mut pool = Vram::new();
    boot.write_block(1, 1, 1, 1, &0x1111u16.to_le_bytes());
    pool.write_block(1, 1, 1, 1, &0x2222u16.to_le_bytes());
    pool.write_block(2, 1, 1, 1, &0x3333u16.to_le_bytes());
    scene.underlay(&boot);
    scene.underlay(&pool);
    assert_eq!(scene.pixel(1, 1), 0x1111);
    assert_eq!(scene.pixel(2, 1), 0x3333);
}

#[test]
fn merge_zero_clut_writes_mark_only_the_cells_they_store() {
    let mut vram = Vram::new();
    vram.write_words_merge_zeros(0, 500, 3, 1, &[0x1234, 0, 0x5678]);
    assert!(vram.is_written(0, 500));
    assert!(!vram.is_written(1, 500));
    assert!(vram.is_written(2, 500));
}

#[test]
fn move_image_copies_a_rect() {
    let mut vram = Vram::new();
    // 2x2 source block at (10, 20).
    for (i, &v) in [0x1111u16, 0x2222, 0x3333, 0x4444].iter().enumerate() {
        let bytes = v.to_le_bytes();
        vram.write_block(10 + (i % 2) as u16, 20 + (i / 2) as u16, 1, 1, &bytes);
    }
    vram.move_image(10, 20, 2, 2, 100, 200);
    assert_eq!(vram.pixel(100, 200), 0x1111);
    assert_eq!(vram.pixel(101, 200), 0x2222);
    assert_eq!(vram.pixel(100, 201), 0x3333);
    assert_eq!(vram.pixel(101, 201), 0x4444);
    // Source untouched.
    assert_eq!(vram.pixel(10, 20), 0x1111);
    // A zero-size rect is a no-op (the retail wrapper rejects it).
    vram.move_image(10, 20, 0, 2, 300, 300);
    vram.move_image(10, 20, 2, 0, 300, 300);
    assert_eq!(vram.pixel(300, 300), 0);
}
