//! Sustained-SFX voice teardown + the global mode-cell setter.
//!
//! PORT: FUN_80017910, FUN_8003C110
//! REF: FUN_800653C8, FUN_80016B6C, FUN_8001DCF8, FUN_8004AD80, FUN_80067480,
//!      FUN_800353E0, FUN_801DD35C, FUN_80065034
//!
//! `FUN_80017910` is the retail teardown for the *sustained* sound-effect
//! voices: if the held-voice count at `gp+0x5D0` is non-zero it calls the
//! driver voice-stop `FUN_800653C8(7 + i)` for each held voice (base voice 7,
//! ids ascending), then clears the count and writes `-1` into the
//! current-sustained-cue cell at `gp+0x40C`. When the count is already zero
//! the function does nothing at all - the cue cell is *not* re-invalidated.
//!
//! Despite an older "scene overlay-slot teardown" label, both cells belong to
//! the SFX pipeline: the only other writer of `gp+0x5D0`/`gp+0x40C` in the
//! dumped corpus is the SFX cue-ring drainer `FUN_80016B6C`, which - when a
//! sustained cue starts - runs the *same* release loop over the previously
//! held voices, keys the new cue's voices on from voice 7 upward
//! (`FUN_80065034` per voice), stores the new count (masked `& 0x1F`) into
//! `gp+0x5D0` and the cue into `gp+0x40C`. `FUN_800653C8` itself bounds its
//! argument at `0x18` (= the 24 SPU voices) and stages it as sound-driver
//! command 0 through `FUN_80067480` - a voice stop, not an overlay-slot free.
//!
//! Retail call sites of the teardown: the boot/mode initializer
//! `FUN_8001DCF8` (mode entry - the timing [`SceneHost::load_scene`]
//! mirrors), the battle anim commit `FUN_8004AD80` (when the actor's anim id
//! changes, `+0x1DB != +0x1DA`, the per-anim sustained cue is stopped), and
//! the debug sound-test "stop all" (teardown followed by an explicit
//! `FUN_800653C8(0..0x18)` sweep).
//!
//! `FUN_8003C110` is the one-instruction companion setter: a *byte* store
//! (`sb`) of its argument into the global `DAT_80073F20`. Observed writers:
//! the field-subsystem state reset `FUN_800353E0` sets it to `0x0C`, the menu
//! dispatcher `FUN_801DD35C` to `0x10`.

use super::*;

/// First SPU voice of the sustained-SFX bank. `FUN_80017910` and the
/// cue-ring drainer both start their loops at `lui 0x7 >> 16` = voice 7.
pub const SUSTAINED_BASE_VOICE: u16 = 7;

/// SPU voice count - `FUN_800653C8` rejects ids `>= 0x18` (returns -1
/// without issuing the driver command).
pub const SPU_VOICE_COUNT: usize = 0x18;

/// Engine model of the retail sustained-SFX voice bookkeeping
/// (`gp+0x5D0` held count / `gp+0x40C` current cue) plus a local 24-slot
/// voice table standing in for the SPU driver's voice state (the engine has
/// no live sound-driver command queue to free into; the table keeps the
/// release honest without inventing a fake allocator).
#[derive(Debug, Clone)]
pub struct SustainedSfx {
    /// `gp+0x5D0` - number of sustained voices currently held, allocated
    /// upward from [`SUSTAINED_BASE_VOICE`]. The retail writer (the cue-ring
    /// drainer) masks the stored count `& 0x1F`.
    held_count: i32,
    /// `gp+0x40C` - the current sustained cue; `-1` = none. Write-mostly
    /// latch in the dumped corpus (set by the drainer, invalidated by the
    /// teardown).
    active_cue: i32,
    /// Local stand-in for the driver's per-voice active state.
    voice_active: [bool; SPU_VOICE_COUNT],
}

impl Default for SustainedSfx {
    fn default() -> Self {
        Self::new()
    }
}

impl SustainedSfx {
    pub fn new() -> Self {
        Self {
            held_count: 0,
            active_cue: -1,
            voice_active: [false; SPU_VOICE_COUNT],
        }
    }

    /// Mirror of the cue-ring drainer's key-on side (`FUN_80016B6C`,
    /// sustained branch): releases nothing here - retail's drainer performs
    /// its own release loop first, engine callers use [`Self::release`] -
    /// just stores the new count (masked `& 0x1F` like the retail store) +
    /// cue and marks voices `7..7+count` active in the local table (ids past
    /// the 24-voice bound are rejected by the driver and stay inactive).
    pub fn key_on(&mut self, cue: i32, count: u32) {
        let count = (count & 0x1F) as i32;
        for i in 0..count {
            let id = SUSTAINED_BASE_VOICE as usize + i as usize;
            if id < SPU_VOICE_COUNT {
                self.voice_active[id] = true;
            }
        }
        self.held_count = count;
        self.active_cue = cue;
    }

    /// The `FUN_80017910` teardown. If any sustained voices are held, stops
    /// each one in ascending order (`SUSTAINED_BASE_VOICE + i`), clears the
    /// held count, and invalidates the cue cell to `-1` - in that retail
    /// order. Returns the voice ids handed to the stop primitive, in call
    /// order (retail passes ids past the 24-voice bound too; the driver
    /// rejects those, mirrored here by the bounds check in
    /// [`Self::stop_voice`]).
    ///
    /// Zero held voices = complete no-op (retail early-outs before the cue
    /// invalidation): returns an empty list, `active_cue` untouched.
    pub fn release(&mut self) -> Vec<u16> {
        if self.held_count == 0 {
            return Vec::new();
        }
        let mut stopped = Vec::new();
        // `blez` guard: a (never observed) negative count skips the loop but
        // still clears the count and invalidates the cue.
        if self.held_count > 0 {
            for i in 0..self.held_count {
                let id = SUSTAINED_BASE_VOICE + i as u16;
                self.stop_voice(id);
                stopped.push(id);
            }
        }
        self.held_count = 0;
        self.active_cue = -1;
        stopped
    }

