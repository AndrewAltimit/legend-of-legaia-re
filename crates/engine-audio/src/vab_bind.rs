//! Bind a parsed VAB sound bank ([`legaia_vab::VabReport`] + raw VAG body
//! bytes) into a [`crate::Spu`] instance.
//!
//! This is the bridge between the asset-extraction track (which parses VAB
//! files off disc) and the engine-reimplementation track (which plays them
//! through the clean-room SPU). One bank is uploaded once via
//! [`VabBank::upload`], then the engine triggers notes via
//! [`VabBank::play_note`] which:
//!  1. picks a tone from the program based on the requested key,
//!  2. allocates an idle SPU voice,
//!  3. sets sample address, ADSR, volume, pitch,
//!  4. fires `key_on`.
//!
//! Pitch math follows the standard libspu key-to-pitch formula:
//!
//! ```text
//!   semitones = (note - center) + (-fine_cents / 100)
//!   pitch_ratio = 2^(semitones / 12)
//!   pitch_register = base_pitch * pitch_ratio  (clipped to 0..=0x3FFF)
//! ```
//!
//! `base_pitch` is the playback pitch when `note == center`. For a 22.05 kHz
//! VAG body played by an SPU running at 44.1 kHz internal, that's
//! `0x1000 * 22050 / 44100 = 0x800`.
//!
//! No Sony bytes - algorithm is the documented libspu surface.

use crate::Spu;
use crate::spu::{
    adsr::AdsrConfig,
    ram::{SpuAllocator, TransferDirection},
    voice::{PITCH_UNITY, SPU_INTERNAL_RATE},
};
use legaia_vab::{VabReport, VagAtr};

/// One program slot of a bank: the program-level attributes retail stages at
/// key-on, plus the program's tone page. Indexed by **program number** (the
/// `ProgAtr` slot a SEQ ProgramChange or an SFX descriptor names) - see
/// [`VabBank::programs`]. An unused slot aliases onto the next used slot's tone
/// page (retail's rank rule) while keeping its own `ProgAtr` mvol/mpan; only a
/// slot past the last used page carries an empty page whose notes never resolve.
#[derive(Debug, Clone)]
pub struct VabProgram {
    /// Program master volume 0..=127 (`ProgAtr.mvol`). Factors into the
    /// key-on volume chain alongside the bank and tone volumes.
    pub mvol: u8,
    /// Program pan 0..=127, 0x40 = centre (`ProgAtr.mpan`). Applied as its
    /// own attenuation stage after the tone pan.
    pub mpan: u8,
    /// The program's tone page (up to 16 `VagAtr` rows).
    pub tones: Vec<VagAtr>,
}

/// Default sample rate of Legaia VAG bodies. The bank header doesn't carry
/// a per-sample rate; the engine has historically used 22.05 kHz across the
/// extracted corpus (see `crates/vab` extractor + the WAV writer that hard-
/// codes 22050).
pub const VAB_SAMPLE_RATE: u32 = 22_050;

/// Per-VAG metadata after upload: where in SPU RAM the body lives.
#[derive(Debug, Clone, Copy)]
pub struct UploadedVag {
    /// Start address in SPU RAM (bytes).
    pub addr: u32,
    /// Body size in bytes.
    pub size: u32,
}

/// A VAB bank, ready for playback. Holds the per-VAG addresses + program
/// table needed to translate "play program P note N" into voice config.
#[derive(Debug, Clone)]
pub struct VabBank {
    pub master_vol: u8,
    pub samples: Vec<Option<UploadedVag>>,
    /// Per-program table indexed by **program number** - the `ProgAtr` slot
    /// a SEQ ProgramChange or an SFX descriptor names - NOT by packed
    /// tone-page order. The file stores one 16-tone page per *used* program
    /// (`ProgAtr.tones != 0`), packed in slot order; `upload` expands those
    /// pages back into slot space the way retail does at VAB open (see
    /// there): an unused slot aliases onto the next used slot's page, and only
    /// slots past the last used page hold an empty page. Those trailing empties
    /// are trimmed, so `len()` reads "last used program + 1".
    pub programs: Vec<VabProgram>,
}

