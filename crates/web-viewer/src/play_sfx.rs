//! Sound-effect channel for the browser **play page**.
//!
//! The page had BGM and nothing else: the native window stages a
//! [`SfxBank`] from the disc executable and a resident program bank into its
//! own SPU region, and the browser host staged neither. This module is that
//! channel, built out of what already exists rather than a second audio path -
//! `legaia_asset::sfx_table` for the descriptors, `legaia_engine_audio`'s
//! [`SfxBank`] / [`SfxScheduler`] for firing and timing, and the live
//! `WebAudioOut` SPU so a cue mixes with the music through one mixer exactly as
//! it does on hardware.
//!
//! # The chain, and where each link comes from
//!
//! 1. **Descriptors** - `SCUS_942.54`'s static table (`DAT_8006F198 + id*8`,
//!    100 entries, see `docs/formats/sfx-table.md`) is parsed at `load_disc`
//!    into an [`SfxBank`]. Pure data; no audio device needed, so the bank is
//!    present whether or not the visitor has enabled sound.
//! 2. **Programs** - the resident class-2 sound bank (PROT 0869, with the
//!    `DAT_8007BD11 == 4` alternate 0875 as a fallback) is uploaded into a
//!    dedicated region at the **top** of SPU RAM the first time a cue needs it.
//!    The scene-BGM allocator is capped below that region
//!    ([`crate::runtime`]), mirroring the native split, so a scene change never
//!    stomps the SFX samples.
//! 3. **Firing** - [`SfxScheduler`] is ticked once per sim frame and matured
//!    cues go through [`SfxBank::play_one_shot`], which keys the descriptor's
//!    consecutive tone regions on idle SPU voices the way the retail drainer
//!    `FUN_80016B6C` does.
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
//! There is a **third** state this page needs and the split above cannot
//! express: an id traced to retail that the port declines to render. The pause
//! menu is that case - its three ids are pinned to instructions in
//! `FUN_80032A44` ([`RETAIL_MENU_CURSOR_CUE`]) yet the port renders them
//! 16..23 semitones flat, so each plays as a low thud. Every row therefore
//! reports `cue` (retail's id) *and* `fires` (what this host enqueues, `null`
//! when withheld), and the count of requests is kept either way. See
//! [`CUE_MENU_CURSOR`] for the measurement and the open question.
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
//! REF: FUN_80016b6c (the cue-ring drainer whose descriptor shape SfxBank mirrors)
//! REF: FUN_80018db0 (the footstep / ambient cadence this feeds movement into)

use crate::runtime::LegaiaRuntime;
use legaia_engine_audio::{PendingCue, SfxBank, SfxScheduler};
use legaia_engine_core::world::SceneMode;
use wasm_bindgen::prelude::*;

/// SPU RAM reserved at the **top** of the map for the resident class-2 SFX
/// bank. Same 192 KiB window the native boot reserves (`SFX_BANK_SPU_BYTES`);
/// the bank's VAG bodies total ~184 KiB, so this holds it with headroom.
pub const SFX_BANK_SPU_BYTES: u32 = 0x30000;
/// Bottom of the BGM region, matching the native boot's `SPU_RESERVED_BYTES`.
pub const SPU_RESERVED_BYTES: u32 = 0x1000;

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
/// `docs/subsystems/field-menu.md`.
pub(crate) const RETAIL_MENU_CURSOR_CUE: u8 = 0x21;
/// Cue id retail's pause menu fires confirming an **enabled** row: `li a1,0x20`
/// at `0x80032d24` in `FUN_80032A44`, stored through the shared
/// `sh a1,0x0(v0)` at `0x80032d40` alongside the `mode = 2` write. A
/// *disabled* row takes the sibling branch and buzzes `0x23` instead
/// (`li a1,0x23` at `0x80032d0c`) - a distinction this host has no path for.
pub(crate) const RETAIL_MENU_CONFIRM_CUE: u8 = 0x20;
/// Cue id retail's pause menu fires on cancel: `li a2,0x37` at `0x80032d74` in
/// `FUN_80032A44`, stored at `0x80032d94`, with `mode = 3`.
pub(crate) const RETAIL_MENU_CANCEL_CUE: u8 = 0x37;

