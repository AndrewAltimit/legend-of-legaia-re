/// A formatted card holds no saves, every frame's XOR checksum is right,
/// and a save written into it lands in block 1 and reads back.
#[test]
fn formatted_card_is_empty_valid_and_writable() {
    use super::*;
    let mut buf = formatted_card_image();
    assert_eq!(buf.len(), CARD_SIZE);
    assert!(parse_card(&buf).unwrap().is_empty());
    let dir = walk_directory(&buf).unwrap();
    assert_eq!(dir.len(), DIR_FRAMES);
    assert!(dir.iter().all(|e| e.state == state::FREE));
    for i in 0..64 {
        let f = &buf[i * DIR_FRAME_SIZE..(i + 1) * DIR_FRAME_SIZE];
        let x = f[..0x7F].iter().fold(0u8, |a, &b| a ^ b);
        assert_eq!(f[0x7F], x, "frame {i} checksum");
    }
    assert_eq!(&buf[63 * DIR_FRAME_SIZE..63 * DIR_FRAME_SIZE + 2], b"MC");
    assert!(crate::emu::detect(&buf).is_ok());
    let mut payload = vec![0u8; BLOCK_SIZE];
    payload[..2].copy_from_slice(&SAVE_BLOCK_MAGIC);
    assert_eq!(
        write_block(&mut buf, &payload, "BASCUS-94254PRO-00").unwrap(),
        1
    );
    assert_eq!(parse_card(&buf).unwrap().len(), 1);
}

#[test]
fn rename_covers_both_name_sites_and_restamps() {
    use crate::character::{NAME_LEN, NAME_OFFSET};
    let mut block = vec![0u8; super::BLOCK_SIZE];
    block[..2].copy_from_slice(&super::SAVE_BLOCK_MAGIC);
    // Roster table slot 0 + character record 0 both say "Vahn".
    block[super::RETAIL_PARTY_NAME_TABLE_OFFSET..super::RETAIL_PARTY_NAME_TABLE_OFFSET + 4]
        .copy_from_slice(b"Vahn");
    let rec0 = super::RETAIL_GAME_DATA_OFFSET + super::RETAIL_CHAR_RECORD_HEADER_SIZE + NAME_OFFSET;
    block[rec0..rec0 + 4].copy_from_slice(b"Vahn");
    super::restamp_sc_block_checksum(&mut block);

    let n = super::rename_retail_character(&mut block, "Vahn", "Lu").unwrap();
    assert_eq!(n, 2, "both sites rewritten");
    assert_eq!(
        &block[super::RETAIL_PARTY_NAME_TABLE_OFFSET..][..3],
        b"Lu\0"
    );
    assert_eq!(&block[rec0..rec0 + 3], b"Lu\0");
    assert!(super::sc_block_checksum_valid(&block), "checksum restamped");
    // A prefix must not match: "Va" stays.
    assert_eq!(
        super::rename_retail_character(&mut block, "L", "X").unwrap(),
        0
    );
    // A too-long replacement is refused.
    assert!(super::rename_retail_character(&mut block, "Lu", &"X".repeat(NAME_LEN)).is_err());
}

use super::*;

fn synth_card_with_one_save() -> Vec<u8> {
    let mut buf = vec![0u8; CARD_SIZE];
    buf[..2].copy_from_slice(&CARD_MAGIC);
    // Directory frame 1: state=FIRST_BLOCK, size=8192, next=0xFFFF.
    let f = DIR_FRAME_SIZE;
    buf[f..f + 4].copy_from_slice(&state::FIRST_BLOCK.to_le_bytes());
    buf[f + 4..f + 8].copy_from_slice(&8192u32.to_le_bytes());
    buf[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    let pc = b"BASCUS-94254LEGAIA";
    buf[f + 10..f + 10 + pc.len()].copy_from_slice(pc);
    // Save block 1 starts with SC magic.
    let b = BLOCK_SIZE;
    buf[b..b + 2].copy_from_slice(&SAVE_BLOCK_MAGIC);
    buf
}

#[test]
fn detects_one_save_block() {
    let card = synth_card_with_one_save();
    let saves = parse_card(&card).unwrap();
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].block, 1);
    assert_eq!(saves[0].file_size, 8192);
    assert!(saves[0].product_code.starts_with("BASCUS-94254"));
    assert_eq!(saves[0].block_chain, vec![1]);
}

