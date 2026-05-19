//! [`legaia_engine_vm::move_buffer::MoveBufferHost`] implementation for
//! the engine's per-scene MOVE buffer pools.
//!
//! Wires the per-actor move-buffer cursor + envelope tick
//! (`engine-vm::move_buffer`, FUN_800204F8 / FUN_80020740) onto the
//! three retail MOVE pool roots that the clean-room engine carries on
//! [`crate::world::World`]:
//!
//! - [`World::move_buffer_root`] mirrors retail `_DAT_8007B888` (MOVE).
//! - [`World::move2_buffer_root`] mirrors retail `_DAT_8007B840` (MOVE2).
//!   Selected when the actor's `cursor_requested` is `>= 0x400`.
//! - [`World::move_buffer_alt_root`] mirrors retail `_DAT_8007B75C`.
//!   Selected when the actor's status-flag word has
//!   [`STATUS_FLAG_ALT_POOL`] set.
//!
//! Each pool blob is the MDT-shaped offset-table layout the slot-1
//! `Asset(0x05) = Move` descriptor produces (see `docs/formats/mdt.md`
//! and [`legaia_mdt::MoveBuffer`]). The resolver below:
//!
//! 1. Picks the pool by inspecting `actor_status_flags` then
//!    `requested_id` (mirroring the dispatch chain documented in
//!    [`vm::move_buffer::MoveBufferHost::resolve_record`]).
//! 2. Reads the per-id offset at `(requested_id & 0x3FF) * 4`.
//! 3. Returns the record bytes starting at that offset, running to
//!    end-of-pool (the move-VM dispatcher consumes only the prefix it
//!    needs).
//!
//! Returns `None` when:
//!  - the selected pool is empty (no scene MOVE table wired yet),
//!  - the index falls past the offset-table region, or
//!  - the offset itself walks past end-of-pool.
//!
//! The clean-room boundary stays intact: no Sony bytes live in this
//! module; the spec is `ghidra/scripts/funcs/800204f8.txt` plus the
//! per-record reader in `legaia-mdt`.
//!
//! [`World::move_buffer_root`]: crate::world::World::move_buffer_root
//! [`World::move2_buffer_root`]: crate::world::World::move2_buffer_root
//! [`World::move_buffer_alt_root`]: crate::world::World::move_buffer_alt_root
//! [`STATUS_FLAG_ALT_POOL`]: legaia_engine_vm::move_buffer::STATUS_FLAG_ALT_POOL
//! [`vm::move_buffer::MoveBufferHost::resolve_record`]:
//! legaia_engine_vm::move_buffer::MoveBufferHost::resolve_record

use legaia_engine_vm::move_buffer::{MOVE2_THRESHOLD, MoveBufferHost, STATUS_FLAG_ALT_POOL};

/// Move id mask matches retail (`requested_id & 0x3FF`). Also the same
/// mask documented in [`legaia_mdt::MOVE_ID_MASK`].
const MOVE_ID_MASK: i16 = 0x03FF;

/// Move-buffer host backed by three borrowed pool slices. Built by
/// [`crate::world::World::tick_actor_physics_with`] just before the
/// per-actor tick loop runs. The struct only holds shared borrows so
/// the surrounding tick loop can keep a mutable borrow on the actor
/// vector via standard struct-field splitting.
pub struct WorldMoveBufferView<'a> {
    /// MOVE pool (retail `_DAT_8007B888`).
    pub move_buf: &'a [u8],
    /// MOVE2 pool (retail `_DAT_8007B840`).
    pub move2_buf: &'a [u8],
    /// Alternate pool (retail `_DAT_8007B75C`).
    pub alt_buf: &'a [u8],
}

impl WorldMoveBufferView<'_> {
    fn pick_pool(&self, status_flags: u32, requested_id: i16) -> &[u8] {
        if status_flags & STATUS_FLAG_ALT_POOL != 0 {
            self.alt_buf
        } else if requested_id >= MOVE2_THRESHOLD {
            self.move2_buf
        } else {
            self.move_buf
        }
    }
}