/// What this host actually enqueues for a cursor move: **nothing** - and not
/// because the id is unknown. [`RETAIL_MENU_CURSOR_CUE`] is pinned to
/// instructions; the link withheld here is one step further down the chain.
///
/// A cue id names a `(program, tone, note)` triple in the static table, and this
/// port keys it with `note - tone.center` pitch math
/// (`legaia_engine_audio::vab_bind::compute_pitch`). In the class-2 bank the
/// page stages, program `0` is a purpose-built **SFX key map** - one distinct
/// VAG per semitone, single-note windows `min == max == 60 + i` that line up
/// 1:1 with these descriptors' note bytes - so the bank and the ids agree. But
/// that program's `center` bytes sit at `79..=88`, so every UI cue keys
/// **16..23 semitones below** its recorded pitch (`0x20` is note 60 against
/// center 83). Rendered off a real disc each one comes out ~0.7 s long at near
/// full scale - a low thud, not a blip. That is what "navigating the pause menu
/// plays punching sounds" is.
///
/// Whether retail's own key-on applies that shift is **not settled**: the SPU
/// programming (`FUN_80065034`) is libsnd and out of clean-room scope, and
/// nothing in the corpus pins the SFX pitch path. Two readings survive - the
/// designer tuned each sample through its `center` and the low thud is
/// authentic, or the SFX path ignores `center` and the sample plays at unity.
/// Filed in `docs/reference/open-rev-eng-threads.md`.
///
/// So the ids stay pinned and reported, the firing site stays wired and
/// counted ([`PlaySfx::menu_cue_requests`]), and the sound is withheld until
/// the pitch path is traced - the shape [`CUE_FOOTSTEP`] already settled on.
/// Flipping these three to `Some(...)` is the whole change once it is.
const CUE_MENU_CURSOR: Option<u8> = None;
/// Confirm counterpart of [`CUE_MENU_CURSOR`] - withheld for the same reason.
const CUE_MENU_CONFIRM: Option<u8> = None;
/// Cancel counterpart of [`CUE_MENU_CURSOR`] - withheld for the same reason.
const CUE_MENU_CANCEL: Option<u8> = None;
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
    /// retail plays nothing (or its id is unpinned). Same convention as
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
              withheld pending the SFX key-on pitch path - see CUE_MENU_CURSOR",
    },
    PlayCue {
        event: "menu_confirm",
        retail_cue: RETAIL_MENU_CONFIRM_CUE,
        fires: CUE_MENU_CONFIRM,
        source: "disc",
        why: "FUN_80032A44 enabled-row confirm (li a1,0x20 at 0x80032d24); \
              withheld pending the SFX key-on pitch path - see CUE_MENU_CURSOR",
    },
    PlayCue {
        event: "menu_cancel",
        retail_cue: RETAIL_MENU_CANCEL_CUE,
        fires: CUE_MENU_CANCEL,
        source: "disc",
        why: "FUN_80032A44 cancel (li a2,0x37 at 0x80032d74); withheld pending \
              the SFX key-on pitch path - see CUE_MENU_CURSOR",
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

/// Live state of the page's SFX channel.
#[derive(Default)]
pub struct PlaySfx {
    /// Descriptors decoded from the disc executable. Empty until `load_disc`.
    pub bank: SfxBank,
    /// Raw class-2 program-bank bytes, kept so a probe can render a cue
    /// through a throwaway SPU without disturbing the live one.
    pub bank_bytes: Option<Vec<u8>>,
    /// PROT entry the program bank came from (`0` when none staged).
    pub bank_index: u32,
    /// Delay scheduler; ticked once per sim frame.
    pub sched: SfxScheduler,
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
    /// tells a wired-but-silent source apart from one that never fires - and
    /// unlike [`Self::fired`] it is observable off wasm, where there is no SPU.
    pub queued: u32,
    /// Named-event cue requests the page has made since it loaded, counted
    /// **before** the `fires` lookup and so independent of whether the row is
    /// withheld. While the menu cues are `None` this is the only counter that
    /// can tell a wired firing site from an unwired one: `queued` cannot,
    /// because a withheld row never reaches the queue. Same role
    /// [`Self::cadence_steps`] plays for the footstep.
    pub menu_cue_requests: u32,
    /// Cues that keyed an SPU voice since the page loaded - the page's readout
    /// and the audibility half of the measurement.
    pub fired: u32,
    /// The most recent `(cue id, first voice)` that keyed on.
    pub last_fired: Option<(u16, u8)>,
    /// Whether the program bank uploaded into the live SPU.
    pub vab_staged: bool,
}

impl LegaiaRuntime {
    /// Decode the SFX descriptor table out of the disc executable. Called from
    /// `load_disc`; a `PROT.DAT`-only load has no executable and leaves the
    /// bank empty, which makes every cue a silent no-op rather than an error.
    pub(crate) fn install_sfx_descriptors(&mut self, scus: &[u8]) {
        if let Some(table) = legaia_asset::sfx_table::SfxTable::from_scus(scus) {
            self.sfx.bank = SfxBank::from_descriptors(
                table
                    .active()
                    .map(|(id, d)| (id, d.program, d.tone, d.note, d.voice_count())),
            );
        }
    }

    /// Read the resident class-2 program bank off the loaded PROT and keep its
    /// bytes. Tries PROT 0869 then the `DAT_8007BD11 == 4` alternate 0875, and
    /// each at VAB offset `+4` (the entry is a chunk-header-prefixed stream)
    /// then `+0`. No-op once staged.
    pub(crate) fn load_sfx_bank_bytes(&mut self) {
        if self.sfx.bank_bytes.is_some() {
            return;
        }
        let Some(host) = self.scene_host.as_ref() else {
            return;
        };
        for idx in [
            crate::sfx_view::SFX_BANK_PROT_INDEX,
            crate::sfx_view::SFX_BANK_ALT_PROT_INDEX,
        ] {
            let Ok(bytes) = host.index.entry_bytes_extended(idx) else {
                continue;
            };
            if [4usize, 0]
                .into_iter()
                .any(|o| legaia_vab::parse(&bytes, o).is_ok())
            {
                self.sfx.bank_index = idx;
                self.sfx.bank_bytes = Some(bytes);
                return;
            }
        }
    }

    /// Queue a cue to fire `frames` sim ticks from now (`0` = this frame).
    pub(crate) fn enqueue_sfx(&mut self, id: u8, frames: u16) {
        self.sfx.queued += 1;
        self.sfx.sched.enqueue(PendingCue::new(id as u16, frames));
    }

    /// This tick's movement magnitude for the footstep cadence: zero when the
    /// player is still, [`WALK_SPEED_UNITS`] when walking. See that constant
    /// for why a walking player cannot simply be handed its world-unit delta.
    fn player_move_magnitude(&mut self) -> i32 {
        let host = self.scene_host.as_ref();
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
            .and_then(|h| h.world.field_player_anim.as_ref())
            .is_some_and(|f| f.walking);
        if walking || displaced {
            WALK_SPEED_UNITS
        } else {
            0
        }
    }

    /// One sim tick of the SFX channel: feed the footstep cadence, advance the
    /// scheduler, and key whatever matured. Called from `tick_frame`.
    pub(crate) fn tick_sfx(&mut self) {
        // The cadence only runs in field-style modes; a suspended scene (menu,
        // minigame, cutscene) is not walking, and retail's field audio update
        // does not run there either.
        let walking_mode = self
            .scene_host
            .as_ref()
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
        self.fire_matured_sfx();
    }

    /// Fire this frame's matured cues into the live SPU. Split out of
    /// [`Self::tick_sfx`] so the direct-play entry point can use it too.
    fn fire_matured_sfx(&mut self) {
        // Off wasm there is no live SPU to key into (`WebAudioOut` is the only
        // audio device this crate has), so the scheduler still advances - which
        // is what the disc-gated tests exercise - but nothing sounds.
        let _batch = self.sfx.sched.tick_frame();
        #[cfg(target_arch = "wasm32")]
        let batch = _batch;
        #[cfg(target_arch = "wasm32")]
        if !batch.is_empty() {
            if !self.stage_sfx_vab() {
                return;
            }
            let Some(out) = self.audio_out.as_ref() else {
                return;
            };
            let Some(vab) = self.sfx_vab.as_ref() else {
                return;
            };
            let bank = &self.sfx.bank;
            let mut fired = Vec::new();
            out.with_spu(|spu| {
                for cue in &batch.fired {
                    if let Some(voice) = bank.play_one_shot(cue.id as u8, spu, vab) {
                        fired.push((cue.id, voice));
                    }
                }
            });
            self.sfx.fired += fired.len() as u32;
            if let Some(last) = fired.last() {
                self.sfx.last_fired = Some(*last);
            }
        }
    }

    /// Upload the class-2 program bank into its dedicated top region of SPU
    /// RAM. Idempotent; returns whether a bank is resident. Needs audio to be
    /// live, so this runs lazily on the first cue rather than at `load_disc`.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn stage_sfx_vab(&mut self) -> bool {
        if self.sfx_vab.is_some() {
            return true;
        }
        self.load_sfx_bank_bytes();
        let Some(out) = self.audio_out.as_ref() else {
            return false;
        };
        let Some(bytes) = self.sfx.bank_bytes.as_ref() else {
            return false;
        };
        let Some((report, off)) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(bytes, o).ok().map(|r| (r, o)))
        else {
            return false;
        };
        let body = &bytes[off..];
        let bank = out.with_spu(|spu| {
            use legaia_engine_audio::spu::ram::{SPU_RAM_BYTES, SpuAllocator};
            // Top region, below nothing - the BGM allocator is capped under it.
            let mut alloc = SpuAllocator::new(
                SPU_RAM_BYTES as u32 - SFX_BANK_SPU_BYTES,
                SFX_BANK_SPU_BYTES,
            );
            legaia_engine_audio::VabBank::upload(spu, &mut alloc, &report, body)
        });
        self.sfx_vab = Some(bank);
        self.sfx.vab_staged = true;
        true
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
    /// This is the page's cue surface and the measurable one: a returned voice
    /// index is proof the live SPU accepted the note, not just that a queue
    /// accepted an id.
    pub fn play_sfx(&mut self, id: u32) -> bool {
        if id > u8::MAX as u32 {
            return false;
        }
        let before = self.sfx.fired;
        self.enqueue_sfx(id as u8, 0);
        self.fire_matured_sfx();
        self.sfx.fired > before
    }

    /// Is the SFX channel able to make a sound right now? True once the
    /// descriptor table decoded, the program bank staged into the live SPU, and
    /// audio is up.
    pub fn play_sfx_ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            !self.sfx.bank.is_empty() && self.sfx_vab.is_some()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            !self.sfx.bank.is_empty()
        }
    }

    /// The channel's state for the page's readout:
    ///
    /// ```json
    /// { "descriptors": 100, "bank_prot": 869, "vab_staged": true,
    ///   "queued": 14, "fired": 12, "last_cue": 33, "last_voice": 4,
    ///   "idle_voices": 20 }
    /// ```
    ///
    /// `queued` counts what the cue *sources* produced and `fired` what the
    /// SPU took; the two differing is the readout that separates "no source
    /// fired" from "fired but inaudible".
    pub fn play_sfx_state_json(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        let idle = self
            .audio_out
            .as_ref()
            .map(|o| o.with_spu(|spu| spu.idle_voice_count()))
            .unwrap_or(0);
        #[cfg(not(target_arch = "wasm32"))]
        let idle = 0usize;
        serde_json::json!({
            "descriptors": self.sfx.bank.len(),
            "bank_prot": self.sfx.bank_index,
            "vab_staged": self.sfx.vab_staged,
            "cadence_steps": self.sfx.cadence_steps,
            "menu_cue_requests": self.sfx.menu_cue_requests,
            "queued": self.sfx.queued,
            "fired": self.sfx.fired,
            "last_cue": self.sfx.last_fired.map(|(id, _)| id),
            "last_voice": self.sfx.last_fired.map(|(_, v)| v),
            "idle_voices": idle,
            "pending": self.sfx.sched.pending_count(),
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
    /// program or sample).
    ///
    /// Deliberately does not touch the live SPU: rendering consumes SPU ticks,
    /// and stealing them from the audio callback would glitch the music. So
    /// this answers "does this descriptor produce sound?" while
    /// [`Self::play_sfx`] answers "did the live mixer take it?" - the two
    /// together are what makes the channel measurable without a microphone.
    pub fn play_sfx_probe_peak(&mut self, id: u32, max_samples: u32) -> u32 {
        use legaia_engine_audio::{
            Spu, VabBank,
            spu::ram::{SPU_RAM_BYTES, SpuAllocator},
        };
        if id > u8::MAX as u32 {
            return 0;
        }
        self.load_sfx_bank_bytes();
        let Some(bytes) = self.sfx.bank_bytes.as_ref() else {
            return 0;
        };
        let Some((report, off)) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(bytes, o).ok().map(|r| (r, o)))
        else {
            return 0;
        };
        let mut spu = Spu::new();
        let mut alloc = SpuAllocator::new(
            SPU_RESERVED_BYTES,
            SPU_RAM_BYTES as u32 - SPU_RESERVED_BYTES,
        );
        let vab = VabBank::upload(&mut spu, &mut alloc, &report, &bytes[off..]);
        if self
            .sfx
            .bank
            .play_one_shot(id as u8, &mut spu, &vab)
            .is_none()
        {
            return 0;
        }
        let cap = max_samples.clamp(1, legaia_engine_audio::SPU_INTERNAL_RATE * 4);
        let mut peak: i16 = 0;
        for _ in 0..cap {
            let (l, r) = spu.tick();
            peak = peak.max(l.saturating_abs()).max(r.saturating_abs());
        }
        peak as u32
    }

    /// Fire the cue mapped to a named event (see
    /// [`Self::play_sfx_events_json`]). Returns `false` for an unknown event, a
    /// row whose cue is withheld, or a cue that did not sound.
    ///
    /// A known-but-withheld row still counts the request
    /// ([`PlaySfx::menu_cue_requests`]), so the page's firing site stays
    /// measurable while [`CUE_MENU_CURSOR`] is `None`.
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

    /// Every menu row is currently withheld. This is the assertion that has to
    /// change when the pitch path in [`CUE_MENU_CURSOR`] is traced, so it is
    /// written to fail rather than to pass silently either way.
    #[test]
    fn menu_cues_are_withheld_while_the_pitch_path_is_open() {
        for c in PLAY_EVENTS {
            assert!(
                c.fires.is_none(),
                "{}: still withheld until the SFX key-on pitch path is pinned",
                c.event
            );
        }
    }

    /// The SPU regions the two banks claim must not overlap, or a scene change
    /// would stomp the resident SFX samples. This is the invariant the native
    /// boot enforces with the same two constants.
    #[test]
    fn bgm_and_sfx_spu_regions_are_disjoint() {
        use legaia_engine_audio::spu::ram::SPU_RAM_BYTES;
        let bgm_start = SPU_RESERVED_BYTES;
        let bgm_end = SPU_RAM_BYTES as u32 - SFX_BANK_SPU_BYTES;
        let sfx_start = bgm_end;
        assert!(bgm_start < bgm_end, "BGM region must be non-empty");
        assert_eq!(sfx_start, bgm_end, "SFX region starts where BGM ends");
        assert_eq!(
            sfx_start + SFX_BANK_SPU_BYTES,
            SPU_RAM_BYTES as u32,
            "the SFX region must reach the top of SPU RAM"
        );
    }
}
