/// PSX-virtual-address base of the character record table.
pub const TABLE_BASE: u32 = 0x80084708;

/// Per-character record stride.
pub const RECORD_STRIDE: u32 = 0x414;

/// Vahn's character record base address.
pub const VAHN_BASE: u32 = TABLE_BASE;
/// Noa's character record base address (slot 1).
pub const NOA_BASE: u32 = TABLE_BASE + RECORD_STRIDE;
/// Gala's character record base address (slot 2).
pub const GALA_BASE: u32 = TABLE_BASE + 2 * RECORD_STRIDE;
/// Fourth party slot record base address.
pub const SLOT3_BASE: u32 = TABLE_BASE + 3 * RECORD_STRIDE;

/// Offset within the record where the level-up event writes the live
/// in-battle stat copy: HP_cur, HP_max, MP_cur, MP_max, SP_cur,
/// SP_max (six u16s) at `+0x104..+0x110`, then six u16 live stats at
/// `+0x110..+0x11C`.
pub const LIVE_WINDOW: (u32, u32) = (0x104, 0x11C);

/// Offset within the record of the persistent stat window (9 u16 LE
/// values: HP_max, MP_max, cap, six stats).
pub const RECORD_WINDOW: (u32, u32) = (0x11C, 0x12E);

/// Offset of the rank counter (single byte, increments by 1 per
/// level-up event).
pub const RANK_COUNTER: u32 = 0x130;

/// Offset of the XP low word (u16 LE).
pub const XP_LO: u32 = 0x004;

/// Per-stat cap constant value. Unchanged across every captured
/// save; the `+0x120` u16 LE field carries this exact value for
/// Vahn, Noa, and Gala in every state.
pub const RECORD_STAT_CAP: u16 = 100;

/// Read a character's record-window u16 LE deltas across two saves.
/// Returns the 9 u16 values for the given record base in `main_ram`.
pub fn read_record_stats(main_ram: &[u8], record_base: u32) -> Option<[u16; 9]> {
    let off = (record_base - 0x80000000) as usize + RECORD_WINDOW.0 as usize;
    let end = off + 18;
    let bytes = main_ram.get(off..end)?;
    let mut out = [0u16; 9];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
    }
    Some(out)
}

/// Read the rank counter for the given record base.
pub fn read_rank_counter(main_ram: &[u8], record_base: u32) -> Option<u8> {
    let off = (record_base - 0x80000000) as usize + RANK_COUNTER as usize;
    main_ram.get(off).copied()
}

/// Read the cumulative XP (u16 LE) at `+0x004`.
pub fn read_xp_u16(main_ram: &[u8], record_base: u32) -> Option<u16> {
    let off = (record_base - 0x80000000) as usize + XP_LO as usize;
    let bytes = main_ram.get(off..off + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}
