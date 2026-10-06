//! Sound-effect channel for the browser **play page**.
//!
//! The page plays its sound through the same director the native window
//! does: [`legaia_engine_session::AudioBgmDirector`], generic over the audio
//! output, here a [`PageSink`] - the live `WebAudioOut` in a browser and a
//! headless [`legaia_engine_audio::TestAudioSink`] off wasm, so the tests
//! exercise the real staging and firing path. The director owns the SFX
//! descriptor bank, the resident program banks (slot 0, the shared slot-2 /
//! slot-6 region, the BGM-tail borrowers), the delay scheduler and the retail
//! ring, the battle duck, and the BGM sequencer
//! ([`crate::play_bgm`]); the per-tick routing is its own
//! (`route_world_sfx`, `enqueue_battle_cues`, `tick_audio_frame`), the calls
//! the native session and window make.
//!
//! What this module keeps is the page's side: the event -> cue table with
//! per-row provenance, the footstep cadence, and the counters the readout and
//! the tests read ([`PlaySfx`]). The browser's audio output exists only after
//! a user gesture, so the director is built then (on wasm) and staged at once,
//! slot 0 and the mode's shared-region bank both, rather than lazily on the
//! first cue.
//!
//! # Cue provenance is reported, not assumed
//!
//! Retail fires cues by writing an id into the ring at `_DAT_8007B6D8`, and
//! only a handful of those writes have been traced. This module therefore
//! carries the same `disc` / `site` split
//! [`crate::sfx_view`] already uses, and [`LegaiaRuntime::play_sfx_events_json`]
//! reports it per event, so the page can say which sounds are the game's and
//! which are the port's pick. Nothing here silently invents a retail cue.
//!
//! Every row reports `cue` (retail's id) *and* `fires` (what this host
//! enqueues, `null` when a row is deliberately silent), and the count of
//! requests is kept either way. The pause menu used the `null` form while the
//! key-on pitch was unsettled - the port keyed those cues an octave below
//! retail, so each blip played as a low thud. That is measured and fixed
//! (`legaia_engine_audio::vab_bind::compute_pitch`), and the three menu cues
//! sound again; see [`CUE_MENU_CURSOR`] for what remains inexact about them.
//!
//! The **footstep cadence** is the interesting case, and its answer is a
//! negative: the *timing* is the ported retail kernel (`FUN_80018db0`,
//! [`FootstepCadence`] - the interval derived from movement magnitude, the
//! `0xB` gate, the `0x4B0` ambient period), but a runtime capture of every cue
//! path while walking a field scene shows retail firing **no cue at all** - and
//! shows that kernel's own step gate never opening while walking either. So
//! this host keeps the cadence wired (it is that port's first host caller, and
//! its counters stay observable) and fires nothing. See [`CUE_FOOTSTEP`].
//!
//! [`FootstepCadence`]: legaia_engine_audio::footstep::FootstepCadence
//!
//! REF: FUN_80016b6c (the cue-ring drainer whose descriptor shape SfxBank mirrors)
//! REF: FUN_80018db0 (the footstep / ambient cadence this feeds movement into)

use crate::runtime::LegaiaRuntime;
use legaia_engine_audio::{AudioSink, SfxBank};
use legaia_engine_core::world::SceneMode;
use legaia_engine_session::bgm::SfxFrameReport;
use std::collections::BTreeMap;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

/// The audio output the page's director keys into: the live WebAudio output
/// in a browser, a headless sink off wasm (no device, same SPU and mixer).
#[cfg(target_arch = "wasm32")]
pub(crate) type PageSink = legaia_engine_audio::WebAudioOut;
/// The audio output the page's director keys into: the live WebAudio output
/// in a browser, a headless sink off wasm (no device, same SPU and mixer).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type PageSink = legaia_engine_audio::TestAudioSink;

/// The page's audio director - the native window's, over [`PageSink`].
pub(crate) type PageDirector = legaia_engine_session::AudioBgmDirector<PageSink>;

/// `_DAT_8007B910`'s reference value, what the readout shows for the duck
/// before a director exists.
const DUCK_LEVEL_REF: u8 = legaia_engine_session::bgm::DUCK_LEVEL_REF;

/// Cue id **retail's pause menu** fires when the list cursor moves.
///
/// Traced to `FUN_80032A44`, the SCUS-resident kind-4 list kernel every
/// pause-menu list window is paged by. The kernel inlines `FUN_80035B50`'s ring
/// enqueue instead of calling it, so each literal sits beside its own store:
/// `li a2,0x21` at `0x80032b9c` / `0x80032c68` / `0x80032c74`, then
/// `sh a2,0x0(v0)` with `v0 = 0x8007B6D8 + head*2`, and the head bookkeeping
/// (`gp+0x158` cursor, `gp+0x15a` park, wrap at 4, timing word cleared at
/// `0x8007C338`) matches that producer exactly. Being SCUS addresses, they
/// carry none of the overlay load-base ambiguity a `0x801C****` dump would.
///
/// [`crate::sfx_view`]'s identically-valued `CUE_CURSOR` is the **Baka Fighter**
/// overlay's own ring write and stays a separate constant deliberately: the two
/// pages reach the same id through different code, and retracing one page's
/// cues must not silently move the other's. Same set in
/// `docs/subsystems/field-menu.md`. The value is the engine's one table
/// ([`legaia_engine_core::menu_cues`]), which the native window fires from
/// too.
pub(crate) const RETAIL_MENU_CURSOR_CUE: u8 = legaia_engine_core::menu_cues::MENU_CURSOR_CUE;
/// Cue id retail's pause menu fires confirming an **enabled** row: `li a1,0x20`
/// at `0x80032d24` in `FUN_80032A44`, stored through the shared
/// `sh a1,0x0(v0)` at `0x80032d40` alongside the `mode = 2` write. A
/// *disabled* row takes the sibling branch and buzzes `0x23` instead
/// (`li a1,0x23` at `0x80032d0c`) - a distinction this host has no path for.
pub(crate) const RETAIL_MENU_CONFIRM_CUE: u8 = legaia_engine_core::menu_cues::MENU_CONFIRM_CUE;
/// Cue id retail's pause menu fires on cancel: `li a2,0x37` at `0x80032d74` in
/// `FUN_80032A44`, stored at `0x80032d94`, with `mode = 3`.
pub(crate) const RETAIL_MENU_CANCEL_CUE: u8 = legaia_engine_core::menu_cues::MENU_CANCEL_CUE;

