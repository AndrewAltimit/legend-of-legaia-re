//! FMV (STR / MDEC) beats on the play page.
//!
//! The field VM arms a movie by parking the world in `SceneMode::Cutscene`
//! with `cutscene.active_fmv` set, and the title's attract countdown arms
//! `fmv_id 0` the same way through its own session. The native window plays
//! the movie through `crates/mdec` + the XA lane (`window/str_player.rs`,
//! `engine-shell/src/cutscene_av.rs`); this module is the browser twin.
//!
//! ## Division of labour
//!
//! The runtime does not keep the raw disc (the page does), so playback is a
//! three-way handshake per movie:
//!
//! 1. **Want.** The runtime resolves the movie's raw-sector window - the
//!    `MV*.STR` file's extent off the disc walk, narrowed to the `fmv_id`'s
//!    frame range through the same
//!    [`legaia_asset::fmv_dispatch::fmv_segment_window`] the native window
//!    seeks with - and publishes it as [`LegaiaRuntime::play_fmv_wanted_json`].
//! 2. **Install.** The page slices those sectors out of its disc bytes and
//!    hands them to [`LegaiaRuntime::play_fmv_install`], which demuxes the
//!    video frames (decoded lazily, one MDEC frame per request) and decodes
//!    the interleaved XA track to PCM - the media page's own `audio.rs`
//!    kernels, so the play page and the media page share one STR reader.
//! 3. **Play + finish.** The page draws [`LegaiaRuntime::play_fmv_frame_rgba`]
//!    onto an overlay canvas, asks [`LegaiaRuntime::play_fmv_audio_start`]
//!    to put the XA track on the engine mixer's XA lane - the native
//!    window's `play_xa(track, 0x4000)`, so the movie sits behind the same
//!    [`legaia_engine_audio::webaudio::WEB_MASTER_TRIM`] and volume slider
//!    as BGM and SFX instead of a second, untrimmed `AudioContext` - clocks
//!    the frame index off [`LegaiaRuntime::play_fmv_audio_cursor_secs`]
//!    (the native `due_video_frame` rule: the picture follows the
//!    soundtrack) and calls [`LegaiaRuntime::play_fmv_finish`] when the last
//!    frame has shown. A page with no engine audio up falls back to
//!    [`LegaiaRuntime::play_fmv_audio_pcm_i16`] and its own context, carrying
//!    the site's trim itself. While a movie is open the world is **held**: the field VM stays
//!    suspended under `SceneMode::Cutscene` and nothing finishes the cutscene
//!    until the page says so. Then the existing post-movie hand-off
//!    (`SceneHost::apply_pending_fmv_handoff`) runs exactly as before.
//!
//! ## Fallbacks (the page never installs)
//!
//! A cached `play-app.js` that predates this lane never loads the FMV script,
//! so it never calls [`LegaiaRuntime::play_fmv_set_supported`]: the runtime
//! then auto-finishes the movie the frame it arms - the pre-existing
//! behaviour, hand-off included. A page that *did* declare support but never
//! installs (the file is missing from the disc walk, the slice failed) is
//! bounded by [`INSTALL_TIMEOUT_FRAMES`]; an install that yields no frames
//! finishes the beat on the next service tick. Skipping the *movie* is never
//! skipping the *hand-off*.
//!
//! ## Skip
//!
//! Retail's play loop `FUN_801CF098` polls the pad only when the slot's id is
//! `0` (`lh v0,-0x4588(v0)` = `_DAT_8007BA78`, `bne v0,zero` back into the
//! loop at `0x801CF4E8`), and then aborts on `_DAT_8007B850 & 0x1F0` -
//! Legaia's packed pad word, where bits 4..8 are Triangle / Circle / Cross /
//! Square / Select. So the attract movie skips on any face button or Select
//! and every mid-game FMV plays out; [`skip_edge_hit`] is that rule over the
//! standard PSX edge word the page and the world already speak. `see
//! ghidra/scripts/funcs/str0970_801cf098.txt` (`0x801CF4D4..0x801CF500`).

use legaia_asset::fmv_dispatch::{FmvTable, STR_OVERLAY_PROT_INDEX, fmv_segment_window};
use legaia_engine_core::cutscene::fmv_is_skippable;
use legaia_engine_core::input::InputState;
#[cfg(test)]
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SceneMode;
use wasm_bindgen::prelude::*;

use crate::audio::{
    DecodedXa, StrVideo, decode_str_frame_rgba, decode_xa_in_memory, demux_str_video,
};
use crate::disc::FileEntry;
use crate::runtime::LegaiaRuntime;