#[test]
fn rejects_wrong_magic() {
    let mut buf = vec![0u8; CARD_SIZE];
    buf[0] = b'X';
    assert!(parse_card(&buf).is_err());
}

#[test]
fn read_block_returns_8kb_slice() {
    let card = synth_card_with_one_save();
    let block = read_block(&card, 1).unwrap();
    assert_eq!(block.len(), BLOCK_SIZE);
    assert_eq!(&block[..2], &SAVE_BLOCK_MAGIC);
}

#[test]
fn read_block_rejects_out_of_range() {
    let card = vec![0u8; CARD_SIZE];
    assert!(read_block(&card, 0).is_none());
    assert!(read_block(&card, 16).is_none());
}

#[test]
fn walk_directory_returns_15_entries() {
    let card = synth_card_with_one_save();
    let dir = walk_directory(&card).unwrap();
    assert_eq!(dir.len(), DIR_FRAMES);
    assert_eq!(dir[0].state, state::FIRST_BLOCK);
    assert_eq!(dir[0].block, 1);
}

fn free_card() -> Vec<u8> {
    let mut buf = vec![0u8; CARD_SIZE];
    buf[..2].copy_from_slice(&CARD_MAGIC);
    // Mark all blocks free.
    for i in 1..=DIR_FRAMES {
        let frame_off = DIR_FRAME_SIZE * i;
        buf[frame_off..frame_off + 4].copy_from_slice(&state::FREE.to_le_bytes());
        let checksum = buf[frame_off..frame_off + 0x7F]
            .iter()
            .fold(0u8, |acc, &b| acc ^ b);
        buf[frame_off + 0x7F] = checksum;
    }
    buf
}

#[test]
fn write_block_single_block() {
    let mut card = free_card();
    let payload = b"Hello Legaia save!";
    let block = write_block(&mut card, payload, "BASCUS-94254TEST").unwrap();
    assert_eq!(block, 1, "first free block should be 1");

    // Directory frame 1: state = FIRST_BLOCK, next = 0xFFFF.
    let frame_off = DIR_FRAME_SIZE;
    let blk_state = u32::from_le_bytes(card[frame_off..frame_off + 4].try_into().unwrap());
    assert_eq!(blk_state, state::FIRST_BLOCK);
    let next = u16::from_le_bytes(card[frame_off + 8..frame_off + 10].try_into().unwrap());
    assert_eq!(next, 0xFFFF);

    // Block 1 data: SC magic + payload.
    let blk_off = BLOCK_SIZE;
    assert_eq!(&card[blk_off..blk_off + 2], &SAVE_BLOCK_MAGIC);
    assert_eq!(&card[blk_off + 2..blk_off + 2 + payload.len()], payload);
}

#[test]
fn write_block_records_block_aligned_size() {
    let mut card = free_card();
    // An 18-byte payload still occupies a whole 8 KiB block, and that is
    // what the frame must declare - not 18, and not BLOCK_SIZE - 2.
    write_block(&mut card, b"Hello Legaia save!", "BASCUS-94254TEST").unwrap();
    let f = DIR_FRAME_SIZE;
    let size = u32::from_le_bytes(card[f + 4..f + 8].try_into().unwrap());
    assert_eq!(size, BLOCK_SIZE as u32);
    assert_eq!(size % BLOCK_SIZE as u32, 0, "size must be block-aligned");
    // And it reads back through the directory walker.
    assert_eq!(parse_card(&card).unwrap()[0].file_size, BLOCK_SIZE as u32);
}

