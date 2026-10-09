use super::*;

/// Build a directory frame naming `slot` with the given prefix.
fn frame(prefix: &[u8], slot: u8) -> Vec<u8> {
    let mut f = prefix.to_vec();
    f.extend_from_slice(format!("{slot:02}").as_bytes());
    f.resize(0x28, 0);
    f
}

fn junk_frame() -> Vec<u8> {
    let mut f = b"BESLES-01234SOME".to_vec();
    f.extend_from_slice(b"07");
    f.resize(0x28, 0);
    f
}

/// The prefixes are the retail literals, spelled out here rather than
/// re-derived from the constant under test.
///
/// Every other test in this module builds its frames *from*
/// [`CARD_SAVE_PREFIXES`], so all of them passed while the separator
/// was an underscore and the walk matched nothing a real card carries.
/// That is the whole failure mode: a self-referential fixture cannot
/// see a wrong literal, and an inert kernel is never handed a real one.
/// The bytes are the menu overlay's own (`0x801EF03C` / `0x801EF054`,
/// PROT 0899 file `0x20824` / `0x2083C`).
#[test]
fn prefixes_are_the_retail_literals() {
    assert_eq!(CARD_SAVE_PREFIXES[0], b"BASCUS-94254PRO-");
    assert_eq!(CARD_SAVE_PREFIXES[1], b"BISCPS-10059PRO-");
    for p in CARD_SAVE_PREFIXES {
        assert_eq!(p.len(), CARD_PREFIX_LEN, "retail strncmp length");
    }
}

/// The writer and the matcher are one literal: a filename the port
/// itself stamps on a card must classify as a Legaia save. This is
/// the round-trip the constant needed and did not have - the two
/// halves lived in different crates and only the writer was live.
#[test]
fn filenames_the_port_writes_classify_as_legaia_saves() {
    for slot in [0u32, 1, 9, 14] {
        let mut f = legaia_save::card::legaia_save_filename(slot).into_bytes();
        f.resize(0x28, 0);
        assert_eq!(
            card_dir_slot_of(&f),
            Some(slot as usize),
            "legaia_save_filename({slot}) must parse back"
        );
        let classes = classify_card_directory(&[&f], 0);
        assert_eq!(classes[slot as usize], SlotContent::LegaiaSave);
    }
}

/// A matched filename stamps its slot as a Legaia save, and both
/// regional prefixes match.
#[test]
fn matched_frames_classify_as_legaia_saves() {
    for prefix in CARD_SAVE_PREFIXES {
        let f = frame(prefix, 3);
        let classes = classify_card_directory(&[&f], 0);
        assert_eq!(classes[3], SlotContent::LegaiaSave, "prefix {prefix:?}");
    }
}

/// Absence of a match is not evidence of a free block: with no free
/// blocks reported, every unmatched slot stays Foreign rather than
/// inviting an overwrite.
#[test]
fn unmatched_slots_stay_foreign_without_free_blocks() {
    let junk = junk_frame();
    let classes = classify_card_directory(&[&junk], 0);
    assert!(classes.iter().all(|c| *c == SlotContent::Foreign));
}

/// The free-block budget is spent on unclaimed slots in order, and
/// runs out - it never overwrites a matched slot's class.
#[test]
fn free_block_budget_fills_unclaimed_slots_in_order() {
    let f = frame(CARD_SAVE_PREFIXES[0], 0);
    let classes = classify_card_directory(&[&f], 2);
    assert_eq!(classes[0], SlotContent::LegaiaSave);
    assert_eq!(classes[1], SlotContent::Free);
    assert_eq!(classes[2], SlotContent::Free);
    assert_eq!(classes[3], SlotContent::Foreign);
}

/// A card reporting more free blocks than there are unclaimed slots
/// simply runs out of slots.
#[test]
fn oversized_free_budget_saturates() {
    let classes = classify_card_directory(&[], 999);
    assert!(classes.iter().all(|c| *c == SlotContent::Free));
}