/// What this host enqueues for a cursor move: [`RETAIL_MENU_CURSOR_CUE`], the
/// same id retail writes into the ring.
///
/// This was `None` for one reason, now settled. A cue id names a
/// `(program, tone, note)` triple, and the port keyed it against
/// `tone.center` through a pitch that also folded in a `22050 / 44100`
/// source-rate factor - so every voice, sound effect and BGM note alike, keyed
/// **an octave below retail**, and a UI blip whose sample is already authored
/// to play back slow came out ~0.7 s of low rumble. That is what "navigating
/// the pause menu plays punching sounds" was.
///
/// Retail's own law is now traced and measured: `FUN_80065034` hands the
/// descriptor's note to `FUN_80066e50`, which indexes a 192-entry table with
/// `note + 60 - center` and shifts by the octave, and unity - `0x1000`,
/// 44.1 kHz - is what a tone plays at when `note == center`. There is no
/// source-rate factor; a 22.05 kHz body is authored with `center` twelve
/// semitones high instead. Confirmed against retail's own staged pitch values
/// in save-state RAM, including these very cues. So **retail does pitch these
/// blips down** - a UI cue keyed 12..26 semitones under its centre is the
/// authored sound, not a defect - and the port now reproduces the register
/// value exactly. Withholding them is no longer the honest choice.
///
/// The *bank* half is settled too, and it was the audible half. The
/// descriptor's `+4` category byte selects the VAB slot as well as the mixer
/// channel, and these four cues are category `0` - retail sounds them out of
/// the slot-0 system bank (PROT 0868). This page used to stage only the
/// category-`2` bank (PROT 0869) and fire everything through it, which failed
/// *quietly* rather than silently: both banks carry a one-VAG-per-semitone UI
/// key map at program 0, so the id resolved to a sibling sample - a genuine
/// retail blip, but roughly twice as long and a fifth lower than the field
/// menu's, because 0869's `center` bytes are authored higher. That is the thump
/// the pause menu made. Both pinned banks are staged now and every cue routes
/// through the director's category routing, so these four key PROT 0868 the way
/// retail does.
const CUE_MENU_CURSOR: Option<u8> = Some(RETAIL_MENU_CURSOR_CUE);
/// Confirm counterpart of [`CUE_MENU_CURSOR`].
const CUE_MENU_CONFIRM: Option<u8> = Some(RETAIL_MENU_CONFIRM_CUE);
/// Cancel counterpart of [`CUE_MENU_CURSOR`].
const CUE_MENU_CANCEL: Option<u8> = Some(RETAIL_MENU_CANCEL_CUE);
/// Cue id fired for a footstep: `None`, and pinned there by capture -
/// **retail plays no footstep sound at all**, so the cadence runs and keys no
/// voice because there is nothing to key.
///
/// The contrast that settles it is
/// `scripts/pcsx-redux/autorun_footstep_cue.lua`, which watches every cue path
/// at once - both ring producers (`FUN_80035B50` / `FUN_80035BD0`), the
/// dispatcher `FUN_8004FCC8`, the per-actor trigger `FUN_800250D4`, the voice
/// programmer `FUN_80065034`, and the four ring slots themselves - and runs one
/// field save state twice for the same number of vsyncs, once standing still
/// and once with the D-pad held. Standing still, a house-interior walk and a
/// kingdom-overworld walk each fire **nothing**. The one walk that fires
/// anything fires exactly two scene-script cues (`0x2E`, `0x2F`) hundreds of
/// vsyncs apart, out of the field VM's script SFX op - triggers the player
/// crossed, not a step cadence. Write-up: `docs/formats/sfx-table.md`.
///
/// So there is no retail id to copy, and a guessed one would not be a
/// near-miss but an arbitrary sample: an id resolves through the descriptor
/// table (`DAT_8006F198 + id*8`) to a `(program, tone)` pair - `0x21` is
/// program `0`, tone `1`, not program `1` as this note previously said - and
/// that pair selects a different sample in every resident bank. Firing `0x21`
/// in a field scene played an impact sample: walking punched.
///
/// The cadence stays wired so [`FUN_80018db0`]'s timing keeps running and stays
/// observable in the HUD counters. Giving the port a footstep is therefore an
/// *enhancement* choice - author a cue and label it `site` - not a fidelity
/// gap waiting on more RE.
const CUE_FOOTSTEP: Option<u8> = None;

/// One event this host is wired for: the cue id **retail** fires there, what
/// this host actually enqueues, and where the id came from.
///
/// Splitting `retail_cue` from `fires` is the point. The page can then state
/// what the game plays *and* that the port is currently withholding it, instead
/// of having to choose between advertising a sound it does not make and hiding
/// a fact it has pinned.
struct PlayCue {
    /// Name the page fires this cue by.
    event: &'static str,
    /// The cue id retail writes into the `_DAT_8007B6D8` ring here.
    retail_cue: u8,
    /// What this host enqueues. `None` = pinned but deliberately silent.
    fires: Option<u8>,
    /// `"disc"` = traced to a retail ring write; `"site"` = a port pick where
    /// retail plays nothing (or its id is unmapped). Same convention as
    /// [`crate::sfx_view`], deliberately.
    source: &'static str,
    /// Why this row's id is what it is, and why it does or does not sound.
    why: &'static str,
}