/// Raw Mode-2 sector size the page slices the disc image by.
pub const RAW_SECTOR_SIZE: usize = 2352;
/// ISO9660 user-data size the file extent is declared in.
const USER_DATA_SIZE: u32 = 2048;
/// Frames a *supported* page gets to install the wanted movie before the
/// runtime gives up and finishes the beat unplayed (ten seconds at the sim
/// rate). The unsupported case does not wait at all.
pub const INSTALL_TIMEOUT_FRAMES: u32 = 600;

/// The buttons retail's `0x1F0` packed-mask test covers - re-exported from
/// the shared kernel so this host's tests keep their name.
pub use legaia_engine_core::cutscene::FMV_SKIP_BUTTONS as SKIP_BUTTONS;
/// Retail's abort test: a skippable `fmv_id`, on any of [`SKIP_BUTTONS`].
/// `edge` is a just-pressed PSX pad word. Lives in
/// [`legaia_engine_core::cutscene`] so the native window can make the same
/// decision.
pub use legaia_engine_core::cutscene::fmv_skip_edge_hit as skip_edge_hit;

/// The same test over the world's own pad edge (the page feeds `set_pad`
/// every tick, so `just_pressed` is the retail newly-pressed word).
fn skip_edge_from_input(fmv_id: i16, input: &InputState) -> bool {
    fmv_is_skippable(fmv_id) && SKIP_BUTTONS.iter().any(|b| input.just_pressed(*b))
}

/// Who armed the movie: the field VM's cutscene mode or the title attract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FmvOrigin {
    Cutscene,
    Attract,
}

/// A movie the runtime wants the page to install: the raw-sector window of
/// the `fmv_id`'s segment on the disc image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WantedFmv {
    pub fmv_id: i16,
    /// Engine-shape path (`MOV/MV1.STR`).
    pub path: String,
    /// Absolute LBA of the first raw sector to slice.
    pub first_sector: u32,
    /// Raw sectors to slice.
    pub sector_count: u32,
}

/// An installed movie: demuxed video (decoded per frame) + decoded audio.
pub(crate) struct OpenFmv {
    video: StrVideo,
    audio: Option<DecodedXa>,
    /// The PCM is handed over exactly once - to the engine mixer
    /// (`play_fmv_audio_start`) or to the page (`play_fmv_audio_pcm_i16`);
    /// the descriptor (rate / channels) stays readable.
    audio_handed: bool,
}

/// Unity XA gain (Q1.14), what the native movie players pass.
const FMV_XA_GAIN_UNITY: u16 = 0x4000;

/// One armed movie, from arming to finish.
pub(crate) struct FmvSlot {
    fmv_id: i16,
    origin: FmvOrigin,
    wanted: Option<WantedFmv>,
    open: Option<OpenFmv>,
    finish_requested: bool,
    wait_frames: u32,
}

/// What the per-frame poll decided for the armed movie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FmvPoll {
    /// Keep the world held - the page is installing or playing.
    Hold,
    /// The beat is over; `played` says whether a movie actually ran (false =
    /// the unsupported / timed-out / undecodable fallback).
    Finished { played: bool },
}

/// Per-runtime movie state.
#[derive(Default)]
pub(crate) struct FmvState {
    /// Set by the page at boot; without it every movie auto-finishes.
    supported: bool,
    /// The dispatch table off PROT 0970, resolved once per runtime.
    table: Option<FmvTable>,
    table_resolved: bool,
    slot: Option<FmvSlot>,
    /// The open movie's track is on the engine mixer's XA lane, so the
    /// finish (or skip) has to take it off again.
    mixer_audio: bool,
}

impl FmvState {
    /// `(origin, fmv_id)` of the armed movie, if any.
    pub(crate) fn armed_for(&self) -> Option<(FmvOrigin, i16)> {
        self.slot.as_ref().map(|s| (s.origin, s.fmv_id))
    }

    /// Arm a movie. `wanted == None` means the window could not be resolved
    /// (file not on the disc walk / cut slot): the next poll finishes it.
    pub(crate) fn arm(&mut self, fmv_id: i16, origin: FmvOrigin, wanted: Option<WantedFmv>) {
        self.slot = Some(FmvSlot {
            fmv_id,
            origin,
            wanted,
            open: None,
            finish_requested: false,
            wait_frames: 0,
        });
    }

    pub(crate) fn clear(&mut self) {
        self.slot = None;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.slot.as_ref().is_some_and(|s| s.open.is_some())
    }

    fn open(&self) -> Option<&OpenFmv> {
        self.slot.as_ref().and_then(|s| s.open.as_ref())
    }

