//! Tests for the byte-level queue appliers **on the live path** - i.e. through
//! [`resolve_action_queue`], the entry point `engine-core` calls once per
//! committed arts input.
//!
//! These pin the retail laws the structural `legaia_art` matchers did not
//! carry: the resident Miracle row's MSB quirk and its 16-byte overwrite, the
//! first-matching-row (not longest-`find`) Super scan, and the single
//! application per build.

use super::*;
use legaia_art::{ActionConstant, Character, Command, MIRACLE_ARTS};

fn bytes_of(queue: &legaia_art::ActionQueue) -> Vec<u8> {
    queue.actions().iter().map(|a| a.as_byte()).collect()
}

// ---------------------------------------------------------------------------
// miracle_row_for / apply_miracle_replace / clear_queue_msb
// ---------------------------------------------------------------------------

#[test]
fn miracle_rows_carry_the_on_disc_msb_quirk() {
    for art in MIRACLE_ARTS {
        let row = miracle_row_for(art.character);
        // The leading four bytes are directions stored `0x8C..0x8F`.
        for (i, b) in row.iter().take(4).enumerate() {
            assert_eq!(
                *b & 0x80,
                0x80,
                "{} row byte {i} should be MSB-set",
                art.name
            );
            assert_eq!(*b & 0x7F, art.replacement[i].as_byte(), "{}", art.name);
        }
        // The starter and art constants that follow are stored plain.
        for (i, action) in art.replacement.iter().enumerate().skip(4) {
            assert_eq!(row[i], action.as_byte(), "{} row byte {i}", art.name);
        }
        // Everything past the string is zero padding, and the padding is what
        // makes the applier's flat 16-byte copy erase the staged input.
        for b in row.iter().skip(art.replacement.len()) {
            assert_eq!(*b, 0, "{} padding", art.name);
        }
    }
}

#[test]
fn miracle_replace_overwrites_the_whole_window_including_padding() {
    let mut queue = [0xAAu8; ACTION_QUEUE_CAP];
    let row = miracle_row_for(Character::Noa);
    apply_miracle_replace(&mut queue, &row);
    assert_eq!(&queue[..MIRACLE_ROW_STRIDE], &row[..]);
    // Retail's copy is exactly 16 bytes wide - `+0x1EF..` is untouched.
    assert!(queue[MIRACLE_ROW_STRIDE..].iter().all(|&b| b == 0xAA));
}

#[test]
fn msb_clear_is_the_add_form_and_only_touches_the_scan_window() {
    let mut queue = [0u8; ACTION_QUEUE_CAP];
    queue[..4].copy_from_slice(&[0x8C, 0x8D, 0x8E, 0x8F]);
    queue[4] = 0x1A;
    // A set byte outside the 16-byte window survives (retail's loop bound).
    queue[QUEUE_SCAN_LEN] = 0x8C;
    clear_queue_msb(&mut queue);
    assert_eq!(&queue[..5], &[0x0C, 0x0D, 0x0E, 0x0F, 0x1A]);
    assert_eq!(queue[QUEUE_SCAN_LEN], 0x8C);
    // Idempotent: a queue with no MSB set is unchanged.
    let before = queue;
    clear_queue_msb(&mut queue);
    assert_eq!(queue[..QUEUE_SCAN_LEN], before[..QUEUE_SCAN_LEN]);
}

#[test]
fn msb_clear_add_form_agrees_with_and_0x7f_for_every_byte() {
    for raw in 0u8..=0xFF {
        let mut q = [0u8; ACTION_QUEUE_CAP];
        q[0] = raw;
        clear_queue_msb(&mut q);
        assert_eq!(q[0], raw & 0x7F, "raw {raw:#04x}");
    }
}

// ---------------------------------------------------------------------------
// The live path
// ---------------------------------------------------------------------------

#[test]
fn live_miracle_matches_the_modeled_replacement_after_msb_clear() {
    for art in MIRACLE_ARTS {
        let queue = resolve_action_queue(art.character, art.commands, &[]);
        let expected: Vec<u8> = art.replacement.iter().map(|a| a.as_byte()).collect();
        assert_eq!(bytes_of(&queue), expected, "{}", art.name);
    }
}

#[test]
fn live_miracle_erases_staged_chained_arts() {
    // Retail's Miracle copy is a flat 16-byte overwrite, so anything the build
    // staged behind the Miracle string is gone - including chained arts that
    // would otherwise have run off the end of the replacement.
    let vahn = MIRACLE_ARTS
        .iter()
        .find(|m| m.character == Character::Vahn)
        .unwrap();
    let queue = resolve_action_queue(
        Character::Vahn,
        vahn.commands,
        &[ActionConstant::Art28, ActionConstant::Art22],
    );
    let expected: Vec<u8> = vahn.replacement.iter().map(|a| a.as_byte()).collect();
    assert_eq!(bytes_of(&queue), expected);
}

