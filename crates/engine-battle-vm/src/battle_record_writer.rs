//! The battle-widget **record writer** at `0x801DB7B0` in PROT 0898.
//!
//! One VA, two routines. `docs/reference/functions/script-vms.md` records the
//! alias: the PROT 0897 (town / field) resident at `0x801DB7B0` is a 28-byte
//! jump-table trampoline, and the PROT 0898 resident is this - a 108-byte,
//! 27-instruction leaf that builds one 12-byte record and returns. Because
//! the two bodies are unrelated, `scripts/ci/port-catalog-ignore.toml` files
//! the bare VA under `[worklist_va_aliased]` ("no single port site exists"),
//! which is why nothing here carries a `PORT:` marker for it: the address
//! does not name one routine. The routine itself is real, and this is it.
//!
//! ```text
//! rec = ctx + id * 0xC + 0x11B4        ; ctx = *0x8007BD24
//! rec[0] = p2                          ; a1
//! rec[1] = 0                           ; always cleared
//! rec[2] = id                          ; a0
//! rec[3] = p3                          ; a2
//! rec[4..6]  = p4                      ; a3, a halfword
//! rec[6..8]  = p5                      ; the fifth argument, off sp+0x10
//! rec[8..10] = stats[+0xA]             ; stats = *(ctx + id*4 + 0x1074)
//! rec[10..12] = stats[+0xC]
//! ```
//!
//! Three details a decompiled reading loses, all visible in the operands:
//!
//! * the stride is built as `((id << 1) + id) << 2`, i.e. `id * 0xC`, and the
//!   record is **twelve** bytes - the last two halfwords are part of it, not a
//!   separate write;
//! * the base is the battle context **pointer** `*0x8007BD24`, re-loaded for
//!   the second half (`lw a1, -0x42dc(t1)` at `0x801DB7E0`), not the global's
//!   own address;
//! * the two copied halfwords come from a **per-id pointer table** at
//!   `ctx + 0x1074`, indexed `id * 4` - a second indirection, so an id with a
//!   null slot there faults rather than copying zeros.
//!
//! The consuming side - the widget state machine that reads `ctx+0x11B4` - is
//! what the engine already models; this is its producer.
//!
//! REF: FUN_801DB7B0 (the PROT 0898 resident; the PROT 0897 resident at the
//! same VA is a different routine)

/// Byte offset of the record array inside the battle context.
pub const WIDGET_RECORD_BASE: u32 = 0x11B4;

/// Stride of one record (`((id << 1) + id) << 2`).
pub const WIDGET_RECORD_STRIDE: usize = 0xC;

/// Byte offset of the per-id stat-block pointer table (`ctx + 0x1074`,
/// indexed `id * 4`).
pub const WIDGET_STAT_TABLE_BASE: u32 = 0x1074;

/// The two halfwords the writer copies out of the stat block, by offset.
pub const WIDGET_STAT_COPY_OFFSETS: [u32; 2] = [0x0A, 0x0C];

/// One record as the writer leaves it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WidgetRecord {
    /// `rec[0]` - the caller's `p2`.
    pub kind: u8,
    /// `rec[1]` - always cleared.
    pub reserved: u8,
    /// `rec[2]` - the record's own id, which is also its index.
    pub id: u8,
    /// `rec[3]` - the caller's `p3`.
    pub param: u8,
    /// `rec[4..6]` - the caller's `p4`.
    pub value_a: u16,
    /// `rec[6..8]` - the caller's `p5`, which arrives on the stack.
    pub value_b: u16,
    /// `rec[8..10]` - stat block `+0x0A`.
    pub stat_a: u16,
    /// `rec[10..12]` - stat block `+0x0C`.
    pub stat_b: u16,
}

/// Build the record `FUN_801DB7B0(id, p2, p3, p4, p5)` writes.
///
/// `stats` is the `(+0x0A, +0x0C)` pair from the block
/// `*(ctx + id * 4 + 0x1074)` points at; the caller resolves it because the
/// table is the host's.
pub fn widget_record(id: u8, p2: u8, p3: u8, p4: u16, p5: u16, stats: (u16, u16)) -> WidgetRecord {
    WidgetRecord {
        kind: p2,
        reserved: 0,
        id,
        param: p3,
        value_a: p4,
        value_b: p5,
        stat_a: stats.0,
        stat_b: stats.1,
    }
}

/// Byte offset of record `id` inside the battle context.
pub fn widget_record_offset(id: u8) -> u32 {
    WIDGET_RECORD_BASE + u32::from(id) * WIDGET_RECORD_STRIDE as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_is_twelve_bytes_at_a_twelve_byte_stride() {
        assert_eq!(widget_record_offset(0), WIDGET_RECORD_BASE);
        assert_eq!(widget_record_offset(1), WIDGET_RECORD_BASE + 0xC);
        // `((id << 1) + id) << 2` for the widest byte id.
        let id = 0xFFu32;
        assert_eq!(
            widget_record_offset(0xFF),
            WIDGET_RECORD_BASE + (((id << 1) + id) << 2)
        );
    }

    #[test]
    fn every_field_lands_where_the_stores_put_it() {
        let r = widget_record(3, 0x11, 0x22, 0x3344, 0x5566, (0x7788, 0x99AA));
        assert_eq!(
            r,
            WidgetRecord {
                kind: 0x11,
                reserved: 0,
                id: 3,
                param: 0x22,
                value_a: 0x3344,
                value_b: 0x5566,
                stat_a: 0x7788,
                stat_b: 0x99AA,
            }
        );
        // `rec[1]` is cleared unconditionally - it is not one of the five
        // arguments.
        assert_eq!(r.reserved, 0);
    }
}