    /// The window still waiting for the page (`None` once installed / never
    /// resolvable).
    pub(crate) fn wanted(&self) -> Option<&WantedFmv> {
        self.slot.as_ref().and_then(|s| s.wanted.as_ref())
    }

    /// The page (or the skip edge) says the movie is over. No-op unless a
    /// movie is open - a finish for a beat still waiting on install is the
    /// timeout's job, not the page's.
    pub(crate) fn request_finish(&mut self) {
        if let Some(s) = self.slot.as_mut()
            && s.open.is_some()
        {
            s.finish_requested = true;
        }
    }

    /// Advance the armed movie one frame. Drops the slot on `Finished`.
    pub(crate) fn poll(&mut self) -> FmvPoll {
        let Some(slot) = self.slot.as_mut() else {
            return FmvPoll::Finished { played: false };
        };
        let verdict = if slot.open.is_some() {
            if slot.finish_requested {
                FmvPoll::Finished { played: true }
            } else {
                FmvPoll::Hold
            }
        } else if slot.wanted.is_some() {
            if !self.supported {
                FmvPoll::Finished { played: false }
            } else {
                slot.wait_frames = slot.wait_frames.saturating_add(1);
                if slot.wait_frames > INSTALL_TIMEOUT_FRAMES {
                    FmvPoll::Finished { played: false }
                } else {
                    FmvPoll::Hold
                }
            }
        } else {
            // Unresolvable window, or an install that yielded nothing.
            FmvPoll::Finished { played: false }
        };
        if matches!(verdict, FmvPoll::Finished { .. }) {
            self.slot = None;
        }
        verdict
    }

    /// Open the wanted movie from its raw sectors. `false` when nothing is
    /// wanted or the slice holds no decodable video frame - in which case
    /// the beat finishes unplayed on the next poll.
    pub(crate) fn install(&mut self, sectors: &[u8]) -> bool {
        let Some(slot) = self.slot.as_mut() else {
            return false;
        };
        let Some(wanted) = slot.wanted.take() else {
            return false;
        };
        if sectors.is_empty() || !sectors.len().is_multiple_of(RAW_SECTOR_SIZE) {
            crate::console_log(&format!(
                "fmv: install of {} rejected - {} bytes is not a whole number of raw sectors",
                wanted.path,
                sectors.len()
            ));
            return false;
        }
        let sector_count = (sectors.len() / RAW_SECTOR_SIZE) as u32;
        if sector_count != wanted.sector_count {
            crate::console_log(&format!(
                "fmv: {} installed {} sectors, wanted {}",
                wanted.path, sector_count, wanted.sector_count
            ));
        }
        let byte_size = sector_count * USER_DATA_SIZE;
        let video = demux_str_video(sectors, 0, byte_size);
        if video.frames.is_empty() {
            crate::console_log(&format!(
                "fmv: {} demuxed no video frames; finishing the beat unplayed",
                wanted.path
            ));
            return false;
        }
        // The cutscene's single track is the dominant channel, as native.
        let audio = decode_xa_in_memory(sectors, 0, byte_size)
            .into_iter()
            .max_by_key(|a| a.pcm.len());
        crate::console_log(&format!(
            "fmv: {} open - {} frames {}x{} @ {:.2} fps, audio: {}",
            wanted.path,
            video.frames.len(),
            video.width,
            video.height,
            video.fps,
            audio
                .as_ref()
                .map(|a| format!(
                    "{} Hz {}",
                    a.sample_rate,
                    if a.stereo { "stereo" } else { "mono" }
                ))
                .unwrap_or_else(|| "none".to_string())
        ));
        slot.open = Some(OpenFmv {
            video,
            audio,
            audio_handed: false,
        });
        true
    }
}

/// Resolve the raw-sector window for `fmv_id`: the `MV*.STR` extent off the
/// disc walk, narrowed to the id's frame range by the dispatch table in PROT
/// 0970 - the same two lookups the native window's `decode_fmv` makes.
/// `None` when the slot is cut or the file is not on the loaded image (a
/// PROT.DAT-only load walks no files).
fn resolve_wanted(
    state: &mut FmvState,
    host: &SceneHost,
    files: &[FileEntry],
    fmv_id: i16,
) -> Option<WantedFmv> {
    let rel = legaia_engine_core::cutscene::fmv_index_to_str_filename(fmv_id)?;
    let want = rel.to_ascii_uppercase();
    let file = files
        .iter()
        .find(|f| f.path.trim_start_matches('/').to_ascii_uppercase() == want)?;
    if !state.table_resolved {
        state.table_resolved = true;
        state.table = host
            .index
            .entry_bytes(STR_OVERLAY_PROT_INDEX)
            .ok()
            .and_then(|b| FmvTable::from_str_overlay(&b[..]));
        if state.table.is_none() {
            crate::console_log("fmv: PROT 0970 dispatch table unavailable; playing whole files");
        }
    }
    let total = file.size.div_ceil(USER_DATA_SIZE);
    let (first_sector, sector_count) = fmv_segment_window(
        state.table.as_ref().and_then(|t| t.entry(fmv_id)),
        file.lba,
        total,
    );
    Some(WantedFmv {
        fmv_id,
        path: rel.to_string(),
        first_sector,
        sector_count,
    })
}