const PLAY_EVENTS: &[PlayCue] = &[
    PlayCue {
        event: "menu_cursor",
        retail_cue: RETAIL_MENU_CURSOR_CUE,
        fires: CUE_MENU_CURSOR,
        source: "disc",
        why: "FUN_80032A44 cursor-step ring write (li a2,0x21 at 0x80032b9c); \
              category 0, so it sounds out of the slot-0 system bank \
              (PROT 0868) - see CUE_MENU_CURSOR",
    },
    PlayCue {
        event: "menu_confirm",
        retail_cue: RETAIL_MENU_CONFIRM_CUE,
        fires: CUE_MENU_CONFIRM,
        source: "disc",
        why: "FUN_80032A44 enabled-row confirm (li a1,0x20 at 0x80032d24); \
              category 0, so it sounds out of the slot-0 system bank \
              (PROT 0868) - see CUE_MENU_CURSOR",
    },
    PlayCue {
        event: "menu_cancel",
        retail_cue: RETAIL_MENU_CANCEL_CUE,
        fires: CUE_MENU_CANCEL,
        source: "disc",
        why: "FUN_80032A44 cancel (li a2,0x37 at 0x80032d74); category 0, so it \
              sounds out of the slot-0 system bank (PROT 0868) - see \
              CUE_MENU_CURSOR",
    },
];

/// The footstep stays out of [`PLAY_EVENTS`] even though that table can now
/// carry a withheld row, because its case is the opposite one: a menu row has a
/// pinned `retail_cue` this host declines to *render*, while retail fires no
/// footstep cue at all, so there is no id to report. Advertising one would
/// invent the fact rather than withhold it. See [`CUE_FOOTSTEP`].
const _: Option<u8> = CUE_FOOTSTEP;

/// World-unit displacement per tick below which the player counts as still.
/// The controller steps 2 units at a time, so anything under one unit is
/// numerical drift rather than a walk.
const WALK_EPSILON: i32 = 1;

/// Movement magnitude handed to the footstep cadence while the player walks.
///
/// **This is a port pick, and it has to be, because the two engines do not
/// carry the same quantity.** Retail feeds `FUN_80018db0` a controller speed
/// word, and the kernel's own constants bound where that word must live for a
/// step to fire at all: `interval = 0xF - (min(speed + 0x20, 0xFA) >> 4)` and
/// the `interval < 0xB` gate together require `speed >= 0x30`, saturating at
/// `0xDA`. The port has no such word - `World` exposes a walking *flag* and a
/// 2-units-per-tick step, and feeding that raw world delta in leaves `interval`
/// at `0xD`, i.e. permanently below the gate, so no step would ever fire.
///
/// A single-speed walker therefore has to be placed somewhere in retail's
/// moving band, and `0x30` is the deliberately conservative end of it: the
/// slowest speed retail treats as moving, so the cadence this produces is the
/// slowest retail would ever produce for a walking player and cannot overstate
/// the step rate.
///
/// Capture adds one thing worth stating plainly: retail's own speed word does
/// **not** reach `0x30` while the player walks a field scene or the kingdom
/// overworld - `_DAT_8007B8A4` stays pinned at `2`, the gate's else-branch, for
/// every observed frame. Feeding `0x30` in here therefore makes the port's
/// cadence fire where retail's stays shut, which is fine precisely because
/// [`CUE_FOOTSTEP`] keys no voice: what runs is a timing counter, not a sound
/// retail does not make.
const WALK_SPEED_UNITS: i32 = 0x30;

/// The page's side of the SFX channel: descriptor tables kept for before a
/// director exists, the footstep cadence, and the readout's counters.
#[derive(Default)]
pub struct PlaySfx {
    /// Descriptors decoded from the disc executable, installed into the
    /// director when it is built. Empty until `load_disc`.
    pub bank: SfxBank,
    /// Cue id -> VAB slot, the routing half of the same descriptor table
    /// ([`legaia_asset::sfx_table::SfxTable::cue_slots`]), installed beside
    /// [`Self::bank`]. Empty until `load_disc`.
    pub cue_slots: BTreeMap<u8, u8>,
    /// Whether the director's resident banks were staged against the loaded
    /// disc (once per director).
    pub resident_staged: bool,
    /// Retail footstep / ambient cadence (`FUN_80018db0`).
    pub cadence: legaia_engine_audio::footstep::FootstepCadence,
    /// Last tick's player XZ, for the movement magnitude the cadence reads.
    pub prev_pos: Option<(i32, i32)>,
    /// Cadence steps the ported `FUN_80018db0` kernel has fired since the page
    /// loaded, counted **before** the cue lookup and so independent of whether
    /// a cue id is pinned. This is what keeps the cadence falsifiable while
    /// [`CUE_FOOTSTEP`] is `None`: a wired kernel that produces nothing is
    /// indistinguishable from an unwired one, and `queued` alone cannot tell
    /// them apart once the voice key is withheld.
    pub cadence_steps: u32,
    /// Cues *enqueued* since the page loaded, whether or not a voice took
    /// them. This is what a cue **source** produces, so it is the signal that
    /// tells a wired-but-silent source apart from one that never fires.
    pub queued: u32,
    /// Named-event cue requests the page has made since it loaded, counted
    /// **before** the `fires` lookup and so independent of whether the row is
    /// withheld. It is what tells a wired firing site from an unwired one while
    /// a row is silent: `queued` cannot, because a withheld row never reaches
    /// the queue. Same role [`Self::cadence_steps`] plays for the footstep.
    pub menu_cue_requests: u32,
    /// Cues that keyed an SPU voice since the page loaded - the page's readout
    /// and the audibility half of the measurement.
    pub fired: u32,
    /// The most recent `(cue id, first voice)` that keyed on.
    pub last_fired: Option<(u16, u8)>,
    /// Queued cues `classify_cue` routed to the CD-XA **voice** leg
    /// (`id >= 0x100`) and the director declined. Counted so the readout can
    /// say the cue reached the mixer and was declined, rather than the cue
    /// never having been produced.
    pub voice_cues_dropped: u32,
    /// The CD-XA lane: arts-voice shouts + battle one-shot clips
    /// ([`crate::play_xa`]).
    pub xa: crate::play_xa::PlayXa,
    /// Ring cues (field-VM op `0x36` sub-`0`, ambient motion op `0x09`) that
    /// came due since the page loaded, whether or not a voice keyed.
    pub ring_due: u32,
    /// The id of the last ring cue that came due, whether or not it keyed.
    pub last_ring_cue: Option<i16>,
}

