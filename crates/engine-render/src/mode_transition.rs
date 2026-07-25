//! Game-**mode-entry prologue** - the pass that resets frame pacing, validates
//! the RAM-cached overlay image, and stashes the field state a battle /
//! cutscene / minigame mode is about to clobber.
//!
//! PORT: FUN_80016230
//!
//! NOT WIRED: the three things it acts on are all outside this crate. The
//! frame-pacing ring is `FUN_80016B6C`'s (ported in
//! `legaia_engine_audio::sfx_ring`, which models the *SFX* ring at
//! `0x8007B6D8`, not this hblank-sample ring at `0x80084098`); the overlay
//! cache is `legaia_engine_core::overlay_loader`'s; and the field snapshot
//! writes `legaia_engine_core`'s warp-coordinate globals. The engine also does
//! not need the VRAM stash at all - it keeps the actor pool in host memory
//! across a mode change instead of parking it in spare VRAM - so
//! [`ACTOR_POOL_STASH_RECT`] is documented rather than executed. What closes
//! the gap is `legaia_engine_core`'s mode dispatcher calling
//! [`mode_entry_prologue`] on every mode change, which is a sibling lane's file
//! scope.
//!
//! REF: FUN_80016b6c - the adaptive frame-skip pass whose 16-sample hblank ring
//! this clears.
//! REF: FUN_8003ebe4 - overlay loader A, re-streamed on a cache miss.
//! REF: FUN_8003de7c - CD read-idle poll, run either side of that load.
//! REF: FUN_800583c8 - `LoadImage`, the VRAM stash.
//! REF: FUN_80058104 - `DrawSync`, bracketing it.
//! REF: FUN_8001a8b0 - the byte `memcpy` that restores a cache hit.
//!
//! # Three jobs
//!
//! **1. Frame pacing.** Sixteen halfwords from `0x80084098` and the byte at
//! `0x800915DC` are zeroed unconditionally, so the adaptive frame-skip factor
//! restarts at `1` rather than carrying the outgoing mode's worst frame into
//! the incoming one.
//!
//! **2. Overlay cache.** On [`MODE_BATTLE_INIT`] only, the cached overlay-A
//! image is checksummed as a plain 32-bit word sum over `len / 4` words -
//! rounding *toward zero* for a negative length, which is the `bgez`/`addiu 3`
//! idiom, and reading the length global fresh on every loop iteration. On a
//! match the submode register takes `3` and the cached bytes are `memcpy`ed
//! straight into the overlay-A destination, skipping the CD entirely; on a miss
//! the register is invalidated to `-1` and overlay `3` (the battle overlay) is
//! re-streamed. In the dev build (`_DAT_8007B8C2 == 0`) the expected sum is
//! substituted for the computed one, so dev **always** hits.
//!
//! **3. Field snapshot.** For the four modes in [`SNAPSHOT_MODES`], and only
//! when the outgoing-mode register `_DAT_8007B7AC` reads `3` (field / town),
//! the player's world X / Z are copied into the warp-transition slots
//! `0x80084568` / `0x8008456C`, the field re-entry flag `_DAT_8007B8B8` is set
//! to `1`, and the whole `0x7B0C`-byte actor-pool block at `0x8007C348` is
//! parked in the VRAM rect [`ACTOR_POOL_STASH_RECT`] between two `DrawSync`es.
//! That `_DAT_8007B7AC == 3` guard is what makes the pass coherent: it only
//! preserves field state when the mode being left *is* the field.
//!
//! Independently of all three, leaving a mode outside `{2, 3}` clears
//! `_DAT_8007B9C4`.
//!
//! Source: `ghidra/scripts/funcs/80016230.txt` (disassembly). The Ghidra C for
//! this body renders the restore `memcpy` with **two** arguments - the length in
//! `a2` survives from the checksum loop and the decompiler drops it, which is
//! exactly the dropped-register-argument artifact.

