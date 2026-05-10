//! Codified mednafen save-state capture observations.
//!
//! Each entry pins a concrete byte-level finding from a `mednafen-state diff`
//! between two save states in the `~/.mednafen/mcs/` corpus. These observations
//! are the authoritative source-of-truth for runtime memory layout that isn't
//! reachable through static analysis of `SCUS_942.54` alone — they bracket
//! what the engine knows is happening at runtime so that downstream consumers
//! (parsers, runtime hosts, integration tests) can assert against pinned
//! offsets instead of re-deriving them from raw saves.
//!
//! Conventions:
//!
//! - Every observation references the slot pair it was captured from. The
//!   matching disc-gated test in `crates/mednafen/tests/real_saves.rs`
//!   exercises the underlying save bytes against the constants below.
//! - "Pinned" means a single-byte / single-region delta has been confirmed
//!   in at least one save pair. "Inferred" means the interpretation is
//!   consistent with the data but not yet cross-validated against a static
//!   writer-search.
//! - Field offsets are quoted relative to the relevant base (character record
//!   base `0x80084708` for character-record observations, PSX virtual address
//!   for global observations).

/// One pinned byte-level RAM delta observed in a save-state diff.
#[derive(Debug, Clone)]
pub struct ByteDelta {
    /// PSX virtual address of the changed byte.
    pub addr: u32,
    /// Pre-event byte value (left side of the diff).
    pub before: u8,
    /// Post-event byte value (right side of the diff).
    pub after: u8,
}

impl ByteDelta {
    /// Compute the signed delta as an `i16` (covers the typical wraparound
    /// range without saturating at the 8-bit boundary).
    pub fn signed_delta(&self) -> i16 {
        i16::from(self.after) - i16::from(self.before)
    }
}

/// Encounter-trigger observation captured from `mc1` (pre-encounter, walking
/// `map01`) → `mc2` (battle just initiated, same `map01` scene).
///
/// Findings:
///
/// - The 133 KB MIPS / data window at `0x801CE808..0x801F3818` differs
///   wholesale between mc1 and mc2 — this is the **battle overlay** loaded
///   on encounter trigger. The rounded extent (`0x801CE800..0x801F4000`,
///   ~150 KB) is the canonical battle-overlay residency window for the
///   current corpus.
/// - The 8-slot battle actor pointer table populates at `0x801C9370+` with
///   stride `0x60` between adjacent slot headers (the empty-actor sentinel
///   `0x20A1 0580` flips to a per-monster pointer + control word).
/// - Scene-bundle / sound-pool writes inside `0x80083000..0x80084000`
///   surface ~600 bytes of formation + BGM resolution work. The active
///   scene index at `0x80084540` does NOT change (still `0x55` = `map01`).
///
/// Use this from engines as a hard-coded fallback for the encounter-trigger
/// transition: when crossing the boundary, the battle overlay is loaded
/// into `OVERLAY_WINDOW`, and the actor pool fills `ACTOR_POOL_WINDOW`.
pub mod encounter_trigger {
    /// Battle overlay residency window (post-trigger). The mc1↔mc2 diff
    /// surfaces 133 KB of changed bytes inside this range, with no changes
    /// outside it (within the wider `0x801C0000..0x80200000` overlay
    /// region after stripping the actor-pool / scene-bundle deltas).
    pub const OVERLAY_WINDOW: (u32, u32) = (0x801CE800, 0x801F4000);

    /// 8-slot battle actor pointer table; populated post-trigger. Each
    /// slot is a `0x60`-byte header (the lower bits of `start_addr` align
    /// to the stride) carrying actor pointer + control word at offset 0.
    pub const ACTOR_POOL_WINDOW: (u32, u32) = (0x801C9370, 0x801C9900);

    /// Active scene-name table. Encounter trigger does NOT change this —
    /// the scene index stays equal to the field scene that triggered.
    pub const SCENE_NAME_TABLE_ADDR: u32 = 0x80084540;

    /// Approximate byte-count change in the overlay window between an
    /// equivalent (mc1, mc2) pair. Used for scoping assertions; tolerate
    /// ±10% drift across captures.
    pub const OVERLAY_BYTES_CHANGED_REF: usize = 133_086;

    /// Approximate byte-count change in the actor-pool window between an
    /// equivalent (mc1, mc2) pair. Captured from the wider
    /// `0x801C9300..0x801CA000` window; the narrower `ACTOR_POOL_WINDOW`
    /// captures a subset.
    pub const ACTOR_POOL_BYTES_CHANGED_REF: usize = 200;

    /// Slot stride between adjacent battle-actor pool entries.
    pub const ACTOR_POOL_SLOT_STRIDE: u32 = 0x60;

    /// Number of slots in the battle-actor pointer table.
    pub const ACTOR_POOL_SLOT_COUNT: usize = 8;
}