impl PlaySfx {
    /// Fold one director frame report into the readout's counters.
    fn count(&mut self, report: SfxFrameReport) {
        self.ring_due += report.ring_due.len() as u32;
        if let Some(&id) = report.ring_due.last() {
            self.last_ring_cue = Some(id);
        }
        self.voice_cues_dropped += report.voice_declined;
        self.fired += report.fired.len() as u32;
        if let Some(&last) = report.fired.last() {
            self.last_fired = Some(last);
        }
    }
}

impl LegaiaRuntime {
    /// The output a director is built over: the live WebAudio output once
    /// the visitor has enabled sound, a headless sink off wasm.
    fn page_sink(&self) -> Option<Arc<PageSink>> {
        #[cfg(target_arch = "wasm32")]
        {
            self.audio_out.clone()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            #[allow(clippy::arc_with_non_send_sync)]
            Some(Arc::new(legaia_engine_audio::TestAudioSink::new(
                legaia_engine_audio::SPU_INTERNAL_RATE,
            )))
        }
    }

    /// The page's director, built over [`Self::page_sink`] on first use and
    /// staged against the loaded disc as soon as a scene host exists: the
    /// descriptor tables, the slot-0 system bank (PROT 0868) and the shared
    /// slot-2 / slot-6 region's bank for the current mode - the native boot's
    /// staging. `None` on wasm until audio is up.
    pub(crate) fn audio_director(&mut self) -> Option<&mut PageDirector> {
        if self.director.is_none() {
            let sink = self.page_sink()?;
            let mut d = PageDirector::new(sink);
            d.set_sfx_bank(self.sfx.bank.clone());
            d.set_sfx_cue_slots(self.sfx.cue_slots.clone());
            self.director = Some(d);
            self.sfx.resident_staged = false;
        }
        let d = self.director.as_mut()?;
        if !self.sfx.resident_staged
            && let Some(host) = self.scene_host.host_mut()
        {
            self.sfx.resident_staged = true;
            let staged = host
                .index
                .entry_bytes_extended(legaia_asset::sfx_table::SLOT0_SYSTEM_BANK_PROT_INDEX)
                .is_ok_and(|bytes| d.stage_resident_slot0(bytes));
            if !staged {
                crate::console_log("play SFX: slot-0 system bank (PROT 0868) did not stage");
            }
            let want = host.world.sync_sfx_residency();
            let index = &host.index;
            d.sync_shared_region(want, |e| index.entry_bytes_extended(e).ok());
        }
        Some(d)
    }

    /// Decode the SFX descriptor table out of the disc executable - both
    /// halves: the `(program, tone, note, voices)` playback fields *and* the
    /// cue -> VAB-slot routing the `+4` category encodes. Called from
    /// `load_disc`; a `PROT.DAT`-only load has no executable and leaves both
    /// empty, which makes every cue a silent no-op rather than an error.
    pub(crate) fn install_sfx_descriptors(&mut self, scus: &[u8]) {
        if let Some(table) = legaia_asset::sfx_table::SfxTable::from_scus(scus) {
            self.sfx.bank = SfxBank::from_descriptors(
                table
                    .active()
                    .map(|(id, d)| (id, d.program, d.tone, d.note, d.flags)),
            );
            self.sfx.cue_slots = table.cue_slots().collect();
            if let Some(d) = self.director.as_mut() {
                d.set_sfx_bank(self.sfx.bank.clone());
                d.set_sfx_cue_slots(self.sfx.cue_slots.clone());
            }
            self.sfx.resident_staged = false;
        }
    }

    /// Queue a cue to fire `frames` sim ticks from now (`0` = this frame).
    ///
    /// The id is the full `u16` cue space (`FUN_8004FCC8`'s, which the
    /// battle's cast cues reach at `0x118..` / `0x20C..`); the director
    /// classifies it at fire time, never truncated. `impl Into<u16>` so the
    /// `u8` descriptor-id callers keep compiling unchanged. Counted whether
    /// or not a director exists to hear it.
    pub(crate) fn enqueue_sfx(&mut self, id: impl Into<u16>, frames: u16) {
        self.sfx.queued += 1;
        let id = id.into();
        if let Some(d) = self.audio_director() {
            d.enqueue_sfx(id, frames, 0, 0);
        }
    }

    /// Drop every queued SFX cue - the scene transition / battle abort clear
    /// the native session runs as `bgm.clear_sfx()` on every scene swap.
    /// Cues queued against the departing scene's timing must not fire into
    /// the next one. Neither tail borrower is dropped: a door stages no bank.
    pub(crate) fn on_scene_change_audio(&mut self) {
        if let Some(d) = self.director.as_mut() {
            d.clear_sfx();
        }
    }