    /// Local model of the driver voice-stop `FUN_800653C8`: validates
    /// `id < 0x18` (retail returns -1 for out-of-range ids without issuing
    /// the command), clears the voice's active flag. The driver-command
    /// submission (`FUN_80067480` command 0 behind a busy flag) is
    /// host-replaced by the table clear. Returns `true` when the id was in
    /// range.
    pub fn stop_voice(&mut self, id: u16) -> bool {
        let Some(slot) = self.voice_active.get_mut(id as usize) else {
            return false;
        };
        *slot = false;
        true
    }

    /// Held sustained-voice count (`gp+0x5D0`).
    pub fn held_count(&self) -> i32 {
        self.held_count
    }

    /// Current sustained cue (`gp+0x40C`); `-1` = none.
    pub fn active_cue(&self) -> i32 {
        self.active_cue
    }

    /// Whether the local voice table marks `id` active. `false` for ids past
    /// the 24-voice bound.
    pub fn voice_active(&self, id: u16) -> bool {
        self.voice_active.get(id as usize).copied().unwrap_or(false)
    }
}

impl SceneHost {
    /// Release every held sustained-SFX voice - the `FUN_80017910` teardown
    /// over [`SceneHost::sustained_sfx`]. Returns the stopped voice ids in
    /// call order. Called by [`SceneHost::load_scene`] (mirroring the
    /// mode-init site `FUN_8001DCF8`); engines driving battle anim
    /// transitions call it directly (the `FUN_8004AD80` site).
    pub fn release_sustained_sfx(&mut self) -> Vec<u16> {
        self.sustained_sfx.release()
    }

    /// Set the global mode cell (`DAT_80073F20`, a byte) - the
    /// `FUN_8003C110` setter. Retail-observed values: `0x0C` from the
    /// field-subsystem reset, `0x10` from the menu dispatcher.
    pub fn set_mode_cell(&mut self, value: u8) {
        self.mode_cell = value;
    }

    /// Current value of the global mode cell (`DAT_80073F20`). Zero until a
    /// setter runs (retail BSS default).
    pub fn mode_cell(&self) -> u8 {
        self.mode_cell
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_frees_held_voices_in_order_then_clears_and_invalidates() {
        let mut s = SustainedSfx::new();
        s.key_on(0x42, 3);
        assert_eq!(s.held_count(), 3);
        assert_eq!(s.active_cue(), 0x42);
        assert!(s.voice_active(7) && s.voice_active(8) && s.voice_active(9));

        let stopped = s.release();
        assert_eq!(stopped, vec![7, 8, 9], "base voice 7, ids ascending");
        assert_eq!(s.held_count(), 0, "count cleared");
        assert_eq!(s.active_cue(), -1, "cue cell invalidated to -1");
        assert!(!s.voice_active(7) && !s.voice_active(8) && !s.voice_active(9));
    }

    #[test]
    fn release_with_zero_held_is_a_complete_no_op() {
        let mut s = SustainedSfx::new();
        assert!(s.release().is_empty(), "no frees issued");
        assert_eq!(s.held_count(), 0);
        assert_eq!(s.active_cue(), -1);

        // Retail early-outs on count == 0 *before* the cue invalidation:
        // release-then-release again must not issue further stops.
        s.key_on(9, 1);
        assert_eq!(s.release(), vec![7]);
        assert!(s.release().is_empty(), "second release is a no-op");
    }

    #[test]
    fn release_passes_out_of_range_ids_that_the_driver_rejects() {
        // The drainer's count store is masked & 0x1F, so counts up to 31 are
        // representable even though only voices 7..24 exist. Retail's loop
        // still calls the stop primitive for every id; FUN_800653C8 rejects
        // ids >= 0x18.
        let mut s = SustainedSfx::new();
        s.key_on(1, 20); // voices 7..27 requested; 24..27 never activate
        assert!(s.voice_active(23));
        assert!(!s.voice_active(24), "past the 24-voice driver bound");

        let stopped = s.release();
        assert_eq!(stopped.len(), 20);
        assert_eq!(stopped.first(), Some(&7));
        assert_eq!(stopped.last(), Some(&26), "ids passed even past bound");
        assert_eq!(s.held_count(), 0);
        assert_eq!(s.active_cue(), -1);
    }

    #[test]
    fn key_on_masks_count_like_the_retail_store() {
        let mut s = SustainedSfx::new();
        s.key_on(2, 0x25); // & 0x1F = 5
        assert_eq!(s.held_count(), 5);
        assert_eq!(s.release(), vec![7, 8, 9, 10, 11]);
    }

    #[test]
    fn stop_voice_bounds_at_the_24_voice_driver_limit() {
        let mut s = SustainedSfx::new();
        assert!(s.stop_voice(0));
        assert!(s.stop_voice(0x17));
        assert!(!s.stop_voice(0x18), "FUN_800653C8 rejects ids >= 0x18");
    }
}
