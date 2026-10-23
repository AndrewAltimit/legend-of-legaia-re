//! Minigame BGM: the `music_01` `[VAB][SEQ]` pair located on the disc and
//! rendered offline to PCM, plus the dance jukebox.

use super::*;

// ------------------------------------------------------------- minigame BGM

/// True when a `music_01` entry carries the wrapped `[VAB][SEQ]` pair the
/// retail BGM path streams, located by the installer walk both play hosts use
/// (`chunk_install::owned_bank_offsets`) rather than a magic hunt - a hunt
/// stops at the first `pQES` byte run after the bank, which inside a VAG body
/// is sample data.
pub(crate) fn music01_pair_ok(buf: &[u8]) -> bool {
    legaia_engine_system::chunk_install::owned_bank_offsets(buf).is_some()
}

/// Stage one `music_01` entry's `[VAB][SEQ]` pair through the from-scratch SPU:
/// locate the bank and score with the installer walk
/// (`chunk_install::owned_bank_offsets`), upload the VAB into SPU RAM, and
/// build a [`Sequencer`] over it - the exact path the engine's BGM director
/// takes.
/// `None` when the pair doesn't decode. The returned sequencer has a
/// loop-to-0 fallback installed so marker-less tracks still repeat (retail
/// BGM loops), which the loop-region detector relies on.
fn stage_music01_seq(
    buf: &[u8],
) -> Option<(
    legaia_engine_audio::Spu,
    legaia_engine_audio::sequencer::Sequencer,
)> {
    let split = legaia_engine_system::chunk_install::owned_bank_offsets(buf)?;
    let vab_off = split.vab;
    let vab_report = legaia_vab::parse(buf, vab_off).ok()?;
    let seq = legaia_seq::Seq::parse(buf.get(split.seq..)?).ok()?;
    // The same global reverb every live host's mixer runs
    // (`StreamResampler`): Studio C at the retail depth, each voice's send
    // set per keyed tone. A bare `Spu::new()` renders the minigame pages'
    // music dry while the native host and the play page play it wet.
    let mut spu = legaia_engine_audio::Spu::new();
    spu.set_retail_reverb();
    let mut alloc = legaia_engine_audio::spu::ram::SpuAllocator::new(0x1000, 0x40_000);
    let bank =
        legaia_engine_audio::VabBank::upload(&mut spu, &mut alloc, &vab_report, &buf[vab_off..]);
    let mut sequencer = legaia_engine_audio::sequencer::Sequencer::new(seq, bank);
    // The sequence volume every live BGM director gives a track.
    sequencer.set_master_vol(legaia_engine_audio::sequencer::RETAIL_BGM_SEQ_VOL);
    // In-stream loop markers take precedence; this is the fallback for the
    // (few) tracks with no markers so every jukebox track loops.
    sequencer.set_loop_to(0);
    Some((spu, sequencer))
}

/// Render one `music_01` entry's `[VAB][SEQ]` pair to `seconds` of
/// interleaved-stereo i16 PCM at the SPU's 44.1 kHz. Empty when the pair
/// doesn't decode.
pub(crate) fn render_music01_bgm(buf: &[u8], seconds: f32) -> Vec<i16> {
    let Some((mut spu, mut sequencer)) = stage_music01_seq(buf) else {
        return Vec::new();
    };
    let samples =
        (seconds.clamp(1.0, 120.0) * legaia_engine_audio::SPU_INTERNAL_RATE as f32) as usize;
    legaia_engine_audio::render_bgm_to_pcm(&mut sequencer, &mut spu, samples)
}

/// Render one `music_01` entry to a **seamless loop slice** (one true SEQ loop
/// period) plus the loop region the browser sets `loopStart`/`loopEnd` from.
/// Bounds the render at `max_seconds`. `None` when the pair doesn't decode.
fn render_music01_loop(buf: &[u8], max_seconds: f32) -> Option<legaia_engine_audio::BgmLoopRender> {
    let (mut spu, mut sequencer) = stage_music01_seq(buf)?;
    let samples =
        (max_seconds.clamp(1.0, 120.0) * legaia_engine_audio::SPU_INTERNAL_RATE as f32) as usize;
    Some(legaia_engine_audio::render_bgm_loop_region(
        &mut sequencer,
        &mut spu,
        samples,
    ))
}

/// One rendered `music_01` track handed to the site jukebox: seamless-loop PCM
/// plus the loop region and sample rate. Getters copy into JS typed arrays; a
/// consumer calls `pcm` once. `ok` is false when the entry didn't decode.
#[wasm_bindgen]
pub struct Music01Render {
    pcm: Vec<i16>,
    loop_start: u32,
    loop_end: u32,
    rate: u32,
}