    /// This tick's movement magnitude for the footstep cadence: zero when the
    /// player is still, [`WALK_SPEED_UNITS`] when walking. See that constant
    /// for why a walking player cannot simply be handed its world-unit delta.
    fn player_move_magnitude(&mut self) -> i32 {
        let host = self.scene_host.host();
        let pos = host
            .and_then(|h| {
                let w = &h.world;
                w.player_actor_slot
                    .and_then(|s| w.actors.get(s as usize))
                    .map(|a| (a.move_state.world_x as i32, a.move_state.world_z as i32))
            })
            .unwrap_or((0, 0));
        let displaced = match self.sfx.prev_pos {
            Some((px, pz)) => (pos.0 - px).abs().max((pos.1 - pz).abs()) >= WALK_EPSILON,
            None => false,
        };
        self.sfx.prev_pos = Some(pos);
        // The walk clip stays running when a step is blocked by a wall, which
        // is retail's walk-in-place; take either signal as "moving".
        let walking = host
            .and_then(|h| h.world.locomotion.player_anim.as_ref())
            .is_some_and(|f| f.walking);
        if walking || displaced {
            WALK_SPEED_UNITS
        } else {
            0
        }
    }

    /// One sim tick of the SFX channel: feed the footstep cadence, route the
    /// world's field-side sources, then the director's frame tail - the duck
    /// ramp and the scheduler / ring drain. Called from `tick_frame`.
    pub(crate) fn tick_sfx(&mut self) {
        // The cadence only runs in field-style modes; a suspended scene (menu,
        // minigame, cutscene) is not walking, and retail's field audio update
        // does not run there either.
        let walking_mode = self
            .scene_host
            .host()
            .is_some_and(|h| matches!(h.world.mode, SceneMode::Field | SceneMode::WorldMap));
        let mag = if walking_mode {
            self.player_move_magnitude()
        } else {
            self.sfx.prev_pos = None;
            0
        };
        let tick = self.sfx.cadence.tick_cadence(mag, mag);
        if tick.step_fired {
            self.sfx.cadence_steps += 1;
            // Silent until retail's footstep cue id is pinned - see CUE_FOOTSTEP.
            if let Some(cue) = CUE_FOOTSTEP {
                self.enqueue_sfx(cue, 0);
            }
        }
        self.route_field_sfx();
        // The duck rests on the world's configured level - a loaded save's
        // own - and ramps in every mode (the ramp back to full outlives the
        // battle).
        let configured = self
            .scene_host
            .host()
            .map(|h| h.world.audio.levels.configured_level);
        let Some(configured) = configured else {
            return;
        };
        if let Some(report) = self
            .audio_director()
            .map(|d| d.tick_audio_frame(configured))
        {
            self.sfx.count(report);
        }
    }

    /// Route this tick's field-side audio: the field's CD-XA one-shots first
    /// (op `0x36`'s XA arm) on the page's XA lane, then the director's
    /// `route_world_sfx` - ring ops, runtime rows, side-band and shared-region
    /// residency, monster banks, voice stops and keys - the native session's
    /// `route_field_sfx` order. With no director the producer queues are
    /// dropped, as every unheard cue is.
    // REF: FUN_80035B50, FUN_80035BAC, FUN_80035BD0
    pub(crate) fn route_field_sfx(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let field_xa = host.world.drain_field_xa_cues();
        // The scene's CD-XA prestage list (filled at scene load): staged
        // ahead of the ops that will ask for them, since this page decodes a
        // clip the bank lacks one request per frame and a line first asked
        // for at its op sounded late. The native window reads the span
        // synchronously and drops the list.
        let prestage = host.world.drain_field_xa_prestage();
        for xa in &prestage {
            self.prestage_xa_clip(xa.clip, xa.channel, xa.duration_sectors);
        }
        for xa in &field_xa {
            self.play_xa_clip(xa.clip, xa.channel, xa.duration_sectors);
        }
        if self.audio_director().is_none() {
            if let Some(host) = self.scene_host.host_mut() {
                let w = &mut host.world;
                let _ = (
                    w.take_sfx_ring_ops(),
                    w.take_sfx_voice_stops(),
                    w.take_sfx_voice_keys(),
                );
            }
            return;
        }
        if let (Some(host), Some(d)) = (self.scene_host.host_mut(), self.director.as_mut()) {
            d.route_world_sfx(&mut host.world, &host.index);
        }
    }

    /// Key one voice from an explicit
    /// [`VoiceAttr`](legaia_engine_audio::VoiceAttr) set through the
    /// director's `key_on_voice_attr` - the Muscle Dome's between-leg tally
    /// roll, whose per-lane cue (`FUN_801D1288`) resolves a whole attr set
    /// rather than an id. Returns whether a voice keyed on.
    pub(crate) fn key_on_voice_attr(&mut self, attr: legaia_engine_audio::VoiceAttr) -> bool {
        self.audio_director()
            .is_some_and(|d| d.key_on_voice_attr(attr))
    }

