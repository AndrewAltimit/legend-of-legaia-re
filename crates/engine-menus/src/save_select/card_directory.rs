//! Memory-card directory classification, the directory table and free-block arithmetic.
//! Split out of `save_select.rs`.

use super::*;

/// Number of memory-card directory frames retail's card walk visits.
///
/// The card holds 15 usable blocks; the class array retail writes is 16
/// entries wide because a parsed slot number is a two-digit field.
pub const CARD_DIR_FRAMES: usize = 15;

/// Width of the per-slot class array retail clears before each walk.
pub const CARD_SLOT_CLASSES: usize = 16;

/// Save-filename prefixes retail matches a directory frame against, one
/// per region. Both are 16 bytes - the exact `strncmp` length retail
/// passes - and the two digits that follow are the slot number.
///
/// The separator is a **hyphen**. This constant spelled it `PRO_` for as
/// long as the walk below was inert, which is the shape the triage page
/// calls "right about the wiring and wrong about the bytes": nothing had
/// ever handed the matcher a real card, so a prefix that matches nothing
/// looked exactly like a prefix that matches everything. The retail
/// literals live in the menu overlay's data segment at `0x801EF03C` /
/// `0x801EF054` (PROT entry 0899 file `0x20824` / `0x2083C`) and read
/// `"BASCUS-94254PRO-"` / `"BISCPS-10059PRO-"`; the directory frames of
/// real cards agree. The USA entry is taken from
/// [`legaia_save::card::LEGAIA_SAVE_FILENAME_PREFIX`] rather than
/// respelled, so the matcher and the writer cannot drift apart again -
/// they are one literal with one owner.
pub const CARD_SAVE_PREFIXES: [&[u8]; 2] = [
    legaia_save::card::LEGAIA_SAVE_FILENAME_PREFIX.as_bytes(),
    b"BISCPS-10059PRO-",
];

/// Length retail compares a directory filename over.
pub(super) const CARD_PREFIX_LEN: usize = 16;

/// Classify a memory card's directory into per-slot [`SlotContent`].
///
/// This is the producer for the class byte [`SlotContent`] models: retail
/// clears the array, walks the 15 directory frames matching each filename
/// against either regional save prefix, and stamps class `1` on every slot
/// a matched filename names. Only *after* that does it spend the card's
/// reported free-block count marking still-unclassified slots class `2`
/// (free) - so a block the walk neither matched nor could afford to call
/// free stays class `0`, which is [`SlotContent::Foreign`].
///
/// That ordering is the whole point: absence of a match is not evidence a
/// block is free. Retail only calls a block free when the card's own
/// free-block count pays for it, which is why an unreadable foreign save
/// captions as "not a Legend of Legaia save" rather than inviting an
/// overwrite.
///
/// `frames` are the raw directory frames (retail reads a `0x28`-byte
/// stride; only the leading filename field matters here). `avail_blocks`
/// is the card's free-block count, which retail queries separately before
/// the walk.
///
/// One deliberate departure: retail's budget loop is bounded only by the
/// budget (`bgtz` on the counter, no slot bound) and its match loop
/// stamps `class[slot]` with no range check, so a malformed card can walk
/// off the end of the 16-byte array. Both loops are bounded here.
///
/// Reached from [`SaveSelectSession::from_card_directory`], which pairs
/// it with the free-block count [`card_free_blocks`] computes off the
/// same directory - so `avail_blocks` no longer has to be guessed by a
/// caller.
///
/// The index-space mismatch that once read as this walk's blocker is what
/// it is now wired *for*. Its class array is keyed by the **filename's
/// save index** ([`card_dir_slot_of`] parses the digits after the
/// `BASCUS-` prefix) while the browser card rack's 5x3 preview grid is
/// keyed by **physical block**, and the two do disagree on a real card -
/// retail files a save by the save-select list position it was standing
/// on, so `-03` can sit in any block. That makes "which save numbers does
/// this card already carry" a question the rack has to ask before it
/// claims a block, and this is the pass that answers it:
/// `LegaiaRuntime::card_save_index` in `web-viewer::cards` reads the
/// [`SlotContent::LegaiaSave`] entries to pick a number no file on the
/// card is already using. Without it the rack derived the number from the
/// block alone and wrote duplicate filenames onto a retail card.
///
/// The other index-space direction - re-keying the preview grid itself
/// into this array - is a separate question and is still open. It is not a
/// prerequisite for the above: a host can key its own grid however it
/// likes and still owe the card unique filenames.
///
/// PORT: FUN_801E1208
pub fn classify_card_directory(
    frames: &[&[u8]],
    avail_blocks: u32,
) -> [SlotContent; CARD_SLOT_CLASSES] {
    // Retail clears the class array (and its sibling scanned-flag array)
    // before every walk. Class 0 is Foreign - "occupied by something
    // unreadable" - which is what an untouched entry means here.
    let mut classes = [SlotContent::Foreign; CARD_SLOT_CLASSES];

    for frame in frames.iter().take(CARD_DIR_FRAMES) {
        let Some(slot) = card_dir_slot_of(frame) else {
            continue;
        };
        if let Some(cell) = classes.get_mut(slot) {
            *cell = SlotContent::LegaiaSave;
        }
    }

    // Spend the card's free-block budget on slots the walk left unclaimed.
    // Retail decrements a counter rather than testing a bound, so a card
    // reporting more free blocks than there are unclaimed slots simply
    // runs out of slots first.
    let mut avail = avail_blocks;
    for cell in classes.iter_mut() {
        if avail == 0 {
            break;
        }
        if *cell == SlotContent::Foreign {
            *cell = SlotContent::Free;
            avail -= 1;
        }
    }

    classes
}