/// Game mode whose entry validates the overlay cache (`BATTLE INIT`).
pub const MODE_BATTLE_INIT: i16 = 0x14;
/// Modes that snapshot the field before entry: `BATTLE INIT`, `EFECT TEST`,
/// `OTHER INIT` (the mode-24 minigame door warp) and `STR INIT`.
pub const SNAPSHOT_MODES: [i16; 4] = [0x14, 0x08, 0x1A, 0x18];
/// Outgoing-mode value the field snapshot requires - `MAIN MODE`, the
/// field / town loop.
pub const SNAPSHOT_FROM_MODE: i16 = 3;
/// Modes that keep `_DAT_8007B9C4` (`MAIN INIT` / `MAIN MODE`).
pub const KEEP_B9C4_MODES: [u16; 2] = [2, 3];

/// Length of the hblank-sample ring at `0x80084098`, in halfwords.
pub const PACING_RING_LEN: usize = 16;
/// Overlay id re-streamed on a cache miss (extraction PROT 0898, battle).
pub const OVERLAY_BATTLE: u8 = 3;
/// Value the submode register takes on a cache hit.
pub const SUBMODE_ON_HIT: i32 = 3;
/// Value it takes on a miss, before the re-stream.
pub const SUBMODE_ON_MISS: i32 = -1;
/// Field re-entry flag value the snapshot writes (`_DAT_8007B8B8`).
pub const REENTRY_FLAG: u32 = 1;
/// Size of the actor-pool block the snapshot parks (`0x8007C348`).
pub const ACTOR_POOL_BYTES: usize = 0x7B0C;

/// The VRAM rect the actor pool is stashed into: `(x, y, w, h)` in
/// 16-bit texels, so `64 * 256 * 2 = 0x8000` bytes - just enough for
/// [`ACTOR_POOL_BYTES`].
pub const ACTOR_POOL_STASH_RECT: (u16, u16, u16, u16) = (960, 0, 64, 256);

/// Word sum over the cached overlay image, exactly as retail computes it:
/// `len / 4` words, the division rounding toward zero, wrapping adds.
///
/// A negative `len` is reachable in principle (the global is a signed word);
/// retail's `bgez` / `addiu v0, a2, 3` biasing makes `-1 / 4 == 0`, so it walks
/// zero words rather than a huge count.
pub fn overlay_cache_checksum(words: &[u32], len: i32) -> u32 {
    let biased = if len < 0 { len.wrapping_add(3) } else { len };
    let count = (biased >> 2).max(0) as usize;
    words
        .iter()
        .take(count)
        .fold(0u32, |acc, &w| acc.wrapping_add(w))
}

/// What the overlay-cache check decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayCacheVerdict {
    /// Not a mode that checks, or no cached image staged.
    Skipped,
    /// Sum matched (or dev substituted it): set the submode register to
    /// [`SUBMODE_ON_HIT`] and `memcpy` `len` bytes into the overlay-A
    /// destination.
    Restore {
        /// Byte count copied - the same `len` the checksum walked.
        len: i32,
    },
    /// Sum missed: invalidate the register and re-stream the overlay,
    /// CD-idle-polled either side.
    Reload {
        /// Overlay id handed to loader A.
        overlay: u8,
    },
}

/// Inputs to the overlay-cache arm.
#[derive(Debug, Clone, Copy, Default)]
pub struct OverlayCacheInputs<'a> {
    /// The cached image, as words (`_DAT_8007B9AC`).
    pub cached: Option<&'a [u32]>,
    /// Its byte length (`_DAT_8007B9DC`).
    pub len: i32,
    /// The expected sum (`_DAT_8007B9A8`).
    pub expected: u32,
    /// `_DAT_8007B8C2 == 0` - the dev build, which substitutes the expected sum
    /// for the computed one and therefore always hits.
    pub dev_build: bool,
}

