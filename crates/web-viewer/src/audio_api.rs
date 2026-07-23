//! `LegaiaAudio` WASM bindings for site/audio.html.
use super::*;

#[wasm_bindgen]
impl LegaiaAudio {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        Self {
            disc: Vec::new(),
            prot: Vec::new(),
            entries: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            audio_out: None,
            str_video: None,
        }
    }

    /// Load a full Mode2/2352 disc image. Extracts `PROT.DAT` via the same
    /// in-memory ISO walker the viewer uses, parses the TOC, and stashes
    /// both slices for later VAB / BGM / XA queries. Returns the PROT entry
    /// count for the JS UI.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<u32, JsValue> {
        let prot = disc::extract_prot_dat(&bytes).ok_or_else(|| {
            JsValue::from_str(
                "audio: not a Mode2/2352 disc image (the audio page requires a full .bin)",
            )
        })?;
        let entries = disc::parse_prot_toc(&prot)
            .ok_or_else(|| JsValue::from_str("audio: PROT.DAT TOC parse failed"))?;
        console_log(&format!(
            "Audio: loaded disc ({} MB), {} PROT entries",
            bytes.len() / 1024 / 1024,
            entries.len()
        ));
        self.entries = entries;
        self.prot = prot;
        self.disc = bytes;
        Ok(self.entries.len() as u32)
    }

    /// JSON list of every VAB sound bank in the loaded disc.
    /// Shape: `[{ prot_index, vab_offset, version, program_count, sample_count, has_seq }, ...]`.
    pub fn enumerate_vabs_json(&self) -> String {
        let v = audio::enumerate_vabs(&self.prot, &self.entries);
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                r#"{{"prot_index":{},"vab_offset":{},"version":{},"program_count":{},"sample_count":{},"has_seq":{}}}"#,
                x.prot_index,
                x.vab_offset,
                x.version,
                x.program_count,
                x.sample_count,
                x.has_seq,
            ));
        }
        s.push(']');
        s
    }

    /// JSON list of every BGM pair (`pBAV` + `pQES` in the same PROT entry).
    /// Shape: `[{ prot_index, vab_offset, seq_offset, program_count, sample_count, ppqn, bpm }, ...]`.
    pub fn enumerate_bgm_pairs_json(&self) -> String {
        let v = audio::enumerate_bgm_pairs(&self.prot, &self.entries);
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                r#"{{"prot_index":{},"vab_offset":{},"seq_offset":{},"program_count":{},"sample_count":{},"ppqn":{},"bpm":{:.1}}}"#,
                x.prot_index,
                x.vab_offset,
                x.seq_offset,
                x.program_count,
                x.sample_count,
                x.ppqn,
                x.bpm,
            ));
        }
        s.push(']');
        s
    }

    /// JSON list of every `*.STR` / `*.XA` file on the disc, with its raw LBA
    /// and byte size. Shape: `[{ path, lba, size }, ...]`.
    pub fn enumerate_xa_files_json(&self) -> String {
        let v = audio::enumerate_xa_files(&self.disc);
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            // Escape only the bare minimum (path is ASCII filename, no quotes).
            s.push_str(&format!(
                r#"{{"path":"{}","lba":{},"size":{}}}"#,
                x.path.replace('\\', "/"),
                x.lba,
                x.size,
            ));
        }
        s.push(']');
        s
    }

    /// Sample rate the JS side should use when playing a VAG-decoded buffer.
    pub fn vab_sample_rate(&self) -> u32 {
        audio::VAB_SAMPLE_RATE
    }

    /// JSON metadata for every VAG sample inside one VAB bank.
    /// Shape: `[{ size_bytes, decoded_samples, duration_ms }, ...]`.
    /// `decoded_samples` is the actual PCM length after walking the ADPCM
    /// blocks (which stop at the first loop-end / garbage block), so it
    /// reflects the audible length, not the raw on-disc body size. Useful
    /// for the UI to dim out tiny/zero-length samples that would be
    /// inaudible.
    pub fn vab_sample_list_json(&self, prot_index: u32, vab_offset: u32) -> String {
        let Some((report, _)) =
            audio::parse_vab_at(&self.prot, &self.entries, prot_index, vab_offset)
        else {
            return "[]".into();
        };
        let mut s = String::from("[");
        for (i, span) in report.vag_samples.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            // Decoding each sample once at enumeration time gives the
            // UI accurate duration. The cost is one full decode per
            // sample - fast in WASM, run once per bank-open.
            let decoded_len = audio::decode_vag_sample(
                &self.prot,
                &self.entries,
                prot_index,
                vab_offset,
                i as u32,
            )
            .map(|p| p.len())
            .unwrap_or(0);
            let duration_ms = (decoded_len as f64 * 1000.0 / audio::VAB_SAMPLE_RATE as f64) as u32;
            s.push_str(&format!(
                r#"{{"size_bytes":{},"decoded_samples":{},"duration_ms":{}}}"#,
                span.size, decoded_len, duration_ms,
            ));
        }
        s.push(']');
        s
    }

    /// Decode one VAG sample to mono i16 PCM at `vab_sample_rate()`.
    /// Empty when the sample doesn't exist or has zero length.
    pub fn decode_vab_sample_i16(
        &self,
        prot_index: u32,
        vab_offset: u32,
        sample_idx: u32,
    ) -> Vec<i16> {
        audio::decode_vag_sample(
            &self.prot,
            &self.entries,
            prot_index,
            vab_offset,
            sample_idx,
        )
        .unwrap_or_default()
    }

    /// Demux + decode an XA stream. Returns the decoded PCM of the first
    /// audio channel (file_no=0, ch_no=0 typically) along with metadata
    /// packed as JSON in the first method, then the PCM via this one.
    ///
    /// Two-step API so the JS side can show metadata (channels, sample rate)
    /// before paying the decode cost.
    pub fn xa_metadata_json(&self, lba: u32, size: u32) -> String {
        let streams = audio::decode_xa_in_memory(&self.disc, lba, size);
        let mut s = String::from("[");
        for (i, x) in streams.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                r#"{{"file_no":{},"ch_no":{},"sample_rate":{},"stereo":{},"sample_count":{}}}"#,
                x.file_no,
                x.ch_no,
                x.sample_rate,
                x.stereo,
                x.pcm.len(),
            ));
        }
        s.push(']');
        s
    }

    /// Decode XA stream and return the i16 PCM for the channel at `stream_idx`
    /// (index into the `xa_metadata_json` array). Empty when out of range.
    pub fn decode_xa_stream_i16(&self, lba: u32, size: u32, stream_idx: u32) -> Vec<i16> {
        let streams = audio::decode_xa_in_memory(&self.disc, lba, size);
        streams
            .into_iter()
            .nth(stream_idx as usize)
            .map(|x| x.pcm)
            .unwrap_or_default()
    }

    /// Open an `MV*.STR` movie for video playback. Demuxes every MDEC video
    /// frame's bitstream off the disc (skipping the interleaved audio) and
    /// caches them, keyed by `lba`. Returns JSON
    /// `{ "width", "height", "frame_count", "fps" }`. Frames are NOT decoded to
    /// RGBA here - call `str_decode_frame(idx)` per displayed frame so the page
    /// pays MDEC cost incrementally (a whole movie's RGBA is hundreds of MB).
    ///
    /// Idempotent for the same `lba`: a second open returns the cached metadata
    /// without re-walking the disc. `.XA` (audio-only) files have no video and
    /// come back with `frame_count: 0`.
    pub fn str_video_open(&mut self, lba: u32, size: u32) -> String {
        if self.str_video.as_ref().map(|(l, _)| *l) != Some(lba) {
            let video = audio::demux_str_video(&self.disc, lba, size);
            self.str_video = Some((lba, video));
        }
        let (_, video) = self.str_video.as_ref().unwrap();
        format!(
            r#"{{"width":{},"height":{},"frame_count":{},"fps":{:.4}}}"#,
            video.width,
            video.height,
            video.frames.len(),
            video.fps,
        )
    }

    /// Decode the frame at `frame_idx` of the currently-open STR movie to a
    /// row-major RGBA8 buffer (`width * height * 4` bytes). Empty when no movie
    /// is open or the index is out of range. Call `str_video_open` first.
    pub fn str_decode_frame(&self, frame_idx: u32) -> Vec<u8> {
        let Some((_, video)) = self.str_video.as_ref() else {
            return Vec::new();
        };
        video
            .frames
            .get(frame_idx as usize)
            .map(audio::decode_str_frame_rgba)
            .unwrap_or_default()
    }

    /// Drop the cached STR movie frames (frees the bitstream buffers).
    pub fn str_video_close(&mut self) {
        self.str_video = None;
    }

    /// Start BGM playback for the given (`prot_index`, `vab_offset`,
    /// `seq_offset`) tuple. Constructs the WebAudio output on the first call
    /// (must be invoked from a user-gesture handler), parses VAB + SEQ,
    /// uploads the bank to the embedded clean-room SPU, and attaches the
    /// sequencer.
    #[cfg(target_arch = "wasm32")]
    pub fn start_bgm(
        &mut self,
        prot_index: u32,
        vab_offset: u32,
        seq_offset: u32,
    ) -> Result<(), JsValue> {
        let e = self
            .entries
            .iter()
            .find(|x| x.index == prot_index)
            .ok_or_else(|| JsValue::from_str("start_bgm: PROT entry not found"))?;
        let off = e.byte_offset as usize;
        let end = (e.byte_offset + e.size_bytes) as usize;
        let buf = self
            .prot
            .get(off..end)
            .ok_or_else(|| JsValue::from_str("start_bgm: entry slice OOB"))?;

        let vab_report = legaia_vab::parse(buf, vab_offset as usize)
            .map_err(|e| JsValue::from_str(&format!("VAB parse: {e}")))?;
        let seq = legaia_seq::Seq::parse(&buf[seq_offset as usize..])
            .map_err(|e| JsValue::from_str(&format!("SEQ parse: {e}")))?;

        // Lazy WebAudio open. Browser autoplay policy requires this to run
        // inside a user gesture - the JS side wires this method up to a
        // button click.
        if self.audio_out.is_none() {
            let out = legaia_engine_audio::WebAudioOut::new()
                .map_err(|e| JsValue::from_str(&format!("WebAudioOut: {e}")))?;
            self.audio_out = Some(out);
        }
        let out = self.audio_out.as_ref().unwrap();

        // Upload bank into the SPU model (which lives inside WebAudioOut's
        // resampler). Then build the sequencer and attach.
        let bank = out.with_spu(|spu| {
            // Full SPU RAM (minus the 4 KB reserved head), as retail and the
            // asset-viewer audition path do - the old 256 KB cap could drop
            // the overflowing samples of a larger music VAB (silent voices).
            let mut alloc = legaia_engine_audio::spu::ram::SpuAllocator::new(
                0x1000,
                legaia_engine_audio::spu::ram::SPU_RAM_BYTES as u32 - 0x1000,
            );
            legaia_engine_audio::VabBank::upload(
                spu,
                &mut alloc,
                &vab_report,
                &buf[vab_offset as usize..],
            )
        });
        let mut sequencer = legaia_engine_audio::sequencer::Sequencer::new(seq, bank);
        // Loop to the start at end-of-track so BGM repeats instead of playing
        // once and stopping (matches the native BGM director's default).
        sequencer.set_loop_to(0);
        out.attach_sequencer(sequencer);
        Ok(())
    }

    /// Stop the currently-playing BGM. Safe to call even when nothing is
    /// playing (no-op).
    #[cfg(target_arch = "wasm32")]
    pub fn stop_bgm(&mut self) {
        if let Some(out) = self.audio_out.as_ref() {
            out.detach_sequencer();
        }
    }

    /// Resume the BGM AudioContext. Browsers often construct the
    /// `AudioContext` in `suspended` state even when the constructor
    /// runs inside a user-gesture handler; the JS side calls this
    /// immediately after `start_bgm` to make the audio actually audible.
    #[cfg(target_arch = "wasm32")]
    pub fn resume_bgm(&mut self) -> js_sys::Promise {
        match self.audio_out.as_ref() {
            Some(out) => out.resume(),
            None => js_sys::Promise::resolve(&JsValue::UNDEFINED),
        }
    }

    /// Pause / resume the active BGM sequencer. Notes that are already
    /// sounding decay through their ADSR envelopes; the sequencer clock
    /// freezes.
    #[cfg(target_arch = "wasm32")]
    pub fn set_bgm_paused(&mut self, paused: bool) {
        if let Some(out) = self.audio_out.as_ref() {
            out.set_sequencer_paused(paused);
        }
    }

    /// Set the BGM playback gain. Retail SEQ + clean-room SPU output sits
    /// around 1% of the i16 range, so the audio page defaults to ~25x to
    /// bring playback to a comfortable level. `1.0` matches the native
    /// engine-shell cpal path.
    #[cfg(target_arch = "wasm32")]
    pub fn set_bgm_gain(&mut self, gain: f32) {
        if let Some(out) = self.audio_out.as_ref() {
            out.set_gain(gain);
        }
    }

    /// Sample rate of the browser's BGM `AudioContext`, or 0 when the BGM
    /// output hasn't been opened yet. Surfaced to the JS console for
    /// diagnostics when playback speed is off.
    #[cfg(target_arch = "wasm32")]
    pub fn bgm_device_rate(&self) -> u32 {
        self.audio_out
            .as_ref()
            .map(|o| o.device_rate())
            .unwrap_or(0)
    }

    /// Render `duration_seconds` worth of interleaved stereo i16 PCM at
    /// the SPU's 44.1 kHz rate for the BGM pair at (`prot_index`,
    /// `vab_offset`, `seq_offset`). Used by the audio page to pre-render
    /// a chunk and play it through `AudioBufferSourceNode` (sample-
    /// accurate timing) instead of through `ScriptProcessorNode` (callback-
    /// paced, drifts on some browsers).
    pub fn render_bgm_pcm_i16(
        &self,
        prot_index: u32,
        vab_offset: u32,
        seq_offset: u32,
        duration_seconds: f32,
    ) -> Vec<i16> {
        let Some(e) = self.entries.iter().find(|x| x.index == prot_index) else {
            return Vec::new();
        };
        let off = e.byte_offset as usize;
        let end = (e.byte_offset + e.size_bytes) as usize;
        let Some(buf) = self.prot.get(off..end) else {
            return Vec::new();
        };
        let Ok(vab_report) = legaia_vab::parse(buf, vab_offset as usize) else {
            return Vec::new();
        };
        let Ok(seq) = legaia_seq::Seq::parse(&buf[seq_offset as usize..]) else {
            return Vec::new();
        };
        let mut spu = legaia_engine_audio::Spu::new();
        let mut alloc = legaia_engine_audio::spu::ram::SpuAllocator::new(
            0x1000,
            legaia_engine_audio::spu::ram::SPU_RAM_BYTES as u32 - 0x1000,
        );
        let bank = legaia_engine_audio::VabBank::upload(
            &mut spu,
            &mut alloc,
            &vab_report,
            &buf[vab_offset as usize..],
        );
        let mut sequencer = legaia_engine_audio::sequencer::Sequencer::new(seq, bank);
        // Loop at end-of-track so a pre-rendered chunk fills its full duration
        // for a track shorter than the request, rather than ending in silence.
        sequencer.set_loop_to(0);
        let duration_samples =
            (duration_seconds * legaia_engine_audio::SPU_INTERNAL_RATE as f32) as usize;
        legaia_engine_audio::render_bgm_to_pcm(&mut sequencer, &mut spu, duration_samples)
    }

    /// Sample rate produced by [`Self::render_bgm_pcm_i16`] (the SPU's
    /// internal 44.1 kHz). Surfaced so the JS side can build a correct
    /// WAV header for `decodeAudioData`.
    pub fn bgm_render_rate(&self) -> u32 {
        legaia_engine_audio::SPU_INTERNAL_RATE
    }
}

impl Default for LegaiaAudio {
    fn default() -> Self {
        Self::new()
    }
}
