//! The field overlay's slot-6 release: stop the top two SPU voices, close VAB
//! slot 6 (the field bank, or a side-band bank streamed over it), and clear
//! the forced-channel latch and the field-bank latch.
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
//! *down* from `0x17` rather than up. Voices `0x17` / `0x16` are the first two
//! the one-shot cue drainer `FUN_80016B6C` keys (`23 - cursor`).
//!
//! `FUN_8001FF58(slot)` is a **VAB** close, not a SEQ release: it tests the
//! 12-byte mixer record at `0x80091508 + slot*12`'s `+0xB` enable byte, clears
//! it, and passes the record's `+8` VAB id to `FUN_80068C80` (`SsVabClose`:
//! `SpuFree` the bank, clear its open-state byte). Record 6's VAB is the field
//! bank PROT 0876 the field init loads, or a side-band bank a `>= 3000`
//! request streamed over it. `0x8007BAFC` is the field-bank latch: the field
//! init loads PROT 0876 into slot 6 only while it is clear
//! (`0x801D6FF4..0x801D7028`), so this routine is what makes the next field
//! init reload the field bank. `_DAT_8007BA88` is the cue drainer's
//! forced-channel latch. See `crate::world::SfxBankResidency`.

/// The number of SPU voices the field's streamed cue holds.
pub const FIELD_VOICE_COUNT: u16 = 2;
/// Highest SPU voice index the field cue uses; the loop counts down from here.
pub const FIELD_TOP_VOICE: u16 = 0x17;
/// VAB slot the routine closes - the field bank's.
pub const FIELD_VAB_SLOT: u16 = 6;
/// First global the routine clears (`_DAT_8007BA88`, the forced-channel latch).
pub const FIELD_CUE_GLOBAL_A: u32 = 0x8007_BA88;
/// Second global the routine clears (`_DAT_8007BAFC`, the field-bank latch).
pub const FIELD_CUE_GLOBAL_B: u32 = 0x8007_BAFC;

/// The teardown steps, in the order retail performs them, as data - so a host
/// can replay them against whatever mixer and resource table it has without
/// this module depending on either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseStep {
    /// `FUN_800653C8(voice)` - stop one SPU voice.
    StopVoice(u16),
    /// `FUN_8001FF58(slot)` - close one VAB slot.
    ReleaseVabSlot(u16),
    /// Zero one 32-bit global.
    ClearGlobal(u32),
}

/// Build the release sequence.
///
/// PORT: FUN_801d8450 - field-VM op `0x36` sub `3` replays these steps
/// through `World::release_field_audio` (`world/vm_hosts.rs`): the voice stops
/// reach both play hosts' SPU, the slot-6 close and the latch clear reach the
/// slot-2 / slot-6 residency the hosts restage from.
pub fn field_audio_release_steps() -> Vec<ReleaseStep> {
    let mut steps = Vec::with_capacity(FIELD_VOICE_COUNT as usize + 3);
    for i in 0..FIELD_VOICE_COUNT {
        steps.push(ReleaseStep::StopVoice(FIELD_TOP_VOICE - i));
    }
    steps.push(ReleaseStep::ReleaseVabSlot(FIELD_VAB_SLOT));
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
                ReleaseStep::ReleaseVabSlot(6),
                ReleaseStep::ClearGlobal(FIELD_CUE_GLOBAL_A),
                ReleaseStep::ClearGlobal(FIELD_CUE_GLOBAL_B),
            ]
        );
    }
}
