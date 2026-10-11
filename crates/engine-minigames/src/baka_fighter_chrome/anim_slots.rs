//! The knockdown effect slot table: install and delete.

// ---------------------------------------------------------------------------
// Knockdown effect slot table
// ---------------------------------------------------------------------------

/// Stride of one action-animation slot record in the per-fighter block.
pub const ANIM_SLOT_STRIDE: usize = 8;
/// Stride of one fighter's animation block (`t2 * 0x60` in the retail index
/// math: `(t2 * 3) << 5`).
pub const ANIM_FIGHTER_STRIDE: usize = 0x60;
/// Slot cap the allocator refuses to grow past.
pub const ANIM_SLOT_CAP: i32 = 8;

/// Result of [`anim_slot_install`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimSlotInstall {
    /// Index of the slot the key landed in.
    pub slot: usize,
    /// Whether the key was already resident (a re-trigger) rather than new.
    pub reused: bool,
    /// New live-slot count for the fighter.
    pub count: i32,
    /// Retail's return value: `-1` on install, `-1` on a full table too - the
    /// routine never reports success differently, which is why the caller
    /// treats it as void.
    pub full: bool,
}

// REPLACED-BY: nothing is owed a port - retail never reaches it. Its only
// caller is the keyframe-editor tick `FUN_801D4FC8` (see [`editor_tick`] for
// the gate evidence), a development screen behind `_DAT_8007B868`, which the
// shipped disc boots to zero and never sets.
/// PORT: FUN_801D57BC - the animation-slot installer.
///
/// `(bank, fighter, key)`: the routine resolves the fighter's slot block, then
/// linearly scans the block's live slots (`count` at `+0x1C`, key at slot
/// `+0x26`) for `key`. It rounds `key` toward zero by `>> 4` first (`+0xF`
/// when negative - note the sign test is on the *pre*-shift value).
///
/// A hit rewrites that slot in place; a miss appends a new one and bumps the
/// count. Either way the slot's three accumulators (`+0x20`, `+0x22`, `+0x24`)
/// are zeroed. When the block already holds [`ANIM_SLOT_CAP`] slots the
/// routine returns immediately and installs nothing.
pub fn anim_slot_install(keys: &mut Vec<i16>, key_raw: i32) -> AnimSlotInstall {
    let key = anim_slot_key(key_raw);
    let count = keys.len() as i32;
    if count >= ANIM_SLOT_CAP {
        return AnimSlotInstall {
            slot: 0,
            reused: false,
            count,
            full: true,
        };
    }
    if let Some(slot) = keys.iter().position(|&k| k == key) {
        AnimSlotInstall {
            slot,
            reused: true,
            count,
            full: false,
        }
    } else {
        keys.push(key);
        AnimSlotInstall {
            slot: keys.len() - 1,
            reused: false,
            count: keys.len() as i32,
            full: false,
        }
    }
}

/// The key both slot routines scan on: the caller's raw fixed-point frame
/// time divided by 16, truncated **toward zero** (`bgez` on the raw value,
/// `+0xF` when negative, then `sra 4`). Both `FUN_801D57BC` and
/// `FUN_801D58E0` open with this identical bias-and-shift, so a port that
/// rounds either one the other way stops finding its own installed slots.
pub(super) fn anim_slot_key(key_raw: i32) -> i16 {
    (if key_raw >= 0 { key_raw } else { key_raw + 0xF } >> 4) as i16
}

/// Result of [`anim_slot_delete`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimSlotDelete {
    /// Index the key was found at, or `None` when the scan missed. Retail
    /// returns `-1` for the miss and the slot-block pointer otherwise, so the
    /// caller can only distinguish the two by sign.
    pub slot: Option<usize>,
    /// Live-slot count after the call. Unchanged on a miss.
    pub count: i32,
}

// REPLACED-BY: nothing is owed a port - retail never reaches it. Same caller
// and same gate as its installer sibling: the editor tick `FUN_801D4FC8`
// behind `_DAT_8007B868`, which the shipped disc never sets (evidence at
// [`editor_tick`]).
/// PORT: FUN_801D58E0 - the animation-slot **remover**, sibling of
/// [`anim_slot_install`].
///
/// Same block resolution (`fighter * 0x60`, count at `+0x1C`, 8-byte slots
/// from `+0x20` with the key half-word at slot `+0x06`) and the same
/// [`anim_slot_key`] bias-and-shift on the query. It scans the live slots for
/// the key; a miss returns `-1` and touches nothing.
///
/// A hit compacts: retail copies each following slot down one place with a
/// pair of unaligned `lwl`/`lwr` + `swl`/`swr` word moves covering the whole
/// 8-byte record (`+0x20` and `+0x24`), then decrements the count. It never
/// clears the slot it just vacated at the top of the block - that entry stays
/// as a stale duplicate above `count` and is simply never scanned again.
///
/// The port's slot model carries the key only, matching [`anim_slot_install`]:
/// the three accumulator half-words retail also moves are zeroed at install
/// and are not modelled anywhere.
pub fn anim_slot_delete(keys: &mut Vec<i16>, key_raw: i32) -> AnimSlotDelete {
    let key = anim_slot_key(key_raw);
    // Retail's scan is bounded by the live count, so an empty block never
    // enters the loop and falls straight through to the `-1` return.
    match keys.iter().position(|&k| k == key) {
        Some(slot) => {
            keys.remove(slot);
            AnimSlotDelete {
                slot: Some(slot),
                count: keys.len() as i32,
            }
        }
        None => AnimSlotDelete {
            slot: None,
            count: keys.len() as i32,
        },
    }
}
