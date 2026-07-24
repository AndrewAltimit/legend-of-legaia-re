//! The field overlay's audio-slot release: stop the two SPU voices the field
//! reserves for its streamed cue, free the SEQ resource slot behind it, and
//! clear the two globals that track it.
//!
//! REF: FUN_800653C8, FUN_8001FF58
//!
//! The port of `FUN_801d8450` itself is [`field_audio_release_steps`], which
//! carries the `PORT` tag and its own wiring disclosure. The tag deliberately
//! does **not** repeat at module level: a `//!  PORT:` line makes the whole
//! file a second, coarser anchor for the same address, and that anchor has no
//! disclosure of its own.
//!
//! # Provenance, and a correction
//!
//! `FUN_801d8450` lives in the field overlay (PROT entry `0897_xxx_dat`,
//! slot-A base `0x801CE818`, file offset `0x9C38`). It is a real, callable
//! entry: 25 instructions opening `addiu sp, sp, -0x20` and closing
//! `jr ra / addiu sp, sp, 0x20` at `0x801D84AC`, and the image contains a
//! `jal 0x801D8450` site.
//!
//! The standalone dump `ghidra/scripts/funcs/overlay_0897_801d8450.txt` shows
//! something else entirely - a frameless fragment opening `lh a0, 0xc(s7)` -
//! and that fragment is what an earlier pass read as evidence that the address
//! is interior to a dispatcher. It is a wrong-base import: the extracted
//! `0897_xxx_dat.BIN` bytes at file `0x9C38` do not contain those
//! instructions. Ported here from the disc bytes, not from that dump.
//!
//! # Body
//!
//! ```text
//! s0 = 0; s1 = 0x17
//! do { FUN_800653C8((s16)(s1 - s0)); s0 += 1; } while (s0 < 2)
//! FUN_8001FF58(6)
//! *(u32 *)0x8007BA88 = 0
//! *(u32 *)0x8007BAFC = 0
//! ```
//!
//! `FUN_800653C8(voice)` is the sound driver's voice stop - the same primitive
//! the sustained-SFX teardown `FUN_80017910` and the debug sound test's
//! stop-all use; it rejects any index `>= 0x18`, which is why the loop counts
//! *down* from `0x17` rather than up. `FUN_8001FF58(slot)` is the SEQ
//! resource-slot release keyed on the 12-byte-stride table at `0x80091508`.
//!
//! So the field reserves the **top two** of the 24 SPU voices (`0x17` and
//! `0x16`) plus SEQ resource slot `6`, and this routine hands all three back.

/// The number of SPU voices the field's streamed cue holds.
pub const FIELD_VOICE_COUNT: u16 = 2;
/// Highest SPU voice index the field cue uses; the loop counts down from here.
pub const FIELD_TOP_VOICE: u16 = 0x17;
/// SEQ resource slot the field cue owns.
pub const FIELD_SEQ_SLOT: u16 = 6;
/// First global the routine clears (`_DAT_8007BA88`).
pub const FIELD_CUE_GLOBAL_A: u32 = 0x8007_BA88;
/// Second global the routine clears (`_DAT_8007BAFC`).
pub const FIELD_CUE_GLOBAL_B: u32 = 0x8007_BAFC;

/// The teardown steps, in the order retail performs them, as data - so a host
/// can replay them against whatever mixer and resource table it has without
/// this module depending on either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseStep {
    /// `FUN_800653C8(voice)` - stop one SPU voice.
    StopVoice(u16),
    /// `FUN_8001FF58(slot)` - release one SEQ resource slot.
    ReleaseSeqSlot(u16),
    /// Zero one 32-bit global.
    ClearGlobal(u32),
}

/// Build the release sequence.
///
/// PORT: FUN_801d8450
// NOT WIRED: `engine-audio`'s mixer has no per-voice stop keyed on the retail
// voice index and `engine-core` does not model the `0x80091508` SEQ resource
// table, so there is no host root to call this from yet.
pub fn field_audio_release_steps() -> Vec<ReleaseStep> {
    let mut steps = Vec::with_capacity(FIELD_VOICE_COUNT as usize + 3);
    for i in 0..FIELD_VOICE_COUNT {
        steps.push(ReleaseStep::StopVoice(FIELD_TOP_VOICE - i));
    }
    steps.push(ReleaseStep::ReleaseSeqSlot(FIELD_SEQ_SLOT));
    steps.push(ReleaseStep::ClearGlobal(FIELD_CUE_GLOBAL_A));
    steps.push(ReleaseStep::ClearGlobal(FIELD_CUE_GLOBAL_B));
    steps
}

/// The voice indices the field cue holds, highest first - the order the retail
/// loop stops them in.
pub fn field_cue_voices() -> Vec<u16> {
    (0..FIELD_VOICE_COUNT)
        .map(|i| FIELD_TOP_VOICE - i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voices_are_the_top_two_counting_down() {
        assert_eq!(field_cue_voices(), vec![0x17, 0x16]);
    }

    #[test]
    fn every_voice_is_inside_the_drivers_24_voice_bound() {
        // `FUN_800653C8` rejects `voice >= 0x18` outright.
        assert!(field_cue_voices().iter().all(|&v| v < 0x18));
    }

    #[test]
    fn step_order_matches_the_body() {
        assert_eq!(
            field_audio_release_steps(),
            vec![
                ReleaseStep::StopVoice(0x17),
                ReleaseStep::StopVoice(0x16),
                ReleaseStep::ReleaseSeqSlot(6),
                ReleaseStep::ClearGlobal(FIELD_CUE_GLOBAL_A),
                ReleaseStep::ClearGlobal(FIELD_CUE_GLOBAL_B),
            ]
        );
    }
}