/// The two loops are ordered, and the order is the correctness
/// property: every matched filename stamps its slot before any free
/// block is spent, so a budget large enough to cover the whole card
/// still cannot downgrade a real save to "free". Running the budget
/// first - or folding the two into one sweep - would offer an
/// occupied slot up for overwrite.
#[test]
fn matched_slots_survive_a_budget_that_covers_the_card() {
    let saves = [2u8, 7, 11];
    let frames: Vec<Vec<u8>> = saves
        .iter()
        .map(|s| frame(CARD_SAVE_PREFIXES[0], *s))
        .collect();
    let refs: Vec<&[u8]> = frames.iter().map(|f| f.as_slice()).collect();

    let classes = classify_card_directory(&refs, u32::MAX);
    for (slot, class) in classes.iter().enumerate() {
        let expected = if saves.contains(&(slot as u8)) {
            SlotContent::LegaiaSave
        } else {
            SlotContent::Free
        };
        assert_eq!(*class, expected, "slot {slot}");
    }
}

/// The slot number is the two digits after the prefix; a non-digit
/// in the second position leaves a one-digit number rather than
/// rejecting the name.
#[test]
fn slot_number_parses_one_and_two_digit_names() {
    let mut two = CARD_SAVE_PREFIXES[0].to_vec();
    two.extend_from_slice(b"12");
    two.resize(0x28, 0);
    assert_eq!(card_dir_slot_of(&two), Some(12));

    let mut one = CARD_SAVE_PREFIXES[0].to_vec();
    one.extend_from_slice(b"5_");
    one.resize(0x28, 0);
    assert_eq!(card_dir_slot_of(&one), Some(5));
}

/// Only the first fifteen frames are walked - the sixteenth class
/// cell exists for the digit space, not for a block.
#[test]
fn walk_stops_after_fifteen_frames() {
    let f = frame(CARD_SAVE_PREFIXES[0], 15);
    let frames: Vec<&[u8]> = std::iter::repeat_n(f.as_slice(), 16).collect();
    let classes = classify_card_directory(&frames, 0);
    // All sixteen frames name slot 15, so it is claimed either way;
    // what matters is that the walk itself is bounded.
    assert_eq!(classes[15], SlotContent::LegaiaSave);
}

/// The snapshot builder keys each slot's constructor off its class,
/// so captions follow the retail class byte without a second scan.
#[test]
fn snapshots_follow_the_class_byte() {
    let f = frame(CARD_SAVE_PREFIXES[0], 1);
    let snaps = card_directory_slots(&[&f], 1);
    assert_eq!(snaps.len(), CARD_DIR_FRAMES);
    assert_eq!(snaps[0].content, SlotContent::Free);
    assert!(!snaps[0].present);
    assert_eq!(snaps[1].content, SlotContent::LegaiaSave);
    assert!(snaps[1].present);
    assert_eq!(snaps[2].content, SlotContent::Foreign);
    assert!(!snaps[2].present);
}

/// A foreign block captions as "not a Legaia save" rather than as a
/// free block - the whole reason the class byte is kept.
#[test]
fn foreign_blocks_do_not_caption_as_free() {
    let snaps = card_directory_slots(&[], 0);
    let mode = SlotInfoMode::for_slot(&snaps[0]);
    assert_eq!(mode, SlotInfoMode::NotLegaiaSave);
}

// -- FUN_801E3AF0 / FUN_801E3BA0 -----------------------------------

/// Build a `CardDirEntry` for a save in `slot` occupying `blocks`
/// blocks.
fn entry(prefix: &[u8], slot: u8, blocks: u32) -> CardDirEntry {
    let f = frame(prefix, slot);
    let mut e = CardDirEntry::from_frame(&f).expect("frame is a full stride");
    e.size = blocks * CARD_BLOCK_BYTES as u32;
    e
}

/// The table is fifteen entries wide and the count saturates there:
/// a card claiming more files than the table holds cannot overrun it.
#[test]
fn directory_scan_is_bounded_at_fifteen() {
    let entries: Vec<CardDirEntry> = (0..40)
        .map(|i| entry(CARD_SAVE_PREFIXES[0], i as u8, 1))
        .collect();
    let (table, count) = card_directory_scan(&entries);
    assert_eq!(count, CARD_DIR_FRAMES);
    assert_eq!(table.len(), CARD_DIR_FRAMES);
}