impl MoveBufferHost for WorldMoveBufferView<'_> {
    fn resolve_record(&self, actor_status_flags: u32, requested_id: i16) -> Option<&[u8]> {
        if requested_id <= 0 {
            return None;
        }
        let pool = self.pick_pool(actor_status_flags, requested_id);
        if pool.is_empty() {
            return None;
        }
        let idx = (requested_id & MOVE_ID_MASK) as usize;
        let off_pos = idx.checked_mul(4)?;
        let off_end = off_pos.checked_add(4)?;
        if off_end > pool.len() {
            return None;
        }
        let raw_off = u32::from_le_bytes(pool[off_pos..off_end].try_into().ok()?) as usize;
        if raw_off == 0 || raw_off >= pool.len() {
            return None;
        }
        Some(&pool[raw_off..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic MOVE pool with one record at offset `record_off`
    /// installed for `id`. Record body: `[0, flag, fc_lo, fc_hi, 0, 0,
    /// divisor, 0]`.
    fn make_pool(id: u16, record_off: usize) -> Vec<u8> {
        // Need room for offset table + record bytes.
        let table_entries = (MOVE_ID_MASK as usize) + 1;
        let table_bytes = table_entries * 4;
        let total = (record_off + 16).max(table_bytes);
        let mut pool = vec![0u8; total];
        // Stamp the offset for `id`.
        let off = (id as usize) * 4;
        pool[off..off + 4].copy_from_slice(&(record_off as u32).to_le_bytes());
        // Stamp a record body (flag=0, frame_count=8, divisor=1).
        pool[record_off] = 0;
        pool[record_off + 1] = 0;
        pool[record_off + 2] = 8;
        pool[record_off + 3] = 0;
        pool[record_off + 4] = 0;
        pool[record_off + 5] = 0;
        pool[record_off + 6] = 1;
        pool[record_off + 7] = 0;
        pool
    }

    #[test]
    fn empty_pools_resolve_to_none() {
        let host = WorldMoveBufferView {
            move_buf: &[],
            move2_buf: &[],
            alt_buf: &[],
        };
        assert!(host.resolve_record(0, 1).is_none());
        assert!(host.resolve_record(STATUS_FLAG_ALT_POOL, 1).is_none());
        assert!(host.resolve_record(0, 0x500).is_none());
    }

    #[test]
    fn move_pool_returns_record_for_valid_id() {
        let pool = make_pool(7, 0x1010);
        let host = WorldMoveBufferView {
            move_buf: &pool,
            move2_buf: &[],
            alt_buf: &[],
        };
        let rec = host.resolve_record(0, 7).expect("record for id 7");
        // Returned slice starts at the record offset.
        assert_eq!(rec[0], 0);
        assert_eq!(rec[2], 8); // frame_count low byte
        assert_eq!(rec[6], 1); // divisor
    }

    #[test]
    fn move2_pool_selected_when_id_above_threshold() {
        let move_pool = make_pool(5, 0x1010);
        let move2_pool = make_pool(0x400 & MOVE_ID_MASK as u16, 0x1020);
        let host = WorldMoveBufferView {
            move_buf: &move_pool,
            move2_buf: &move2_pool,
            alt_buf: &[],
        };
        // id 5 is in MOVE pool, id 0x400 is in MOVE2 pool.
        let rec = host
            .resolve_record(0, MOVE2_THRESHOLD)
            .expect("MOVE2 record");
        // Move2 pool stamps record_off=0x1020 -> we get the body
        // populated by make_pool.
        assert_eq!(rec[2], 8);
    }

    #[test]
    fn alt_pool_selected_when_status_flag_set() {
        let move_pool = make_pool(3, 0x1010);
        // The alt pool installs id 3 at a different offset; the
        // resolver should pick the alt pool regardless of the
        // requested id when the status flag is set.
        let alt_pool = make_pool(3, 0x1020);
        let host = WorldMoveBufferView {
            move_buf: &move_pool,
            move2_buf: &[],
            alt_buf: &alt_pool,
        };
        let rec = host.resolve_record(STATUS_FLAG_ALT_POOL, 3).expect("alt");
        // Both record bodies have the same shape; the test asserts
        // the alt-pool path doesn't error and returns a non-empty
        // slice.
        assert!(!rec.is_empty());
    }

    #[test]
    fn zero_id_returns_none() {
        let pool = make_pool(7, 0x1010);
        let host = WorldMoveBufferView {
            move_buf: &pool,
            move2_buf: &[],
            alt_buf: &[],
        };
        assert!(host.resolve_record(0, 0).is_none());
        assert!(host.resolve_record(0, -3).is_none());
    }

    #[test]
    fn unmapped_id_returns_none() {
        let pool = make_pool(7, 0x1010);
        let host = WorldMoveBufferView {
            move_buf: &pool,
            move2_buf: &[],
            alt_buf: &[],
        };
        // id 8 has a zero offset in the table -> resolver returns None.
        assert!(host.resolve_record(0, 8).is_none());
    }

    #[test]
    fn offset_past_pool_end_returns_none() {
        let table_entries = (MOVE_ID_MASK as usize) + 1;
        let mut pool = vec![0u8; table_entries * 4 + 8];
        // Install a bogus offset that points past pool end.
        let off = 5 * 4;
        let bad = (pool.len() as u32 + 100).to_le_bytes();
        pool[off..off + 4].copy_from_slice(&bad);
        let host = WorldMoveBufferView {
            move_buf: &pool,
            move2_buf: &[],
            alt_buf: &[],
        };
        assert!(host.resolve_record(0, 5).is_none());
    }

    #[test]
    fn move_id_mask_strips_high_bits() {
        // id 0x1007 masks to 0x07 (after & 0x3FF), and since
        // MOVE2_THRESHOLD is 0x400, ids >= 0x400 land in MOVE2. The
        // pre-mask compare uses the raw signed id, so 0x1007 selects
        // MOVE2 even though it masks to 7 for the offset lookup.
        let move_pool = vec![0u8; 4096];
        let mut move2_pool = vec![0u8; 4096];
        // Index 7 in the MOVE2 table -> record at 0x800.
        let off = 7 * 4;
        move2_pool[off..off + 4].copy_from_slice(&0x800u32.to_le_bytes());
        let host = WorldMoveBufferView {
            move_buf: &move_pool,
            move2_buf: &move2_pool,
            alt_buf: &[],
        };
        let rec = host.resolve_record(0, 0x1007).expect("masked id");
        assert!(!rec.is_empty());
    }
}
