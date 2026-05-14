//! Per-prim renderer dispatch tables consumed by `FUN_80043390`.
//!
//! `FUN_80043390` (the SCUS-side per-primitive TMD renderer at the leaf of
//! the actor-mesh-chain walk, case-5 path of `FUN_8001ADA4`) dispatches a
//! primitive to a per-mode renderer via one of two function-pointer tables.
//! The selector flag is `_DAT_1F800394 & 1`:
//!
//! | Flag | Table base | Rows | Where it lives |
//! |---|---|---|---|
//! | clear | `0x8007657C` | 4 (alpha 0/50/A0/F0) | SCUS_942.54 (always-resident) |
//! | set   | `0x801F8968` | 1 (alpha 0 only)    | World-map overlay (paged) |
//!
//! The SCUS path adds `_DAT_1F800028 ∈ {0, 0x50, 0xA0, 0xF0}` to the
//! base index so four alpha-state rows are selectable; the overlay path
//! skips the alpha offset entirely, so only the first row is meaningful -
//! everything past row 0 in the overlay table is overlay CODE, NOT
//! more table entries.
//!
//! ```text
//! row stride: 0x50 bytes = 20 function-pointer slots
//! col stride: 4 bytes (one slot)
//!
//! SCUS path:    addr = 0x8007657C + (mode << 2) + alpha_offset
//! Overlay path: addr = 0x801F8968 + (mode << 2)         // no alpha
//! ```
//!
//! Slots 0..7 of every row are zero; slots 8..11 carry the "low-mode" prim
//! renderers shared between SCUS and overlay; slots 12..19 carry the
//! "high-mode" renderers - which are exactly where the overlay table swaps
//! in its eight overlay-resident emit leaves at
//! `0x801F7644..0x801F8690`.
//!
//! When the world-map top-view is active, the runtime sets
//! `_DAT_1F800394 & 1` so every per-actor TMD render call routes through
//! the overlay-resident high-mode renderers - which IS the bulk-continent
//! emit mechanism. The mechanism isn't a "procedural terrain generator";
//! it's mode-switched ordinary TMD rendering of the actor mesh chains
//! (the kingdom slot-1 landmark pack plus the world-map character TMDs).
//!
//! This module decodes either table out of a save state's main RAM and
//! exposes typed accessors so engine-side and test-side code can answer
//! "which renderer is invoked for primitive mode X in alpha state Y" without
//! hand-walking RAM.

use crate::extract::{PSX_RAM_KSEG0, PSX_RAM_SIZE, read_u32_le};

/// SCUS-resident base table address. Always populated; lives in
/// `SCUS_942.54` so the same bytes appear in every save state.
pub const SCUS_TABLE_BASE: u32 = 0x8007_657C;

/// Overlay-resident base table address. Populated only when the
/// world-map overlay (walk or top-view variant) is paged in; the slot
/// values are zero otherwise.
pub const OVERLAY_TABLE_BASE: u32 = 0x801F_8968;

/// One function-pointer slot is 4 bytes.
pub const SLOT_BYTES: u32 = 4;
/// Each alpha-state row spans 20 slots (low-mode shared + high-mode
/// per-table).
pub const SLOTS_PER_ROW: usize = 20;
/// Total bytes per alpha-state row.
pub const ROW_BYTES: u32 = SLOT_BYTES * SLOTS_PER_ROW as u32;
/// `FUN_80043390` cycles through four alpha states (`0x00`, `0x50`,
/// `0xA0`, `0xF0`) on the SCUS path; the overlay path uses only row 0.
pub const SCUS_ALPHA_ROWS: usize = 4;
pub const OVERLAY_ALPHA_ROWS: usize = 1;

/// Slot positions populated by `FUN_80043390`.
pub const LOW_MODE_START: usize = 8;
pub const LOW_MODE_END: usize = 12; // exclusive
pub const HIGH_MODE_START: usize = 12;
pub const HIGH_MODE_END: usize = 20; // exclusive