    /// Render one cue on a throwaway SPU with its own fresh upload of the
    /// program bank **its category names** on the live director, and report
    /// `(peak, active_samples)`: the loudest absolute sample, and how far in
    /// the cue was last non-zero. Deliberately does not touch the live SPU -
    /// rendering consumes ticks, and stealing them from the audio callback
    /// would glitch the music.
    fn probe_render(&mut self, id: u32, max_samples: u32) -> (u32, u32) {
        use legaia_engine_audio::{
            Spu, VabBank,
            spu::ram::{SPU_RAM_BYTES, SpuAllocator},
            spu_layout::SPU_RESERVED_BYTES,
        };
        let Ok(id) = u8::try_from(id) else {
            return (0, 0);
        };
        let Some(prot) = self.cue_bank_prot(id) else {
            return (0, 0);
        };
        let Some(bytes) = self
            .scene_host
            .host()
            .and_then(|h| h.index.entry_bytes_extended(prot).ok())
        else {
            return (0, 0);
        };
        let Some((report, vab_offset)) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        else {
            return (0, 0);
        };
        let mut spu = Spu::new();
        let mut alloc = SpuAllocator::new(
            SPU_RESERVED_BYTES,
            SPU_RAM_BYTES as u32 - SPU_RESERVED_BYTES,
        );
        let vab = VabBank::upload(&mut spu, &mut alloc, &report, &bytes[vab_offset..]);
        if self.sfx.bank.play_one_shot(id, &mut spu, &vab).is_none() {
            return (0, 0);
        }
        let cap = max_samples.clamp(1, legaia_engine_audio::SPU_INTERNAL_RATE * 4);
        let mut peak: i16 = 0;
        let mut active = 0u32;
        for i in 0..cap {
            let (l, r) = spu.tick();
            if l != 0 || r != 0 {
                active = i + 1;
            }
            peak = peak.max(l.saturating_abs()).max(r.saturating_abs());
        }
        (peak as u32, active)
    }