#[wasm_bindgen]
impl Music01Render {
    /// Interleaved-stereo i16 PCM (`[l0, r0, l1, r1, ...]`) at [`Self::rate`].
    #[wasm_bindgen(getter)]
    pub fn pcm(&self) -> Vec<i16> {
        self.pcm.clone()
    }
    /// Frame index where the repeatable loop body starts (`AudioBufferSourceNode.loopStart`
    /// = `loop_start / rate`). `0` when no loop region was found.
    #[wasm_bindgen(getter)]
    pub fn loop_start(&self) -> u32 {
        self.loop_start
    }
    /// Frame index where the loop body ends (`loopEnd`); one SEQ period after
    /// [`Self::loop_start`]. Equals the frame length of [`Self::pcm`].
    #[wasm_bindgen(getter)]
    pub fn loop_end(&self) -> u32 {
        self.loop_end
    }
    /// PCM sample rate (the SPU's 44.1 kHz).
    #[wasm_bindgen(getter)]
    pub fn rate(&self) -> u32 {
        self.rate
    }
    /// Whether the track decoded (non-empty PCM).
    #[wasm_bindgen(getter)]
    pub fn ok(&self) -> bool {
        !self.pcm.is_empty()
    }
}

/// The dance's disco jukebox: the two tracks the dance overlay itself loads
/// (mode-selected, extraction 1048/1054) plus the rest of the Sol-disco family
/// that plays on the casino/disco floor around it - all live in the same
/// `music_01` bank. Each row is `(global BGM id, role)`; the human label comes
/// from [`music_labels`]. Ids that don't decode on a given disc are dropped
/// from [`LegaiaMinigames::dance_jukebox_json`].
///
/// The bank map is **piecewise**, so an extraction entry is `988 + slot` in
/// this range, not `990 + slot`: the overlay's two songs at extraction
/// 1048/1054 are sound-test slots 60/66 (`M116`/`M120`, "Sol disco final"
/// 1 and 2) = ids 2060/2066. They are the same two tracks the floor rows
/// name, so the roles are merged here rather than listed twice. The first
/// row is the jukebox default the page selects.
const DANCE_JUKEBOX: &[(u16, &str)] = &[
    (2060, "Dance stage (overlay track A) - Sol disco final"),
    (2066, "Dance stage (overlay track B) - Sol disco final II"),
    (2055, "Sol disco floor"),
    (2059, "Sol disco - qualifier"),
];

