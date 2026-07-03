use super::*;
use crate::battle_data_pack::{BattleDataPack, Record};

fn rec(index: usize, id: u32) -> Record {
    Record {
        index,
        id,
        data_offset: 0,
        size: 0x800,
    }
}

fn pack_with_ids(ids: &[u32]) -> BattleDataPack {
    BattleDataPack {
        table_offset: 0x40,
        records: ids.iter().enumerate().map(|(i, &id)| rec(i, id)).collect(),
        data_base: 0x8000,
    }
}

#[test]
fn art_bank_parses_synthetic_records() {
    // record0 image with the bank pointer at +0x58 -> 0x100:
    // [u32 count = 2] + two 0xD0-stride records.
    let mut img = vec![0u8; 0x100 + 4 + 2 * ART_RECORD_STRIDE];
    img[0x58..0x5C].copy_from_slice(&0x100u32.to_le_bytes());
    img[0x100..0x104].copy_from_slice(&2u32.to_le_bytes());
    let base0 = 0x104;
    // Record 0: combo [1,2,4], source 3, named, entry fields.
    img[base0..base0 + 3].copy_from_slice(&[1, 2, 4]);
    img[base0 + 0x0A] = 3;
    img[base0 + 0x10..base0 + 0x18].copy_from_slice(b"Test Art");
    img[base0 + ART_ENTRY_OFFSET] = 0x18; // entry tag
    img[base0 + ART_ENTRY_OFFSET + 0x77] = 21; // attach key
    img[base0 + ART_ENTRY_OFFSET + 0x78] = 2; // rate
    img[base0 + ART_ENTRY_OFFSET + 0x84] = 0; // rate_alt (main archive)
    // Embedded-entry face tracks: eye record 0 at record +0xB0
    // (= entry +0x8C), mouth record 1 at record +0xBC + 3.
    img[base0 + 0xB0..base0 + 0xB3].copy_from_slice(&[2, 4, 9]);
    img[base0 + 0xBC + 3..base0 + 0xBC + 6].copy_from_slice(&[1, 10, 14]);
    // Record 1: empty combo/name, source 5, base-archive marker.
    let base1 = base0 + ART_RECORD_STRIDE;
    img[base1 + 0x0A] = 5;
    img[base1 + ART_ENTRY_OFFSET + 0x78] = 1;
    img[base1 + ART_ENTRY_OFFSET + 0x84] = 0xFF;

    let bank = art_animation_bank(&img).expect("bank parses");
    assert_eq!(bank.len(), 2);
    assert_eq!(bank[0].anim_id, 0x10);
    assert_eq!(bank[0].combo, vec![1, 2, 4]);
    assert_eq!(bank[0].stream_source, 3);
    assert_eq!(bank[0].name, "Test Art");
    assert_eq!(bank[0].entry_tag, 0x18);
    assert_eq!(bank[0].attach_key, 21);
    assert_eq!(bank[0].rate, 2);
    assert!(!bank[0].uses_base_archive());
    assert_eq!(bank[0].entry_offset, base0 + ART_ENTRY_OFFSET);
    // The embedded entry's face tracks (record +0xB0 / +0xBC).
    let face = bank[0].face.expect("record 0 face tracks");
    assert_eq!(
        face.eyes[0],
        crate::face_anim::FaceTrackRecord {
            frame: 2,
            start: 4,
            end: 9,
        }
    );
    assert_eq!(
        face.mouth[1],
        crate::face_anim::FaceTrackRecord {
            frame: 1,
            start: 10,
            end: 14,
        }
    );
    assert!(!face.is_empty());
    assert_eq!(bank[1].anim_id, 0x11);
    assert!(bank[1].combo.is_empty());
    assert!(bank[1].name.is_empty());
    assert!(bank[1].uses_base_archive());
    assert!(
        bank[1].face.expect("record 1 face tracks").is_empty(),
        "untouched record parses as empty tracks"
    );

    // A combo byte outside 1..=4 is rejected.
    img[base0] = 9;
    assert!(art_animation_bank(&img).is_err());
    img[base0] = 1;
    // A non-printable name byte is rejected.
    img[base0 + 0x10] = 0x01;
    assert!(art_animation_bank(&img).is_err());
}

