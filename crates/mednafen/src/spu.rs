//! Typed accessors over the PSX `SPU` section.
//!
//! Mednafen's PSX module stores the full 512 KiB of SPU RAM under the `SPU`
//! section as the sub-entry `SPURAM`, alongside the 24 per-voice register
//! snapshots and the global SPU register file (master volume sweep, reverb,
//! noise, voice-on/off masks). This module mirrors the shape of [`PsxGpu`]
//! and exposes the entries Legaia audio-parity work actually needs: SPU RAM
//! bytes, per-voice state snapshots, key-on/-off masks, master volume, and
//! reverb mode.
//!
//! Voice-state sub-entry naming follows mednafen's internal layout exactly,
//! e.g. `Voices[7].StartAddr`, `Voices[7].Pitch`, `Voices[7].ADSR.Phase`,
//! `(Voices[7].Sweep[0]).Current`. The 24-voice array is `Voices[0]..Voices[23]`.
//! Master volume is stored as the *current accumulated* output of mednafen's
//! global sweep registers (`(GlobalSweep[0]).Current` for left,
//! `(GlobalSweep[1]).Current` for right) - i.e. what the SPU actually
//! emitted on the last cycle, not the libspu MVOL write that drives the
//! sweep target.
//!
//! ADSR phase values are mednafen's internal enum (4-byte u32 each); we
//! expose them as raw `u32` rather than re-mapping to the engine-audio
//! [`crate::engine_audio::spu::adsr::Phase`] - downstream code does the
//! cross-walk because mednafen and the engine-audio model disagree on the
//! "off" representation (mednafen tracks per-phase release sub-states that
//! we treat as plain `Release`).

use crate::container::SaveState;

/// Number of hardware voices on the PSX SPU (matches
/// `engine_audio::spu::NUM_VOICES`).
pub const NUM_VOICES: usize = 24;

/// Size of the PSX SPU RAM blob (512 KiB).
pub const SPU_RAM_BYTES: usize = 512 * 1024;

/// Snapshot of one PSX voice at the moment of capture. Fields are populated
/// lazily by [`PsxSpu::voice_state`] - if the underlying sub-entry is
/// missing the field stays `None`. This makes the accessor robust against
/// older mednafen save-state schemas that may not carry every register.
#[derive(Debug, Clone, Default)]
pub struct SpuVoiceState {
    /// SPU RAM address where this voice's ADPCM stream starts.
    pub start_addr: Option<u32>,
    /// Current SPU RAM address (where the decoder will read the next
    /// 16-byte block). Equal to `start_addr` immediately after key-on,
    /// then advances forward through the stream.
    pub cur_addr: Option<u32>,
    /// Latched loop-back address. PSX hardware records this on the first
    /// ADPCM block that carries the `loop_start` flag.
    pub loop_addr: Option<u32>,
    /// Pitch counter (libspu unit: `0x1000 = 1.0x` sample rate).
    pub pitch: Option<u16>,
    /// Fractional sample position within the current 28-sample ADPCM block,
    /// in mednafen's internal `CurPhase` representation (4-byte u32).
    pub cur_phase: Option<u32>,
    /// ADSR envelope phase enum (mednafen-internal; non-zero typically
    /// means the voice is audible). Engine-side convergence treats any
    /// non-zero value as "voice is active".
    pub adsr_phase: Option<u32>,
    /// Current ADSR envelope level (0..=0x7FFF).
    pub adsr_env_level: Option<u16>,
    /// Voice left output volume (current value of `Sweep[0]`). PSX hardware
    /// drives this through a sweep register; this is what the SPU emitted
    /// on the last cycle.
    pub vol_left: Option<i16>,
    /// Voice right output volume (current value of `Sweep[1]`).
    pub vol_right: Option<i16>,
    /// Raw 32-bit ADSR-control word as stored by mednafen
    /// (`Voices[N].ADSRControl`). The libspu `(adsr1, adsr2)` words can be
    /// recovered as `(low_u16, high_u16)`.
    pub adsr_control: Option<u32>,
}

impl SpuVoiceState {
    /// Cheap "is this voice currently audible" predicate. True when the
    /// ADSR phase is non-zero (mednafen's `Off` phase is `0`). Used by the
    /// audio-trace oracle's convergence check: "engine has at least one
    /// frame where the same voice indices are active as retail".
    pub fn is_active(&self) -> bool {
        matches!(self.adsr_phase, Some(p) if p != 0)
    }
}