/// Parse the slot number a directory frame's filename encodes, or `None`
/// when the filename is not one of this game's saves.
///
/// Retail reads the two bytes straight after the 16-byte prefix as ASCII
/// digits. The second digit is optional: it only folds into the number
/// when it actually is a digit, so a one-digit name parses as its single
/// digit rather than being rejected.
///
/// Public because a host that writes to a card needs the same parse for a
/// reason retail does not have: retail's save number is the save-select
/// list position it is already standing on, so it never has to read one
/// back off a card, while a host addressing a **block** has to ask the
/// card what number that block's file already carries.
///
/// REF: FUN_801E1208 (the filename match + digit parse it inlines).
pub fn card_dir_slot_of(frame: &[u8]) -> Option<usize> {
    if frame.len() < CARD_PREFIX_LEN + 2 {
        return None;
    }
    let matched = CARD_SAVE_PREFIXES
        .iter()
        .any(|p| &frame[..CARD_PREFIX_LEN] == *p);
    if !matched {
        return None;
    }
    let hi = frame[CARD_PREFIX_LEN].wrapping_sub(b'0');
    let lo = frame[CARD_PREFIX_LEN + 1].wrapping_sub(b'0');
    let slot = if lo < 10 {
        hi as usize * 10 + lo as usize
    } else {
        hi as usize
    };
    Some(slot)
}

/// Build the session's slot list straight off a card directory.
///
/// Pairs [`classify_card_directory`] with the two content-keyed
/// [`SlotSnapshot`] constructors so a card-backed host gets a slot list
/// whose captions already follow the retail class byte. Slots the walk
/// classified as [`SlotContent::LegaiaSave`] come back marked `present`
/// but without preview data - a host fills those in by reading the block
/// itself, which is a separate card read in retail too.
pub fn card_directory_slots(frames: &[&[u8]], avail_blocks: u32) -> Vec<SlotSnapshot> {
    classify_card_directory(frames, avail_blocks)
        .into_iter()
        .take(CARD_DIR_FRAMES)
        .enumerate()
        .map(|(i, content)| {
            let slot = i as u8;
            match content {
                SlotContent::Free => SlotSnapshot::empty(slot),
                SlotContent::Foreign => SlotSnapshot::foreign(slot),
                SlotContent::LegaiaSave => SlotSnapshot {
                    present: true,
                    damaged: false,
                    content,
                    label: format!("Slot {slot}"),
                    ..SlotSnapshot::empty(slot)
                },
            }
        })
        .collect()
}

// ---------------------------------------------------------------------
// Memory-card directory table + free-block arithmetic
// ---------------------------------------------------------------------

/// Byte stride of one PSX BIOS directory entry (`DIRENTRY`).
pub const CARD_DIRENTRY_STRIDE: usize = 0x28;

/// Filename field width inside a `DIRENTRY`.
///
/// `FUN_801E3AF0` clears exactly `0x13..=0x0` - twenty bytes - before
/// each walk, which is what fixes this width.
pub const CARD_DIRENTRY_NAME_LEN: usize = 0x14;

/// Offset of the `size` field inside a `DIRENTRY`.
pub const CARD_DIRENTRY_SIZE_OFFSET: usize = 0x18;

/// Bytes in one memory-card block. `FUN_801E3BA0` divides the summed
/// file sizes by this (as an arithmetic `>> 13`) to get blocks used.
pub const CARD_BLOCK_BYTES: i32 = 0x2000;

/// Blocks a standard PSX memory card exposes. `FUN_801E3BA0` subtracts
/// the used count from this literal `0xf`.
pub const CARD_TOTAL_BLOCKS: i32 = 0xf;