#[test]
fn art_me_slot_mapping() {
    // Vahn/Noa/Gala/Terra -> readef slots (3c+1, 3c+2).
    assert_eq!(art_me_slot(0, false), 1);
    assert_eq!(art_me_slot(0, true), 2);
    assert_eq!(art_me_slot(1, false), 4);
    assert_eq!(art_me_slot(2, true), 8);
    assert_eq!(art_me_slot(3, false), 10);
    assert_eq!(art_me_slot(3, true), 11);
}

#[test]
fn select_prefers_equipped_id_over_default() {
    // Two sections: [5, 4, 0] and [9, 0].
    let pack = pack_with_ids(&[5, 4, 0, 9, 0, 0, 0, 0]);
    let sel = select_sections(&pack, &[4, 9, 0, 0, 0]).unwrap();
    assert_eq!(sel[0].id, 4);
    assert_eq!(sel[1].id, 9);
    // Unequipped slots take the id = 0 defaults.
    assert_eq!(sel[2].id, 0);
}

#[test]
fn select_falls_back_to_default() {
    let pack = pack_with_ids(&[5, 4, 0, 9, 0, 0, 0, 0]);
    let sel = select_sections(&pack, &[7, 7, 7, 7, 7]).unwrap();
    assert!(sel.iter().all(|r| r.id == 0), "all defaults");
}

#[test]
fn select_requires_five_sections() {
    let pack = pack_with_ids(&[5, 0]);
    assert!(select_sections(&pack, &[0; 5]).is_err());
}

/// Build a minimal one-object Legaia TMD whose primitive section holds
/// one textured FT3 group (two prims, authoring texpages `0x15`/`0x16`,
/// CLUT row 480) followed by one untextured F3 group, for exercising
/// the relocation rewrite. Returns the buffer plus the byte offsets of
/// the two textured prims' texture blocks and the untextured prim's
/// payload start.
fn synthetic_tmd() -> (Vec<u8>, [usize; 2], usize) {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x8000_0002u32.to_le_bytes()); // magic
    buf.extend_from_slice(&0u32.to_le_bytes()); // flags = 0 (relative)
    buf.extend_from_slice(&1u32.to_le_bytes()); // nobj
    // Object entry: prim section right after the entry, vertices after
    // the prim section. *_top offsets are relative to the object-table
    // start (byte 12).
    let prim_top = OBJ_ENTRY_BYTES; // abs 0x28
    // prim section: FT3 group (8 + 3*20) + F3 group (8 + 2*20) + term(4)
    let prim_size = 8 + 3 * 20 + 8 + 2 * 20 + 4;
    let vert_top = prim_top + prim_size;
    for w in [
        vert_top as u32,
        3, // n_vert
        0,
        0, // normals
        prim_top as u32,
        3, // n_prim
        0, // scale
    ] {
        buf.extend_from_slice(&w.to_le_bytes());
    }
    // FT3 group: count=2, flags=0x20 (textured triangle, vertex
    // indices at byte 14), ilen=5 (20-byte prims), mode 0x24 (TME).
    let ft3_start = buf.len();
    buf.extend_from_slice(&[0x02, 0x00, 0x20, 0x00, 0x04, 0x05, 0x00, 0x24]);
    // Texture block = bytes 4..14 of each prim:
    // [u0, v0, cba_lo, cba_hi, u1, v1, tsb_lo, tsb_hi, u2, v2].
    let ft3_prim = |cba: u16, tsb: u16, buf: &mut Vec<u8>| {
        buf.extend_from_slice(&[0x80, 0x80, 0x80, 0x24]); // colour word
        buf.extend_from_slice(&[0, 0]);
        buf.extend_from_slice(&cba.to_le_bytes());
        buf.extend_from_slice(&[8, 0]);
        buf.extend_from_slice(&tsb.to_le_bytes());
        buf.extend_from_slice(&[0, 8]);
        for idx in [0u16, 8, 16] {
            buf.extend_from_slice(&idx.to_le_bytes());
        }
    };
    let block_a = buf.len() + 4;
    ft3_prim(0x7804, 0x0035, &mut buf); // row 480 col 64, page 0x15 abr=1
    let block_b = buf.len() + 4;
    ft3_prim(0xF800, 0x0016, &mut buf); // row 480 bit15-set, page 0x16
    buf.resize(buf.len() + 20, 0); // footer slot
    assert_eq!(buf.len() - ft3_start, 8 + 3 * 20);
    // F3 group (untextured): count=1, flags=0x10, ilen=5, mode 0x20
    // (TME clear). Payload bytes mimic a texture block and must stay
    // untouched.
    buf.extend_from_slice(&[0x01, 0x00, 0x10, 0x00, 0x04, 0x05, 0x00, 0x20]);
    let f3_payload = buf.len();
    buf.extend_from_slice(&[0xAA; 20]);
    buf.resize(buf.len() + 20, 0); // footer slot
    buf.extend_from_slice(&0u32.to_le_bytes()); // group terminator
    assert_eq!(buf.len(), 12 + vert_top);
    buf.resize(buf.len() + 3 * 8, 0); // 3 zero vertices
    (buf, [block_a, block_b], f3_payload)
}