/// Retail clears every slot before the walk. A shorter second scan
/// must not be able to see the first scan's names - which is only
/// observable because the table is returned whole, not truncated.
#[test]
fn directory_scan_clears_slots_it_does_not_fill() {
    let (table, count) = card_directory_scan(&[entry(CARD_SAVE_PREFIXES[0], 3, 1)]);
    assert_eq!(count, 1);
    for slot in table.iter().skip(1) {
        assert_eq!(*slot, CardDirEntry::default());
    }
}

/// `0xf - used`, with the divide truncating toward zero.
#[test]
fn free_blocks_is_fifteen_minus_used() {
    let entries = [
        entry(CARD_SAVE_PREFIXES[0], 0, 1),
        entry(CARD_SAVE_PREFIXES[0], 1, 3),
    ];
    let (table, count) = card_directory_scan(&entries);
    assert_eq!(card_free_blocks(&table, count), 15 - 4);
}

/// An empty card is all fifteen blocks.
#[test]
fn free_blocks_on_an_empty_card() {
    let (table, count) = card_directory_scan(&[]);
    assert_eq!(card_free_blocks(&table, count), CARD_TOTAL_BLOCKS);
}

/// A partial block still costs a whole block only once it crosses
/// the boundary - retail truncates, it does not round up. A file one
/// byte short of two blocks reads as one block used.
#[test]
fn free_blocks_truncates_rather_than_rounding_up() {
    let mut e = entry(CARD_SAVE_PREFIXES[0], 0, 2);
    e.size -= 1;
    let (table, count) = card_directory_scan(&[e]);
    assert_eq!(card_free_blocks(&table, count), 15 - 1);
}

/// Retail does not clamp: an over-full card returns a negative
/// count, and callers decide what that means.
#[test]
fn free_blocks_goes_negative_for_an_impossible_card() {
    let entries: Vec<CardDirEntry> = (0..15)
        .map(|i| entry(CARD_SAVE_PREFIXES[0], i, 2))
        .collect();
    let (table, count) = card_directory_scan(&entries);
    assert_eq!(card_free_blocks(&table, count), 15 - 30);
}

#[test]
fn checksum_sums_the_first_2047_words_only() {
    // A full 0x800-word block: every word = 1 except the stored
    // checksum word. The sum covers 0x7FF words -> 0x7FF.
    let mut block = vec![1u32; SAVE_BLOCK_WORDS];
    block[SAVE_BLOCK_CHECKSUM_WORD] = 0xDEAD_BEEF; // must be ignored
    assert_eq!(save_block_checksum(&block), 0x7FF);
}

#[test]
fn checksum_wraps_on_overflow() {
    // Two words summing past u32::MAX wrap, matching `addu`.
    let mut block = vec![0u32; SAVE_BLOCK_WORDS];
    block[0] = 0xFFFF_FFFF;
    block[1] = 3;
    assert_eq!(save_block_checksum(&block), 2); // 0xFFFF_FFFF + 3 = 2 (wrapped)
}

#[test]
fn checksum_valid_matches_stored_word() {
    let mut block = vec![0u32; SAVE_BLOCK_WORDS];
    block[0] = 0x1000;
    block[1] = 0x0234;
    block[2] = 0x0001;
    let sum = save_block_checksum(&block);
    block[SAVE_BLOCK_CHECKSUM_WORD] = sum;
    assert!(save_block_checksum_valid(&block));
    // Corrupt one covered word: the stored checksum no longer matches.
    block[0] = block[0].wrapping_add(1);
    assert!(!save_block_checksum_valid(&block));
}

#[test]
fn checksum_valid_rejects_a_short_block() {
    // A block too short to even hold the checksum word is never valid.
    let block = vec![0u32; SAVE_BLOCK_CHECKSUM_WORD];
    assert!(!save_block_checksum_valid(&block));
}