/// One entry of the directory table retail fills at `0x801F32A8`.
///
/// Only the two fields retail itself touches are modelled: the filename
/// (which the classifier matches a save prefix against) and the byte
/// size (which the free-block count sums). The BIOS `DIRENTRY`'s
/// `attr` / `next` / `system` fields are never read by either function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardDirEntry {
    pub name: [u8; CARD_DIRENTRY_NAME_LEN],
    pub size: u32,
}

impl Default for CardDirEntry {
    fn default() -> Self {
        Self {
            name: [0; CARD_DIRENTRY_NAME_LEN],
            size: 0,
        }
    }
}

impl CardDirEntry {
    /// Build an entry from a raw `0x28`-byte directory frame. Frames
    /// shorter than the stride are rejected rather than zero-extended -
    /// retail never sees a short frame because the BIOS writes the whole
    /// struct.
    pub fn from_frame(frame: &[u8]) -> Option<Self> {
        if frame.len() < CARD_DIRENTRY_STRIDE {
            return None;
        }
        let mut name = [0u8; CARD_DIRENTRY_NAME_LEN];
        name.copy_from_slice(&frame[..CARD_DIRENTRY_NAME_LEN]);
        let size = u32::from_le_bytes([
            frame[CARD_DIRENTRY_SIZE_OFFSET],
            frame[CARD_DIRENTRY_SIZE_OFFSET + 1],
            frame[CARD_DIRENTRY_SIZE_OFFSET + 2],
            frame[CARD_DIRENTRY_SIZE_OFFSET + 3],
        ]);
        Some(Self { name, size })
    }
}

/// Fill the fixed 15-entry directory table from an enumeration of the
/// card's files, and return how many entries it holds.
///
/// Retail formats `"bu%1d%1d:*"` from the port + card digits, zeroes all
/// fifteen `0x28`-byte table slots (name bytes `0x13..=0x0` and the size
/// word at `+0x18` - the other `DIRENTRY` fields are left as they lie),
/// then walks `firstfile` / `nextfile` over the table. The count it
/// returns is what the caller feeds [`card_free_blocks`].
///
/// The wildcard is `*`, so the pattern selects a *device*, not a name -
/// every file on the chosen card matches. The BIOS does the matching, so
/// what is ported here is the table clear, the fifteen-slot bound and the
/// count. `entries` is the enumeration the BIOS would have produced.
///
/// One faithful subtlety worth keeping: retail's count loop increments in
/// the `beq`'s **delay slot**, so the increment happens on the exiting
/// iteration too and the function subtracts one at the end. The net
/// result is a plain "number of files", which is what this returns.
///
/// PORT: FUN_801E3AF0
///
/// Wired: the browser card rack enumerates the player's inserted card
/// image's directory frames into [`CardDirEntry`]s and fills this table
/// (`web-viewer::cards::card_block_snapshots`), pairing it with
/// [`card_free_blocks`] to price the free-cell captions of the 5x3 grid.
pub fn card_directory_scan(entries: &[CardDirEntry]) -> ([CardDirEntry; CARD_DIR_FRAMES], usize) {
    // Retail clears every slot before the walk, so a shorter enumeration
    // than the previous one cannot leave stale names behind.
    let mut table: [CardDirEntry; CARD_DIR_FRAMES] = Default::default();
    let mut count = 0usize;
    for (slot, entry) in entries.iter().take(CARD_DIR_FRAMES).enumerate() {
        table[slot] = entry.clone();
        count = slot + 1;
    }
    (table, count)
}

/// Blocks still free on the card, from the first `count` table entries.
///
/// Retail sums each entry's `size` word, applies the standard MIPS
/// signed-division bias (`if (sum < 0) sum += 0x1fff`) so the following
/// arithmetic `>> 13` truncates toward zero, and returns `0xf - blocks`.
///
/// The result is **not clamped** - a card whose files sum past fifteen
/// blocks yields a negative count, exactly as retail does, so callers
/// decide what an impossible card means. [`SaveSelectSession::from_card_directory`]
/// floors it at zero before spending it as a budget.
///
/// PORT: FUN_801E3BA0
///
/// Wired: costs the [`card_directory_scan`] table the browser card rack
/// fills off an inserted card image; the resulting budget decides whether
/// an unclaimed grid cell captions free or foreign
/// (`web-viewer::cards::card_block_snapshots`). The engine's own
/// disk-backed LGSF slot list has no block budget to spend.
pub fn card_free_blocks(table: &[CardDirEntry], count: usize) -> i32 {
    let mut used: i32 = 0;
    for entry in table.iter().take(count) {
        used = used.wrapping_add(entry.size as i32);
    }
    let biased = if used < 0 {
        used.wrapping_add(CARD_BLOCK_BYTES - 1)
    } else {
        used
    };
    CARD_TOTAL_BLOCKS - (biased >> 13)
}