impl VabBank {
    /// Upload every VAG body in `report` into `spu`'s RAM, allocating
    /// through `alloc`. The raw `bank_buf` is the same byte slice that
    /// was passed to `legaia_vab::parse` so `VagSampleSpan::byte_offset`
    /// indexes are valid.
    pub fn upload(
        spu: &mut Spu,
        alloc: &mut SpuAllocator,
        report: &VabReport,
        bank_buf: &[u8],
    ) -> Self {
        let mut samples: Vec<Option<UploadedVag>> = Vec::with_capacity(report.vag_samples.len());
        spu.ram.set_direction(TransferDirection::CpuToSpu);
        for span in &report.vag_samples {
            if span.size == 0 {
                samples.push(None);
                continue;
            }
            let body = &bank_buf[span.byte_offset..span.byte_offset + span.size];
            // Allocate aligned to 16 (one ADPCM block).
            match alloc.alloc(span.size as u32) {
                Some(addr) => {
                    spu.ram.write_at(addr, body);
                    samples.push(Some(UploadedVag {
                        addr,
                        size: span.size as u32,
                    }));
                }
                None => {
                    log::warn!(
                        "vab_bind: SPU RAM exhausted at sample index {} ({} bytes)",
                        span.index,
                        span.size
                    );
                    samples.push(None);
                }
            }
        }
        // Expand the packed tone pages into program-number space. The file's
        // tone region carries one page per *used* program, packed in slot
        // order, so a program number resolves to its page by rank among the
        // used slots.
        //
        // PORT: FUN_80068d94 - retail computes exactly this mapping at VAB
        // open: it walks the full ProgAtr table writing the running count of
        // used programs seen so far into each entry's +8 reserved word, and
        // the program-change consumer FUN_80068b98 reads that byte back as
        // the tone-page index. Indexing the packed pages with the raw
        // program number instead mis-tones every program past the first
        // unused slot and drops every program number >= ps outright - on a
        // sparse bank (most music banks) that collapses the whole score onto
        // a few low pages.
        //
        // Retail stores the counter *before* the used check, so a slot's page
        // index is the count of USED slots that precede it: a used slot maps to
        // its own next page, and an UNUSED slot aliases onto the SAME page the
        // next used slot gets. The engine reproduces this so a ProgramChange to
        // an unused slot resolves to the aliased page rather than silence -
        // real retail BGM does this (e.g. music banks where a channel selects a
        // gap slot; see tests/real_seq_program_change_coverage.rs). The unused
        // slot keeps its own ProgAtr mvol/mpan (retail reads ProgAtr[P] for the
        // volume chain) but borrows the next used slot's tone region.
        //
        // Only the *past-the-last-used-page* case is NOT reproduced: there
        // retail's alias index runs beyond the packed tone region and reads
        // garbage; the engine leaves those trailing slots an empty (silent)
        // page instead of replaying undefined bytes.
        let mut programs = Vec::with_capacity(report.programs.len());
        let mut page = 0usize;
        for prog in &report.programs {
            if page < report.tones.len() {
                programs.push(VabProgram {
                    mvol: prog.mvol,
                    mpan: prog.mpan,
                    tones: report.tones[page].clone(),
                });
                // Only a used slot advances the page counter; an unused slot
                // shares the page of the used slot that follows it.
                if prog.tones != 0 {
                    page += 1;
                }
            } else {
                programs.push(VabProgram {
                    mvol: 0x7F,
                    mpan: 0x40,
                    tones: Vec::new(),
                });
            }
        }
        while programs.last().is_some_and(|p| p.tones.is_empty()) {
            programs.pop();
        }
        Self {
            master_vol: report.header.mvol,
            samples,
            programs,
        }
    }

    /// Play `note` (MIDI key, 0..=127) through `program` index using `voice`
    /// on the SPU. Velocity 0..=127 scales the per-tone volume.
    ///
    /// Returns `false` when the program / tone / sample isn't valid (so the
    /// engine can log + skip without panicking on bad bank data).
    pub fn play_note(
        &self,
        spu: &mut Spu,
        voice: usize,
        program: usize,
        note: u8,
        velocity: u8,
    ) -> bool {
        let Some(prog) = self.programs.get(program) else {
            return false;
        };
        let Some(tone) = prog.tones.iter().find(|t| note >= t.min && note <= t.max) else {
            return false;
        };
        self.fire(spu, voice, prog, tone, note, velocity)
    }