#[test]
fn relocate_rewrites_textured_prims_into_the_slot_band() {
    let (mut buf, [block_a, block_b], _) = synthetic_tmd();
    let n = relocate_tsb_cba(&mut buf, 1).expect("relocate slot 1");
    assert_eq!(n, 2, "both textured prims rewritten");
    // Prim A: CLUT row 480 col 64 -> row 482 col 64 (column kept);
    // texpage 0x15 -> slot 1's first page 0x1A, ABR bits kept.
    let cba_a = u16::from_le_bytes([buf[block_a + 2], buf[block_a + 3]]);
    let tsb_a = u16::from_le_bytes([buf[block_a + 6], buf[block_a + 7]]);
    assert_eq!(cba_a, (482 << 6) | 4);
    assert_eq!(tsb_a, 0x0020 | 0x1A);
    // Prim B: bit 15 of the CBA preserved; page 0x16 (non-0x15) ->
    // slot 1's second page 0x1B.
    let cba_b = u16::from_le_bytes([buf[block_b + 2], buf[block_b + 3]]);
    let tsb_b = u16::from_le_bytes([buf[block_b + 6], buf[block_b + 7]]);
    assert_eq!(cba_b, 0x8000 | (482 << 6));
    assert_eq!(tsb_b, 0x001B);
}

#[test]
fn relocate_slot_table_matches_the_documented_band() {
    // docs/formats/character-mesh.md § Battle render: runtime texpages
    // (512,256)+(576,256) / (640,256)+(704,256) / (768,256)+(832,256),
    // CLUT rows 481 / 482 / 483 for slots 0 / 1 / 2.
    for (slot, pages, row) in [
        (0u8, (512u16, 576u16), 481u16),
        (1, (640, 704), 482),
        (2, (768, 832), 483),
    ] {
        let (mut buf, [block_a, block_b], _) = synthetic_tmd();
        relocate_tsb_cba(&mut buf, slot).expect("relocate");
        let tsb_a = u16::from_le_bytes([buf[block_a + 6], buf[block_a + 7]]);
        let tsb_b = u16::from_le_bytes([buf[block_b + 6], buf[block_b + 7]]);
        assert_eq!((tsb_a & 0xF) * 64, pages.0, "slot {slot} first page");
        assert_eq!((tsb_a >> 4) & 1, 1, "slot {slot} page y = 256");
        assert_eq!((tsb_b & 0xF) * 64, pages.1, "slot {slot} second page");
        for block in [block_a, block_b] {
            let cba = u16::from_le_bytes([buf[block + 2], buf[block + 3]]);
            assert_eq!((cba >> 6) & 0x1FF, row, "slot {slot} CLUT row");
        }
    }
}

