//! The pending sound release: arm, take and detach.

use super::*;

impl World {
    /// Arm the timed sound-source auto-release for `deadline` vsyncs
    /// (`gp+0x814`). [`Self::tick`] counts it down by the frame step.
    ///
    /// Retail's arm half writes five `gp` cells, not three: on top of the
    /// timer's armed flag (`gp+0x808`) / deadline (`gp+0x814`) / elapsed
    /// (`gp+0x81C`) it latches the caller's tag (`gp+0x810`) and the live
    /// **audio level** `_DAT_8007B910` (`lw a1,-0x46f0(a1)` at `0x800267B0`
    /// into `gp+0x80C`), then tail-calls the libsnd volume shim
    /// `FUN_80062004(*(i16*)0x80070536, (level << 15) >> 16, deadline | 1)`
    /// (`0x800267E4`). Those two extra cells land in [`crate::world::AudioState::sound_arm`] so a
    /// host driving the shim has the exact arguments; the engine has no live
    /// field-mode volume ramp of its own, so the latched level is the value
    /// retail's MAN loader rests `_DAT_8007B910` on at every scene load - the
    /// configured level `_DAT_8008457C` ([`crate::world::AudioState::levels`]),
    /// `0xD7` from a cold boot and a loaded save's own word after a load.
    ///
    /// PORT: FUN_800267A8
    /// REF: FUN_800267FC, FUN_80062004
    pub fn arm_sound_release(&mut self, deadline_vsyncs: i32) {
        self.audio.sound_release.arm(deadline_vsyncs);
        self.audio.pending_sound_release = false;
        self.audio.sound_arm = Some(legaia_engine_system::scus_leaf_kernels::TimedSoundArm::arm(
            0,
            deadline_vsyncs.max(0) as u32,
            self.audio.levels.configured_level,
        ));
    }

    /// Drain the "the sound-release deadline expired" event.
    pub fn take_pending_sound_release(&mut self) -> bool {
        std::mem::take(&mut self.audio.pending_sound_release)
    }

    /// Run the one-shot sound setup `FUN_8002689C`. Returns `true` only on
    /// the first call - retail's `gp+0x804` latch gates every later one out,
    /// which is why the mode-INIT chain can call it freely.
    ///
    /// Nothing is detached: behind the latch retail makes two volume calls,
    /// the cmd-6 SPU command `FUN_80065440(0x32, 0x32)` and the master
    /// volume `SsSetMVol` `FUN_80062AA0(0x7F, 0x7F)`. This port models the
    /// latch only; the two volume writes are left to the audio host's own
    /// defaults. (The function keeps the name an earlier reading gave it.)
    ///
    /// PORT: FUN_8002689c
    pub fn detach_sound(&mut self) -> bool {
        self.audio.sound_detach.detach()
    }
}
