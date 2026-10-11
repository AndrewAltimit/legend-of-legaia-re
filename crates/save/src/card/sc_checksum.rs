//! The additive checksum Legaia stamps in the last word of an SC block.

use super::*;

// ---------------------------------------------------------------------
// Save-block checksum
// ---------------------------------------------------------------------

/// Number of little-endian u32 words in one SC save block.
pub const SC_BLOCK_WORDS: usize = BLOCK_SIZE / 4;

/// Word index the additive block checksum is stored at - the block's last
/// word. The sum covers every word *before* it.
pub const SC_BLOCK_CHECKSUM_WORD: usize = SC_BLOCK_WORDS - 1;

/// Byte offset of the additive block checksum inside an SC save block.
pub const RETAIL_BLOCK_CHECKSUM_OFFSET: usize = SC_BLOCK_CHECKSUM_WORD * 4;

/// The same sum over a block a caller already holds as words.
///
/// A convenience face, not a second model: `checksum_covers_every_word_but_the
/// _last` asserts it agrees with [`sc_block_checksum`] byte for byte. Retail's
/// `a0` is a byte pointer, so the byte form is the one that carries the
/// `PORT:` and the one that runs.
///
/// REF: FUN_801E38D8
pub fn sc_block_checksum_words(words: &[u32]) -> u32 {
    words
        .iter()
        .take(SC_BLOCK_CHECKSUM_WORD)
        .fold(0u32, |sum, &w| sum.wrapping_add(w))
}

/// The additive save-block checksum of an SC block. `None` when the block is
/// too short to hold the checksum word.
///
/// Retail walks `0x7FF` little-endian u32 words from the block base with a
/// wrapping accumulator and returns the total, stopping one word short of
/// the block's final word:
///
/// ```text
/// 801e38d8  clear a1            ; sum = 0
/// 801e38dc  move  v1,a1         ; i   = 0
/// 801e38e0  lw    v0,0x0(a0)    ; w   = block[i]
/// 801e38e4  addiu v1,v1,0x1     ; i  += 1
/// 801e38e8  addu  a1,a1,v0      ; sum = sum +. w   (wrapping)
/// 801e38ec  slti  v0,v1,0x7ff   ; loop while i < 0x7ff
/// 801e38f0  bne   v0,zero,...
/// 801e38f4  _addiu a0,a0,0x4    ; block ptr += 4  (delay slot, always)
/// 801e38f8  jr    ra
/// 801e38fc  _move v0,a1         ; return sum
/// ```
///
/// The base it sums from is the block's **first byte** - the `SC` magic -
/// not the game-data region at [`RETAIL_GAME_DATA_OFFSET`]: the composer
/// `FUN_801E1934` zero-fills a whole `0x2000` staging block, copies
/// `0x1A18` bytes of live state over its front, sums that buffer and stores
/// the result at `+0x1FFC`, and the writer hands the same `0x2000` bytes to
/// the BIOS. The load direction reads the block back whole and compares.
///
/// PORT: FUN_801E38D8 (`ghidra/scripts/funcs/overlay_menu_801e38d8.txt`)
pub fn sc_block_checksum(sc_block: &[u8]) -> Option<u32> {
    if sc_block.len() < BLOCK_SIZE {
        return None;
    }
    Some(
        sc_block[..RETAIL_BLOCK_CHECKSUM_OFFSET]
            .as_chunks::<4>()
            .0
            .iter()
            .fold(0u32, |sum, c| {
                sum.wrapping_add(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            }),
    )
}

/// Whether a block's stored checksum word matches a fresh
/// [`sc_block_checksum`] - the test retail's load path applies before it
/// accepts a block.
///
/// The compare lives in state `5` of the load/save driver `FUN_801DD35C`
/// (`0x801df880`: `jal FUN_801E38D8` on the read buffer, then
/// `lw v1,0x1ffc(s1); beq v1,v0`). A match advances to sub-state `0x16`
/// and the block is copied into live RAM; a mismatch latches
/// `DAT_801EF140 = 2` and routes to sub-state `0x13`, the "Damaged data."
/// arm. A short block can never be valid.
///
/// REF: FUN_801DD35C (`ghidra/scripts/funcs/overlay_menu_801dd35c.txt`)
pub fn sc_block_checksum_valid(sc_block: &[u8]) -> bool {
    let Some(sum) = sc_block_checksum(sc_block) else {
        return false;
    };
    let stored = &sc_block[RETAIL_BLOCK_CHECKSUM_OFFSET..RETAIL_BLOCK_CHECKSUM_OFFSET + 4];
    u32::from_le_bytes([stored[0], stored[1], stored[2], stored[3]]) == sum
}

/// Recompute and store a block's checksum word, returning the value stored.
///
/// This is the last thing retail's save composer does (`FUN_801E1934` at
/// `0x801e1be0`: sum the staging block, `sw v0,0x1ffc(v1)`), so it is the
/// last thing an in-place edit of a real block has to do too - otherwise
/// the retail loader captions the save "Damaged data." and refuses it.
/// Every `write_retail_*` writer in this module calls it.
///
/// `None` (and no write) when the block is shorter than [`BLOCK_SIZE`] -
/// a caller editing a bare game-data window rather than a whole card block
/// has no checksum word to restamp.
///
/// REF: FUN_801E1934 (`ghidra/scripts/funcs/overlay_menu_801e1934.txt`)
pub fn restamp_sc_block_checksum(sc_block: &mut [u8]) -> Option<u32> {
    let sum = sc_block_checksum(sc_block)?;
    sc_block[RETAIL_BLOCK_CHECKSUM_OFFSET..RETAIL_BLOCK_CHECKSUM_OFFSET + 4]
        .copy_from_slice(&sum.to_le_bytes());
    Some(sum)
}