#[test]
fn upload_block_applies_stp_and_band_math() {
    // Block: clut_x=8, clut_n=2, entries [0x1D40, 0x0000], 2x1 pixels.
    let rect = TextureRect {
        x0: 0x40,
        y0: 0x80,
        w: 2,
        h: 1,
    };
    let mut block = Vec::new();
    block.extend_from_slice(&8u16.to_le_bytes());
    block.extend_from_slice(&2u16.to_le_bytes());
    block.extend_from_slice(&0x1D40u16.to_le_bytes());
    block.extend_from_slice(&0u16.to_le_bytes());
    block.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
    let u = parse_upload_block(&block, rect, 2).expect("parse");
    // STP forced on non-zero entries only.
    assert_eq!(u.clut, vec![0x9D40, 0x0000]);
    assert_eq!(u.clut_x, 8);
    assert_eq!(u.clut_row(), 0x1E3); // row 483 for party slot 2
    assert_eq!(u.pixels, vec![0xAA, 0xBB, 0xCC, 0xDD]);
    // Band math: x = x0 + 0x200 + slot*0x80, y = y0 + 0x100.
    assert_eq!(u.fb_x(), 0x40 + 0x200 + 2 * 0x80);
    assert_eq!(u.fb_y(), 0x80 + 0x100);
}

#[test]
fn upload_block_rejects_short_payload() {
    let rect = TextureRect {
        x0: 0,
        y0: 0,
        w: 0x20,
        h: 0x80,
    };
    // clut_n = 0 but only 8 pixel bytes follow - far short of w*h*2.
    let mut block = vec![0u8; 4];
    block.extend_from_slice(&[0u8; 8]);
    assert!(parse_upload_block(&block, rect, 0).is_err());
}

#[test]
fn section_upload_gates_on_the_flag_halfword() {
    // Decoded slot: header with tmd_body_end = 0x20, flag clear.
    let mut decoded = vec![0u8; 0x20];
    decoded[0xC..0x10].copy_from_slice(&0x20u32.to_le_bytes());
    // Pool block: clut_n = 0 + 0x20*0x80 halfwords of pixels.
    decoded.extend_from_slice(&[0u8; 4]);
    decoded.extend_from_slice(&vec![0x55u8; 0x20 * 0x80 * 2]);
    assert!(
        section_texture_upload(&decoded, 0, 0)
            .expect("unflagged ok")
            .is_none(),
        "flag @ +0x12 clear: no upload"
    );
    decoded[0x12] = 1;
    let u = section_texture_upload(&decoded, 0, 0)
        .expect("flagged ok")
        .expect("upload present");
    assert_eq!(u.rect, SECTION_TEXTURE_RECTS[0]);
    assert_eq!(u.pixels.len(), 0x20 * 0x80 * 2);
}

#[test]
fn placement_rects_tile_each_band_exactly() {
    // The 5 section rects + 2 record[0] rects cover the 128x256
    // halfword band exactly once (no gaps, no overlaps).
    let mut cover = vec![0u32; 0x80 * 0x100];
    for r in SECTION_TEXTURE_RECTS.iter().chain(&RECORD0_TEXTURE_RECTS) {
        for y in r.y0..r.y0 + r.h {
            for x in r.x0..r.x0 + r.w {
                cover[y as usize * 0x80 + x as usize] += 1;
            }
        }
    }
    assert!(cover.iter().all(|&c| c == 1), "band tiled exactly once");
}

#[test]
fn relocate_leaves_untextured_groups_alone() {
    let (mut buf, _, f3_payload) = synthetic_tmd();
    relocate_tsb_cba(&mut buf, 0).expect("relocate");
    assert_eq!(
        &buf[f3_payload..f3_payload + 20],
        &[0xAA; 20],
        "untextured prim payload untouched"
    );
}