    /// Play the tone at an **explicit index** inside `program`, rather than
    /// resolving it by key range the way [`Self::play_note`] does.
    ///
    /// This is the shape of the retail *SFX* path, which differs from the
    /// sequencer's: the SFX descriptor's `+1` byte names the ADSR region
    /// directly (`FUN_80065034` is handed program `+0`, region `+1` `+ i`, and
    /// the note-level attribute `+2`), so a cue's tone is an index, and a
    /// multi-voice cue walks consecutive regions. Key-range resolution would
    /// silently miss those cues whose descriptor note falls outside the tone's
    /// authored `min..=max` window (several retail cues do, e.g. the generic
    /// strike cue `0x1A`). See `docs/formats/sfx-table.md`.
    ///
    /// Returns `false` when the program / tone / sample isn't valid.
    pub fn play_tone(
        &self,
        spu: &mut Spu,
        voice: usize,
        program: usize,
        tone_index: usize,
        note: u8,
        velocity: u8,
    ) -> bool {
        let Some(prog) = self.programs.get(program) else {
            return false;
        };
        let Some(tone) = prog.tones.get(tone_index) else {
            return false;
        };
        self.fire(spu, voice, prog, tone, note, velocity)
    }

    /// Configure + key on `voice` for one resolved tone. Shared by
    /// [`Self::play_note`] (key-range lookup) and [`Self::play_tone`]
    /// (explicit region index).
    fn fire(
        &self,
        spu: &mut Spu,
        voice: usize,
        prog: &VabProgram,
        tone: &legaia_vab::VagAtr,
        note: u8,
        velocity: u8,
    ) -> bool {
        // tone.vag is 1-based in PSX VAB format. legaia_vab::VabReport's
        // `vag_samples` is 0-indexed (samples[0..vs]), so subtract 1.
        if tone.vag <= 0 {
            return false;
        }
        let sample_idx = (tone.vag - 1) as usize;
        let Some(Some(vag)) = self.samples.get(sample_idx) else {
            return false;
        };
        if voice >= spu.voices.len() {
            return false;
        }
        let pitch = compute_pitch(note, tone, VAB_SAMPLE_RATE, SPU_INTERNAL_RATE);
        // PORT: FUN_80067550 (head) - the key-on volume chain, retail's
        // staged integer arithmetic with its truncation points:
        //
        //   step = vel * bank_mvol * 0x3FFF / 0x3F01
        //   vol  = step * prog_mvol * tone_vol / 0x3F01
        //
        // Four 0..=127 factors (velocity, bank master, program master, tone)
        // against 127^2 twice widen the product into the SPU's 14-bit
        // register domain; a full-scale product lands exactly on 0x3FFF.
        // All intermediates fit i32 (max 0x3FFF * 127 * 127 < 2^31).
        let vel = velocity as i32;
        let bank_master = self.master_vol as i32;
        let step = (vel * bank_master * 0x3FFF) / 0x3F01;
        let combined = ((step * prog.mvol as i32 * tone.vol as i32) / 0x3F01).min(0x3FFF) as i16;
        // Retail attenuates once per pan source, in order: tone pan, then
        // program pan. The channel pan is the sequencer's own source,
        // applied on top by `channel_mix`.
        let (l, r) = pan_split(combined, tone.pan as i32);
        let (vol_l, vol_r) = pan_attenuate(l, r, prog.mpan as i32);
        {
            let v = &mut spu.voices[voice];
            v.start_addr = vag.addr;
            v.loop_addr = None;
            v.pitch = pitch;
            v.vol_left = vol_l;
            v.vol_right = vol_r;
            v.adsr_cfg = AdsrConfig::from_words(tone.adsr1, tone.adsr2);
        }
        {
            let crate::spu::Spu {
                ref mut voices,
                ref ram,
                ..
            } = *spu;
            voices[voice].key_on(ram);
        }
        spu.record_key_on(voice);
        true
    }

