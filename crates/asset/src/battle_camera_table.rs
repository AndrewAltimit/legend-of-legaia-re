//! Battle-camera **per-character height** table (battle-action overlay, PROT
//! 0898, runtime VA `0x801F4D2C`).
//!
//! The battle camera's framing builder `FUN_801D5854` (dump:
//! `ghidra/scripts/funcs/overlay_battle_action_801d5854.txt`) sets the
//! eye-space translation's `Y` component from a 16-bit table keyed on the
//! acting party member's **character identity**, not on the seat they occupy:
//!
//! ```text
//! 801d5a08  lui   v0,0x8008
//! 801d5a0c  addiu v0,v0,-0x42f0     ; v0 = 0x8007BD10 (party slot -> char id)
//! 801d5a10  addu  v0,s5,v0          ; s5 = actor slot
//! 801d5a14  lbu   v0,0x0(v0)        ; char_id, 1-based (1=Vahn 2=Noa 3=Gala 4=Terra)
//! 801d5a18  addiu v1,v1,0x4d2c      ; v1 = table base 0x801F4D2C
//! 801d5a1c  addiu v0,v0,-0x1        ; char_id - 1
//! 801d5a20  sll   v0,v0,0x1         ; * 2 (u16 stride)
//! 801d5a24  addu  v0,v0,v1
//! 801d5a28  lhu   v1,0x0(v0)        ; TR.y for this character
//! ```
//!
//! Both framing cases that build a per-character close-up (`FUN_801D5854`
//! case `0`, the command submenu, and case `3`, its mirrored sibling) read
//! this same table; every other case uses a literal or a computed height. The
//! same 1-based `DAT_8007BD10` character selector keys the per-character
//! element table (see [`crate::element_affinity`]).
//!
//! ## Extent
//!
//! [`CAMERA_HEIGHT_LEN`] entries - one per playable character id. The four
//! halfwords are immediately followed by a table of `0x801Fxxxx` pointers, so
//! the extent is structural rather than inferred from a terminator.
//!
//! ## Provenance
//!
//! Static overlay data: VA `0x801F4D2C` maps to PROT 0898 file offset
//! [`CAMERA_HEIGHT_FILE_OFFSET`] under the same link base
//! ([`OVERLAY_LINK_BASE`]) that pins the move-power table (`0x801F4F5C` →
//! `0x26744`; see [`crate::move_power`]) and the element-affinity matrix. The
//! offset falls inside the overlay's RAM-verified byte-identical `.text` +
//! `.rodata` window, so the disc bytes are the runtime bytes.

/// CDNAME / PROT index of the battle-action overlay holding the table.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize = 898;

/// The battle-action overlay's link/load base (`VA − file_offset`).
pub const OVERLAY_LINK_BASE: u32 = 0x801C_E818;

/// Runtime VA of the per-character camera-height table.
pub const CAMERA_HEIGHT_VA: u32 = 0x801F_4D2C;

/// Raw PROT 0898 file offset of the table (= `VA − OVERLAY_LINK_BASE`).
pub const CAMERA_HEIGHT_FILE_OFFSET: usize = 0x26514;

/// Entries in the table - one per playable character id.
pub const CAMERA_HEIGHT_LEN: usize = 4;

/// The parsed per-character battle-camera height table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleCameraHeights {
    /// Raw `TR.y` halfwords in table order (index = `char_id − 1`).
    heights: [u16; CAMERA_HEIGHT_LEN],
}

impl BattleCameraHeights {
    /// Parse the table out of the raw PROT 0898 entry bytes. `None` when the
    /// buffer is too short to be that overlay.
    pub fn parse(prot_0898: &[u8]) -> Option<BattleCameraHeights> {
        let end = CAMERA_HEIGHT_FILE_OFFSET + CAMERA_HEIGHT_LEN * 2;
        if prot_0898.len() < end {
            return None;
        }
        let mut heights = [0u16; CAMERA_HEIGHT_LEN];
        for (i, h) in heights.iter_mut().enumerate() {
            let o = CAMERA_HEIGHT_FILE_OFFSET + i * 2;
            *h = u16::from_le_bytes([prot_0898[o], prot_0898[o + 1]]);
        }
        Some(BattleCameraHeights { heights })
    }

    /// Camera `TR.y` for a **1-based** character id (retail's
    /// `DAT_8007BD10[slot]`: 1 = Vahn, 2 = Noa, 3 = Gala, 4 = Terra). `None`
    /// for `0` or an id past the table.
    pub fn height_for_char_id(&self, char_id: u8) -> Option<u16> {
        let idx = (char_id as usize).checked_sub(1)?;
        self.heights.get(idx).copied()
    }

    /// Camera `TR.y` by **table index** (`char_id − 1`, i.e. the party-record
    /// selector the engine's 0-based party slots use directly).
    pub fn height_for_index(&self, index: usize) -> Option<u16> {
        self.heights.get(index).copied()
    }

    /// All entries in table order.
    pub fn heights(&self) -> &[u16; CAMERA_HEIGHT_LEN] {
        &self.heights
    }
}

/// Parse helper mirroring the other format modules.
pub fn parse(prot_0898: &[u8]) -> Option<BattleCameraHeights> {
    BattleCameraHeights::parse(prot_0898)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_offset_matches_the_link_base() {
        assert_eq!(
            CAMERA_HEIGHT_VA - OVERLAY_LINK_BASE,
            CAMERA_HEIGHT_FILE_OFFSET as u32
        );
    }

    #[test]
    fn char_id_is_one_based_and_bounded() {
        let mut buf = vec![0u8; CAMERA_HEIGHT_FILE_OFFSET + CAMERA_HEIGHT_LEN * 2];
        for i in 0..CAMERA_HEIGHT_LEN {
            let v = (0x100 * (i as u16 + 1)).to_le_bytes();
            buf[CAMERA_HEIGHT_FILE_OFFSET + i * 2] = v[0];
            buf[CAMERA_HEIGHT_FILE_OFFSET + i * 2 + 1] = v[1];
        }
        let t = BattleCameraHeights::parse(&buf).expect("parses");
        assert_eq!(t.height_for_char_id(0), None, "char id is 1-based");
        assert_eq!(t.height_for_char_id(1), Some(0x100));
        assert_eq!(t.height_for_char_id(4), Some(0x400));
        assert_eq!(t.height_for_char_id(5), None, "past the table");
        assert_eq!(t.height_for_index(0), t.height_for_char_id(1));
    }

    #[test]
    fn short_buffer_is_rejected() {
        assert!(BattleCameraHeights::parse(&[0u8; 16]).is_none());
    }
}