/// One row of the dispatch table for a single alpha state. Twenty
/// function pointers; slots 0..8 are always zero.
#[derive(Debug, Clone)]
pub struct DispatchRow {
    /// The alpha-state offset within the table (`0x00`, `0x50`, `0xA0`,
    /// `0xF0`).
    pub alpha_offset: u32,
    /// Raw u32 slot values, in order.
    pub slots: [u32; SLOTS_PER_ROW],
}

impl DispatchRow {
    /// Slots 8..11 - the "low-mode" prim renderers (POLY_F3 / POLY_FT3 /
    /// POLY_G3 / POLY_GT3 family entry points).
    pub fn low_mode(&self) -> &[u32] {
        &self.slots[LOW_MODE_START..LOW_MODE_END]
    }

    /// Slots 12..19 - the "high-mode" prim renderers (POLY_F4 / POLY_FT4
    /// / POLY_G4 / POLY_GT4 family entry points). The world-map overlay
    /// swaps these slots in `OVERLAY_TABLE_BASE` to overlay-resident
    /// renderers; SCUS_TABLE_BASE keeps them pointing at SCUS code.
    pub fn high_mode(&self) -> &[u32] {
        &self.slots[HIGH_MODE_START..HIGH_MODE_END]
    }
}

/// The full per-alpha-state dispatch table at a given base address.
#[derive(Debug, Clone)]
pub struct DispatchTable {
    pub base: u32,
    pub rows: Vec<DispatchRow>,
}

/// Slot-8..11 quartet that `FUN_80043390` shares between SCUS and
/// overlay dispatch tables. Used as a fingerprint to recognise "is this
/// region actually a dispatch table?" vs "is it leftover overlay code
/// that happens to live at the same address?"
pub const SHARED_LOW_MODE_QUARTET: [u32; 4] = [0x8004_409C, 0x8004_423C, 0x8004_4434, 0x8004_45B0];

impl DispatchTable {
    /// Returns `true` when every slot of every row is zero (the overlay
    /// table is unpopulated when the world-map overlay isn't paged in).
    pub fn is_empty(&self) -> bool {
        self.rows.iter().all(|r| r.slots.iter().all(|s| *s == 0))
    }

    /// Returns `true` when row 0's low-mode slots (8..11) match the
    /// SCUS-shared quartet. False for "leftover code that happens to
    /// occupy the same RAM region", which is what
    /// `OVERLAY_TABLE_BASE` looks like when the world-map overlay isn't
    /// paged in. Always true for the SCUS-resident table.
    pub fn looks_like_dispatch_table(&self) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        let lows = &self.rows[0].slots[LOW_MODE_START..LOW_MODE_END];
        lows == SHARED_LOW_MODE_QUARTET
    }

    /// All distinct function pointers that appear anywhere in the
    /// "high-mode" slot range (12..19) across every row. The set returned
    /// is the candidate prim-emitter leaves.
    pub fn high_mode_targets(&self) -> Vec<u32> {
        let mut seen = std::collections::BTreeSet::new();
        for row in &self.rows {
            for s in row.high_mode() {
                if *s != 0 {
                    seen.insert(*s);
                }
            }
        }
        seen.into_iter().collect()
    }
}

