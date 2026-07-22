//! Global sound-system state the frame-begin driver services.
//!
//! `FUN_8001698C` (the frame-begin driver - see
//! [`crate::world::World::take_frame_begin_skip`] and
//! `legaia_engine_audio::sfx_ring`) calls two small SCUS kernels every frame
//! before anything else in the frame runs. Both are ported here.
//!
//! PORT: FUN_800267fc - the timed sound-source auto-release.
//! PORT: FUN_8002689c - the one-shot sound detach.
//!
//! The libsnd calls both of them end in are out of clean-room scope; what is
//! portable is the **scheduling**, which is where the behaviour lives.
//! REF: FUN_80065440, FUN_80062AA0, FUN_8002657C, FUN_80064370
//! REF: FUN_8001698C

/// Timed auto-release of the bound sound source (`FUN_800267FC`).
///
/// Retail keeps three `gp`-relative cells for this:
///
/// | Cell | Meaning |
/// |---|---|
/// | `gp+0x808` | armed flag - zero makes the whole function a no-op |
/// | `gp+0x814` | deadline, in vsyncs |
/// | `gp+0x81C` | elapsed, in vsyncs |
///
/// Every frame, while armed: if `elapsed < deadline` the elapsed count
/// advances by the **adaptive frame step** `DAT_1F800393` (`lbu v0,0x393(v0)`
/// at `0x8002687C`) and nothing else happens. Otherwise the flag clears and
/// the release fires.
///
/// The `subu`/`bgez` pair at `0x8002681C` compares `deadline - elapsed >= 0`,
/// so the release fires on the frame elapsed first **reaches** the deadline,
/// not the frame after. Denominating the accumulator in the frame step is the
/// same cadence-invariance every other retail duration has: a deadline of 60
/// expires after 60 vsyncs at any cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SoundReleaseTimer {
    /// `gp+0x808`. Zero = disarmed.
    pub armed: bool,
    /// `gp+0x814`, vsyncs.
    pub deadline: i32,
    /// `gp+0x81C`, vsyncs.
    pub elapsed: i32,
}

/// What one [`SoundReleaseTimer::tick`] resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundReleaseTick {
    /// Not armed - the function returned immediately.
    Idle,
    /// Still counting; carries the new elapsed total.
    Counting { elapsed: i32 },
    /// The deadline was reached this frame. The timer is now disarmed.
    ///
    /// `release_voice` is `true` only when retail would actually have run the
    /// teardown: the bound sound-source record's active halfword (`+8` of the
    /// record at `0x8007052C`) is non-zero **and** the field/dual-mode gate
    /// `_DAT_8007B868` is zero. When it is `false` the flag still clears - the
    /// timer disarms either way.
    Fired { release_voice: bool },
}

impl SoundReleaseTimer {
    /// Arm the timer for `deadline` vsyncs from now.
    pub fn arm(&mut self, deadline: i32) {
        self.armed = true;
        self.deadline = deadline;
        self.elapsed = 0;
    }

    /// One frame of `FUN_800267FC`.
    ///
    /// `frame_step` is `DAT_1F800393`; `source_active` is the record's `+8`
    /// halfword being non-zero; `dual_mode_gate` is `_DAT_8007B868` (retail's
    /// field path holds it at zero, which is when the teardown runs).
    pub fn tick(
        &mut self,
        frame_step: u8,
        source_active: bool,
        dual_mode_gate: bool,
    ) -> SoundReleaseTick {
        if !self.armed {
            return SoundReleaseTick::Idle;
        }
        // 0x8002681C: `deadline - elapsed >= 0` keeps counting.
        if self.deadline - self.elapsed >= 0 {
            self.elapsed += i32::from(frame_step);
            return SoundReleaseTick::Counting {
                elapsed: self.elapsed,
            };
        }
        // 0x80026830: the flag clears before the gates are even read.
        self.armed = false;
        SoundReleaseTick::Fired {
            release_voice: source_active && !dual_mode_gate,
        }
    }
}

/// The master volume `FUN_8002689C` installs, on both the single-shot SPU
/// command (`FUN_80065440(0x32, 0x32)`) and the SsAPI master
/// (`FUN_80062AA0(0x7F, 0x7F)`).
///
/// Both take the same value in each of their two arguments - the `move a1,a0`
/// in each delay slot, which the decompiled C renders as a single-argument
/// call (the "dropped register arguments" artifact).
pub const DETACH_SPU_LEVEL: u8 = 0x32;
/// Master volume set alongside [`DETACH_SPU_LEVEL`].
pub const DETACH_MASTER_VOLUME: u8 = 0x7F;

/// The one-shot sound detach (`FUN_8002689C`), reduced to its portable part:
/// an **idempotent latch**.
///
/// Retail gates the whole body on `gp+0x804`; a non-zero value returns
/// immediately, so the two volume writes happen exactly once however many
/// times the mode-INIT chain calls it. `FUN_80025C68` (mode 0 CONFIG INIT)
/// is the caller that made this look like an unconditional teardown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SoundDetachLatch {
    detached: bool,
}

impl SoundDetachLatch {
    /// Run the detach. Returns `true` the first time only.
    pub fn detach(&mut self) -> bool {
        if self.detached {
            return false;
        }
        self.detached = true;
        true
    }

    /// Whether the detach has already run (`gp+0x804`).
    pub fn is_detached(&self) -> bool {
        self.detached
    }