/// Vahn / Fire-Book-I observation captured from `mc4` (battle command menu
/// parked on Fire Book I) → `mc5` (Fire Book I just used on Vahn).
///
/// **Finding (pinned).** Inside Vahn's character record (`0x80084708..+0x414`)
/// exactly one byte region differs between mc4 and mc5: a 3-byte cluster at
/// `+0x185..+0x188`.
///
/// ```text
/// mc4: +0x185 = 0x01   +0x186 = 0x0C   +0x187 = 0x00
/// mc5: +0x185 = 0x02   +0x186 = 0x03   +0x187 = 0x0C
/// ```
///
/// **Interpretation (inferred).** The byte at `+0x185` reads as a length
/// prefix incrementing from 1 to 2; the trailing two bytes read as list
/// entries with the new entry inserted at position 0. The byte values
/// `0x03` and `0x0C` correspond to action-constant `Attack` and direction
/// `Left` respectively in [`legaia_art::queue::ActionConstant`], which is
/// a recently-issued action history rather than a permanent learn flag.
///
/// **Caveat.** The user's reported in-game action (Fire Book I usage to
/// learn a Hyper Art) suggests the post-event state should encode a new
/// learned art. The inserted byte value `0x03` does not match any of the
/// retail learned-art constants (those occupy the `0x1B..=0x32` range —
/// see `legaia_art::tables`). Two consistent interpretations remain:
///
/// 1. The 3-byte cluster is a transient command-history buffer that the
///    item-use animation populated, unrelated to the permanent Hyper-Art
///    flag; the actual Hyper-Art learn write lives at a different offset
///    not surfaced by mc4↔mc5 (e.g. a global story-flag word at
///    `_DAT_1F80_0394` or a mask field outside the character record).
/// 2. The cluster is the per-character recent-action buffer the runtime
///    pre-fills before the Fire Book animation plays.
///
/// Either way, `+0x185..+0x188` is **the only** record-internal write the
/// Fire Book event produced. Engines that want to detect "Vahn just used
/// Fire Book I" can read this region and compare against the `BEFORE` /
/// `AFTER` constants below; engines that want to set the learn flag
/// directly should NOT round-trip through this field until a writer-search
/// against the captured battle-action overlay confirms the semantic.
pub mod vahn_fire_book_use {
    /// Vahn's character-record base in retail RAM.
    pub const VAHN_RECORD_BASE: u32 = 0x80084708;

    /// Offset of the changed cluster within Vahn's record.
    pub const CHANGED_OFFSET: u32 = 0x185;

    /// Length of the changed cluster.
    pub const CHANGED_LEN: usize = 3;

    /// Pre-event bytes at `VAHN_RECORD_BASE + CHANGED_OFFSET`.
    pub const BEFORE: [u8; 3] = [0x01, 0x0C, 0x00];

    /// Post-event bytes at `VAHN_RECORD_BASE + CHANGED_OFFSET`.
    pub const AFTER: [u8; 3] = [0x02, 0x03, 0x0C];

    /// Absolute address of the cluster (handy for direct callers).
    pub const fn changed_addr() -> u32 {
        VAHN_RECORD_BASE + CHANGED_OFFSET
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_delta_signed_delta_arithmetic() {
        let d = ByteDelta {
            addr: 0x80084708 + 0x10E,
            before: 0x3A,
            after: 0x42,
        };
        assert_eq!(d.signed_delta(), 8);

        let neg = ByteDelta {
            addr: 0x80084708 + 0x11C,
            before: 0xDD,
            after: 0x03,
        };
        // 0x03 - 0xDD = -218 (the actual u16 LE field underneath wraps,
        // but the byte-only signed delta is what we surface).
        assert_eq!(neg.signed_delta(), -218);
    }

    #[test]
    fn encounter_trigger_overlay_window_covers_documented_range() {
        let (lo, hi) = encounter_trigger::OVERLAY_WINDOW;
        assert!(lo < hi);
        assert!(lo <= 0x801CE808);
        assert!(hi >= 0x801F3818);
        // Sanity: window spans roughly the documented 150 KB.
        assert!((hi - lo) as usize >= 0x20_000);
        assert!((hi - lo) as usize <= 0x40_000);
    }

    #[test]
    fn encounter_trigger_actor_pool_stride_is_consistent() {
        let (lo, hi) = encounter_trigger::ACTOR_POOL_WINDOW;
        let span = hi - lo;
        let n = encounter_trigger::ACTOR_POOL_SLOT_COUNT as u32;
        let stride = encounter_trigger::ACTOR_POOL_SLOT_STRIDE;
        assert!(span >= n * stride);
    }

    #[test]
    fn vahn_fire_book_changed_addr_is_inside_record() {
        let addr = vahn_fire_book_use::changed_addr();
        assert!(addr >= vahn_fire_book_use::VAHN_RECORD_BASE);
        assert!(addr < vahn_fire_book_use::VAHN_RECORD_BASE + 0x414);
    }

    #[test]
    fn vahn_fire_book_pattern_matches_pinned_capture() {
        // Pre-event has count=1, list=[0x0C], slot[1]=0x00.
        assert_eq!(vahn_fire_book_use::BEFORE, [0x01, 0x0C, 0x00]);
        // Post-event has count=2, list=[0x03, 0x0C].
        assert_eq!(vahn_fire_book_use::AFTER, [0x02, 0x03, 0x0C]);
        // Count byte incremented by 1 (regardless of interpretation).
        assert_eq!(
            vahn_fire_book_use::AFTER[0] - vahn_fire_book_use::BEFORE[0],
            1
        );
        // Pre-event entry at position 0 (`0x0C`) appears at position 1
        // post-event — consistent with insertion at the front.
        assert_eq!(vahn_fire_book_use::AFTER[2], vahn_fire_book_use::BEFORE[1]);
    }
}