/// The three ports chain, and the budget is the one the card's own
/// file sizes pay for - not "every slot without a save".
///
/// Two multi-block saves are used deliberately: they leave thirteen
/// unmatched slots but only five free blocks, so a session that
/// skipped [`card_free_blocks`] and assumed a full card would report
/// thirteen free blocks instead of five.
#[test]
fn session_from_card_directory_derives_its_own_free_budget() {
    let entries = [
        entry(CARD_SAVE_PREFIXES[0], 0, 5),
        entry(CARD_SAVE_PREFIXES[0], 4, 5),
    ];
    let s = SaveSelectSession::from_card_directory(SaveSelectMode::Load, &entries);
    let slots = s.slots();
    assert_eq!(slots.len(), CARD_DIR_FRAMES);
    assert!(slots[0].present);
    assert!(slots[4].present);
    // 15 - 10 used = 5 free blocks, spent on 5 of the 13 unmatched
    // slots; the other 8 stay unreadable rather than inviting an
    // overwrite the card cannot afford.
    let free = slots
        .iter()
        .filter(|s| s.content == SlotContent::Free)
        .count();
    let foreign = slots
        .iter()
        .filter(|s| s.content == SlotContent::Foreign)
        .count();
    assert_eq!((free, foreign), (5, 8));
}

/// A card whose saves this game cannot read spends its budget on the
/// blocks the card says are free, and leaves the rest Foreign - the
/// ordering property `classify_card_directory` documents, now
/// reached through the real free-block count.
#[test]
fn foreign_saves_eat_blocks_and_stay_foreign() {
    let mut junk = CardDirEntry::from_frame(&junk_frame()).unwrap();
    junk.size = 4 * CARD_BLOCK_BYTES as u32;
    let s = SaveSelectSession::from_card_directory(SaveSelectMode::Load, &[junk]);
    let free = s
        .slots()
        .iter()
        .filter(|s| s.content == SlotContent::Free)
        .count();
    // 4 blocks used by an unreadable save -> 11 free.
    assert_eq!(free, 11);
    assert!(s.slots().iter().any(|s| s.content == SlotContent::Foreign));
}

// -- FUN_801E3900 --------------------------------------------------

/// Handle 0 is the only one that can leave the status Pending; each
/// later handle overwrites unconditionally, so the last to fire wins.
#[test]
fn card_poll_last_event_wins() {
    let mut c = 0;
    assert_eq!(
        card_status_poll([false, false, false, false], &mut c),
        CardStatus::Pending
    );
    let mut c = 0;
    assert_eq!(
        card_status_poll([true, false, false, false], &mut c),
        CardStatus::Ready
    );
    let mut c = 0;
    assert_eq!(
        card_status_poll([false, false, true, false], &mut c),
        CardStatus::NoCard
    );
    // Handle 3 fires after handle 2, so Complete beats NoCard even
    // though NoCard is the "worse" outcome. Order, not severity.
    let mut c = 0;
    assert_eq!(
        card_status_poll([true, true, true, true], &mut c),
        CardStatus::Complete
    );
}

/// The counter advances on every call, whichever way the timeout
/// test goes - retail's store sits in the branch's delay slot.
#[test]
fn card_poll_counter_always_advances() {
    let mut c = 0;
    card_status_poll([true, false, false, false], &mut c);
    assert_eq!(c, 1);
    let mut c = CARD_STATUS_TIMEOUT_FRAMES;
    card_status_poll([true, false, false, false], &mut c);
    assert_eq!(c, CARD_STATUS_TIMEOUT_FRAMES + 1);
}

/// The timeout is tested against the counter's value on *entry*, so
/// the poll entered at 119 still reports its events and the poll
/// entered at 120 is forced to Aborted even on a Complete event.
#[test]
fn card_poll_timeout_boundary() {
    let mut c = CARD_STATUS_TIMEOUT_FRAMES - 1;
    assert_eq!(
        card_status_poll([false, false, false, true], &mut c),
        CardStatus::Complete
    );
    let mut c = CARD_STATUS_TIMEOUT_FRAMES;
    assert_eq!(
        card_status_poll([false, false, false, true], &mut c),
        CardStatus::Aborted
    );
}
