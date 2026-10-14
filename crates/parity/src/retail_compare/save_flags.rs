//! The system-flag bank of a lifted save: reading the raised ids and
//! raising or lowering them.

/// Offset of the system-flag bank (`0x80085758`) inside the story-flag
/// bitmap a save carries (`0x80085600`).
pub(super) const SYSTEM_FLAG_WINDOW: usize = 0x158;

/// Lower the system flags `ids` in a lifted save.
pub fn clear_system_flags(save: &mut legaia_save::SaveFile, ids: &[u16]) {
    for &idx in ids {
        if let Some(b) = save
            .ext
            .story_flag_bits
            .get_mut(SYSTEM_FLAG_WINDOW + usize::from(idx >> 3))
        {
            *b &= !(0x80u8 >> (idx & 7));
        }
    }
}

/// Raise or lower one system flag in a lifted save.
pub fn set_system_flag(save: &mut legaia_save::SaveFile, idx: u16, on: bool) {
    if let Some(b) = save
        .ext
        .story_flag_bits
        .get_mut(SYSTEM_FLAG_WINDOW + usize::from(idx >> 3))
    {
        let bit = 0x80u8 >> (idx & 7);
        if on {
            *b |= bit;
        } else {
            *b &= !bit;
        }
    }
}

/// Every raised bit of the save's system-flag bank, as the flag ids
/// `World::system_flag_set` takes (MSB-first within a byte).
pub fn system_flag_ids(save: &legaia_save::SaveFile) -> Vec<u16> {
    let bits = save
        .ext
        .story_flag_bits
        .get(SYSTEM_FLAG_WINDOW..)
        .unwrap_or(&[]);
    let mut out = Vec::new();
    for (b, &byte) in bits.iter().enumerate() {
        for k in 0..8 {
            if byte & (0x80 >> k) != 0 {
                out.push((b * 8 + k) as u16);
            }
        }
    }
    out
}
