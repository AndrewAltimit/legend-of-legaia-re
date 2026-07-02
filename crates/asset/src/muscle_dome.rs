//! Muscle Dome minigame - **resident in the battle-action overlay (PROT 0898)**.
//!
//! The Muscle Dome card-battle arena is *not* a separate overlay. Its match
//! state machine [`FUN_801d0748`] and all its data (the deck/hand tables at
//! `0x801f4b8c`/`0x801f4b94`, the per-step sub-draw script-record table
//! `PTR_DAT_801f4d34`, the victory-message string table `0x801f4dfc`) are
//! resident in the **battle-action overlay** (PROT entry 0898, base
//! `0x801CE818` - the same overlay [`crate::move_power`] reads). The
//! Duckstation "`overlay_muscle_dome.bin`" capture was that overlay's slot.
//!
//! This resolves the long-open "muscle-dome overlay identity" thread: the
//! arena runs on the battle engine (its fighters are battle actors in
//! `&DAT_801c9370`, card plays resolve through the battle-action path), so it
//! ships *inside* the battle overlay rather than aliasing it. The `0977`
//! "Ronginus" entry is only the mode-24 sub-id-5 *door/init* slot (arena
//! roster + `other6` paths), not the match SM.
//!
//! ## What is pinned here
//!
//! `FUN_801d0748`'s prologue reads the Muscle Dome context base
//! `_DAT_8007bd24` (`lui v0,0x8008; lw v0,-0x42dc(v0)`), a signature unique to
//! the arena controller; it lands at battle-overlay file offset
//! [`MATCH_SM_FILE_OFFSET`]. The deck / script / victory tables sit in the
//! `0x801f4xxx` data band of the same overlay. [`verify_resident`] confirms the
//! overlay image hosts them (the disc-reproducible identity check); the deck
//! byte semantics live in `docs/subsystems/minigame-muscle-dome.md`.

/// PROT index of the host overlay (the battle-action overlay).
pub const MUSCLE_OVERLAY_PROT_INDEX: usize = 898;

/// Load base of the battle-action overlay.
pub const MUSCLE_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// VA of the Muscle Dome context base pointer `_DAT_8007bd24` (read by the
/// match SM prologue).
pub const MUSCLE_CTX_PTR_VA: u32 = 0x8007_BD24;

/// VA of the match-controller `FUN_801d0748`.
pub const MATCH_SM_VA: u32 = 0x801D_0748;

/// File offset of the match controller within the overlay image.
pub const MATCH_SM_FILE_OFFSET: usize = (MATCH_SM_VA - MUSCLE_OVERLAY_BASE_VA) as usize;

/// VA of the per-slot deck/hand move-index table (`&DAT_801f4b8c`).
pub const DECK_TABLE_VA: u32 = 0x801F_4B8C;

/// VA of the per-slot card sprite-id table (`&DAT_801f4b94`).
pub const HAND_SPRITE_TABLE_VA: u32 = 0x801F_4B94;

/// Hand size - the deal loop builds exactly four card slots.
pub const HAND_SLOTS: usize = 4;

/// First / last valid hand command id: the deck entries are the four
/// direction-command ids `0xC..=0xF` (the weapon-swing runtime slots; a
/// card's cost is the same per-(char,cmd) record `+0x74` byte the Arts
/// gauge reads, `DAT_801c9360[char][cmd]+0x74`).
pub const HAND_COMMAND_MIN: u8 = 0x0C;
/// See [`HAND_COMMAND_MIN`].
pub const HAND_COMMAND_MAX: u8 = 0x0F;

/// VA of the per-step sub-draw script-record pointer table (`PTR_DAT_801f4d34`).
pub const SUBDRAW_PTR_TABLE_VA: u32 = 0x801F_4D34;

/// VA of the victory-message string-pointer table.
pub const VICTORY_MSG_TABLE_VA: u32 = 0x801F_4DFC;

/// The match-controller prologue signature: `lui v0,0x8008; lw v0,-0x42dc(v0);
/// addiu sp,sp,-0x48` (little-endian machine code). The `lui`/`lw` pair loads
/// `_DAT_8007bd24`, unique to the Muscle Dome controller.
pub const MATCH_SM_SIGNATURE: [u8; 12] = [
    0x08, 0x80, 0x02, 0x3c, // lui   v0, 0x8008
    0x24, 0xbd, 0x42, 0x8c, // lw    v0, -0x42dc(v0)
    0xb8, 0xff, 0xbd, 0x27, // addiu sp, sp, -0x48
];

/// Whether a `u32` value is a VA inside the given overlay image.
fn in_overlay(va: u32, len: usize) -> bool {
    va >= MUSCLE_OVERLAY_BASE_VA && ((va - MUSCLE_OVERLAY_BASE_VA) as usize) < len
}