/// The disc-pinned BGM source for one browser minigame: `(prot_index, why)`.
///
/// - `baka`: the duel overlay starts its own track - `FUN_801CF00C` loads raw
///   loader index `0x415` = extraction 1043 (`music_01` sound-test slot **55**,
///   `M112` "Sol disco fever" - the Baka Fighter is a Sol minigame). The bank
///   map is piecewise, so extraction 1043 is slot 55, not 53; the "boss
///   overture" label came from the superseded `990 + slot` base. See
///   `docs/subsystems/minigame-baka-fighter.md`.
/// - `slot`: the overlay starts **no** track (it inherits the host scene's);
///   the casino floor `0543_koin1` starts BGM id 2018 via field-VM op `0x35`
///   = `music_01` slot 18, "Sol casino". See
///   `docs/subsystems/minigame-slot-machine.md`.
/// - `dance` is served by its own pair of methods (the overlay picks one of
///   two songs by mode; see `dance_presentation.rs`).
fn minigame_bgm_source(game: &str) -> Option<(u32, &'static str)> {
    match game {
        "baka" => Some((
            legaia_minigame_formats::baka_opponents::BAKA_BGM_PROT_INDEX as u32,
            "overlay init FUN_801CF00C loads music_01 slot 55 (Sol disco fever)",
        )),
        "slot" => Some((
            legaia_minigame_formats::slot_payout::SLOT_HOST_BGM_PROT_INDEX as u32,
            "host scene 0543_koin1 op-0x35 BGM 2018 = music_01 slot 18 (Sol casino)",
        )),
        _ => None,
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Whether `game`'s BGM (`"slot"` / `"baka"`) resolves on this disc:
    /// `{"ok":true,"prot":1043,"why":"..."}`. The dance's two-song check is
    /// [`Self::dance_bgm_ready_json`].
    pub fn minigame_bgm_ready_json(&self, game: &str) -> String {
        let Some((idx, why)) = minigame_bgm_source(game) else {
            return r#"{"ok":false}"#.to_string();
        };
        let ok = entry_bytes(&self.prot, &self.entries, idx)
            .map(music01_pair_ok)
            .unwrap_or(false);
        format!(r#"{{"ok":{ok},"prot":{idx},"why":{}}}"#, jstr(why))
    }

    /// Render `seconds` of `game`'s BGM to interleaved-stereo i16 PCM at
    /// [`Self::minigame_bgm_rate`]. Empty when the entry didn't decode.
    pub fn minigame_bgm_pcm_i16(&self, game: &str, seconds: f32) -> Vec<i16> {
        let Some((idx, _)) = minigame_bgm_source(game) else {
            return Vec::new();
        };
        let Some(buf) = entry_bytes(&self.prot, &self.entries, idx) else {
            return Vec::new();
        };
        render_music01_bgm(buf, seconds)
    }

    /// Sample rate of [`Self::minigame_bgm_pcm_i16`] (the SPU's 44.1 kHz).
    pub fn minigame_bgm_rate(&self) -> u32 {
        legaia_engine_audio::SPU_INTERNAL_RATE
    }

    /// Resolve a global-pool BGM id (`>= 2000`) to its `music_01` entry bytes,
    /// through the piecewise bank map (the bank is not a single linear run).
    fn music01_entry_for_bgm(&self, bgm_id: u16) -> Option<&[u8]> {
        let entry = legaia_engine_system::music_labels::prot_entry_for_bgm_id(bgm_id)?;
        entry_bytes(&self.prot, &self.entries, entry)
    }

    /// Render a global-pool BGM id (`2000 + sound-test slot`) to a **seamless
    /// loop** render: PCM plus the loop region the browser drives
    /// `loopStart`/`loopEnd` from. This is the jukebox / minigame playback
    /// path, superseding the fixed-window [`Self::minigame_bgm_pcm_i16`] hard
    /// loop. Bounds the render at `max_seconds`. Returns an empty render when
    /// the id isn't a bank slot or its `[VAB][SEQ]` pair doesn't decode.
    pub fn music01_bgm_render(&self, bgm_id: u16, max_seconds: f32) -> Music01Render {
        let empty = Music01Render {
            pcm: Vec::new(),
            loop_start: 0,
            loop_end: 0,
            rate: legaia_engine_audio::SPU_INTERNAL_RATE,
        };
        let Some(buf) = self.music01_entry_for_bgm(bgm_id) else {
            return empty;
        };
        match render_music01_loop(buf, max_seconds) {
            Some(r) => Music01Render {
                pcm: r.pcm,
                loop_start: r.loop_start_sample as u32,
                loop_end: r.loop_end_sample as u32,
                rate: legaia_engine_audio::SPU_INTERNAL_RATE,
            },
            None => empty,
        }
    }

    /// The dance's disco jukebox as JSON: one row per selectable track that
    /// decodes on this disc -
    /// `{"tracks":[{"bgm":2058,"label":"M114 - ...","role":"Dance stage (overlay track A)"},...]}`.
    /// The first two rows are the tracks the dance overlay actually loads
    /// (mode-selected, extraction 1048/1054); the rest are the Sol-disco
    /// floor family in the same bank. Rows whose `[VAB][SEQ]` pair is absent
    /// (e.g. a track dropped from the NA disc) are omitted.
    pub fn dance_jukebox_json(&self) -> String {
        let mut rows = Vec::new();
        for &(bgm, role) in DANCE_JUKEBOX {
            let Some(buf) = self.music01_entry_for_bgm(bgm) else {
                continue;
            };
            if !music01_pair_ok(buf) {
                continue;
            }
            let label = legaia_engine_system::music_labels::label_for_bgm_id(bgm)
                .unwrap_or_else(|| format!("BGM {bgm}"));
            rows.push(format!(
                r#"{{"bgm":{bgm},"label":{},"role":{}}}"#,
                jstr(&label),
                jstr(role)
            ));
        }
        format!(r#"{{"tracks":[{}]}}"#, rows.join(","))
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// The global BGM id the page plays for `game` (`"baka"`, `"slot"`,
    /// `"muscle"`, `"dance"`, `"fishing"`), or `-1` for none - the engine's
    /// `MinigameSubId::standalone_bgm_id`, so the page carries no track
    /// literal of its own.
    pub fn minigame_bgm_id(&self, game: &str) -> i32 {
        use legaia_engine_mode::minigame_entry::MinigameSubId as M;
        let sub = match game {
            "baka" => M::BakaFighter,
            "slot" => M::SlotMachine,
            "muscle" => M::MuscleDome,
            "dance" => M::Dance,
            "fishing" => M::Fishing,
            _ => return -1,
        };
        sub.standalone_bgm_id().map_or(-1, i32::from)
    }
}
