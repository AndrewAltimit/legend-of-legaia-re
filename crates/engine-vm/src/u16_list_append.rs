//! The pre-increment `u16` list append (`FUN_8001FA68`), the sprite path's
//! eight-instruction leaf in `SCUS_942.54`.
//!
//! Read out of the instruction stream, cross-checked against
//! `extracted/SCUS_942.54` at `0x800 + va - 0x80010000`
//! (`ghidra/scripts/funcs/8001fa68.txt` is the *spec*; no `SCUS_942.54` bytes
//! live in this crate).
//!
//! REF: FUN_8003F3FC
//!
//! REPLACED-BY: `legaia_engine_system::movie::cutscene::sprite_stack_push`. That covers
//! [`list_append_u16`], which is the same address ported a second time and is
//! live on both hosts through the fog-particle pool. Its producer
//! (`FUN_8003F3FC`, the per-particle update) is ported, and the `jal` at
//! `0x8003F800` that names this routine is that port's free-slot return.

/// Append `value` to a u16 list whose element count sits in a separate
/// `i16` - `FUN_8001FA68`.
///
/// PORT: FUN_8001FA68
///
/// REPLACED-BY: `legaia_engine_system::movie::cutscene::sprite_stack_push`, which is
/// this same address ported a second time and reached on both hosts.
/// `FUN_8003F3FC` - named here as the missing producer - is ported: it is the
/// per-particle half of `legaia_engine_field::fog_particles::FogPool::render_step`,
/// and the `jal` at `0x8003F800` this routine is the callee of is that port's
/// `free_slot`, which returns a dead record's index through
/// `sprite_stack_push`. So the producer exists, has a host, and already calls
/// an equivalent - what is left here is a duplicate, not a gap. It stays
/// because it is written against the `u16` element type of the sprite list and
/// documents the unchecked-store edge the halfword port does not state; no
/// host is owed a second call site.
///
/// The eight-instruction body is:
///
/// ```text
/// lh   v0, 0(a0)     ; count
/// addiu v0, v0, 1    ; pre-increment
/// sh   v0, 0(a0)     ; store it back
/// sll  v0, v0, 1     ; count * 2
/// addu v0, v0, a1
/// sh   a3, 0(v0)     ; entries[new_count] = value
/// ```
///
/// Two things the C rendering does not show:
///
/// - The store indexes at the **new** count, so with a count starting at
///   `0` the first append lands in `entries[1]` and slot `0` is never
///   written. Callers that want a dense list start the count at `-1`.
/// - `a2` is never read. The one traced caller, `FUN_8003F3FC` at
///   `0x8003F7FC`, loads `a2` from `lh 0x2(a0)` - the list's capacity -
///   immediately before the `jal`, and the callee ignores it. Retail
///   therefore performs **no** bounds check on this append. Ghidra's
///   inferred signature drops the argument entirely, which is the
///   "dropped register arguments" artifact; the `jal` and its
///   surroundings are what settle it.
///
/// This port cannot reproduce the unchecked store, so it returns the
/// written index, or `None` when the new count falls outside `entries`.
/// In that case the count is still incremented, matching retail's
/// unconditional `sh v0, 0(a0)`.
pub fn list_append_u16(count: &mut i16, entries: &mut [u16], value: u16) -> Option<usize> {
    *count = count.wrapping_add(1);
    let idx = *count;
    if idx < 0 {
        return None;
    }
    let idx = idx as usize;
    match entries.get_mut(idx) {
        Some(slot) => {
            *slot = value;
            Some(idx)
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u16_append_pre_increments_so_slot_zero_stays_unwritten() {
        let mut entries = [0u16; 4];
        let mut count: i16 = 0;
        assert_eq!(list_append_u16(&mut count, &mut entries, 0xAAAA), Some(1));
        assert_eq!(count, 1);
        assert_eq!(entries, [0, 0xAAAA, 0, 0]);
    }

    #[test]
    fn u16_append_from_minus_one_fills_densely() {
        let mut entries = [0u16; 3];
        let mut count: i16 = -1;
        assert_eq!(list_append_u16(&mut count, &mut entries, 1), Some(0));
        assert_eq!(list_append_u16(&mut count, &mut entries, 2), Some(1));
        assert_eq!(list_append_u16(&mut count, &mut entries, 3), Some(2));
        assert_eq!(entries, [1, 2, 3]);
        assert_eq!(count, 2);
    }

    #[test]
    fn u16_append_still_increments_the_count_when_it_overruns() {
        // Retail's `sh v0, 0(a0)` is unconditional - the count advances
        // whether or not the store lands in range.
        let mut entries = [0u16; 2];
        let mut count: i16 = 1;
        assert_eq!(list_append_u16(&mut count, &mut entries, 9), None);
        assert_eq!(count, 2);
        assert_eq!(entries, [0, 0]);
    }
}