/// Read a little-endian `u32` at an overlay VA.
fn read_va(overlay: &[u8], va: u32) -> Option<u32> {
    let off = (va.checked_sub(MUSCLE_OVERLAY_BASE_VA)?) as usize;
    let b = overlay.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Confirm the Muscle Dome match SM + its pointer tables are resident in the
/// supplied battle-action overlay image (PROT 0898 as-loaded bytes). Returns
/// `true` when the match-controller signature is at [`MATCH_SM_FILE_OFFSET`] and
/// the sub-draw / victory tables hold in-overlay pointers - i.e. the arena lives
/// in this overlay.
pub fn verify_resident(overlay: &[u8]) -> bool {
    // Match-SM signature at the expected offset.
    let sig_ok = overlay
        .get(MATCH_SM_FILE_OFFSET..MATCH_SM_FILE_OFFSET + MATCH_SM_SIGNATURE.len())
        .map(|s| s == MATCH_SM_SIGNATURE)
        .unwrap_or(false);
    if !sig_ok {
        return false;
    }
    // First sub-draw script-record pointer resolves in-overlay.
    let subdraw_ok = read_va(overlay, SUBDRAW_PTR_TABLE_VA)
        .map(|p| in_overlay(p, overlay.len()))
        .unwrap_or(false);
    // First victory-message pointer resolves in-overlay.
    let victory_ok = read_va(overlay, VICTORY_MSG_TABLE_VA)
        .map(|p| in_overlay(p, overlay.len()))
        .unwrap_or(false);
    subdraw_ok && victory_ok
}

/// Decode the four **hand command ids** (`DAT_801f4b8c[0..4]`): per hand
/// slot, the direction-command id the deal loop assigns to that card and
/// the commit path (`FUN_801d388c` case `0xb`) appends into the fighter's
/// `+0x1df` action queue. `None` unless all four are distinct ids in
/// `HAND_COMMAND_MIN..=HAND_COMMAND_MAX` (the structural validity check).
pub fn hand_command_ids(overlay: &[u8]) -> Option<[u8; HAND_SLOTS]> {
    let off = (DECK_TABLE_VA - MUSCLE_OVERLAY_BASE_VA) as usize;
    let b = overlay.get(off..off + HAND_SLOTS)?;
    let ids = [b[0], b[1], b[2], b[3]];
    let valid = ids
        .iter()
        .all(|&id| (HAND_COMMAND_MIN..=HAND_COMMAND_MAX).contains(&id));
    let distinct = (0..HAND_SLOTS).all(|i| (i + 1..HAND_SLOTS).all(|j| ids[i] != ids[j]));
    (valid && distinct).then_some(ids)
}

/// Decode the four per-slot card **sprite ids** (`DAT_801f4b94[0..4]`) - the
/// deal loop's card-face selector (with a `+2` "unlearned" variant gated on
/// the character record's per-move flag).
pub fn hand_sprite_ids(overlay: &[u8]) -> Option<[u8; HAND_SLOTS]> {
    let off = (HAND_SPRITE_TABLE_VA - MUSCLE_OVERLAY_BASE_VA) as usize;
    let b = overlay.get(off..off + HAND_SLOTS)?;
    Some([b[0], b[1], b[2], b[3]])
}

/// Count the victory-message string pointers at [`VICTORY_MSG_TABLE_VA`]
/// (consecutive in-overlay pointers, stopping at the first that isn't).
pub fn victory_message_count(overlay: &[u8]) -> usize {
    let mut n = 0;
    while let Some(p) = read_va(overlay, VICTORY_MSG_TABLE_VA + (n as u32) * 4) {
        if !in_overlay(p, overlay.len()) {
            break;
        }
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_tables_decode() {
        let mut buf = vec![0u8; 0x27000];
        let deck = (DECK_TABLE_VA - MUSCLE_OVERLAY_BASE_VA) as usize;
        buf[deck..deck + 4].copy_from_slice(&[0x0C, 0x0F, 0x0E, 0x0D]);
        let spr = (HAND_SPRITE_TABLE_VA - MUSCLE_OVERLAY_BASE_VA) as usize;
        buf[spr..spr + 4].copy_from_slice(&[13, 16, 17, 12]);
        assert_eq!(hand_command_ids(&buf), Some([0x0C, 0x0F, 0x0E, 0x0D]));
        assert_eq!(hand_sprite_ids(&buf), Some([13, 16, 17, 12]));
        // Duplicate / out-of-range ids are rejected.
        buf[deck] = 0x0F;
        assert_eq!(hand_command_ids(&buf), None);
        buf[deck] = 0x10;
        assert_eq!(hand_command_ids(&buf), None);
    }

    #[test]
    fn offsets_and_signature() {
        assert_eq!(MATCH_SM_FILE_OFFSET, 0x1F30);
        // The lui/lw pair loads _DAT_8007bd24.
        assert_eq!(0x8008u32 << 16, 0x8008_0000);
        assert_eq!(0x8008_0000u32.wrapping_sub(0x42dc), MUSCLE_CTX_PTR_VA);
        assert_eq!(MATCH_SM_SIGNATURE.len(), 12);
    }

    #[test]
    fn verify_resident_rejects_empty() {
        assert!(!verify_resident(&[]));
        assert!(!verify_resident(&[0u8; 0x30000]));
    }
}
