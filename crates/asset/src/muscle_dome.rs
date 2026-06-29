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

#[cfg(test)]
mod tests {
    use super::*;

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