    /// Clear the latch. No retail writer of `gp+0x804` back to zero is known
    /// in the dumped corpus; provided so a host can reset between runs.
    pub fn reset(&mut self) {
        self.detached = false;
    }
}

/// The double-buffered DISPENV/DRAWENV pair initialiser (`FUN_80020038`).
///
/// PORT: FUN_80020038
///
/// Six instructions, three stores, all relative to the **pair** base - the
/// same `0x8007BF30 + 0x74 * index` records the frame-end driver swaps:
///
/// | Store | Pair offset | DRAWENV field |
/// |---|---|---|
/// | `sh 0x1F,0x28(a0)` | `+0x28` | `tpage` (DRAWENV `+0x14`) |
/// | `sb 0,0x2a(a0)` | `+0x2A` | `dtd` - **dither off** (DRAWENV `+0x16`) |
/// | `sb 1,0x2c(a0)` | `+0x2C` | `isbg` - clear-on-`PutDrawEnv` (DRAWENV `+0x18`) |
///
/// The `dtd` slot is the interesting one, because it is not left at this
/// value: `FUN_80016B6C` refreshes it from the global `_DAT_8007BA66` every
/// frame (`lbu` then `sb v1,0x2a(v0)` at `0x80017208`/`0x80017210`), boot
/// installs `1` there (`sh s2,-0x459a(at)` with `s2 = 1` in `FUN_8001D424`),
/// and a field-VM opcode slice at `0x801E350C` overwrites it from a one-byte
/// script operand. So **retail dither is on by default and script-controlled**
/// per scene - the `0` here is only the pre-first-frame state.
///
/// (The engine's own rasteriser defaults the other way: dither is off unless
/// `Renderer::set_psx_mode` is enabled. That is a deliberate project default,
/// not a claim about retail - see the render split in `CLAUDE.md`.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawEnvInit {
    /// `+0x28` - DRAWENV `tpage`.
    pub tpage: u16,
    /// `+0x2A` - DRAWENV `dtd` (dither).
    pub dither: bool,
    /// `+0x2C` - DRAWENV `isbg` (clear the frame on `PutDrawEnv`).
    pub clear_background: bool,
}

/// The literal values `FUN_80020038` stores.
pub const DRAW_ENV_INIT: DrawEnvInit = DrawEnvInit {
    tpage: 0x1F,
    dither: false,
    clear_background: true,
};

/// Retail's boot value of the per-frame dither source `_DAT_8007BA66`
/// (`FUN_8001D424`, `sh s2,-0x459a(at)` with `s2 = 1`).
pub const DITHER_BOOT_VALUE: bool = true;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disarmed_timer_is_a_complete_no_op() {
        let mut t = SoundReleaseTimer::default();
        assert_eq!(t.tick(1, true, false), SoundReleaseTick::Idle);
    }

    #[test]
    fn the_release_deadline_is_denominated_in_vsyncs() {
        let mut t = SoundReleaseTimer::default();
        t.arm(4);
        // Cadence 2: two ticks cover the four vsyncs.
        assert_eq!(
            t.tick(2, true, false),
            SoundReleaseTick::Counting { elapsed: 2 }
        );
        assert_eq!(
            t.tick(2, true, false),
            SoundReleaseTick::Counting { elapsed: 4 }
        );
        // elapsed == deadline still counts (the `>= 0` comparison), so the
        // frame after is the one that fires.
        assert_eq!(
            t.tick(2, true, false),
            SoundReleaseTick::Counting { elapsed: 6 }
        );
        assert_eq!(
            t.tick(2, true, false),
            SoundReleaseTick::Fired {
                release_voice: true
            }
        );
        assert!(!t.armed, "the flag clears on the firing frame");
    }

    #[test]
    fn the_gates_suppress_the_teardown_but_not_the_disarm() {
        let mut t = SoundReleaseTimer::default();
        t.arm(0);
        // deadline 0 - elapsed 0 == 0, still counting.
        assert!(matches!(
            t.tick(1, false, false),
            SoundReleaseTick::Counting { .. }
        ));
        assert_eq!(
            t.tick(1, false, false),
            SoundReleaseTick::Fired {
                release_voice: false
            }
        );
        assert!(!t.armed);

        // The dual-mode gate suppresses it too.
        let mut t = SoundReleaseTimer::default();
        t.arm(0);
        t.tick(1, true, true);
        assert_eq!(
            t.tick(1, true, true),
            SoundReleaseTick::Fired {
                release_voice: false
            }
        );
    }

    #[test]
    fn the_sound_detach_runs_exactly_once() {
        let mut l = SoundDetachLatch::default();
        assert!(l.detach());
        assert!(l.is_detached());
        assert!(!l.detach(), "gp+0x804 gates the second call out entirely");
        l.reset();
        assert!(l.detach());
    }

    #[test]
    fn draw_env_init_matches_the_three_retail_stores() {
        // The three literal stores, plus the boot value the frame driver
        // then overwrites `dtd` with.
        assert_eq!(
            (
                DRAW_ENV_INIT.tpage,
                DRAW_ENV_INIT.dither,
                DRAW_ENV_INIT.clear_background,
                DITHER_BOOT_VALUE,
            ),
            (0x1F, false, true, true),
            "sh 0x1F,0x28 / sb 0,0x2a / sb 1,0x2c; boot sh 1 -> 0x8007BA66"
        );
    }
}