/// Run the cache check for `mode`.
pub fn overlay_cache_check(mode: i16, inputs: &OverlayCacheInputs<'_>) -> OverlayCacheVerdict {
    if mode != MODE_BATTLE_INIT {
        return OverlayCacheVerdict::Skipped;
    }
    let Some(cached) = inputs.cached else {
        return OverlayCacheVerdict::Skipped;
    };
    let computed = if inputs.dev_build {
        inputs.expected
    } else {
        overlay_cache_checksum(cached, inputs.len)
    };
    if computed == inputs.expected {
        OverlayCacheVerdict::Restore { len: inputs.len }
    } else {
        OverlayCacheVerdict::Reload {
            overlay: OVERLAY_BATTLE,
        }
    }
}

/// The field state parked for the mode being entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSnapshot {
    /// `0x80084568` - the player's world X.
    pub warp_x: i32,
    /// `0x8008456C` - the player's world Z.
    pub warp_z: i32,
    /// `_DAT_8007B8B8`.
    pub reentry_flag: u32,
    /// The VRAM rect the actor pool was stashed into.
    pub stash_rect: (u16, u16, u16, u16),
}

/// The whole prologue's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeEntryPrologue {
    /// How many pacing-ring halfwords were zeroed.
    pub pacing_ring_cleared: usize,
    /// Overlay-cache verdict.
    pub overlay: OverlayCacheVerdict,
    /// `true` when `_DAT_8007B9C4` is cleared (every mode outside `{2, 3}`).
    pub clears_b9c4: bool,
    /// The field snapshot, when the mode + outgoing-mode gate both passed.
    pub snapshot: Option<FieldSnapshot>,
}

