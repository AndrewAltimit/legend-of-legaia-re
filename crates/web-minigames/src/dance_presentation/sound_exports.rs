//! The dance's sound exports: cue bank, finish cues and award sounds, the
//! hit stings and the BGM pair.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    // ------------------------------------------------------------- dance sound

    /// The dance's own cue bank (descriptors PROT 1228, samples PROT 1231):
    /// `[{ "id":528, "program":0, "tone":1, "note":66, "rate":44100 }, ...]`.
    /// Empty when either entry didn't decode - PROT 1231 sits in the PROT
    /// TOC's zeroed tail, so an image whose TOC truncates early loses it.
    pub fn dance_sfx_json(&self) -> String {
        let Some(bank) = self.dance_pres.as_ref().and_then(|p| p.sfx.as_ref()) else {
            return "[]".to_string();
        };
        let rows = bank
            .cues()
            .iter()
            .map(|c| {
                let rate = bank.decode(c.id).map(|(_, r)| r).unwrap_or(0);
                format!(
                    r#"{{"id":{},"program":{},"tone":{},"note":{},"rate":{}}}"#,
                    c.id, c.program, c.tone, c.note, rate
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// Decode one dance cue to mono PCM (`i16`). Empty when absent.
    pub fn dance_sfx_pcm(&self, cue: u16) -> Vec<i16> {
        self.dance_pres
            .as_ref()
            .and_then(|p| p.sfx.as_ref())
            .and_then(|b| b.decode(cue).ok())
            .map(|(pcm, _)| pcm)
            .unwrap_or_default()
    }

    /// Playback rate for [`Self::dance_sfx_pcm`] (`0` when absent).
    pub fn dance_sfx_rate(&self, cue: u16) -> u32 {
        self.dance_pres
            .as_ref()
            .and_then(|p| p.sfx.as_ref())
            .and_then(|b| b.decode(cue).ok())
            .map(|(_, rate)| rate)
            .unwrap_or(0)
    }

    /// The retail cue ids (`FUN_801d1af4` sites): miss, the three combo-tier
    /// stings, the run-start and intro cues.
    pub fn dance_sfx_cue_ids(&self) -> String {
        format!(
            r#"{{"miss":{},"cool":{},"great":{},"fever":{},"start":{},"intro":{},"finish":{},"count1":{},"count2":{},"count3":{}}}"#,
            dance_art::CUE_DANCE_MISS,
            dance_art::CUE_DANCE_COOL,
            dance_art::CUE_DANCE_GREAT,
            dance_art::CUE_DANCE_FEVER,
            dance_art::CUE_DANCE_START,
            dance_art::CUE_DANCE_INTRO,
            0x206,
            0x207,
            0x208,
            0x209,
        )
    }

    /// The song-end countdown's cues since the last call (`3 2 1 FINISH!`,
    /// `0x209..=0x206`, written by the overlay's own part programs) - the
    /// page plays them by id through [`Self::dance_sfx_cue_ids`]' names.
    pub fn dance_take_finish_cues(&mut self) -> Vec<u16> {
        self.dance
            .as_mut()
            .map(|g| g.take_finish_cues())
            .unwrap_or_default()
    }

    /// The human's award sounds since the last call - what `FUN_801d1af4`'s
    /// dancer-0 arm raises for the press just judged
    /// (`legaia_engine_minigames::dance::award_sounds`), as JSON:
    /// `[{"cue":528}, {"sting":5}, {"sting":null}]`. A cue plays by id; a
    /// sting keys both [`Self::dance_sting_pcm`] layers at `r`, and `null`
    /// is the tier-2 site's `rand() % 3`, the page's to draw. A plain matched
    /// note, an unlanded groovy move and an empty triangle stock raise
    /// nothing.
    pub fn dance_take_award_sounds(&mut self) -> String {
        use legaia_engine_minigames::dance::DanceAwardSound;
        let sounds = self
            .dance
            .as_mut()
            .map(|g| g.take_award_sounds())
            .unwrap_or_default();
        let rows = sounds
            .iter()
            .map(|s| match *s {
                DanceAwardSound::Cue(id) => format!(r#"{{"cue":{id}}}"#),
                DanceAwardSound::Sting { random: true, .. } => r#"{"sting":null}"#.to_string(),
                DanceAwardSound::Sting { r, .. } => format!(r#"{{"sting":{r}}}"#),
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// One layer of a good-step **hit sting**. Retail keys these directly
    /// (`FUN_801d3d78`, bypassing the cue ring): two voices together, at the
    /// tones and note [`dance_hit_sting_voices`] resolves `r` to. Mono i16 PCM;
    /// empty when the bank does not hold that tone.
    ///
    /// The retail `r` space is not just the tier-2 award's `rand() % 3`
    /// (`legaia_engine_minigames::dance::STING_RANDOM_VARIANTS`) - the three
    /// groovy-move tiers all key `STING_TIER_VARIANT` as well, so `r = 5` is a
    /// real sting and this accepts it.
    pub fn dance_sting_pcm(&self, r: u8, layer: u8) -> Vec<i16> {
        self.dance_sting(r, layer)
            .map(|(pcm, _)| pcm)
            .unwrap_or_default()
    }

    /// Playback rate for [`Self::dance_sting_pcm`] (`0` when absent).
    pub fn dance_sting_rate(&self, r: u8, layer: u8) -> u32 {
        self.dance_sting(r, layer).map(|(_, r)| r).unwrap_or(0)
    }

    // --------------------------------------------------------------- dance BGM

    /// Whether the dance BGM pair (VAB + SEQ in one `music_01` entry)
    /// resolves: `{"ok":true,"prot":1048,"alt":true}`. The overlay starts one
    /// of two songs by mode (`FUN_801cf470` state 6 branches on the mode
    /// global); `alt = false` picks extraction 1048, `true` picks 1054.
    pub fn dance_bgm_ready_json(&self) -> String {
        let probe = |idx: usize| {
            entry_bytes(&self.prot, &self.entries, idx as u32)
                .map(super::music01_pair_ok)
                .unwrap_or(false)
        };
        format!(
            r#"{{"ok":{},"prot":{},"alt":{}}}"#,
            probe(dance_art::DANCE_BGM_PROT_INDEX),
            dance_art::DANCE_BGM_PROT_INDEX,
            probe(dance_art::DANCE_BGM_ALT_PROT_INDEX),
        )
    }

    /// Render `seconds` of the dance BGM to interleaved stereo i16 PCM at
    /// [`Self::dance_bgm_rate`], through the from-scratch SPU + sequencer -
    /// the same path the audio page uses. Empty when the pair didn't decode.
    pub fn dance_bgm_pcm_i16(&self, alt: bool, seconds: f32) -> Vec<i16> {
        let idx = if alt {
            dance_art::DANCE_BGM_ALT_PROT_INDEX
        } else {
            dance_art::DANCE_BGM_PROT_INDEX
        };
        let Some(buf) = entry_bytes(&self.prot, &self.entries, idx as u32) else {
            return Vec::new();
        };
        super::render_music01_bgm(buf, seconds)
    }

    /// Sample rate of [`Self::dance_bgm_pcm_i16`] (the SPU's 44.1 kHz).
    pub fn dance_bgm_rate(&self) -> u32 {
        legaia_engine_audio::SPU_INTERNAL_RATE
    }
}

impl LegaiaMinigames {
    /// Decode one sting layer, keyed against the tone's own centre note.
    ///
    /// The `(program, tone, note)` triple is [`dance_hit_sting_voices`]'s, not
    /// this page's: it is the ported `FUN_801d3d78`, so the bank index below is
    /// a read of the retail `program` argument rather than a literal `1` that
    /// happens to agree, and the tone / note arithmetic lives in one place.
    fn dance_sting(&self, r: u8, layer: u8) -> Option<(Vec<i16>, u32)> {
        if layer > 1 {
            return None;
        }
        let voice = *dance_hit_sting_voices(r as u16).get(layer as usize)?;
        let (report, bytes) = self
            .dance_pres
            .as_ref()
            .and_then(|p| p.sting_vab.as_ref())?;
        let atr = report
            .tones
            .get(voice.program as usize)?
            .get(usize::try_from(voice.tone).ok()?)?;
        if atr.vag <= 0 {
            return None;
        }
        let span = report.vag_samples.get(atr.vag as usize - 1)?;
        let body = bytes.get(span.byte_offset..span.byte_offset + span.size)?;
        let pcm = legaia_vab::decode_vag_aligned(body).ok()?;
        let semitones = voice.note as f64 - atr.center as f64;
        let rate = (44100.0 * 2f64.powf(semitones / 12.0)).round();
        Some((pcm, rate.clamp(4000.0, 96_000.0) as u32))
    }
}