#[test]
fn live_super_tail_replace_fires_for_every_shipped_super() {
    for sa in legaia_art::SUPER_ARTS {
        // Feed the find pattern as the queue the build stages: the leading
        // starter/art pairs arrive as chained arts, the connectors as command
        // input is not expressible, so drive the byte path directly and check
        // it against the live entry point's own applier ordering.
        let mut q = [0u8; ACTION_QUEUE_CAP];
        q[..sa.find.len()].copy_from_slice(sa.find);
        let mut marks = [0u32; ACTION_QUEUE_CAP];
        let (find_rows, replace_rows) = super_rows_for(sa.character);
        clear_queue_msb(&mut q);
        let hit = apply_super_tail_replace(&mut q, &mut marks, &find_rows, &replace_rows)
            .unwrap_or_else(|| panic!("{} did not fire", sa.name));
        assert_eq!(hit.start, 0, "{}", sa.name);
        assert_eq!(&q[..sa.replace.len()], sa.replace, "{}", sa.name);
    }
}

#[test]
fn live_super_scan_is_table_order_not_longest_find() {
    // Retail's row loop stops at the first full tail match. Build a synthetic
    // pair where a short row precedes a longer one that also matches: the
    // byte applier must take the short row, where a longest-`find` ranking
    // would take the long one.
    let mut find_rows = [[0u8; SUPER_FIND_STRIDE]; SUPER_ROWS];
    let mut replace_rows = [[0u8; SUPER_REPLACE_STRIDE]; SUPER_ROWS];
    // Row 0: find `[0x19, 0x27]` -> replace `[0x1A, 0x2B]`.
    find_rows[0][..3].copy_from_slice(&[2, 0x19, 0x27]);
    replace_rows[0][..2].copy_from_slice(&[0x1A, 0x2B]);
    // Row 1: find `[0x0F, 0x19, 0x27]` -> replace `[0x1A, 0x2C]`.
    find_rows[1][..4].copy_from_slice(&[3, 0x0F, 0x19, 0x27]);
    replace_rows[1][..2].copy_from_slice(&[0x1A, 0x2C]);

    let mut q = [0u8; ACTION_QUEUE_CAP];
    q[..3].copy_from_slice(&[0x0F, 0x19, 0x27]);
    let mut marks = [0u32; ACTION_QUEUE_CAP];
    let hit = apply_super_tail_replace(&mut q, &mut marks, &find_rows, &replace_rows).unwrap();
    assert_eq!(hit.row, 0, "first matching row wins");
    assert_eq!(hit.start, 1);
    assert_eq!(&q[..3], &[0x0F, 0x1A, 0x2B]);
}

#[test]
fn live_super_applies_once_not_to_fixpoint() {
    // A replace whose own tail re-matches a row: retail applies it exactly
    // once per queue build (`FUN_801EF9E4` is called once and its row loop
    // exits on the first hit), so the second-order match must NOT fire.
    let mut find_rows = [[0u8; SUPER_FIND_STRIDE]; SUPER_ROWS];
    let mut replace_rows = [[0u8; SUPER_REPLACE_STRIDE]; SUPER_ROWS];
    // Row 0: `[0x19, 0x27]` -> `[0x19, 0x28]`, and row 1 matches `[0x19, 0x28]`.
    find_rows[0][..3].copy_from_slice(&[2, 0x19, 0x27]);
    replace_rows[0][..2].copy_from_slice(&[0x19, 0x28]);
    find_rows[1][..3].copy_from_slice(&[2, 0x19, 0x28]);
    replace_rows[1][..2].copy_from_slice(&[0x1A, 0x2C]);

    let mut q = [0u8; ACTION_QUEUE_CAP];
    q[..2].copy_from_slice(&[0x19, 0x27]);
    let mut marks = [0u32; ACTION_QUEUE_CAP];
    apply_super_tail_replace(&mut q, &mut marks, &find_rows, &replace_rows).unwrap();
    assert_eq!(
        &q[..2],
        &[0x19, 0x28],
        "the second-order match must not fire"
    );
}

#[test]
fn live_plain_input_is_untouched_by_both_appliers() {
    let queue = resolve_action_queue(
        Character::Vahn,
        &[Command::Up, Command::Up],
        &[ActionConstant::Art28],
    );
    assert_eq!(bytes_of(&queue), vec![0x0F, 0x0F, 0x19, 0x28]);
}

#[test]
fn live_build_is_capped_at_the_retail_scan_window() {
    // Retail's build loop bound is 16 bytes; a longer staged input truncates
    // rather than spilling into `+0x1EF..`, which holds unrelated actor fields.
    let chained = [ActionConstant::Art28; 10];
    let queue = resolve_action_queue(Character::Vahn, &[Command::Up], &chained);
    assert!(queue.len() <= QUEUE_SCAN_LEN, "len {}", queue.len());
    assert_eq!(queue.actions()[0], ActionConstant::Up);
}