#[test]
fn write_block_size_covers_every_block_of_a_chain() {
    let mut card = free_card();
    // Two blocks' worth of payload: one byte past what a single block
    // holds, so the chain spans two and the frame must say so.
    let payload = vec![0xABu8; BLOCK_SIZE - 1];
    write_block(&mut card, &payload, "BASCUS-94254TEST").unwrap();

    let saves = parse_card(&card).unwrap();
    assert_eq!(saves[0].block_chain.len(), 2);
    assert_eq!(
        saves[0].file_size,
        (2 * BLOCK_SIZE) as u32,
        "a two-block chain declares two blocks of bytes"
    );
    // The size lives on the first frame only; continuations leave it 0.
    let f2 = DIR_FRAME_SIZE * saves[0].block_chain[1] as usize;
    assert_eq!(
        u32::from_le_bytes(card[f2 + 4..f2 + 8].try_into().unwrap()),
        0
    );
}

#[test]
fn write_block_checksum_is_correct() {
    let mut card = free_card();
    write_block(&mut card, b"checksum test payload", "BASCUS-94254TEST").unwrap();
    let frame_off = DIR_FRAME_SIZE;
    let expected = card[frame_off..frame_off + 0x7F]
        .iter()
        .fold(0u8, |acc, &b| acc ^ b);
    assert_eq!(card[frame_off + 0x7F], expected, "XOR checksum mismatch");
}

#[test]
fn write_block_product_code_stored() {
    let mut card = free_card();
    write_block(&mut card, b"data", "BASCUS-94254LEGAIA").unwrap();
    let frame_off = DIR_FRAME_SIZE;
    let pc_bytes = &card[frame_off + 10..frame_off + 30];
    assert!(pc_bytes.starts_with(b"BASCUS-94254LEGAIA"));
}

#[test]
fn write_block_rejects_full_card() {
    let mut card = synth_card_with_one_save();
    // Fill remaining blocks with state FIRST_BLOCK so none are free.
    for i in 2..=DIR_FRAMES {
        let frame_off = DIR_FRAME_SIZE * i;
        card[frame_off..frame_off + 4].copy_from_slice(&state::FIRST_BLOCK.to_le_bytes());
    }
    assert!(write_block(&mut card, b"data", "BASCUS-94254TEST").is_err());
}

fn fresh_sc_block() -> Vec<u8> {
    let mut block = vec![0u8; BLOCK_SIZE];
    block[..2].copy_from_slice(&SAVE_BLOCK_MAGIC);
    block
}

#[test]
fn write_retail_story_flags_round_trips_through_reader() {
    let mut block = fresh_sc_block();
    let mut bits = vec![0u8; RETAIL_STORY_FLAGS_SIZE];
    for (i, b) in bits.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(7) ^ 0x5A;
    }
    let n = write_retail_story_flags(&mut block, &bits).unwrap();
    assert_eq!(n, RETAIL_STORY_FLAGS_SIZE);
    let read = read_retail_story_flags(&block).expect("read back");
    assert_eq!(read, bits.as_slice());
}

#[test]
fn write_retail_story_flags_pads_short_input() {
    let mut block = fresh_sc_block();
    let short = vec![0xAA; 16];
    let n = write_retail_story_flags(&mut block, &short).unwrap();
    assert_eq!(n, 16);
    let read = read_retail_story_flags(&block).unwrap();
    assert!(read[..16].iter().all(|&b| b == 0xAA));
    assert!(read[16..].iter().all(|&b| b == 0));
}

#[test]
fn write_retail_inventory_round_trips() {
    let mut block = fresh_sc_block();
    // Ids start at 1: `0` is the empty-slot sentinel, not an item.
    let pairs: Vec<(u8, u8)> = (1..=RETAIL_INVENTORY_SLOTS as u8)
        .map(|i| (i, i.min(crate::retail_inventory::STACK_CAP)))
        .collect();
    let n = write_retail_inventory(&mut block, &pairs).unwrap();
    assert_eq!(n, RETAIL_INVENTORY_SLOTS);
    let raw = read_retail_inventory(&block).unwrap();
    for (i, &(id, count)) in pairs.iter().enumerate() {
        assert_eq!(raw[i * 2], id);
        assert_eq!(raw[i * 2 + 1], count);
    }
}

