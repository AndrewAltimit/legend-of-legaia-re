//! Capture observation: the per-actor battle-animation dispatch-pointer table.

/// Actor record stride.
pub const ACTOR_RECORD_STRIDE: u32 = 0x2D4;

/// Slot 0 (party leader) actor record base. The 8-slot pool
/// continues at `+ ACTOR_RECORD_STRIDE` for each subsequent slot.
pub const SLOT0_ACTOR_RECORD_BASE: u32 = 0x800EC9E8;

/// Per-actor anim-PC region (16 bytes). Holds increment-style
/// per-bone or per-frame cursors.
pub const ANIM_PC_FIELD_OFFSET: u32 = 0x1D8;
/// Length of the anim-PC region.
pub const ANIM_PC_FIELD_LEN: u32 = 0x10;

/// Per-frame anim flag accumulator (18 bytes).
pub const ANIM_FRAME_FLAGS_OFFSET: u32 = 0x1F4;
/// Length of the flag accumulator.
pub const ANIM_FRAME_FLAGS_LEN: u32 = 0x12;

/// 4 × u32 anim dispatch pointer table.
pub const ANIM_DISPATCH_PTR_TABLE_OFFSET: u32 = 0x234;
/// 4 × u32 = 16 bytes.
pub const ANIM_DISPATCH_PTR_TABLE_LEN: usize = 16;

/// Resolve the absolute address of the dispatch-pointer slot 0
/// for a given actor record base.
pub fn dispatch_ptr_addr(actor_record_base: u32) -> u32 {
    actor_record_base + ANIM_DISPATCH_PTR_TABLE_OFFSET
}

/// Read the four u32 dispatch pointers from a contiguous main-RAM
/// slice. Returns `None` if the actor record base is outside the
/// PSX RAM window or the slice is too short.
pub fn read_dispatch_pointers(main_ram: &[u8], actor_record_base: u32) -> Option<[u32; 4]> {
    let off = (actor_record_base - 0x80000000) as usize + ANIM_DISPATCH_PTR_TABLE_OFFSET as usize;
    let bytes = main_ram.get(off..off + ANIM_DISPATCH_PTR_TABLE_LEN)?;
    let mut out = [0u32; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        let chunk = &bytes[i * 4..i * 4 + 4];
        *slot = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    Some(out)
}
