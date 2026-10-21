//! The slot machine's own sound cues, decoded off the disc's runtime bank.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    // ------------------------------------------------------------- slot sound
    //
    // The machine's own cues, decoded off the disc: the reel-stop click, the
    // payout tick, the reach sting. These are runtime-bank ids (>= 0x200), so
    // they resolve through the slot overlay's `efect.dat` descriptor block
    // (PROT 1199) into the VAB it loads alongside it (PROT 1198). Nothing here
    // is a substitute sound - if a cue does not resolve, the page stays silent.

    /// The cue ids this disc's slot bank actually defines, with the VAB voice
    /// each one keys:
    ///
    /// ```json
    /// [ { "id": 522, "program": 1, "tone": 6, "note": 66, "rate": 46616 }, ... ]
    /// ```
    ///
    /// `id` is decimal (`522` = `0x20A`, the reel-stop click).
    pub fn slot_sfx_json(&self) -> String {
        let Some(bank) = self.slot_sfx.as_ref() else {
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

    /// Decode one cue to mono PCM (`i16`). Empty when the cue isn't in the bank.
    pub fn slot_sfx_pcm(&self, cue: u16) -> Vec<i16> {
        self.slot_sfx
            .as_ref()
            .and_then(|b| b.decode(cue).ok())
            .map(|(pcm, _)| pcm)
            .unwrap_or_default()
    }

    /// The rate [`Self::slot_sfx_pcm`]'s samples must be played back at - the
    /// cue's note against the VAG's own centre note *is* the pitch, so this
    /// carries it. `0` when the cue isn't in the bank.
    pub fn slot_sfx_rate(&self, cue: u16) -> u32 {
        self.slot_sfx
            .as_ref()
            .and_then(|b| b.decode(cue).ok())
            .map(|(_, rate)| rate)
            .unwrap_or(0)
    }

    /// The reel-spin motor **loop**, mono i16. Not a ring cue: the reel SM
    /// keys class-2 VAB program 1 / tone 0 at note `0x3C` directly
    /// (`FUN_801CF0D8` -> `func_0x80065034(0x13, 2, 1, 0, 0x3C, ...)`) and
    /// releases the voice on all-reels-stop - the page loops this buffer for
    /// as long as the reels turn. Empty when the VAB didn't decode.
    pub fn slot_spin_pcm(&self) -> Vec<i16> {
        self.slot_spin_tone()
            .map(|(pcm, _)| pcm)
            .unwrap_or_default()
    }

    /// Playback rate for [`Self::slot_spin_pcm`] (`0` when absent).
    pub fn slot_spin_rate(&self) -> u32 {
        self.slot_spin_tone().map(|(_, r)| r).unwrap_or(0)
    }

    /// The retail cue ids, so the page never has to hard-code a number:
    /// `{"reel_stop":522,"payout_tick":521,"reach":512,"reach1":513,"reach2":514}`.
    pub fn slot_sfx_cue_ids(&self) -> String {
        format!(
            r#"{{"reel_stop":{},"payout_tick":{},"reach":{},"reach1":{},"reach2":{}}}"#,
            minigame_sfx::CUE_SLOT_REEL_STOP,
            minigame_sfx::CUE_SLOT_PAYOUT_TICK,
            minigame_sfx::CUE_SLOT_REACH,
            minigame_sfx::CUE_SLOT_REACH_1,
            minigame_sfx::CUE_SLOT_REACH_2,
        )
    }
}

impl LegaiaMinigames {
    /// Decode the reel-spin motor tone (see [`Self::slot_spin_pcm`]).
    fn slot_spin_tone(&self) -> Option<(Vec<i16>, u32)> {
        self.slot_sfx.as_ref().and_then(|b| {
            b.decode_tone(
                minigame_sfx::SLOT_SPIN_PROGRAM,
                minigame_sfx::SLOT_SPIN_TONE,
                minigame_sfx::SLOT_SPIN_NOTE,
            )
            .ok()
        })
    }
}