/// Run the prologue for a mode change.
///
/// * `mode` - the mode being entered (`_DAT_8007B83C`).
/// * `outgoing_mode` - `_DAT_8007B7AC`; the snapshot needs
///   [`SNAPSHOT_FROM_MODE`].
/// * `player_xz` - the player actor's `+0x14` / `+0x18`, sign-extended.
pub fn mode_entry_prologue(
    mode: i16,
    outgoing_mode: i16,
    player_xz: (i16, i16),
    overlay: &OverlayCacheInputs<'_>,
) -> ModeEntryPrologue {
    let snapshot =
        (SNAPSHOT_MODES.contains(&mode) && outgoing_mode == SNAPSHOT_FROM_MODE).then(|| {
            FieldSnapshot {
                warp_x: i32::from(player_xz.0),
                warp_z: i32::from(player_xz.1),
                reentry_flag: REENTRY_FLAG,
                stash_rect: ACTOR_POOL_STASH_RECT,
            }
        });
    ModeEntryPrologue {
        pacing_ring_cleared: PACING_RING_LEN,
        overlay: overlay_cache_check(mode, overlay),
        clears_b9c4: !KEEP_B9C4_MODES.contains(&(mode as u16)),
        snapshot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty<'a>() -> OverlayCacheInputs<'a> {
        OverlayCacheInputs::default()
    }

    #[test]
    fn the_stash_rect_is_exactly_big_enough_for_the_actor_pool() {
        let (_, _, w, h) = ACTOR_POOL_STASH_RECT;
        let bytes = usize::from(w) * usize::from(h) * 2;
        assert_eq!(bytes, 0x8000);
        assert!(bytes >= ACTOR_POOL_BYTES);
    }

    #[test]
    fn checksum_is_a_plain_word_sum_over_len_over_four_words() {
        let words = [1u32, 2, 3, 4, 0xDEAD_BEEF];
        assert_eq!(overlay_cache_checksum(&words, 16), 1 + 2 + 3 + 4);
        assert_eq!(overlay_cache_checksum(&words, 8), 1 + 2);
        // A partial trailing word is not counted.
        assert_eq!(overlay_cache_checksum(&words, 15), 1 + 2 + 3);
    }

    #[test]
    fn checksum_wraps_rather_than_overflowing() {
        let words = [u32::MAX, 2];
        assert_eq!(overlay_cache_checksum(&words, 8), 1);
    }

    #[test]
    fn a_negative_length_walks_zero_words() {
        // The `bgez` / `addiu 3` bias makes -1..-3 round to zero, not to a
        // huge unsigned count.
        for len in [-1i32, -2, -3] {
            assert_eq!(overlay_cache_checksum(&[1, 2, 3], len), 0);
        }
    }

    #[test]
    fn only_battle_init_checks_the_cache() {
        let words = [7u32];
        let inputs = OverlayCacheInputs {
            cached: Some(&words),
            len: 4,
            expected: 7,
            dev_build: false,
        };
        assert_eq!(
            overlay_cache_check(MODE_BATTLE_INIT, &inputs),
            OverlayCacheVerdict::Restore { len: 4 }
        );
        for mode in [2i16, 3, 8, 0x18, 0x1A] {
            assert_eq!(
                overlay_cache_check(mode, &inputs),
                OverlayCacheVerdict::Skipped,
                "mode {mode:#x}"
            );
        }
    }

    #[test]
    fn a_missed_sum_reloads_the_battle_overlay() {
        let words = [7u32];
        let inputs = OverlayCacheInputs {
            cached: Some(&words),
            len: 4,
            expected: 8,
            dev_build: false,
        };
        assert_eq!(
            overlay_cache_check(MODE_BATTLE_INIT, &inputs),
            OverlayCacheVerdict::Reload {
                overlay: OVERLAY_BATTLE
            }
        );
    }

    #[test]
    fn the_dev_build_always_hits() {
        let words = [7u32];
        let inputs = OverlayCacheInputs {
            cached: Some(&words),
            len: 4,
            expected: 0xFFFF,
            dev_build: true,
        };
        assert_eq!(
            overlay_cache_check(MODE_BATTLE_INIT, &inputs),
            OverlayCacheVerdict::Restore { len: 4 }
        );
    }

    #[test]
    fn no_cached_image_skips_the_arm_entirely() {
        assert_eq!(
            overlay_cache_check(MODE_BATTLE_INIT, &empty()),
            OverlayCacheVerdict::Skipped
        );
    }

    #[test]
    fn the_snapshot_needs_both_a_listed_mode_and_the_field_as_the_outgoing_one() {
        for mode in SNAPSHOT_MODES {
            let p = mode_entry_prologue(mode, SNAPSHOT_FROM_MODE, (0x100, -0x200), &empty());
            let s = p.snapshot.expect("mode {mode:#x} should snapshot");
            assert_eq!(s.warp_x, 0x100);
            assert_eq!(s.warp_z, -0x200);
            assert_eq!(s.reentry_flag, REENTRY_FLAG);
            assert_eq!(s.stash_rect, ACTOR_POOL_STASH_RECT);

            // Coming from anywhere but the field, nothing is preserved.
            let p = mode_entry_prologue(mode, 0, (0x100, -0x200), &empty());
            assert!(p.snapshot.is_none());
        }
        for mode in [0i16, 1, 2, 3, 0x13, 0x16, 0x1C] {
            let p = mode_entry_prologue(mode, SNAPSHOT_FROM_MODE, (0, 0), &empty());
            assert!(p.snapshot.is_none(), "mode {mode:#x}");
        }
    }

    #[test]
    fn pacing_ring_is_always_cleared() {
        let p = mode_entry_prologue(0, 0, (0, 0), &empty());
        assert_eq!(p.pacing_ring_cleared, PACING_RING_LEN);
    }

    #[test]
    fn b9c4_is_kept_only_for_the_two_field_modes() {
        for mode in 0i16..=0x1B {
            let p = mode_entry_prologue(mode, 0, (0, 0), &empty());
            let keep = mode == 2 || mode == 3;
            assert_eq!(p.clears_b9c4, !keep, "mode {mode:#x}");
        }
    }
}
