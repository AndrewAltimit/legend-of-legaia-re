//! Audio preview through `legaia-engine-audio`: one-shot VAG samples and SEQ
//! playback on the from-scratch SPU + sequencer, opened lazily so a machine
//! without an audio device still runs the rest of the app.

use anyhow::{Context, Result};
use legaia_engine_audio::spu::ram::SPU_RAM_BYTES;
use legaia_engine_audio::{AudioOut, AudioSink, Sequencer, Spu, SpuAllocator, VabBank};

use crate::analyze::{Analysis, SeqRef, VabRef};

#[derive(Default)]
pub struct Player {
    out: Option<AudioOut>,
    /// What is playing, for the UI ("sample 3", "SEQ @ 0x1234").
    pub now_playing: Option<String>,
}

impl Player {
    fn out(&mut self) -> Result<&AudioOut> {
        if self.out.is_none() {
            self.out = Some(AudioOut::new().context("opening the audio output device")?);
        }
        Ok(self.out.as_ref().expect("just opened"))
    }

    pub fn play_pcm(&mut self, pcm: Vec<i16>, rate: u32, label: String) -> Result<()> {
        self.stop();
        self.out()?.play_pcm_mono(pcm, rate);
        self.now_playing = Some(label);
        Ok(())
    }

    /// Play `seq` with the instruments of bank `vab`, both from entry `a`.
    pub fn play_seq(
        &mut self,
        a: &Analysis,
        vab: &VabRef,
        seq: &SeqRef,
        looped: bool,
    ) -> Result<()> {
        self.stop();
        let song = legaia_seq::Seq::parse(&a.bytes[seq.offset..]).context("parse SEQ")?;
        let out = self.out()?;
        let bank = out.with_spu(|spu: &mut Spu| {
            let mut alloc = SpuAllocator::new(0x1000, SPU_RAM_BYTES as u32 - 0x1000);
            VabBank::upload(spu, &mut alloc, &vab.report, &a.bytes)
        });
        let mut sequencer = Sequencer::new(song, bank);
        if looped {
            sequencer.set_loop_to(0);
        }
        out.attach_sequencer(sequencer);
        self.now_playing = Some(format!(
            "SEQ @ 0x{:X} with VAB @ 0x{:X}",
            seq.offset, vab.offset
        ));
        Ok(())
    }

    /// `(tick, bpm, finished)` of the playing SEQ.
    pub fn seq_progress(&self) -> Option<(u64, f32, bool)> {
        let p = self.out.as_ref()?.sequencer_progress()?;
        Some((p.tick, p.bpm, p.finished))
    }

    pub fn stop(&mut self) {
        if let Some(out) = &self.out {
            out.detach_sequencer();
            out.stop();
        }
        self.now_playing = None;
    }
}