    /// The tone that would be selected for `(program, note)` carries a
    /// pitch-bend range in the VAB attributes: `pbmin` semitones of downward
    /// bend at full-down wheel, `pbmax` semitones up at full-up. Returns
    /// `(pbmin, pbmax)` so the sequencer can scale a `0xEn` wheel value by the
    /// note's own range (a tone with `(0, 0)` does not respond to the wheel).
    /// `(0, 0)` is also the fallback when the program/tone can't be resolved.
    pub fn pitch_bend_range(&self, program: usize, note: u8) -> (u8, u8) {
        self.programs
            .get(program)
            .and_then(|p| p.tones.iter().find(|t| note >= t.min && note <= t.max))
            .map(|t| (t.pbmin, t.pbmax))
            .unwrap_or((0, 0))
    }

    /// The VAB `prior` byte of the tone that would be selected for
    /// `(program, note)` - the note's requested **allocation priority**.
    ///
    /// Retail stages this byte (VagAtr `+0` of the resolved tone) into the
    /// driver's note-staging block before running the voice-allocation scan,
    /// where it seeds the steal threshold: only sounding voices whose own
    /// priority is `<=` the request may be stolen. `None` when the program /
    /// tone doesn't resolve (the note would not key on at all).
    ///
    /// REF: FUN_80066308 (stages the tone attrs), FUN_80066B00 (consumes the
    /// staged priority as the scan threshold).
    pub fn tone_prior(&self, program: usize, note: u8) -> Option<u8> {
        self.programs
            .get(program)
            .and_then(|p| p.tones.iter().find(|t| note >= t.min && note <= t.max))
            .map(|t| t.prior)
    }

    /// Whether `(program, note)` would actually key on a voice - i.e. it
    /// resolves to a tone whose sample is present and uploaded. This is the
    /// exact success condition of [`Self::fire`] (tone found by key range,
    /// `vag > 0`, sample resident), computed without a voice or side effects.
    ///
    /// The sequencer calls this **before** allocating a voice: a note that
    /// can't sound (empty tone slot, or a sample that didn't fit in SPU RAM)
    /// must not steal a voice that is currently sounding, or the steal drops
    /// an audible note in exchange for silence.
    pub fn can_play(&self, program: usize, note: u8) -> bool {
        let Some(prog) = self.programs.get(program) else {
            return false;
        };
        let Some(tone) = prog.tones.iter().find(|t| note >= t.min && note <= t.max) else {
            return false;
        };
        if tone.vag <= 0 {
            return false;
        }
        matches!(self.samples.get((tone.vag - 1) as usize), Some(Some(_)))
    }
}

/// Compute the SPU pitch register value for `note` against `tone.center`,
/// `tone.shift` (centi-semitones), and the source/dest sample rates.
fn compute_pitch(note: u8, tone: &VagAtr, src_rate: u32, dst_rate: u32) -> u16 {
    let semitones = note as f64 - tone.center as f64 - (tone.shift as i8 as f64) / 100.0;
    let ratio = 2f64.powf(semitones / 12.0);
    let base = (PITCH_UNITY as f64) * (src_rate as f64) / (dst_rate as f64);
    let pitch = (base * ratio).round() as i64;
    // Hardware clamps the pitch-counter step at 0x4000 (4.0x / 176.4 kHz).
    pitch.clamp(1, 0x4000) as u16
}

/// Split a combined volume into (left, right) for a tone's 0..=127 pan value
/// (0x40 = centre), using libsnd's voice-volume pan law (`FUN_80067550`): a
/// pan left of centre attenuates the **right** by `pan/0x3f`, a pan right of
/// centre attenuates the **left** by `(0x7f - pan)/0x3f`. The near side is
/// left alone - the law only ever attenuates, so it cannot overflow the
/// register.
///
/// This is the same law `sequencer::apply_channel_pan` already applies to the
/// CC10 channel pan, and deliberately so: libsnd runs this attenuation once
/// per pan source (channel / sequence / tone), so both sources must share it.
///
/// The earlier form here scaled both sides by `/64`, which BOOSTS the near
/// side to ~2x and clamps. That was invisible while key-on volume was stuck
/// in 0..=127 - the clamp was unreachable - but at the correct 0..=0x3FFF
/// scale 836 of the 7724 retail tones would have hit it at full velocity
/// (census in tests/real_vab_tone_attributes.rs). Widening the volume without
/// fixing the pan law would have traded a quiet engine for a clipping one.
fn pan_split(vol: i16, pan: i32) -> (i16, i16) {
    pan_attenuate(vol, vol, pan)
}

