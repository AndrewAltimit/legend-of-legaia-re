//! Per-channel arithmetic: the pitch-bend factor, pan attenuation and the effective velocity.

/// Center value of the 14-bit pitch-bend wheel - no bend.
pub(super) const PITCH_BEND_CENTER: u16 = 0x2000;

/// Convert a 14-bit pitch-bend wheel value into a pitch multiplier (`1.0` at
/// center) using the sounding tone's own bend range (semitones): `up` at
/// full-up wheel, `down` at full-down. The range is the VAB tone's
/// `pbmax`/`pbmin` (disc-sourced, per [`VabBank::pitch_bend_range`]), so a
/// tone with a `(0, 0)` range does not respond to the wheel at all - exactly
/// as libsnd applies the per-tone range rather than a global constant. Shared
/// by NoteOn (bend a fresh note) and the `0xEn` handler (re-bend sounding
/// notes).
pub(super) fn pitch_bend_factor(bend: u16, down: u8, up: u8) -> f64 {
    let norm = (bend as f64 - PITCH_BEND_CENTER as f64) / PITCH_BEND_CENTER as f64;
    let range = if norm >= 0.0 { up } else { down } as f64;
    let semitones = norm * range;
    2f64.powf(semitones / 12.0)
}

/// Scale a base SPU pitch register by a bend factor, clamped to the valid
/// `1..=0x3FFF` register range (the same clamp `compute_pitch` applies).
pub(super) fn bend_pitch(base: u16, factor: f64) -> u16 {
    ((base as f64 * factor).round() as i64).clamp(1, 0x3FFF) as u16
}

/// Center value of the 7-bit channel-pan controller (CC10) - no pan.
pub(super) const PAN_CENTER: u8 = 0x40;

/// Apply a channel pan (CC10, `0..=127`) to a voice's `(left, right)` volume
/// pair using libsnd's pan law: a pan left of center (`< 0x40`) attenuates
/// the **right** side by `pan/0x3f`; a pan right of center attenuates the
/// **left** side by `(0x7f - pan)/0x3f`. This is the per-pan-source stage the
/// libsnd voice-volume builder applies on top of the tone's own L/R (it runs
/// this same attenuation once per pan source - channel / sequence / tone).
/// The engine bakes the tone pan into the base L/R via `pan_split`, so this
/// adds only the channel pan that `play_note` does not see.
pub(super) fn apply_channel_pan(left: i16, right: i16, pan: u8) -> (i16, i16) {
    let pan = pan.min(0x7f) as i32;
    if pan < 0x40 {
        (left, (right as i32 * pan / 0x3f) as i16)
    } else {
        ((left as i32 * (0x7f - pan) / 0x3f) as i16, right)
    }
}

/// The sequencer-path tail of `FUN_80067550` over a head
/// [`VabBank::seq_note_volume`] already carried through the sequence volume
/// and the tone and program pans: the channel pan (the staged `0x801CE34D`),
/// then the square taper `v * v / 0x3FFF` per side. Retail's SFX path (owner
/// `0x21`) skips the taper; this is the sequencer's.
pub(super) fn channel_tail(head: (i16, i16), pan: u8) -> (i16, i16) {
    let (l, r) = apply_channel_pan(head.0, head.1, pan);
    let sq = |v: i16| ((v as i32 * v as i32) / 0x3FFF) as i16;
    (sq(l), sq(r))
}

/// Channel volume (CC7) folded into a note's velocity the way libsnd does
/// it at note-on: `FUN_80066308` stages `vel * chvol / 127` (the channel
/// record's `+0x60 + ch * 2` halfword, signed `/0x7F` magic at
/// `0x800663E4..0x80066428`) as the velocity the volume chain then reads.
pub(super) fn effective_velocity(velocity: u8, channel_volume: u8) -> u8 {
    (velocity as i32 * channel_volume.min(127) as i32 / 127) as u8
}