/// Decode the dispatch table at `base` out of main RAM, reading
/// `rows` alpha rows × `SLOTS_PER_ROW` slots starting at `base`.
///
/// Use `SCUS_ALPHA_ROWS` for `SCUS_TABLE_BASE` and `OVERLAY_ALPHA_ROWS`
/// for `OVERLAY_TABLE_BASE` - the overlay path skips the alpha offset
/// in `FUN_80043390`, so bytes past row 0 of the overlay table are
/// overlay CODE rather than additional table entries.
///
/// Returns `Err` if `base` plus the table extent runs past main RAM.
pub fn decode(ram: &[u8], base: u32, rows: usize) -> anyhow::Result<DispatchTable> {
    let table_bytes = ROW_BYTES * rows as u32;
    if base < PSX_RAM_KSEG0 || base + table_bytes > PSX_RAM_KSEG0 + PSX_RAM_SIZE as u32 {
        anyhow::bail!("dispatch table at 0x{base:08X} (+{table_bytes:#x}) runs past main RAM",);
    }
    let mut rows_out = Vec::with_capacity(rows);
    for alpha_idx in 0..rows as u32 {
        let alpha_offset = alpha_idx * ROW_BYTES;
        let mut slots = [0u32; SLOTS_PER_ROW];
        for (slot_idx, slot) in slots.iter_mut().enumerate() {
            let addr = base + alpha_offset + slot_idx as u32 * SLOT_BYTES;
            *slot = read_u32_le(ram, addr)?;
        }
        rows_out.push(DispatchRow {
            alpha_offset,
            slots,
        });
    }
    Ok(DispatchTable {
        base,
        rows: rows_out,
    })
}

/// Convenience: decode both the SCUS table (4 alpha rows) and the
/// overlay table (1 alpha row) in one go. The overlay table's `rows`
/// will be all-zero if the world-map overlay isn't paged into RAM in
/// the save state being inspected.
pub fn decode_both(ram: &[u8]) -> anyhow::Result<(DispatchTable, DispatchTable)> {
    Ok((
        decode(ram, SCUS_TABLE_BASE, SCUS_ALPHA_ROWS)?,
        decode(ram, OVERLAY_TABLE_BASE, OVERLAY_ALPHA_ROWS)?,
    ))
}

/// Classify a function-pointer slot value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// Slot is zero (unused).
    Zero,
    /// Slot points into the SCUS_942.54 load region (`0x80010000+`)
    /// but outside the overlay window. Static-resident handler.
    Scus,
    /// Slot points into the documented overlay window (`0x801C0000..
    /// 0x801F9000`). Paged handler, only valid when the corresponding
    /// overlay is loaded.
    Overlay,
    /// Slot points outside both known regions - either garbage or a
    /// region this module hasn't classified yet.
    Other,
}