/// One application of the same pan law over an already-split `(left, right)`
/// pair. Retail runs this once per pan source in order - tone pan, then
/// program pan (`ProgAtr.mpan`), then the sequencer's channel pan - each
/// stage only ever attenuating its far side.
fn pan_attenuate(left: i16, right: i16, pan: i32) -> (i16, i16) {
    let pan = pan.clamp(0, 0x7f);
    if pan < 0x40 {
        (left, (right as i32 * pan / 0x3f) as i16)
    } else {
        ((left as i32 * (0x7f - pan) / 0x3f) as i16, right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_tone(center: u8, vag: i16, vol: u8, pan: u8) -> VagAtr {
        VagAtr {
            prior: 0,
            mode: 0,
            vol,
            pan,
            center,
            shift: 0,
            min: 0,
            max: 127,
            vibw: 0,
            vibt: 0,
            porw: 0,
            port: 0,
            pbmin: 0,
            pbmax: 0,
            reserved1: 0,
            reserved2: 0,
            adsr1: 0,
            adsr2: 0,
            prog: 0,
            vag,
            reserved3: [0; 4],
        }
    }

    /// Note at center plays at the source/dest rate ratio.
    #[test]
    fn pitch_at_center_matches_rate_ratio() {
        let tone = dummy_tone(60, 1, 127, 64);
        let pitch = compute_pitch(60, &tone, 22_050, 44_100);
        // Expected: 0x1000 * 22050/44100 = 0x800.
        assert_eq!(pitch, 0x800);
    }

    /// One semitone above center bumps pitch by 2^(1/12) ≈ 1.0595.
    #[test]
    fn pitch_one_semitone_above_is_higher() {
        let tone = dummy_tone(60, 1, 127, 64);
        let p_center = compute_pitch(60, &tone, 22_050, 44_100);
        let p_above = compute_pitch(61, &tone, 22_050, 44_100);
        assert!(p_above > p_center);
        let ratio = p_above as f64 / p_center as f64;
        assert!((ratio - 2f64.powf(1.0 / 12.0)).abs() < 0.001);
    }

    /// Pan=0 silences right; pan=127 silences left; pan=64 is roughly equal.
    #[test]
    fn pan_split_endpoints_silence_opposite_side() {
        let (l, r) = pan_split(0x3FFF, 0);
        assert!(l > 0);
        assert_eq!(r, 0);
        let (l, r) = pan_split(0x3FFF, 127);
        assert_eq!(l, 0);
        assert!(r > 0);
        let (l, r) = pan_split(0x3FFF, 64);
        // Center pan: left ≈ vol * 63/64, right = vol. Difference is ~vol/64.
        assert!((l as i32 - r as i32).abs() <= 0x100);
    }

    /// VabBank::play_note returns false for an invalid program index without
    /// panicking; voice state is left untouched.
    #[test]
    fn play_note_invalid_program_returns_false() {
        let mut spu = Spu::new();
        let bank = VabBank {
            master_vol: 127,
            samples: vec![],
            programs: vec![],
        };
        let ok = bank.play_note(&mut spu, 0, 99, 60, 100);
        assert!(!ok);
        // Voice 0 still in default Off state.
        assert!(spu.voices[0].is_off());
    }

    /// `can_play` mirrors `fire`'s success condition exactly: it is true only
    /// when the tone resolves AND its sample is resident. A resolved tone whose
    /// sample slot is missing/empty, or whose `vag` is out of range, is false -
    /// which is what keeps the sequencer from stealing a voice for a note that
    /// would then fail to sound.
    #[test]
    fn can_play_tracks_sample_residency() {
        let uploaded = UploadedVag { addr: 0, size: 16 };
        // Program 0 tone points at sample 1 (1-based vag); program 1 at sample 2.
        let bank = VabBank {
            master_vol: 127,
            // Sample index 0 present (backs vag=1), index 1 absent (backs vag=2).
            samples: vec![Some(uploaded), None],
            programs: vec![
                VabProgram {
                    mvol: 127,
                    mpan: 0x40,
                    tones: vec![dummy_tone(60, 1, 100, 0x40)],
                },
                VabProgram {
                    mvol: 127,
                    mpan: 0x40,
                    tones: vec![dummy_tone(60, 2, 100, 0x40)],
                },
            ],
        };
        // Resolves and sample resident -> playable, and consistent with play_note.
        assert!(bank.can_play(0, 60));
        let mut spu = Spu::new();
        assert!(bank.play_note(&mut spu, 0, 0, 60, 100));
        // Resolves but sample absent -> not playable (would steal-then-fail).
        assert!(!bank.can_play(1, 60));
        let mut spu = Spu::new();
        assert!(!bank.play_note(&mut spu, 0, 1, 60, 100));
        // Program out of range -> not playable.
        assert!(!bank.can_play(9, 60));
    }

    fn prog(tones: u8, mvol: u8) -> legaia_vab::ProgAtr {
        legaia_vab::ProgAtr {
            tones,
            mvol,
            prior: 0,
            mode: 0,
            mpan: 0x40,
            reserved0: 0,
            attr: 0,
            reserved1: 0,
            reserved2: 0,
        }
    }

    fn mk_report(
        progs: Vec<legaia_vab::ProgAtr>,
        tones: Vec<Vec<VagAtr>>,
    ) -> legaia_vab::VabReport {
        legaia_vab::VabReport {
            header: legaia_vab::VabHeader {
                magic: 0,
                version: 0,
                vab_id: 0,
                fsize: 0,
                ps: tones.len() as u16,
                ts: 0,
                vs: 0,
                mvol: 127,
                pan: 0x40,
                attr1: 0,
                attr2: 0,
            },
            header_offset: 0,
            programs: progs,
            tones,
            vag_samples: vec![],
            vag_table_spacer: 0,
        }
    }

    /// A ProgramChange to an UNUSED program slot aliases onto the next used
    /// slot's tone page (retail's `+8` used-counter is written before the used
    /// check), keeping the unused slot's own `mvol`; a slot past the last used
    /// page stays empty (retail reads garbage there, which we don't replay).
    #[test]
    fn unused_slot_program_aliases_to_next_used_page() {
        // Slots: used, UNUSED, used, unused(trailing). Two used -> two packed
        // pages, made distinguishable by tone `center` (10 = page A, 20 = B).
        let page_a = vec![dummy_tone(10, 1, 100, 0x40)];
        let page_b = vec![dummy_tone(20, 1, 100, 0x40)];
        let report = mk_report(
            vec![
                prog(1, 0x70), // slot 0 used   -> page A
                prog(0, 0x55), // slot 1 UNUSED -> aliases to slot 2's page (B)
                prog(1, 0x60), // slot 2 used   -> page B
                prog(0, 0x40), // slot 3 unused, trailing -> past region -> empty
            ],
            vec![page_a, page_b],
        );
        let mut spu = Spu::new();
        let mut alloc = crate::spu::ram::SpuAllocator::new(0x1000, 0x1_0000);
        let bank = VabBank::upload(&mut spu, &mut alloc, &report, &[]);

        // Used slots keep their own next page.
        assert_eq!(bank.programs[0].tones[0].center, 10, "slot 0 = page A");
        assert_eq!(bank.programs[2].tones[0].center, 20, "slot 2 = page B");
        // THE FIX: the unused in-range slot resolves to the next used slot's
        // page (B) instead of silence, and keeps its OWN ProgAtr mvol.
        assert_eq!(bank.programs[1].tones.len(), 1, "slot 1 aliased, not empty");
        assert_eq!(bank.programs[1].tones[0].center, 20, "slot 1 -> page B");
        assert_eq!(bank.programs[1].mvol, 0x55, "slot 1 keeps its own mvol");
        // The trailing unused slot is past the last used page -> empty ->
        // trimmed off the tail, so the bank ends at the last used slot.
        assert_eq!(bank.programs.len(), 3, "trailing empty slot trimmed");
    }
}