/// The block that lands is one retail could have produced, not a
/// transcript of the engine's list. Three shapes the old positional copy
/// wrote through, all of which retail's own accessors never generate.
#[test]
fn write_retail_inventory_composes_through_the_retail_accessors() {
    let mut block = fresh_sc_block();
    // 137 of one item (over the cap), the same id twice (two stacks), and
    // an `id == 0` entry (a phantom occupied slot).
    let n =
        write_retail_inventory(&mut block, &[(0x10, 137), (0x10, 4), (0, 9), (0x22, 2)]).unwrap();
    let raw = read_retail_inventory(&block).unwrap();
    assert_eq!(n, 2, "two live stacks, not four slots");
    assert_eq!(raw[0], 0x10);
    assert_eq!(
        raw[1],
        crate::retail_inventory::STACK_CAP,
        "counts clamp at 99"
    );
    assert_eq!(raw[2], 0x22, "the duplicate merged, so 0x22 moved up");
    assert_eq!(raw[3], 2);
    assert!(
        raw[4..].iter().all(|&b| b == 0),
        "the id-0 entry wrote no slot"
    );
    // Contrast: the same list laid in positionally is a different block.
    // Without this the assertions above would also pass on a composer
    // that happened to be the identity for this input.
    let mut positional = [0u8; 8];
    for (i, &(id, count)) in [(0x10u8, 137u8), (0x10, 4), (0, 9), (0x22, 2)]
        .iter()
        .enumerate()
    {
        positional[i * 2] = id;
        positional[i * 2 + 1] = count;
    }
    assert_ne!(&raw[..8], &positional[..]);
}

/// The composer's bound is the **array**, not the 72-slot display page:
/// 200 distinct ids all land (id `0` is the empty sentinel and is
/// skipped), where the page-wide window used to drop everything past slot
/// 71.
#[test]
fn write_retail_inventory_fills_the_whole_array_not_the_display_page() {
    let mut block = fresh_sc_block();
    let pairs: Vec<(u8, u8)> = (0..200u32).map(|i| ((i & 0xFF) as u8, 1)).collect();
    let n = write_retail_inventory(&mut block, &pairs).unwrap();
    assert_eq!(n, 199, "199 non-zero ids, squeezed to slots 0..=198");
    assert!(n > RETAIL_INVENTORY_SLOTS, "past the display page");
    let raw = read_retail_item_window(&block).expect("full window");
    assert_eq!(raw.len(), RETAIL_ITEM_WINDOW_SIZE);
    assert_ne!(raw[72 * 2], 0, "slot 72 is occupied");
}

/// The array writer lands its slots verbatim - holes included - where the
/// list writer would have squeezed them out.
#[test]
fn write_retail_item_window_keeps_holes_and_slot_order() {
    let mut block = fresh_sc_block();
    let mut slots = vec![(0u8, 0u8); crate::retail_inventory::ITEM_SLOTS_TOTAL];
    slots[3] = (0x77, 5);
    slots[200] = (0x78, 4);
    let n = write_retail_item_window(&mut block, &slots).unwrap();
    assert_eq!(n, 201, "the highest occupied slot plus one");
    let raw = read_retail_item_window(&block).expect("full window");
    assert_eq!((raw[3 * 2], raw[3 * 2 + 1]), (0x77, 5));
    assert_eq!((raw[200 * 2], raw[200 * 2 + 1]), (0x78, 4));
    assert_eq!(raw[0], 0, "slot 0 stays a hole");
}

#[test]
fn save_icon_block_identity_stamps_the_full_header() {
    let mut block = vec![0u8; BLOCK_SIZE];
    write_retail_block_identity(&mut block, 0, None).unwrap();
    // All four bytes, not just the magic - `+2` is the icon-frame
    // descriptor and `+3` the block count.
    assert_eq!(&block[..4], &SAVE_BLOCK_HEADER);
}