impl LegaiaRuntime {
    /// Arm `fmv_id` for `origin`, resolving its sector window off the loaded
    /// disc. Shared by the cutscene service and the title attract.
    pub(crate) fn fmv_arm(&mut self, fmv_id: i16, origin: FmvOrigin) {
        let wanted = self
            .scene_host
            .as_ref()
            .and_then(|host| resolve_wanted(&mut self.fmv, host, &self.disc_files, fmv_id));
        match wanted.as_ref() {
            Some(w) => crate::console_log(&format!(
                "fmv: armed fmv_id={fmv_id} {} sectors {}..+{} ({})",
                w.path,
                w.first_sector,
                w.sector_count,
                if self.fmv.supported {
                    "waiting for the page to install"
                } else {
                    "page has no FMV playback; finishing unplayed"
                }
            )),
            None => crate::console_log(&format!(
                "fmv: fmv_id={fmv_id} has no movie on the loaded image (cut slot / no disc walk); \
                 finishing unplayed"
            )),
        }
        self.fmv.arm(fmv_id, origin, wanted);
    }

    /// Pause the scene sequencer under the movie, the way the native window
    /// does when it stages the XA track. Not resumed here: the post-movie
    /// hand-off enters a scene whose script starts its own BGM (which
    /// un-pauses), exactly as native; the attract resumes explicitly.
    fn fmv_pause_sequencer(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(out) = self.audio_out.as_ref() {
            out.set_sequencer_paused(true);
        }
    }

    pub(crate) fn fmv_resume_sequencer(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(out) = self.audio_out.as_ref() {
            out.set_sequencer_paused(false);
        }
    }