    /// The PROT entry cue `id` keys out of on the live director (its slot
    /// after the closed-slot rule), `None` when nothing would sound it.
    fn cue_bank_prot(&mut self, id: u8) -> Option<u32> {
        let d = self.audio_director()?;
        d.prot_for_slot(d.sfx_slot_for_cue(id)?)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Fire one sound cue by descriptor id, this frame. Returns `true` when the
    /// cue keyed an SPU voice - i.e. the id is in the disc's descriptor table
    /// *and* its program / tone resolved in the resident bank *and* a voice was
    /// free. A `false` means the cue was silently dropped, matching retail's
    /// "no program / no voice -> skip".
    ///
    /// The cue fires without ticking the scheduler: a tick here aged every
    /// other queued cue a frame per blip, where the native window's menu cues
    /// wait for its single per-frame tick.
    ///
    /// `id` is the full `u16` dispatch space: a cast-voice id (`>= 0x100`)
    /// is accepted, classified at fire time and declined on the voice leg
    /// (counted in `voice_cues_dropped`), so the return is `false` for it -
    /// never a truncated descriptor keyed by mistake.
    pub fn play_sfx(&mut self, id: u32) -> bool {
        let Ok(id) = u16::try_from(id) else {
            return false;
        };
        self.sfx.queued += 1;
        let Some(report) = self.audio_director().map(|d| d.fire_now(id)) else {
            return false;
        };
        let keyed = !report.fired.is_empty();
        self.sfx.count(report);
        keyed
    }

    /// One sim tick of the SFX scheduler while a menu-overlay screen (the
    /// pause menu, a shop, the prize exchange) freezes the field: the page
    /// does not run `tick_frame` under one, and this is the scheduler step
    /// its frame loop still owes. Retail runs those screens at game mode
    /// `0x17`, whose per-frame handler `FUN_80025F74` still calls the cue
    /// drainer `FUN_80016B6C` (`jal` at `0x80025F9C`) every frame, so a cue
    /// that was delayed when the screen opened keeps ageing and fires on
    /// time. The native window's twin is `tick_menu_sfx` in its frozen arms.
    /// Returns the cues that keyed a voice this step.
    // REF: FUN_80025F74, FUN_80016B6C
    pub fn play_tick_overlay_sfx(&mut self) -> u32 {
        let Some(report) = self.audio_director().map(|d| d.tick_sfx_frame()) else {
            return 0;
        };
        let keyed = report.fired.len() as u32;
        self.sfx.count(report);
        keyed
    }

    /// Is the SFX channel able to make a sound right now? True once the
    /// descriptor table decoded and a program bank staged into the live SPU
    /// (on wasm: once audio is up).
    pub fn play_sfx_ready(&self) -> bool {
        !self.sfx.bank.is_empty() && self.director.as_ref().is_some_and(|d| d.has_sfx_vab())
    }

    /// The channel's state for the page's readout:
    ///
    /// ```json
    /// { "descriptors": 100, "bank_prot": 876, "shared_slot": 6,
    ///   "banks": [ { "slot": 0, "prot": 868 }, { "slot": 6, "prot": 876 } ],
    ///   "vab_staged": true, "queued": 14, "fired": 12, "last_cue": 33,
    ///   "last_voice": 4, "idle_voices": 20 }
    /// ```
    ///
    /// `queued` counts what the cue *sources* produced and `fired` what the
    /// SPU took; the two differing is the readout that separates "no source
    /// fired" from "fired but inaudible". `banks` is the staged slot -> PROT
    /// map; `bank_prot` / `shared_slot` name the bank the shared slot-2 /
    /// slot-6 region holds for the current mode (PROT 0876 in slot 6 in the
    /// field, PROT 0869 in slot 2 in battle).
    pub fn play_sfx_state_json(&self) -> String {
        let d = self.director.as_ref();
        let idle = d
            .map(|d| d.audio().with_spu(|spu| spu.idle_voice_count()))
            .unwrap_or(0);
        let banks: Vec<serde_json::Value> = d
            .map(|d| {
                d.staged_sfx_slots()
                    .into_iter()
                    .filter_map(|slot| {
                        let prot = d.prot_for_slot(slot)?;
                        Some(serde_json::json!({ "slot": slot, "prot": prot }))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let shared = d.and_then(|d| d.shared_region());
        serde_json::json!({
            "descriptors": self.sfx.bank.len(),
            "bank_prot": shared.map(|b| b.prot_entry).unwrap_or(0),
            "shared_slot": shared.map(|b| b.slot),
            "banks": banks,
            "vab_staged": d.is_some_and(|d| d.has_sfx_vab_slot(0)),
            "cadence_steps": self.sfx.cadence_steps,
            "menu_cue_requests": self.sfx.menu_cue_requests,
            "queued": self.sfx.queued,
            "fired": self.sfx.fired,
            "last_cue": self.sfx.last_fired.map(|(id, _)| id),
            "last_voice": self.sfx.last_fired.map(|(_, v)| v),
            "idle_voices": idle,
            "pending": d.map(|d| d.sfx_pending()).unwrap_or(0),
            "voice_cues_dropped": self.sfx.voice_cues_dropped,
            "duck_level": d.map(|d| d.duck_level()).unwrap_or(DUCK_LEVEL_REF),
            "duck_target": d.map(|d| d.duck_target()).unwrap_or(DUCK_LEVEL_REF),
            "reward_bank_staged": d.is_some_and(|d| d.has_reward_bank()),
            "ring_due": self.sfx.ring_due,
            "last_ring_cue": self.sfx.last_ring_cue,
        })
        .to_string()
    }

    /// The event -> cue map with per-event provenance, so the page never
    /// hard-codes a cue id and can label which sounds are retail's:
    ///
    /// ```json
    /// [ { "event": "menu_confirm", "cue": 32, "fires": null,
    ///     "source": "disc", "why": "..." } ]
    /// ```
    ///
    /// `cue` is the id **retail** fires there; `fires` is what this host
    /// enqueues, and `null` means the id is pinned but deliberately withheld
    /// (see [`CUE_MENU_CURSOR`]). A page that renders only `cue` would claim a
    /// sound the host does not make, so both fields are reported.
    pub fn play_sfx_events_json(&self) -> String {
        let rows: Vec<serde_json::Value> = PLAY_EVENTS
            .iter()
            .map(|c| {
                serde_json::json!({
                    "event": c.event, "cue": c.retail_cue, "fires": c.fires,
                    "source": c.source, "why": c.why,
                })
            })
            .collect();
        serde_json::json!(rows).to_string()
    }

    /// **Diagnostic**: render one cue through a *throwaway* SPU + a fresh
    /// upload of the program bank and return its peak absolute sample. `0`
    /// means the cue would be inaudible on this disc (missing descriptor,
    /// program or sample) or no director is up yet.
    ///
    /// This answers "does this descriptor produce sound?" while
    /// [`Self::play_sfx`] answers "did the live mixer take it?" - the two
    /// together are what makes the channel measurable without a microphone.
    pub fn play_sfx_probe_peak(&mut self, id: u32, max_samples: u32) -> u32 {
        self.probe_render(id, max_samples).0
    }

    /// **Diagnostic** sibling of [`Self::play_sfx_probe_peak`] over the same
    /// throwaway render: how many samples in before the cue last produced a
    /// non-zero sample, i.e. how long it sounds. `0` for a cue that renders
    /// silence.
    ///
    /// This is the observable that catches a **pitch** regression, which a peak
    /// cannot: mis-keying a cue by an octave leaves it just as loud and takes
    /// twice as long to play. See
    /// `legaia_engine_audio::vab_bind::compute_pitch`.
    pub fn play_sfx_probe_active_samples(&mut self, id: u32, max_samples: u32) -> u32 {
        self.probe_render(id, max_samples).1
    }

    /// The VAB slot a cue's `+4` category names, before any fallback: `0` for
    /// the shared UI cues, `2` for battle / duel, `6` / `11` for the two
    /// categories whose slot has no traced PROT entry. `255` when the id isn't
    /// in the disc table (no real category uses `0xFF`).
    pub fn play_sfx_cue_slot(&self, id: u32) -> u32 {
        u8::try_from(id)
            .ok()
            .and_then(|id| self.sfx.cue_slots.get(&id).copied())
            .unwrap_or(0xFF) as u32
    }

    /// The PROT entry a cue **actually** sounds out of on this host, i.e. its
    /// slot after the closed-slot rule. `0` when no bank could be read (a
    /// `PROT.DAT`-only load, no scene staged, or - on wasm - audio not up).
    ///
    /// This is the observable the routing is measured by: two cues in different
    /// retail categories must report different entries, which is a fact about
    /// the page's own behaviour rather than about the descriptor table.
    pub fn play_sfx_cue_bank_prot(&mut self, id: u32) -> u32 {
        u8::try_from(id)
            .ok()
            .and_then(|id| self.cue_bank_prot(id))
            .unwrap_or(0)
    }

    /// Fire the cue mapped to a named event (see
    /// [`Self::play_sfx_events_json`]). Returns `false` for an unknown event, a
    /// row whose cue is withheld, or a cue that did not sound.
    ///
    /// A known-but-withheld row still counts the request
    /// ([`PlaySfx::menu_cue_requests`]), so the page's firing site stays
    /// measurable even for a row whose cue is `None`.
    pub fn play_sfx_event(&mut self, event: &str) -> bool {
        let Some(row) = PLAY_EVENTS.iter().find(|c| c.event == event) else {
            return false;
        };
        self.sfx.menu_cue_requests += 1;
        let Some(cue) = row.fires else {
            return false;
        };
        self.play_sfx(cue as u32)
    }

    /// Fire the pause menu's blip for this frame's just-pressed `edge` (the
    /// PSX pad word the page feeds `set_pad`). Which blip, if any, is the
    /// engine's one rule ([`legaia_engine_core::menu_cues::menu_edge_blip`]),
    /// the same call the native window makes: `start_closes_menu` is whether
    /// this frame's Start closes the whole menu (the root row list) rather
    /// than reaching a sub-screen. Returns whether a cue sounded; a request
    /// is counted in `menu_cue_requests` either way, as
    /// [`Self::play_sfx_event`] counts it.
    pub fn play_menu_edge_blip(&mut self, edge: u16, start_closes_menu: bool) -> bool {
        let Some(blip) = legaia_engine_core::menu_cues::menu_edge_blip(edge, start_closes_menu)
        else {
            return false;
        };
        let event = match blip {
            legaia_engine_core::menu_cues::MenuBlip::Cursor => "menu_cursor",
            legaia_engine_core::menu_cues::MenuBlip::Confirm => "menu_confirm",
            legaia_engine_core::menu_cues::MenuBlip::Cancel => "menu_cancel",
        };
        self.play_sfx_event(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every advertised event resolves to a descriptor id inside the table's
    /// 100-entry space, and every row declares its provenance as one of the two
    /// values the pages switch on. A row's `fires` id, when present, must be the
    /// retail one - this host may withhold a cue but must never substitute a
    /// different sample for it.
    #[test]
    fn every_event_has_an_in_range_cue_and_a_declared_source() {
        assert!(!PLAY_EVENTS.is_empty());
        for c in PLAY_EVENTS {
            let (event, cue) = (c.event, c.retail_cue);
            assert!(
                cue <= 0x63,
                "{event}: cue {cue:#x} is outside the static table's 0x00..=0x63 id space"
            );
            assert!(
                matches!(c.source, "disc" | "site"),
                "{event}: source must be disc or site, got {}",
                c.source
            );
            assert!(!c.why.is_empty(), "{event}: needs a provenance note");
            if let Some(f) = c.fires {
                assert_eq!(
                    f, cue,
                    "{event}: a fired cue must be retail's id, not a substitute"
                );
            }
        }
    }

    /// The pause-menu ids this host pins are the ones `FUN_80032A44` writes.
    /// Hard-coded here rather than aliased from [`crate::sfx_view`] so the two
    /// pages' cue sets stay independent - and asserted equal to the duel
    /// overlay's values, which documents that they coincide *and* fails loudly
    /// if a future retrace moves either set without the other being reviewed.
    #[test]
    fn menu_cue_ids_are_the_traced_scus_list_kernel_ids() {
        assert_eq!(RETAIL_MENU_CONFIRM_CUE, 0x20);
        assert_eq!(RETAIL_MENU_CURSOR_CUE, 0x21);
        assert_eq!(RETAIL_MENU_CANCEL_CUE, 0x37);
        // The Baka Fighter page must keep firing exactly what it fired before.
        assert_eq!(crate::sfx_view::CUE_CONFIRM, RETAIL_MENU_CONFIRM_CUE);
        assert_eq!(crate::sfx_view::CUE_CURSOR, RETAIL_MENU_CURSOR_CUE);
        assert_eq!(crate::sfx_view::CUE_CANCEL, RETAIL_MENU_CANCEL_CUE);
    }

    /// A direct-play blip fires its one cue without ticking the scheduler:
    /// a delayed cue already queued still needs its whole count of ticks.
    #[test]
    fn a_direct_blip_does_not_age_the_queue() {
        let mut rt = LegaiaRuntime::new();
        rt.enqueue_sfx(0x2Eu16, 2);
        let queued_before = rt.sfx.queued;
        let _ = rt.play_sfx(u32::from(RETAIL_MENU_CURSOR_CUE));
        assert_eq!(rt.sfx.queued, queued_before + 1, "the request is counted");
        let pending = |rt: &LegaiaRuntime| rt.director.as_ref().unwrap().sfx_pending();
        assert_eq!(pending(&rt), 1, "only the delayed cue waits");
        // 2 -> 1 -> 0 -> fire: three overlay steps, none spent by the blip.
        rt.play_tick_overlay_sfx();
        rt.play_tick_overlay_sfx();
        assert_eq!(pending(&rt), 1, "still delayed");
        rt.play_tick_overlay_sfx();
        assert_eq!(pending(&rt), 0, "matured under the screen");
    }

    /// The page's menu-edge export asks the engine's rule: Start fires a
    /// cancel only where it closes the menu, and every blip is a counted
    /// request.
    #[test]
    fn the_menu_edge_export_follows_the_engine_rule() {
        let mut rt = LegaiaRuntime::new();
        let requests = |rt: &LegaiaRuntime| rt.sfx.menu_cue_requests;
        let start = legaia_engine_core::input::PadButton::Start as u16;
        let cross = legaia_engine_core::input::PadButton::Cross as u16;
        let _ = rt.play_menu_edge_blip(start, true);
        assert_eq!(requests(&rt), 1, "a closing Start blips");
        let _ = rt.play_menu_edge_blip(start, false);
        assert_eq!(requests(&rt), 1, "Start in a sub-screen blips nothing");
        let _ = rt.play_menu_edge_blip(start | cross, false);
        assert_eq!(requests(&rt), 2, "the frame's Cross still confirms");
        let _ = rt.play_menu_edge_blip(0, true);
        assert_eq!(requests(&rt), 2, "no edge, no request");
    }

    /// Every menu row fires retail's own id. The withheld form these rows used
    /// while the key-on pitch was unsettled is gone, and this asserts it stayed
    /// gone: a row silently reverting to `None` is exactly the regression that
    /// looks like "the page just has no sound" rather than like a bug.
    #[test]
    fn every_menu_row_fires_retails_id() {
        for c in PLAY_EVENTS {
            assert_eq!(
                c.fires,
                Some(c.retail_cue),
                "{}: must fire retail's own cue id",
                c.event
            );
        }
    }

    /// A cast-voice id is counted as declined, never keyed as the populated
    /// descriptor its low byte names.
    #[test]
    fn a_voice_cue_is_declined_and_counted() {
        let mut rt = LegaiaRuntime::new();
        assert!(!rt.play_sfx(0x20C));
        assert_eq!(rt.sfx.voice_cues_dropped, 1);
        assert_eq!(rt.sfx.fired, 0);
    }
}