#[test]
fn save_icon_block_identity_writes_all_three_frames_and_the_palette() {
    let mut block = vec![0u8; BLOCK_SIZE];
    let icon = RetailBlockIcon {
        clut: [0xA5; RETAIL_ICON_CLUT_BYTES],
        pixels: [0x3C; RETAIL_ICON_FRAME_BYTES],
    };
    write_retail_block_identity(&mut block, 0, Some(&icon)).unwrap();
    assert_eq!(
        &block[RETAIL_ICON_CLUT_OFFSET..RETAIL_ICON_CLUT_OFFSET + RETAIL_ICON_CLUT_BYTES],
        &icon.clut
    );
    // Retail writes the same tile into all three frame slots even though
    // the header declares one frame.
    for off in RETAIL_ICON_FRAME_OFFSETS {
        assert_eq!(&block[off..off + RETAIL_ICON_FRAME_BYTES], &icon.pixels);
    }
    // Nothing past the header region moved but the checksum word, which
    // now covers the identity just written.
    assert!(
        block[RETAIL_GAME_DATA_OFFSET..RETAIL_BLOCK_CHECKSUM_OFFSET]
            .iter()
            .all(|&b| b == 0)
    );
    assert!(sc_block_checksum_valid(&block));
}

#[test]
fn save_icon_title_digits_are_one_based_and_full_width() {
    assert_eq!(save_title_digits(0), [0x4F, 0x50], "slot 0 shows 01");
    assert_eq!(save_title_digits(8), [0x4F, 0x58]);
    assert_eq!(save_title_digits(9), [0x50, 0x4F], "slot 9 shows 10");
    assert_eq!(save_title_digits(14), [0x50, 0x54], "slot 14 shows 15");
}

#[test]
fn save_icon_identity_patches_digits_only_into_a_real_title() {
    // A block with no title: the digits must not be stamped into
    // emptiness, or the BIOS shows a name that is two numerals.
    let mut empty = vec![0u8; BLOCK_SIZE];
    write_retail_block_identity(&mut empty, 4, None).unwrap();
    assert!(
        empty[RETAIL_TITLE_OFFSET..RETAIL_ICON_CLUT_OFFSET]
            .iter()
            .all(|&b| b == 0)
    );

    // A block that already carries a title gets its two digits updated.
    let mut titled = vec![0u8; BLOCK_SIZE];
    titled[RETAIL_TITLE_OFFSET..RETAIL_ICON_CLUT_OFFSET].fill(0x20);
    write_retail_block_identity(&mut titled, 4, None).unwrap();
    let title = &titled[RETAIL_TITLE_OFFSET..RETAIL_ICON_CLUT_OFFSET];
    assert_eq!(title[RETAIL_TITLE_DIGIT_OFFSETS[0]], 0x4F);
    assert_eq!(
        title[RETAIL_TITLE_DIGIT_OFFSETS[1]], 0x54,
        "slot 4 shows 05"
    );
    // Every other title byte is untouched.
    for (i, &b) in title.iter().enumerate() {
        if !RETAIL_TITLE_DIGIT_OFFSETS.contains(&i) {
            assert_eq!(b, 0x20, "title byte {i} moved");
        }
    }
}

#[test]
fn save_icon_filename_uses_a_hyphen_and_the_slot_suffix() {
    // The separator is a hyphen, verified against the menu overlay's own
    // literal and against real cards' directory frames.
    assert_eq!(legaia_save_filename(0), "BASCUS-94254PRO-00");
    assert_eq!(legaia_save_filename(1), "BASCUS-94254PRO-01");
    assert_eq!(legaia_save_filename(14), "BASCUS-94254PRO-14");
    assert!(legaia_save_filename(3).starts_with(LEGAIA_SAVE_FILENAME_PREFIX));
}

#[test]
fn save_icon_block_identity_rejects_a_short_block() {
    let mut short = vec![0u8; 0x40];
    assert!(write_retail_block_identity(&mut short, 0, None).is_err());
}