/// Top-level helper over the `SPU` section. Mirrors [`PsxGpu`] in shape.
#[derive(Debug, Clone, Copy)]
pub struct PsxSpu<'a> {
    save: &'a SaveState,
}

impl<'a> PsxSpu<'a> {
    pub fn new(save: &'a SaveState) -> Self {
        Self { save }
    }

    /// 512 KiB of SPU RAM bytes. Returns `None` if the save state doesn't
    /// expose the `SPURAM` entry (some non-PSX mednafen modules).
    pub fn spu_ram_bytes(&self) -> Option<&'a [u8]> {
        let bytes = self.save.entry_bytes("SPU", "SPURAM")?;
        if bytes.len() != SPU_RAM_BYTES {
            return None;
        }
        Some(bytes)
    }

    /// Read one voice's state snapshot. `idx` must be `< NUM_VOICES`.
    /// Returns `None` if no fields could be resolved.
    pub fn voice_state(&self, idx: usize) -> Option<SpuVoiceState> {
        if idx >= NUM_VOICES {
            return None;
        }
        let s = SpuVoiceState {
            start_addr: self.voice_u32(idx, "StartAddr"),
            cur_addr: self.voice_u32(idx, "CurAddr"),
            loop_addr: self.voice_u32(idx, "LoopAddr"),
            pitch: self.voice_u16(idx, "Pitch"),
            cur_phase: self.voice_u32(idx, "CurPhase"),
            adsr_phase: self.voice_u32_dotted(idx, "ADSR.Phase"),
            adsr_env_level: self.voice_u16_dotted(idx, "ADSR.EnvLevel"),
            vol_left: self.voice_sweep_current(idx, 0),
            vol_right: self.voice_sweep_current(idx, 1),
            adsr_control: self.voice_u32(idx, "ADSRControl"),
        };
        // Any field set → return a populated snapshot; otherwise None so
        // callers can distinguish "save state didn't capture SPU" from "all
        // 24 voices are zero".
        if s.start_addr.is_some()
            || s.cur_addr.is_some()
            || s.loop_addr.is_some()
            || s.pitch.is_some()
            || s.cur_phase.is_some()
            || s.adsr_phase.is_some()
            || s.adsr_env_level.is_some()
            || s.vol_left.is_some()
            || s.vol_right.is_some()
            || s.adsr_control.is_some()
        {
            Some(s)
        } else {
            None
        }
    }

    /// Full 24-voice state array. Slots with no data are populated with
    /// `SpuVoiceState::default()` rather than dropped, so the array
    /// indices line up 1:1 with [`engine_audio::spu::Spu::voices`].
    pub fn voices(&self) -> [SpuVoiceState; NUM_VOICES] {
        std::array::from_fn(|i| self.voice_state(i).unwrap_or_default())
    }

    /// Bitmask of voices that have a pending key-on (`VoiceOn` register).
    /// libspu writes set this; the SPU clears the bit on the next cycle
    /// after kicking the envelope into Attack.
    pub fn voice_on_mask(&self) -> Option<u32> {
        self.u32_entry("VoiceOn")
    }

    /// Bitmask of voices with a pending key-off (`VoiceOff` register).
    pub fn voice_off_mask(&self) -> Option<u32> {
        self.u32_entry("VoiceOff")
    }

    /// Bitmask of voices whose current ADPCM block carried an end-flag
    /// without repeat (PSX `ENDX` status). Useful for distinguishing
    /// "voice naturally finished" from "host issued key-off".
    pub fn block_end_mask(&self) -> Option<u32> {
        self.u32_entry("BlockEnd")
    }

    /// Reverb mode register (libspu `SpuCommonAttr.reverb` width). Mednafen
    /// stores this as a u32 even though the libspu API takes a u8.
    pub fn reverb_mode(&self) -> Option<u32> {
        self.u32_entry("Reverb_Mode")
    }

    /// SPU control register (`SPUCNT`, 2 bytes). Bit-level layout matches
    /// the PSX hardware spec.
    pub fn spu_control(&self) -> Option<u16> {
        self.u16_entry("SPUControl")
    }

    /// Master volume `(left, right)` as the current accumulated output of
    /// mednafen's global sweep registers. Distinct from the libspu MVOL
    /// write that *drives* the sweep target - this is the post-sweep value
    /// the SPU multiplies the mix by on the cycle the save state captures.
    pub fn master_volume(&self) -> Option<(i16, i16)> {
        let l = self.global_sweep_current(0)?;
        let r = self.global_sweep_current(1)?;
        Some((l, r))
    }

    // --- internal helpers ---------------------------------------------------

    fn entry_bytes(&self, name: &str) -> Option<&'a [u8]> {
        self.save.entry_bytes("SPU", name)
    }

    fn u32_entry(&self, name: &str) -> Option<u32> {
        let b = self.entry_bytes(name)?;
        if b.len() < 4 {
            return None;
        }
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u16_entry(&self, name: &str) -> Option<u16> {
        let b = self.entry_bytes(name)?;
        if b.len() < 2 {
            return None;
        }
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    fn voice_u32(&self, idx: usize, suffix: &str) -> Option<u32> {
        let name = format!("Voices[{idx}].{suffix}");
        self.u32_entry(&name)
    }

    fn voice_u16(&self, idx: usize, suffix: &str) -> Option<u16> {
        let name = format!("Voices[{idx}].{suffix}");
        self.u16_entry(&name)
    }

    fn voice_u32_dotted(&self, idx: usize, suffix: &str) -> Option<u32> {
        // Same as voice_u32 but written separately so call sites are
        // self-documenting (the dotted variant is the ADSR nested struct).
        let name = format!("Voices[{idx}].{suffix}");
        self.u32_entry(&name)
    }

    fn voice_u16_dotted(&self, idx: usize, suffix: &str) -> Option<u16> {
        let name = format!("Voices[{idx}].{suffix}");
        self.u16_entry(&name)
    }

    fn voice_sweep_current(&self, voice_idx: usize, sweep_idx: usize) -> Option<i16> {
        // Mednafen names the per-voice sweep entries with a parenthesised
        // prefix: "(Voices[7].Sweep[0]).Current". The 2-byte "Current"
        // field carries the most recent output of the sweep generator.
        let name = format!("(Voices[{voice_idx}].Sweep[{sweep_idx}]).Current");
        let b = self.entry_bytes(&name)?;
        if b.len() < 2 {
            return None;
        }
        Some(i16::from_le_bytes([b[0], b[1]]))
    }

    fn global_sweep_current(&self, idx: usize) -> Option<i16> {
        let name = format!("(GlobalSweep[{idx}]).Current");
        let b = self.entry_bytes(&name)?;
        if b.len() < 2 {
            return None;
        }
        Some(i16::from_le_bytes([b[0], b[1]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{MDFN_HEADER_LEN, MDFN_MAGIC, SECTION_NAME_LEN};

    fn build_save_with_spu(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, value) in entries {
            body.push(name.len() as u8);
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(&(value.len() as u32).to_le_bytes());
            body.extend_from_slice(value);
        }
        let mut name_buf = [0u8; SECTION_NAME_LEN];
        name_buf[..3].copy_from_slice(b"SPU");
        let mut payload = Vec::new();
        payload.extend_from_slice(MDFN_MAGIC);
        payload.extend_from_slice(&[0u8; MDFN_HEADER_LEN - MDFN_MAGIC.len()]);
        payload.extend_from_slice(&name_buf);
        payload.extend_from_slice(&(body.len() as u32).to_le_bytes());
        payload.extend_from_slice(&body);
        payload
    }

    #[test]
    fn spu_ram_bytes_returns_full_buffer() {
        let mut ram = vec![0u8; SPU_RAM_BYTES];
        ram[0x100] = 0xAB;
        ram[0x101] = 0xCD;
        let payload = build_save_with_spu(&[("SPURAM", ram)]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let spu = PsxSpu::new(&save);
        let bytes = spu.spu_ram_bytes().unwrap();
        assert_eq!(bytes.len(), SPU_RAM_BYTES);
        assert_eq!(bytes[0x100], 0xAB);
        assert_eq!(bytes[0x101], 0xCD);
    }

    #[test]
    fn spu_ram_rejects_wrong_size() {
        let ram = vec![0u8; SPU_RAM_BYTES - 1];
        let payload = build_save_with_spu(&[("SPURAM", ram)]);
        let save = SaveState::from_decompressed(payload).unwrap();
        assert!(PsxSpu::new(&save).spu_ram_bytes().is_none());
    }

    #[test]
    fn voice_state_reads_known_subset() {
        let payload = build_save_with_spu(&[
            ("Voices[3].StartAddr", 0x1000u32.to_le_bytes().to_vec()),
            ("Voices[3].CurAddr", 0x10A0u32.to_le_bytes().to_vec()),
            ("Voices[3].LoopAddr", 0x1080u32.to_le_bytes().to_vec()),
            ("Voices[3].Pitch", 0x1000u16.to_le_bytes().to_vec()),
            ("Voices[3].CurPhase", 0x0800u32.to_le_bytes().to_vec()),
            ("Voices[3].ADSR.Phase", 1u32.to_le_bytes().to_vec()),
            ("Voices[3].ADSR.EnvLevel", 0x4000u16.to_le_bytes().to_vec()),
            (
                "Voices[3].ADSRControl",
                0xDEADBEEFu32.to_le_bytes().to_vec(),
            ),
            (
                "(Voices[3].Sweep[0]).Current",
                0x3FFFi16.to_le_bytes().to_vec(),
            ),
            (
                "(Voices[3].Sweep[1]).Current",
                (-0x100i16).to_le_bytes().to_vec(),
            ),
        ]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let spu = PsxSpu::new(&save);
        let v = spu.voice_state(3).unwrap();
        assert_eq!(v.start_addr, Some(0x1000));
        assert_eq!(v.cur_addr, Some(0x10A0));
        assert_eq!(v.loop_addr, Some(0x1080));
        assert_eq!(v.pitch, Some(0x1000));
        assert_eq!(v.cur_phase, Some(0x0800));
        assert_eq!(v.adsr_phase, Some(1));
        assert_eq!(v.adsr_env_level, Some(0x4000));
        assert_eq!(v.adsr_control, Some(0xDEADBEEF));
        assert_eq!(v.vol_left, Some(0x3FFF));
        assert_eq!(v.vol_right, Some(-0x100));
        assert!(v.is_active());
    }

    #[test]
    fn voice_state_missing_returns_none() {
        let payload = build_save_with_spu(&[]);
        let save = SaveState::from_decompressed(payload).unwrap();
        assert!(PsxSpu::new(&save).voice_state(0).is_none());
    }

    #[test]
    fn voice_state_index_out_of_range_returns_none() {
        let payload =
            build_save_with_spu(&[("Voices[0].StartAddr", 0x1000u32.to_le_bytes().to_vec())]);
        let save = SaveState::from_decompressed(payload).unwrap();
        assert!(PsxSpu::new(&save).voice_state(NUM_VOICES).is_none());
    }

    #[test]
    fn voice_state_phase_zero_is_off() {
        let payload = build_save_with_spu(&[("Voices[5].ADSR.Phase", 0u32.to_le_bytes().to_vec())]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let v = PsxSpu::new(&save).voice_state(5).unwrap();
        assert_eq!(v.adsr_phase, Some(0));
        assert!(!v.is_active());
    }

    #[test]
    fn voices_array_has_24_entries() {
        let payload = build_save_with_spu(&[]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let voices = PsxSpu::new(&save).voices();
        assert_eq!(voices.len(), NUM_VOICES);
        assert!(voices.iter().all(|v| v.adsr_phase.is_none()));
    }

    #[test]
    fn global_registers_round_trip() {
        let payload = build_save_with_spu(&[
            ("VoiceOn", 0x0000_0007u32.to_le_bytes().to_vec()),
            ("VoiceOff", 0x0000_0001u32.to_le_bytes().to_vec()),
            ("BlockEnd", 0x0000_0100u32.to_le_bytes().to_vec()),
            ("Reverb_Mode", 7u32.to_le_bytes().to_vec()),
            ("SPUControl", 0xC000u16.to_le_bytes().to_vec()),
            ("(GlobalSweep[0]).Current", 0x3F00i16.to_le_bytes().to_vec()),
            ("(GlobalSweep[1]).Current", 0x3F00i16.to_le_bytes().to_vec()),
        ]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let spu = PsxSpu::new(&save);
        assert_eq!(spu.voice_on_mask(), Some(0x0000_0007));
        assert_eq!(spu.voice_off_mask(), Some(0x0000_0001));
        assert_eq!(spu.block_end_mask(), Some(0x0000_0100));
        assert_eq!(spu.reverb_mode(), Some(7));
        assert_eq!(spu.spu_control(), Some(0xC000));
        assert_eq!(spu.master_volume(), Some((0x3F00, 0x3F00)));
    }

    #[test]
    fn master_volume_partial_returns_none() {
        // Left-only sweep entry → master_volume can't form a pair, returns
        // None. Distinct from "both halves zero".
        let payload =
            build_save_with_spu(&[("(GlobalSweep[0]).Current", 0x3F00i16.to_le_bytes().to_vec())]);
        let save = SaveState::from_decompressed(payload).unwrap();
        assert!(PsxSpu::new(&save).master_volume().is_none());
    }
}
