//! Two small field-overlay helpers that seed a pooled **menu actor**:
//! `FUN_801E5834` (spawn) and `FUN_801E58A8` (row-count seed). Both live in
//! PROT 0897 at base `0x801CE818`.
//!
//! ## `FUN_801E5834` - pooled menu-actor spawn
//!
//! Allocates one entry from the actor pool `_DAT_8007C34C` for the fixed
//! descriptor `0x801F2978` through `FUN_80020DE0`, and on a non-null result
//! writes five halfwords: `+0x54 = 0` (the phase byte every handler in this
//! band dispatches on) and the four arguments into `+0x50`, `+0x14`, `+0x16`
//! and `+0x9C`. A null allocation is silently dropped - there is no retry and
//! no error path.
//!
//! ## `FUN_801E58A8` - list row-count seed
//!
//! Writes the sentinel `+0x5E = -2`, then derives the actor's row count
//! `+0x5C` from three globals and one actor flag bit. Read out of the
//! disassembly (the arithmetic is easy to mis-read from the decompiled C,
//! which renders `x*8 - x` as a multiply):
//!
//! ```text
//!   base  = u16 @ 0x8007BDD8
//!   pages = u16 @ 0x8007B8F8
//!   extra = word @ 0x8007B6AC
//!
//!   if base == 99:                     rows = pages + 1        ; clear bit
//!   else if actor[+0x10] & 0x01000000:
//!       if extra != 0:                 rows = base + extra - 1 ; clear, tick, re-set
//!       else:                          rows = base + pages*7
//!   else:                              rows = base
//! ```
//!
//! The `pages * 7` term is `(pages << 3) - pages` in the body. The flag bit
//! `0x01000000` is cleared **around** the `FUN_800204F8` tick in the
//! `extra != 0` arm and restored immediately after, which is the only reason
//! that arm returns early instead of falling into the shared tick - both
//! paths tick the actor exactly once.
//!
//! `see ghidra/scripts/funcs/801e5834.txt`,
//! `see ghidra/scripts/funcs/801e58a8.txt`

/// The `base == 99` special case in the row-count seed.
pub const BASE_SENTINEL: u16 = 99;

/// The actor flag bit the row-count seed tests and toggles.
pub const ROW_FLAG_BIT: u32 = 0x0100_0000;

/// The sentinel written to `actor[+0x5E]` on every call.
pub const ROW_SENTINEL: i16 = -2;

/// The five fields `FUN_801E5834` writes into a freshly-allocated pool entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuActorSeed {
    /// `+0x54` - phase, always zeroed on spawn.
    pub phase: u16,
    /// `+0x50` - sub-handler id (the first argument).
    pub handler: u16,
    /// `+0x14` - screen X (the second argument).
    pub x: u16,
    /// `+0x16` - screen Y (the third argument).
    pub y: u16,
    /// `+0x9C` - the dwell / parameter halfword (the fourth argument).
    pub param: u16,
}

/// Build the field set a pooled menu-actor spawn installs.
///
/// The allocation itself (`FUN_80020DE0` against the pool `_DAT_8007C34C`
/// and the descriptor `0x801F2978`) is host plumbing; this is the write set
/// that follows a successful one. Retail drops the whole write on a null
/// allocation, which the caller expresses by not calling this.
///
/// PORT: FUN_801e5834
///
/// NOT WIRED: the engine allocates menu actors through
/// `legaia_engine_core::actor_alloc_host` with typed fields, and no host
/// root spawns this particular descriptor.
pub fn menu_actor_seed(handler: u16, x: u16, y: u16, param: u16) -> MenuActorSeed {
    MenuActorSeed {
        phase: 0,
        handler,
        x,
        y,
        param,
    }
}

/// What one row-count seed produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowCountSeed {
    /// The value written to `actor[+0x5C]`.
    pub rows: u16,
    /// The value of the `0x01000000` flag bit after the call.
    pub flag_set: bool,
}

/// Derive the list row count.
///
/// `flag_set` is `actor[+0x10] & 0x01000000 != 0` on entry.
///
/// PORT: FUN_801e58a8
///
/// NOT WIRED: no engine list model reads these three globals; the pause-menu
/// and dev-menu row counts in `engine-ui` come from typed Rust lists.
pub fn row_count_seed(base: u16, pages: u16, extra: u32, flag_set: bool) -> RowCountSeed {
    if base == BASE_SENTINEL {
        return RowCountSeed {
            rows: pages.wrapping_add(1),
            flag_set: false,
        };
    }
    if !flag_set {
        return RowCountSeed {
            rows: base,
            flag_set: false,
        };
    }
    if extra != 0 {
        // The bit is cleared around the tick and restored, so it ends set.
        return RowCountSeed {
            rows: base.wrapping_add(extra as u16).wrapping_sub(1),
            flag_set: true,
        };
    }
    RowCountSeed {
        rows: base.wrapping_add(pages.wrapping_mul(7)),
        flag_set: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_zeroes_the_phase_and_keeps_the_four_arguments() {
        let s = menu_actor_seed(0x22, 0x50, 0x30, 9);
        assert_eq!(s.phase, 0);
        assert_eq!((s.handler, s.x, s.y, s.param), (0x22, 0x50, 0x30, 9));
    }

    #[test]
    fn sentinel_base_uses_pages_plus_one_and_clears_the_flag() {
        let r = row_count_seed(BASE_SENTINEL, 4, 77, true);
        assert_eq!(r.rows, 5);
        assert!(!r.flag_set);
    }

    #[test]
    fn sentinel_base_wins_over_the_flag_being_clear() {
        let r = row_count_seed(BASE_SENTINEL, 0, 0, false);
        assert_eq!(r.rows, 1);
    }

    #[test]
    fn flag_clear_passes_the_base_through() {
        let r = row_count_seed(12, 4, 77, false);
        assert_eq!(r.rows, 12);
        assert!(!r.flag_set);
    }

    #[test]
    fn flag_set_with_extra_adds_extra_minus_one_and_restores_the_flag() {
        let r = row_count_seed(12, 4, 5, true);
        assert_eq!(r.rows, 16);
        assert!(r.flag_set);
    }

    #[test]
    fn flag_set_without_extra_scales_pages_by_seven() {
        // (pages << 3) - pages, not (pages << 3).
        let r = row_count_seed(12, 4, 0, true);
        assert_eq!(r.rows, 12 + 28);
        assert!(r.flag_set);
    }
}
