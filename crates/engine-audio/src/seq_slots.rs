//! SEQ resource-slot table - the 12-byte-stride record array at
//! `0x80091508` that tracks which side-band SEQ/VAB resources currently
//! hold an open libsnd sequence handle.
//!
//! Each retail record carries two host pointers (`+0x0` destination,
//! `+0x4` staging - owned by the loader, not modeled here), a sign-extended
//! id byte at `+0x8` (which doubles as the SsSeqClose access id), and a
//! loaded flag at `+0xB`. The installer walker (`FUN_8001E54C`, ported as
//! `engine-core::chunk_install`) sets the flag when a VAB/SEQ upload lands
//! in the slot; the release below is its teardown counterpart. The hardware
//! side of the close (SsSeqClose = `FUN_80068C80`: `SpuFree` the resident
//! body, drop the open-sequence count) stays behind a caller-supplied
//! closure so this table stays a pure bookkeeping model.
//! // REF: FUN_8001E54C // REF: FUN_80068C80

/// One record of the `0x80091508` table, reduced to the two fields the
/// release path touches. Mirrors `engine-core::chunk_install::SeqSlot`
/// (the installer's view of the same record).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeqResourceSlot {
    /// SEQ handle / access id byte (record `+0x8`, read sign-extended).
    /// This is the value handed to SsSeqClose on release.
    pub handle: i8,
    /// Loaded flag (record `+0xB`): set by the installer's VAB-upload
    /// types (1 / 3), cleared here on release.
    pub loaded: bool,
}

/// The SEQ resource-slot table (`0x80091508`, 12-byte stride).
#[derive(Debug, Clone, Default)]
pub struct SeqResourceTable {
    slots: Vec<SeqResourceSlot>,
}

impl SeqResourceTable {
    /// Table with `n` empty (unloaded, handle 0) slots.
    pub fn new(n: usize) -> Self {
        Self {
            slots: vec![SeqResourceSlot::default(); n],
        }
    }

    /// Shared view of slot `index`, `None` when out of range.
    pub fn slot(&self, index: usize) -> Option<&SeqResourceSlot> {
        self.slots.get(index)
    }

    /// Mutable view of slot `index` (installer-side bookkeeping: stamp the
    /// handle byte, raise the loaded flag on upload).
    pub fn slot_mut(&mut self, index: usize) -> Option<&mut SeqResourceSlot> {
        self.slots.get_mut(index)
    }

    // PORT: FUN_8001FF58 - SEQ resource-slot release: index the 12-byte-
    // stride table at 0x80091508; when the loaded flag (+0xB) is set, clear
    // it and SsSeqClose (FUN_80068C80) the slot's handle byte (+0x8).
    /// Release slot `index`'s open SEQ handle. When the slot is loaded the
    /// flag is cleared **first** (retail stores the zero in the `jal` delay
    /// slot) and `close` runs with the slot's handle byte; an unloaded or
    /// out-of-range slot is a no-op. Returns `true` when a close fired.
    ///
    /// Retail takes the index as a sign-extended byte (the caller passes a
    /// record's own `+0x8` id byte back in); the out-of-range guard is
    /// clean-room hardening over the unchecked retail indexing.
    pub fn release<F: FnOnce(i8)>(&mut self, index: usize, close: F) -> bool {
        let Some(slot) = self.slots.get_mut(index) else {
            return false;
        };
        if !slot.loaded {
            return false;
        }
        slot.loaded = false;
        close(slot.handle);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_closes_and_clears_a_loaded_slot() {
        let mut table = SeqResourceTable::new(4);
        *table.slot_mut(2).unwrap() = SeqResourceSlot {
            handle: 7,
            loaded: true,
        };
        let mut closed = Vec::new();
        assert!(table.release(2, |h| closed.push(h)));
        assert_eq!(closed, vec![7], "close fired with the slot's handle byte");
        assert!(!table.slot(2).unwrap().loaded, "loaded flag cleared");
        assert_eq!(
            table.slot(2).unwrap().handle,
            7,
            "handle byte survives the release (retail only zeroes +0xB)"
        );
    }

    #[test]
    fn release_of_an_unloaded_slot_is_a_no_op() {
        let mut table = SeqResourceTable::new(4);
        table.slot_mut(1).unwrap().handle = 3; // handle set, flag clear
        let mut closed = Vec::new();
        assert!(!table.release(1, |h| closed.push(h)));
        assert!(closed.is_empty(), "no close on an unloaded slot");
        assert_eq!(
            *table.slot(1).unwrap(),
            SeqResourceSlot {
                handle: 3,
                loaded: false
            },
            "slot untouched"
        );
    }

    #[test]
    fn release_is_idempotent() {
        let mut table = SeqResourceTable::new(1);
        *table.slot_mut(0).unwrap() = SeqResourceSlot {
            handle: -2, // sign-extended byte handles are legal
            loaded: true,
        };
        let mut count = 0;
        assert!(table.release(0, |_| count += 1));
        assert!(!table.release(0, |_| count += 1), "second release no-ops");
        assert_eq!(count, 1, "close fired exactly once");
    }

    #[test]
    fn release_out_of_range_is_a_no_op() {
        let mut table = SeqResourceTable::new(2);
        let mut fired = false;
        assert!(!table.release(5, |_| fired = true));
        assert!(!fired);
    }
}