#[test]
fn write_retail_char_records_round_trips() {
    let mut block = fresh_sc_block();
    let records: Vec<Vec<u8>> = (0..3u8)
        .map(|n| {
            let mut r = vec![0u8; RETAIL_CHAR_RECORD_STRIDE];
            // Reader stops at the first all-zero slot, so every record
            // here carries at least one nonzero byte to round-trip.
            r[0] = n + 1;
            r[RETAIL_CHAR_RECORD_STRIDE - 1] = (n + 1).wrapping_mul(11);
            r
        })
        .collect();
    let n = write_retail_char_records(&mut block, &records).unwrap();
    assert_eq!(n, 3);
    let read = read_retail_char_records(&block, 4).unwrap();
    assert_eq!(read.len(), 3, "fourth slot is empty so reader stops");
    for (i, rec) in read.iter().enumerate() {
        assert_eq!(rec[0], (i as u8) + 1);
        assert_eq!(
            rec[RETAIL_CHAR_RECORD_STRIDE - 1],
            ((i as u8) + 1).wrapping_mul(11)
        );
    }
}

#[test]
fn read_retail_char_records_huge_max_does_not_panic() {
    // A pathological `max_records` must not overflow the `i * STRIDE`
    // computation into a slice-index panic; the walk just stops at the
    // first empty slot / buffer end.
    let block = fresh_sc_block();
    let _ = read_retail_char_records(&block, usize::MAX);
    // Also a too-small block returns None rather than panicking.
    assert!(read_retail_char_records(&[0u8; 4], usize::MAX).is_none());
}

#[test]
fn write_retail_char_records_does_not_zero_trailing_slots() {
    // The retail SC layout places the story-flag bitmap (+0x14C0) and
    // inventory (+0x1818) regions inside what would otherwise be the
    // trailing record slots. So the writer must leave those bytes
    // untouched - any pre-existing content past the last written record
    // must survive.
    let mut block = fresh_sc_block();
    // Drop a sentinel inside the story-flag region (which lives
    // at +0x14C0, inside the 4th would-be record slot).
    block[RETAIL_STORY_FLAGS_OFFSET + 0x10] = 0xAB;
    let one = vec![{
        let mut r = vec![0u8; RETAIL_CHAR_RECORD_STRIDE];
        r[5] = 99;
        r
    }];
    write_retail_char_records(&mut block, &one).unwrap();
    let read = read_retail_char_records(&block, 4).unwrap();
    assert_eq!(read.len(), 1, "first slot only");
    assert_eq!(read[0][5], 99);
    assert_eq!(
        block[RETAIL_STORY_FLAGS_OFFSET + 0x10],
        0xAB,
        "story-flag region survives the record write"
    );
}

#[test]
fn char_record_region_is_base_anchored_with_name_at_0x2a7() {
    // Regression for the off-by-0x2A7 record anchor. The record region
    // begins at the true record base (game+0x3C8 = live RAM 0x80084708),
    // NOT the visible name field (game+0x66F). A record built with known
    // stats + name must round-trip through write/read + `CharacterRecord`
    // with the stats reading back from their documented offsets, and the
    // name must land in the SC block at game+0x66F (SC +0x86F for slot 0),
    // i.e. record base + `NAME_OFFSET`.
    use crate::character::{CharacterRecord, HpMpSp, NAME_OFFSET};

    let mut block = fresh_sc_block();
    let mut rec = CharacterRecord::zeroed();
    rec.set_hp_mp_sp(HpMpSp {
        hp_cur: 180,
        hp_max: 180,
        mp_cur: 20,
        mp_max: 20,
        sp_cur: 0,
        sp_max: 0,
    });
    rec.set_name("Vahn");
    write_retail_char_records(&mut block, std::slice::from_ref(&rec.raw)).unwrap();

    let read = read_retail_char_records(&block, 4).unwrap();
    assert_eq!(read.len(), 1, "first slot only");
    let parsed = CharacterRecord::parse(&read[0]).unwrap();
    assert_eq!(
        parsed.hp_mp_sp().hp_max,
        180,
        "HP must read back from the +0x104 pair, not a 0x2A7-shifted field"
    );
    assert_eq!(parsed.name(), "Vahn");

    // The name lands at SC +0x86F = game+0x66F = record base + NAME_OFFSET.
    let sc_name = RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE + NAME_OFFSET;
    assert_eq!(sc_name, RETAIL_GAME_DATA_OFFSET + 0x66F);
    assert_eq!(&block[sc_name..sc_name + 4], b"Vahn");
}