/// Classify a slot value.
pub fn classify(slot: u32) -> SlotKind {
    if slot == 0 {
        return SlotKind::Zero;
    }
    // Overlay window: 0x801C0000..0x801F9000 (per docs/subsystems/world-map.md;
    // the world-map overlay extends past the once-documented 0x801EFFFF).
    if (0x801C_0000..0x801F_9000).contains(&slot) {
        return SlotKind::Overlay;
    }
    // SCUS code region (loose bounds): the binary loads at 0x80010000 and
    // is well under 1 MiB, so anything below 0x80100000 we treat as SCUS.
    if (0x8001_0000..0x8010_0000).contains(&slot) {
        return SlotKind::Scus;
    }
    SlotKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::PSX_RAM_SIZE;

    fn synth_ram() -> Vec<u8> {
        vec![0u8; PSX_RAM_SIZE]
    }

    fn put_u32(ram: &mut [u8], addr: u32, value: u32) {
        let off = (addr - PSX_RAM_KSEG0) as usize;
        ram[off..off + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn empty_overlay_table_classifies_as_empty() {
        let ram = synth_ram();
        let t = decode(&ram, OVERLAY_TABLE_BASE, OVERLAY_ALPHA_ROWS).unwrap();
        assert!(t.is_empty());
        assert!(t.high_mode_targets().is_empty());
        assert_eq!(t.rows.len(), 1);
    }

    #[test]
    fn populated_table_round_trips() {
        let mut ram = synth_ram();
        // Populate one row of the SCUS table: slots 8..11 (low-mode)
        // and 12..19 (high-mode).
        let mut expected = [0u32; SLOTS_PER_ROW];
        for (slot_idx, exp) in expected.iter_mut().enumerate() {
            let value = if slot_idx >= LOW_MODE_START {
                0x8004_0000 + (slot_idx as u32) * 0x10
            } else {
                0
            };
            *exp = value;
            put_u32(
                &mut ram,
                SCUS_TABLE_BASE + (slot_idx as u32) * SLOT_BYTES,
                value,
            );
        }
        let t = decode(&ram, SCUS_TABLE_BASE, SCUS_ALPHA_ROWS).unwrap();
        assert_eq!(t.rows[0].slots, expected);
        assert!(!t.is_empty());
        assert_eq!(t.high_mode_targets().len(), HIGH_MODE_END - HIGH_MODE_START);
    }

    #[test]
    fn classifier_distinguishes_scus_from_overlay() {
        assert_eq!(classify(0), SlotKind::Zero);
        assert_eq!(classify(0x8004_409C), SlotKind::Scus);
        assert_eq!(classify(0x801F_7644), SlotKind::Overlay);
        assert_eq!(classify(0x801C_0000), SlotKind::Overlay);
        assert_eq!(classify(0x801F_8FFF), SlotKind::Overlay);
        // Just above the overlay window we documented.
        assert_eq!(classify(0x801F_9000), SlotKind::Other);
        // Way out of range.
        assert_eq!(classify(0x90000000), SlotKind::Other);
    }

    #[test]
    fn high_mode_targets_dedup() {
        let mut ram = synth_ram();
        // Populate two rows where rows have an overlapping high-mode
        // target. After dedup the targets list should keep the unique set.
        for slot_idx in HIGH_MODE_START..HIGH_MODE_END {
            put_u32(
                &mut ram,
                SCUS_TABLE_BASE + (slot_idx as u32) * SLOT_BYTES,
                0x8004_3000,
            );
            put_u32(
                &mut ram,
                SCUS_TABLE_BASE + ROW_BYTES + (slot_idx as u32) * SLOT_BYTES,
                0x8004_3000,
            );
        }
        let t = decode(&ram, SCUS_TABLE_BASE, SCUS_ALPHA_ROWS).unwrap();
        let targets = t.high_mode_targets();
        assert_eq!(targets, vec![0x8004_3000]);
    }

    #[test]
    fn decode_both_returns_one_overlay_row_and_four_scus_rows() {
        let ram = synth_ram();
        let (scus, overlay) = decode_both(&ram).unwrap();
        assert_eq!(scus.rows.len(), SCUS_ALPHA_ROWS);
        assert_eq!(overlay.rows.len(), OVERLAY_ALPHA_ROWS);
    }

    #[test]
    fn looks_like_dispatch_table_recognises_shared_quartet() {
        let mut ram = synth_ram();
        // Empty overlay table is NOT a dispatch table (all slots zero).
        let t = decode(&ram, OVERLAY_TABLE_BASE, OVERLAY_ALPHA_ROWS).unwrap();
        assert!(!t.looks_like_dispatch_table());
        // Random non-quartet bytes also fail.
        for slot_idx in LOW_MODE_START..LOW_MODE_END {
            put_u32(
                &mut ram,
                OVERLAY_TABLE_BASE + (slot_idx as u32) * SLOT_BYTES,
                0xDEAD_BEEF,
            );
        }
        let t = decode(&ram, OVERLAY_TABLE_BASE, OVERLAY_ALPHA_ROWS).unwrap();
        assert!(!t.looks_like_dispatch_table());
        // The exact SCUS-shared quartet passes.
        for (i, slot_idx) in (LOW_MODE_START..LOW_MODE_END).enumerate() {
            put_u32(
                &mut ram,
                OVERLAY_TABLE_BASE + (slot_idx as u32) * SLOT_BYTES,
                SHARED_LOW_MODE_QUARTET[i],
            );
        }
        let t = decode(&ram, OVERLAY_TABLE_BASE, OVERLAY_ALPHA_ROWS).unwrap();
        assert!(t.looks_like_dispatch_table());
    }
}
