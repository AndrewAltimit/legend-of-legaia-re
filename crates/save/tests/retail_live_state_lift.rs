//! The three pieces of retail's live-state window a card load restores that
//! the SC-block lift used to drop:
//!
//! - the **whole system-flag bank** `0x80085758..0x80085958` (the story window
//!   stopped at `0x80085800`, flags `0x540` up were lost);
//! - the **present party** - count `0x80084594`, member list `0x80084598` -
//!   rather than the number of populated records (the New Game template
//!   populates all four);
//! - the **field position** snapshot `0x80084568` / `0x8008456C` retail's MAN
//!   loader seats a card load at.
//!
//! Each lifts off a synthetic block, writes back through the composer, and
//! round-trips through `LGSF`. No disc, no card.

use legaia_save::card::{
    RETAIL_FIELD_POS_X_OFFSET, RETAIL_FIELD_POS_Z_OFFSET, RETAIL_INVENTORY_OFFSET,
    RETAIL_PARTY_COUNT_OFFSET, RETAIL_PARTY_LEADER_OFFSET, RETAIL_PARTY_MEMBERS_OFFSET,
    RETAIL_STORY_FLAGS_OFFSET, RETAIL_STORY_FLAGS_SIZE, read_retail_field_position,
    read_retail_present_party,
};
use legaia_save::{
    BLOCK_SIZE, CharacterRecord, RETAIL_SC_PARTY_RECORDS, SAVE_BLOCK_HEADER, SaveFile,
    sc_block_checksum_valid,
};

/// SC offset of the system-flag bank (`0x80085758 - 0x80084140`).
const SC_SYSTEM_FLAGS: usize = 0x1618;
/// Offset of the bank inside the lifted story window.
const WINDOW_SYSTEM: usize = 0x158;

fn set_flag(block: &mut [u8], flag: u16) {
    block[SC_SYSTEM_FLAGS + usize::from(flag >> 3)] |= 0x80 >> (flag & 7);
}

/// A retail-shaped block: four populated records (the New Game template's
/// shape), a one-member present party, late-bank flags, a position.
fn vahn_alone_block() -> Vec<u8> {
    let mut block = vec![0u8; BLOCK_SIZE];
    block[..SAVE_BLOCK_HEADER.len()].copy_from_slice(&SAVE_BLOCK_HEADER);
    let records: Vec<Vec<u8>> = ["Vahn", "Noa", "Gala", "Terra"]
        .iter()
        .map(|n| {
            let mut r = CharacterRecord::zeroed();
            r.set_name(n);
            r.raw.to_vec()
        })
        .collect();
    legaia_save::write_retail_char_records(&mut block, &records).unwrap();
    // One flag below the old cut (0x53F), three above it, and the bank's
    // last one.
    for f in [0x053F, 0x0540, 0x056D, 0x06C4, 0x0FFF] {
        set_flag(&mut block, f);
    }
    block[RETAIL_PARTY_COUNT_OFFSET] = 1;
    block[RETAIL_PARTY_LEADER_OFFSET] = 0;
    block[RETAIL_PARTY_MEMBERS_OFFSET] = 0;
    block[RETAIL_FIELD_POS_X_OFFSET..RETAIL_FIELD_POS_X_OFFSET + 4]
        .copy_from_slice(&0x0E40i32.to_le_bytes());
    block[RETAIL_FIELD_POS_Z_OFFSET..RETAIL_FIELD_POS_Z_OFFSET + 4]
        .copy_from_slice(&(-0x40i32).to_le_bytes());
    block
}

fn flag_in(bits: &[u8], flag: u16) -> bool {
    bits.get(WINDOW_SYSTEM + usize::from(flag >> 3))
        .is_some_and(|b| b & (0x80 >> (flag & 7)) != 0)
}

#[test]
fn the_story_window_reaches_the_end_of_the_system_flag_bank() {
    // The bank is 0x200 bytes from 0x80085758, ending at the item array.
    assert_eq!(
        RETAIL_STORY_FLAGS_OFFSET + WINDOW_SYSTEM + 0x200,
        RETAIL_INVENTORY_OFFSET
    );
    assert_eq!(
        RETAIL_STORY_FLAGS_OFFSET + RETAIL_STORY_FLAGS_SIZE,
        RETAIL_INVENTORY_OFFSET
    );
}