    /// Put the open movie's XA track on the engine mixer - the native
    /// window's `out.play_xa(track, 0x4000)` - so it rides the page's
    /// volume slider and the web master trim like every other sound. Hands
    /// the PCM over once; `false` when there is nothing to stage or no engine
    /// audio to stage it on (the page then falls back to its own context).
    fn fmv_audio_stage(&mut self) -> bool {
        let Some(open) = self.fmv.slot.as_mut().and_then(|s| s.open.as_mut()) else {
            return false;
        };
        if open.audio_handed {
            return false;
        }
        let Some(audio) = open.audio.as_ref() else {
            return false;
        };
        #[cfg(target_arch = "wasm32")]
        if let Some(out) = self.audio_out.as_ref() {
            let channels = if audio.stereo {
                legaia_xa::Channels::Stereo
            } else {
                legaia_xa::Channels::Mono
            };
            out.play_xa(
                audio.pcm.clone(),
                audio.sample_rate,
                channels,
                false,
                FMV_XA_GAIN_UNITY,
            );
            open.audio_handed = true;
            self.fmv.mixer_audio = true;
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (audio, FMV_XA_GAIN_UNITY);
        false
    }

    /// Take a staged movie track off the mixer once the beat is over, so a
    /// skipped movie does not keep talking under the scene it hands off to.
    pub(crate) fn fmv_audio_stop(&mut self) {
        if !self.fmv.mixer_audio {
            return;
        }
        self.fmv.mixer_audio = false;
        #[cfg(target_arch = "wasm32")]
        if let Some(out) = self.audio_out.as_ref() {
            out.stop_xa();
        }
    }

    /// Service an armed FMV beat once per [`LegaiaRuntime::tick_frame`].
    ///
    /// While `SceneMode::Cutscene` carries an `active_fmv`, arm the movie
    /// (once), route retail's skip edge, and poll: `Hold` leaves the world
    /// parked - the field VM is already suspended under the mode - and
    /// `Finished` ends the cutscene and runs the post-movie hand-off. Returns
    /// the label of the scene the hand-off entered (empty when none did).
    pub(crate) fn service_cutscene_fmv(&mut self) -> String {
        let mut fmv_handoff_scene = String::new();
        let Some(host) = self.scene_host.as_ref() else {
            return fmv_handoff_scene;
        };
        let active = (host.world.mode == SceneMode::Cutscene)
            .then_some(host.world.cutscene.active_fmv)
            .flatten();
        let Some(fmv_id) = active else {
            // A cutscene slot with no cutscene behind it (the world left the
            // mode some other way): drop it so the page tears the picture
            // down on its next poll.
            if self
                .fmv
                .armed_for()
                .is_some_and(|(o, _)| o == FmvOrigin::Cutscene)
            {
                self.fmv.clear();
            }
            return fmv_handoff_scene;
        };
        if self.fmv.armed_for() != Some((FmvOrigin::Cutscene, fmv_id)) {
            self.fmv_arm(fmv_id, FmvOrigin::Cutscene);
        }
        let Some(host) = self.scene_host.as_mut() else {
            return fmv_handoff_scene;
        };
        // Retail's pad abort: the attract id only, on a face button / Select.
        if skip_edge_from_input(fmv_id, &host.world.input) {
            self.fmv.request_finish();
        }
        let mut finished = false;
        match self.fmv.poll() {
            FmvPoll::Hold => {}
            FmvPoll::Finished { played } => {
                finished = true;
                host.world.finish_cutscene();
                if !played {
                    crate::console_log(&format!("cutscene: fmv_id={fmv_id} finished unplayed"));
                }
                // Skipping the *movie* is not skipping the *hand-off*.
                // Retail's master dispatch writes a next-scene label after
                // playback (`town01` -> fmv 1 -> `town0b`), so ending
                // without this left the page in the trigger scene - a
                // different place from where the other two hosts land. Same
                // shared kernel, same one-shot `World::take_finished_fmv`
                // edge.
                if let Some(outcome) = host.apply_pending_fmv_handoff() {
                    if let legaia_engine_core::scene::FmvHandoffOutcome::Entered { scene, .. } =
                        &outcome
                    {
                        fmv_handoff_scene = (*scene).to_string();
                    }
                    crate::console_log(&format!("cutscene: {outcome}"));
                }
            }
        }
        if finished {
            self.fmv_audio_stop();
        }
        fmv_handoff_scene
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The page declares (at boot) that it will install + play movies. Until
    /// this is called every FMV beat auto-finishes the frame it arms - the
    /// behaviour a cached bundle without the FMV script keeps.
    pub fn play_fmv_set_supported(&mut self, on: bool) {
        self.fmv.supported = on;
    }

    pub fn play_fmv_supported(&self) -> bool {
        self.fmv.supported
    }

    /// The movie the runtime wants installed, as
    /// `{ "fmv_id": n, "path": "MOV/MV3.STR", "first_sector": lba,
    ///    "sector_count": m }` - the raw-sector window of just that
    /// `fmv_id`'s segment - or `null` when nothing is waiting (no movie
    /// armed, or the armed one is already installed). The page slices
    /// `discBytes.subarray(first_sector * 2352, (first_sector +
    /// sector_count) * 2352)` and hands it to [`Self::play_fmv_install`].
    pub fn play_fmv_wanted_json(&self) -> String {
        match self.fmv.wanted() {
            Some(w) => serde_json::json!({
                "fmv_id": w.fmv_id,
                "path": w.path,
                "first_sector": w.first_sector,
                "sector_count": w.sector_count,
            })
            .to_string(),
            None => "null".to_string(),
        }
    }

    /// Open the wanted movie from its raw 2352-byte sectors. Demuxes the
    /// video frames (decoded lazily per [`Self::play_fmv_frame_rgba`]) and
    /// decodes the interleaved XA track to PCM. `true` on success; `false`
    /// when nothing was wanted or the slice carries no video frame, after
    /// which the beat finishes unplayed on the next tick.
    pub fn play_fmv_install(&mut self, sectors: &[u8]) -> bool {
        let ok = self.fmv.install(sectors);
        if ok {
            self.fmv_pause_sequencer();
        }
        ok
    }

    /// `true` while an installed movie owns the screen (from install until
    /// the tick after [`Self::play_fmv_finish`]).
    pub fn play_fmv_active(&self) -> bool {
        self.fmv.is_open()
    }

    /// The armed movie's `fmv_id`, or `-1`.
    pub fn play_fmv_id(&self) -> i32 {
        self.fmv.armed_for().map_or(-1, |(_, id)| i32::from(id))
    }

    /// Whether retail lets a pad press abort the armed movie (`fmv_id 0`,
    /// the attract / intro, only). Informational - the skip itself is
    /// routed engine-side off the pad edge, not by the page.
    pub fn play_fmv_skippable(&self) -> bool {
        self.fmv
            .armed_for()
            .is_some_and(|(_, id)| fmv_is_skippable(id))
    }

    /// The decoded XA track as interleaved 16-bit PCM, handed over **once**:
    /// the first call after install returns the samples, later calls return
    /// an empty array. Empty too when the movie carries no audio.
    pub fn play_fmv_audio_pcm_i16(&mut self) -> Vec<i16> {
        let Some(open) = self.fmv.slot.as_mut().and_then(|s| s.open.as_mut()) else {
            return Vec::new();
        };
        if open.audio_handed {
            return Vec::new();
        }
        open.audio_handed = true;
        open.audio
            .as_ref()
            .map(|a| a.pcm.clone())
            .unwrap_or_default()
    }

    /// Start the open movie's audio on the engine mixer's XA lane (behind
    /// the master trim and the volume slider, as the native window plays
    /// it). `true` once the track is running there - clock the picture off
    /// [`Self::play_fmv_audio_cursor_secs`]; `false` when the movie is
    /// silent, the track was already handed over, or no engine audio is up,
    /// in which case [`Self::play_fmv_audio_pcm_i16`] still offers the PCM.
    pub fn play_fmv_audio_start(&mut self) -> bool {
        self.fmv_audio_stage()
    }

    /// Seconds of the staged track the mixer has played - the device-paced
    /// clock the frame index follows - or `-1` when no movie track is on
    /// the mixer.
    pub fn play_fmv_audio_cursor_secs(&self) -> f64 {
        if !self.fmv.mixer_audio {
            return -1.0;
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(secs) = self.audio_out.as_ref().and_then(|o| o.xa_cursor_secs()) {
            return secs;
        }
        -1.0
    }

    /// Sample rate of the movie's audio track (0 when none).
    pub fn play_fmv_audio_rate(&self) -> u32 {
        self.fmv
            .open()
            .and_then(|o| o.audio.as_ref())
            .map_or(0, |a| a.sample_rate)
    }

    /// Interleaved channel count of the audio track (0 when none).
    pub fn play_fmv_audio_channels(&self) -> u32 {
        self.fmv
            .open()
            .and_then(|o| o.audio.as_ref())
            .map_or(0, |a| if a.stereo { 2 } else { 1 })
    }

    /// Decode video frame `index` to RGBA8 (`width * height * 4`). Empty
    /// when no movie is open, the index is past the end, or the frame fails
    /// to decode (the page keeps the previous picture).
    pub fn play_fmv_frame_rgba(&self, index: u32) -> Vec<u8> {
        self.fmv
            .open()
            .and_then(|o| o.video.frames.get(index as usize))
            .map(decode_str_frame_rgba)
            .unwrap_or_default()
    }

    /// `[width, height]` of the open movie; `[0, 0]` when none.
    pub fn play_fmv_size(&self) -> Vec<u32> {
        self.fmv
            .open()
            .map(|o| vec![o.video.width, o.video.height])
            .unwrap_or_else(|| vec![0, 0])
    }

    /// Playback rate recovered from the sector stride (~15 fps); 0 when none.
    pub fn play_fmv_fps(&self) -> f64 {
        self.fmv.open().map_or(0.0, |o| o.video.fps)
    }

    /// Number of video frames in the open movie; 0 when none.
    pub fn play_fmv_frame_count(&self) -> u32 {
        self.fmv.open().map_or(0, |o| o.video.frames.len() as u32)
    }

    /// The page reports playback over (last frame shown). The world is
    /// released on the next tick / title step, where the post-movie hand-off
    /// runs; [`Self::play_fmv_active`] reads `false` from then on. No-op
    /// unless a movie is open.
    pub fn play_fmv_finish(&mut self) {
        self.fmv.request_finish();
    }

    /// Write the field-VM FMV trigger the way op `0x4C 0xE2` does (retail
    /// stores the id in `_DAT_8007BA78` and the next mode dispatch enters
    /// the cutscene): the next [`Self::tick_frame`] flips the world into
    /// `SceneMode::Cutscene` and arms the movie. The page's debug hook and
    /// the disc-gated oracle's way of reaching a mid-game movie without
    /// walking a scene script to its trigger. `false` with no scene loaded
    /// or outside the field (retail's dispatch only reads the trigger from
    /// field mode).
    pub fn play_fmv_trigger(&mut self, fmv_id: i16) -> bool {
        let Some(host) = self.scene_host.as_mut() else {
            return false;
        };
        if host.world.mode != SceneMode::Field {
            return false;
        }
        host.world.cutscene.pending_fmv_trigger = Some(fmv_id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_core::scene::SceneHost;

    /// A disc-free PROT.DAT: header + a three-entry TOC over a handful of
    /// zeroed sectors. Enough for `SceneHost::from_prot_bytes`; no scene
    /// loads, which is exactly the point - the hold logic must not need one.
    fn synthetic_prot() -> Vec<u8> {
        const SECTOR: usize = 2048;
        let sectors = 8usize;
        let mut img = vec![0u8; sectors * SECTOR];
        // [pad, file_num - 1, header_sectors]
        img[4..8].copy_from_slice(&3u32.to_le_bytes());
        img[8..12].copy_from_slice(&1u32.to_le_bytes());
        // TOC at +8: entry p reads toc[p+2] / toc[p+3].
        let lbas = [1u32, 3, 5, 7, 8];
        for (i, l) in lbas.iter().enumerate() {
            let o = 8 + (i + 2) * 4;
            img[o..o + 4].copy_from_slice(&l.to_le_bytes());
        }
        img
    }

    fn runtime_in_cutscene(fmv_id: i16) -> LegaiaRuntime {
        let mut rt = LegaiaRuntime::new();
        let mut host = SceneHost::from_prot_bytes(synthetic_prot(), None).expect("synthetic host");
        host.world.mode = SceneMode::Field;
        host.world.cutscene.return_mode = Some(SceneMode::Field);
        host.world.mode = SceneMode::Cutscene;
        host.world.cutscene.active_fmv = Some(fmv_id);
        rt.scene_host = Some(host);
        rt
    }

    fn mode(rt: &LegaiaRuntime) -> SceneMode {
        rt.scene_host.as_ref().unwrap().world.mode
    }

    /// Unsupported page (a cached bundle): the beat finishes the frame it
    /// arms and the hand-off edge is drained - today's behaviour, kept.
    #[test]
    fn unsupported_page_auto_finishes_and_drains_the_handoff() {
        let mut rt = runtime_in_cutscene(1);
        assert_eq!(rt.service_cutscene_fmv(), "");
        assert_eq!(mode(&rt), SceneMode::Field);
        let host = rt.scene_host.as_mut().unwrap();
        assert_eq!(host.world.cutscene.active_fmv, None);
        assert_eq!(
            host.world.take_finished_fmv(),
            None,
            "the hand-off drained the finished edge"
        );
        assert!(!rt.play_fmv_active());
    }

    /// Supported page, movie installed: the world holds under Cutscene for
    /// as long as the page plays, and finishes on `play_fmv_finish`.
    #[test]
    fn supported_page_holds_the_world_until_finish() {
        let mut rt = runtime_in_cutscene(1);
        rt.play_fmv_set_supported(true);
        // No disc walk on a synthetic host, so the window can't resolve off
        // the file list; arm a window by hand the way a disc load would.
        rt.fmv.arm(
            1,
            FmvOrigin::Cutscene,
            Some(WantedFmv {
                fmv_id: 1,
                path: "MOV/MV2.STR".into(),
                first_sector: 1000,
                sector_count: 20,
            }),
        );
        // Waiting for install: held.
        assert_eq!(rt.service_cutscene_fmv(), "");
        assert_eq!(mode(&rt), SceneMode::Cutscene);
        assert_ne!(rt.play_fmv_wanted_json(), "null");
        // Install a synthetic movie: raw sectors carrying no video would be
        // rejected, so stand a decoded one in directly.
        {
            let slot = rt.fmv.slot.as_mut().unwrap();
            slot.wanted = None;
            slot.open = Some(OpenFmv {
                video: StrVideo {
                    width: 320,
                    height: 224,
                    fps: 15.0,
                    frames: vec![crate::audio::StrVideoFrame {
                        width: 320,
                        height: 224,
                        bitstream: Vec::new(),
                    }],
                },
                audio: Some(DecodedXa {
                    file_no: 1,
                    ch_no: 0,
                    sample_rate: 37_800,
                    stereo: true,
                    pcm: vec![1, -1, 2, -2],
                }),
                audio_handed: false,
            });
        }
        assert!(rt.play_fmv_active());
        assert_eq!(rt.play_fmv_wanted_json(), "null");
        for _ in 0..50 {
            assert_eq!(rt.service_cutscene_fmv(), "");
            assert_eq!(mode(&rt), SceneMode::Cutscene, "held while the movie plays");
            assert_eq!(
                rt.scene_host.as_ref().unwrap().world.cutscene.active_fmv,
                Some(1)
            );
        }
        // Off wasm there is no mixer to stage on: the engine declines and
        // leaves the one-shot PCM hand-off for the page's own context.
        assert!(!rt.play_fmv_audio_start());
        assert_eq!(rt.play_fmv_audio_cursor_secs(), -1.0);
        // One-shot PCM hand-off.
        assert_eq!(rt.play_fmv_audio_pcm_i16(), vec![1, -1, 2, -2]);
        assert!(rt.play_fmv_audio_pcm_i16().is_empty());
        assert_eq!(rt.play_fmv_audio_rate(), 37_800);
        assert_eq!(rt.play_fmv_audio_channels(), 2);
        assert_eq!(rt.play_fmv_size(), vec![320, 224]);
        assert_eq!(rt.play_fmv_frame_count(), 1);
        assert!(!rt.play_fmv_skippable(), "fmv 1 is a mid-game movie");
        // The page reports the end: the next service releases the world and
        // applies the hand-off (fmv 1 -> `town0b`, which this host cannot
        // load - the outcome is `Failed`, but the edge is consumed).
        rt.play_fmv_finish();
        assert!(rt.play_fmv_active(), "release happens on the service tick");
        assert_eq!(rt.service_cutscene_fmv(), "");
        assert_eq!(mode(&rt), SceneMode::Field);
        assert!(!rt.play_fmv_active());
        let host = rt.scene_host.as_mut().unwrap();
        assert_eq!(host.world.cutscene.active_fmv, None);
        assert_eq!(host.world.take_finished_fmv(), None);
    }

    /// Supported page that never installs: bounded by the timeout.
    #[test]
    fn supported_page_that_never_installs_times_out() {
        let mut rt = runtime_in_cutscene(2);
        rt.play_fmv_set_supported(true);
        rt.fmv.arm(
            2,
            FmvOrigin::Cutscene,
            Some(WantedFmv {
                fmv_id: 2,
                path: "MOV/MV3.STR".into(),
                first_sector: 1,
                sector_count: 1,
            }),
        );
        for _ in 0..INSTALL_TIMEOUT_FRAMES {
            rt.service_cutscene_fmv();
            assert_eq!(mode(&rt), SceneMode::Cutscene);
        }
        rt.service_cutscene_fmv();
        assert_eq!(mode(&rt), SceneMode::Field);
    }

    /// An install of bytes carrying no video finishes the beat unplayed.
    #[test]
    fn install_without_video_finishes_unplayed() {
        let mut rt = runtime_in_cutscene(2);
        rt.play_fmv_set_supported(true);
        rt.fmv.arm(
            2,
            FmvOrigin::Cutscene,
            Some(WantedFmv {
                fmv_id: 2,
                path: "MOV/MV3.STR".into(),
                first_sector: 1,
                sector_count: 2,
            }),
        );
        assert!(!rt.play_fmv_install(&vec![0u8; RAW_SECTOR_SIZE * 2]));
        assert!(!rt.play_fmv_install(&[1, 2, 3]), "not whole sectors");
        assert!(!rt.play_fmv_active());
        rt.service_cutscene_fmv();
        assert_eq!(mode(&rt), SceneMode::Field);
    }

    /// Retail's abort rule: face buttons / Select, `fmv_id 0` only.
    #[test]
    fn skip_edge_follows_retail_mask_and_id_gate() {
        assert!(skip_edge_hit(0, PadButton::Cross as u16));
        assert!(skip_edge_hit(0, PadButton::Select as u16));
        assert!(skip_edge_hit(0, PadButton::Triangle as u16));
        assert!(!skip_edge_hit(0, PadButton::Start as u16));
        assert!(!skip_edge_hit(0, PadButton::Up as u16));
        assert!(!skip_edge_hit(0, PadButton::L1 as u16));
        assert!(!skip_edge_hit(1, PadButton::Cross as u16));
        assert!(!skip_edge_hit(0, 0));
    }

    /// The skip edge, routed through the world's own pad, ends an open
    /// attract-id movie mid-play.
    #[test]
    fn world_pad_edge_skips_an_open_fmv_zero() {
        let mut rt = runtime_in_cutscene(0);
        rt.play_fmv_set_supported(true);
        rt.fmv.arm(0, FmvOrigin::Cutscene, None);
        {
            let slot = rt.fmv.slot.as_mut().unwrap();
            slot.open = Some(OpenFmv {
                video: StrVideo {
                    width: 320,
                    height: 224,
                    fps: 15.0,
                    frames: vec![crate::audio::StrVideoFrame {
                        width: 320,
                        height: 224,
                        bitstream: Vec::new(),
                    }],
                },
                audio: None,
                audio_handed: false,
            });
        }
        rt.set_pad(0);
        assert_eq!(rt.service_cutscene_fmv(), "");
        assert_eq!(mode(&rt), SceneMode::Cutscene);
        rt.set_pad(PadButton::Cross as u16);
        rt.service_cutscene_fmv();
        assert_eq!(mode(&rt), SceneMode::Field, "Cross edge aborts fmv 0");
        assert!(!rt.play_fmv_active());
    }
}