// -- FUN_801E38D8 / FUN_801E1934 / FUN_801DD35C state 5 --------------

#[test]
fn checksum_covers_every_word_but_the_last() {
    // 0x7FF words of 1 -> 0x7FF; the stored word itself is excluded.
    let mut block = vec![0u8; BLOCK_SIZE];
    for w in 0..SC_BLOCK_CHECKSUM_WORD {
        block[w * 4] = 1;
    }
    block[RETAIL_BLOCK_CHECKSUM_OFFSET] = 0xFF; // must not be summed
    assert_eq!(
        sc_block_checksum(&block),
        Some(SC_BLOCK_CHECKSUM_WORD as u32)
    );
    // The word-slice kernel and the byte form agree.
    let words: Vec<u32> = block
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    assert_eq!(
        sc_block_checksum_words(&words),
        sc_block_checksum(&block).unwrap()
    );
}

#[test]
fn checksum_wraps_and_needs_a_full_block() {
    let mut block = vec![0u8; BLOCK_SIZE];
    block[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
    block[4..8].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(sc_block_checksum(&block), Some(2)); // 0xFFFF_FFFF + 3
    // Anything short of a whole block has no checksum word at all.
    assert_eq!(sc_block_checksum(&block[..BLOCK_SIZE - 1]), None);
    assert!(!sc_block_checksum_valid(&block[..BLOCK_SIZE - 1]));
    assert_eq!(restamp_sc_block_checksum(&mut vec![0u8; 0x1A18]), None);
}

/// The oracle by contrast: a restamped block validates, a byte poked
/// afterwards does not, and restamping again repairs it. This is the
/// whole reason every `write_retail_*` writer restamps.
#[test]
fn a_field_edit_invalidates_the_checksum_until_it_is_restamped() {
    let mut block = vec![0u8; BLOCK_SIZE];
    block[..2].copy_from_slice(&SAVE_BLOCK_MAGIC);
    assert!(restamp_sc_block_checksum(&mut block).is_some());
    assert!(sc_block_checksum_valid(&block));

    // A raw poke leaves the word stale.
    block[RETAIL_GOLD_OFFSET] ^= 0x5A;
    assert!(!sc_block_checksum_valid(&block), "stale after a raw poke");
    assert!(restamp_sc_block_checksum(&mut block).is_some());
    assert!(sc_block_checksum_valid(&block));

    // Every writer in this module does the restamp for its caller.
    write_retail_gold(&mut block, 12_345).unwrap();
    assert!(sc_block_checksum_valid(&block), "gold writer restamps");
    write_retail_coins(&mut block, 777).unwrap();
    assert!(sc_block_checksum_valid(&block), "coin writer restamps");
    write_retail_inventory(&mut block, &[(0x10, 3)]).unwrap();
    assert!(sc_block_checksum_valid(&block), "inventory writer restamps");
    write_retail_story_flags(&mut block, &[0xAA; 8]).unwrap();
    assert!(
        sc_block_checksum_valid(&block),
        "story-flag writer restamps"
    );
    write_retail_char_records(&mut block, &[vec![0x11; RETAIL_CHAR_RECORD_STRIDE]]).unwrap();
    assert!(sc_block_checksum_valid(&block), "record writer restamps");

    // And the values survive the restamp.
    assert_eq!(read_retail_gold(&block), Some(12_345));
    assert_eq!(read_retail_coins(&block), Some(777));
}