#[test]
fn the_lift_keeps_every_system_flag_including_the_late_bank() {
    let sf = SaveFile::from_retail_sc_block(&vahn_alone_block(), RETAIL_SC_PARTY_RECORDS).unwrap();
    for f in [0x053F, 0x0540, 0x056D, 0x06C4, 0x0FFF] {
        assert!(
            flag_in(&sf.ext.story_flag_bits, f),
            "flag {f:#05X} dropped by the lift"
        );
    }
}

#[test]
fn the_lift_reads_the_present_party_not_the_record_count() {
    let sf = SaveFile::from_retail_sc_block(&vahn_alone_block(), RETAIL_SC_PARTY_RECORDS).unwrap();
    assert_eq!(sf.party.members.len(), 4, "all four records still lift");
    assert_eq!(
        sf.ext_v2.active_party,
        vec![0],
        "Vahn alone is a one-member party"
    );
}

#[test]
fn a_free_block_names_no_present_party() {
    let block = vec![0u8; BLOCK_SIZE];
    assert_eq!(read_retail_present_party(&block), None);
    let mut over = block.clone();
    over[RETAIL_PARTY_COUNT_OFFSET] = 5;
    assert_eq!(
        read_retail_present_party(&over),
        None,
        "count past four is not a party"
    );
}

#[test]
fn the_lift_reads_the_field_position_snapshot() {
    let sf = SaveFile::from_retail_sc_block(&vahn_alone_block(), RETAIL_SC_PARTY_RECORDS).unwrap();
    assert_eq!(sf.ext_v2.field_position, Some((0x0E40, -0x40)));
}

#[test]
fn the_composer_writes_back_what_the_lift_read() {
    let src = vahn_alone_block();
    let sf = SaveFile::from_retail_sc_block(&src, RETAIL_SC_PARTY_RECORDS).unwrap();
    let mut out = vec![0u8; BLOCK_SIZE];
    sf.write_into_retail_sc_block(&mut out).unwrap();
    assert!(sc_block_checksum_valid(&out));
    assert_eq!(
        out[SC_SYSTEM_FLAGS..SC_SYSTEM_FLAGS + 0x200],
        src[SC_SYSTEM_FLAGS..SC_SYSTEM_FLAGS + 0x200],
        "the whole bank round-trips"
    );
    assert_eq!(read_retail_present_party(&out), Some(vec![0]));
    assert_eq!(out[RETAIL_PARTY_LEADER_OFFSET], 0);
    assert_eq!(read_retail_field_position(&out), Some((0x0E40, -0x40)));
    // Sign-extended words, as `FUN_80016230` stores them.
    assert_eq!(
        out[RETAIL_FIELD_POS_Z_OFFSET..RETAIL_FIELD_POS_Z_OFFSET + 4],
        (-0x40i32).to_le_bytes()
    );
    let again = SaveFile::from_retail_sc_block(&out, RETAIL_SC_PARTY_RECORDS).unwrap();
    assert_eq!(again.ext.story_flag_bits, sf.ext.story_flag_bits);
    assert_eq!(again.ext_v2.active_party, sf.ext_v2.active_party);
    assert_eq!(again.ext_v2.field_position, sf.ext_v2.field_position);
}

#[test]
fn lgsf_round_trips_the_position_and_the_whole_window() {
    let sf = SaveFile::from_retail_sc_block(&vahn_alone_block(), RETAIL_SC_PARTY_RECORDS).unwrap();
    let back = SaveFile::parse(&sf.write()).unwrap();
    assert_eq!(back.ext.story_flag_bits, sf.ext.story_flag_bits);
    assert_eq!(back.ext_v2.field_position, Some((0x0E40, -0x40)));
    assert_eq!(back.ext_v2.active_party, vec![0]);
}

#[test]
fn a_save_without_a_position_writes_no_lgx8_block() {
    let mut sf =
        SaveFile::from_retail_sc_block(&vahn_alone_block(), RETAIL_SC_PARTY_RECORDS).unwrap();
    sf.ext_v2.field_position = None;
    let bytes = sf.write();
    assert!(!bytes.windows(4).any(|w| w == b"LGX8"));
    assert_eq!(SaveFile::parse(&bytes).unwrap().ext_v2.field_position, None);
}

#[test]
fn a_snapshot_never_taken_names_no_position() {
    let mut block = vahn_alone_block();
    block[RETAIL_FIELD_POS_X_OFFSET..RETAIL_FIELD_POS_Z_OFFSET + 4].fill(0);
    assert_eq!(read_retail_field_position(&block), None);
    let sf = SaveFile::from_retail_sc_block(&block, RETAIL_SC_PARTY_RECORDS).unwrap();
    assert_eq!(sf.ext_v2.field_position, None);
}
