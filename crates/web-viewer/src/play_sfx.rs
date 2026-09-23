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
//! 2. **Programs** - a cue names its own bank. The descriptor's `+4` category
//!    selects a VAB slot (`legaia_asset::sfx_table::slot_for_category`), and
//!    the two of the four such slots this page can fit in its SPU region
//!    (slot 0 = PROT 0868, slot 2 = PROT 0869) are both uploaded into one
//!    dedicated region at the **top** of SPU RAM the first time a cue needs
//!    them, out of a single `SpuAllocator` so they pack rather than overlap.
//!    The scene-BGM allocator is capped below that region
//!    ([`crate::runtime`]), mirroring the native split, so a scene change never
//!    stomps the SFX samples.
//! 3. **Firing** - [`SfxScheduler`] is ticked once per sim frame and matured
//!    cues go through [`SfxBank::play_one_shot`] against the bank their own
//!    category names, keying the descriptor's consecutive tone regions on idle
//!    SPU voices the way the retail drainer `FUN_80016B6C` does.
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
//! REF: FUN_80016b6c (the cue-ring drainer whose descriptor shape SfxBank mirrors)
//! REF: FUN_80018db0 (the footstep / ambient cadence this feeds movement into)

use crate::runtime::LegaiaRuntime;
use legaia_asset::sfx_table::{
    FALLBACK_VAB_SLOT, PINNED_SLOT_BANKS, SLOT11_REWARD_BANK_PROT_INDEX,
};
use legaia_engine_audio::{CueDispatch, PendingCue, SfxBank, SfxScheduler, VabBank, classify_cue};
use legaia_engine_core::world::SceneMode;
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

/// SPU RAM reserved at the **top** of the map for the resident SFX banks -
/// **both** pinned slots, packed out of one allocator. Same window the native
/// boot reserves (`SFX_BANK_SPU_BYTES`), and the two must stay equal.
///
/// The value is arithmetic, not a round number. PROT 0868's VAG bodies total
/// 59136 bytes and PROT 0869's 188128, so the pair needs 247264; every VAG in
/// both is already a multiple of the allocator's 16-byte ADPCM block, so 0x3D000
/// (249856) holds them with 2592 to spare. It cannot go higher: the BGM region
/// is what is left (`512 KiB - SPU_RESERVED_BYTES - this`), and at 0x3E000 that
/// falls to 266240, which is under the two largest scene BGM VABs on the disc
/// (269632 and 268496) - i.e. the next step up starts silencing music that
/// plays today. It cannot go lower either: 0x3C000 does not fit both banks.
/// Pinned by `sfx_bank_region_fits_both_pinned_banks`.
pub const SFX_BANK_SPU_BYTES: u32 = 0x3D000;
/// Bottom of the BGM region, matching the native boot's `SPU_RESERVED_BYTES`.
pub const SPU_RESERVED_BYTES: u32 = 0x1000;

/// `_DAT_8007B910`'s reference value (`0xD7`, seeded by the cold reset
/// `FUN_8001FFA4`): the un-ducked BGM level the battle's `0x51` arm ramps
/// back to. Same constant as the native director's `DUCK_LEVEL_REF`.
pub(crate) const DUCK_LEVEL_REF: u8 = 0xD7;

/// VAB slot the battle-end reward bank (PROT 0889, cue `0x50`) is installed
/// in - retail streams it at results time (`FUN_8004E568` phase 4,
/// `FUN_8001E54C(0xB, ...)`), and both hosts stage it transiently the same
/// way ([`LegaiaRuntime::stage_transient_reward_bank`]). Same value as the
/// native director's `TRANSIENT_REWARD_SLOT`.
pub(crate) const TRANSIENT_REWARD_SLOT: u8 = 11;

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
/// through [`PlaySfx::slot_for_cue`], so these four key PROT 0868 the way
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

/// One resident program bank: which PROT entry it came from and its raw
/// bytes, kept so a probe can re-upload it into a throwaway SPU without
/// disturbing the live one.
pub struct StagedBankBytes {
    /// PROT extraction index the bytes were read from.
    pub prot: u32,
    /// Whole entry, VAB header at [`Self::vab_offset`].
    pub bytes: Vec<u8>,
    /// Where the VAB header starts (`4` for a chunk-header-prefixed stream,
    /// `0` for a bare bank).
    pub vab_offset: usize,
}

/// Live state of the page's SFX channel.
pub struct PlaySfx {
    /// Descriptors decoded from the disc executable. Empty until `load_disc`.
    pub bank: SfxBank,
    /// Cue id -> VAB slot, the routing half of the same descriptor table
    /// ([`legaia_asset::sfx_table::SfxTable::cue_slots`]). Empty until
    /// `load_disc`; a cue with no entry falls back exactly like an unstaged
    /// slot does.
    pub cue_slots: BTreeMap<u8, u8>,
    /// Raw program-bank bytes per **VAB slot**, for the pinned slots only
    /// (`0` = PROT 0868, `2` = PROT 0869). Empty until the first cue.
    pub bank_bytes: BTreeMap<u8, StagedBankBytes>,
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
    /// withheld. It is what tells a wired firing site from an unwired one while
    /// a row is silent: `queued` cannot, because a withheld row never reaches
    /// the queue. Same role [`Self::cadence_steps`] plays for the footstep. The
    /// menu rows fire again, so this and `queued` now climb together - keeping
    /// both is what would make a future withholding visible rather than
    /// indistinguishable from deleting the call.
    pub menu_cue_requests: u32,
    /// Cues that keyed an SPU voice since the page loaded - the page's readout
    /// and the audibility half of the measurement.
    pub fired: u32,
    /// The most recent `(cue id, first voice)` that keyed on.
    pub last_fired: Option<(u16, u8)>,
    /// Whether the program banks uploaded into the live SPU.
    pub vab_staged: bool,
    /// Whether [`LegaiaRuntime::load_sfx_bank_bytes`] has run against a
    /// staged scene host. A flag rather than `bank_bytes.is_empty()` because
    /// the transient reward slot can land in `bank_bytes` before any pinned
    /// bank has been read, and must not make the pinned read look done.
    pub bank_bytes_loaded: bool,
    /// The battle audio duck, in retail's own units: `_DAT_8007B910` is the
    /// live level (seeded [`DUCK_LEVEL_REF`]), ramped one unit per vsync
    /// toward a target the action SM sets - `ref * 75 / 100` under a summon,
    /// back to `ref` in the Done band's `0x51` arm - and applied to the BGM
    /// through `SsSeqSetVol`. `duck_level` mirrors the cell; `duck_target`
    /// the arm's clamp. The browser twin of the native director's pair.
    pub duck_level: u8,
    pub duck_target: u8,
    /// SPU RAM base the transient reward bank (slot 11) was uploaded at,
    /// `None` while it is not staged. It borrows the free tail of the BGM
    /// region, so a track restage invalidates it
    /// ([`LegaiaRuntime::reconcile_reward_bank`]).
    pub reward_bank_base: Option<u32>,
    /// Queued cues `classify_cue` routed to the CD-XA **voice** leg
    /// (`id >= 0x100`) and this host dropped, because - like the native
    /// window - it stages no bank for the cast-voice clip files. Counted so
    /// the readout can say the cue reached the mixer and was declined,
    /// rather than the cue never having been produced.
    pub voice_cues_dropped: u32,
    /// The CD-XA lane: arts-voice shouts + battle one-shot clips
    /// ([`crate::play_xa`]).
    pub xa: crate::play_xa::PlayXa,
}

impl Default for PlaySfx {
    fn default() -> Self {
        Self {
            bank: SfxBank::default(),
            cue_slots: BTreeMap::new(),
            bank_bytes: BTreeMap::new(),
            sched: SfxScheduler::default(),
            cadence: Default::default(),
            prev_pos: None,
            cadence_steps: 0,
            queued: 0,
            menu_cue_requests: 0,
            fired: 0,
            last_fired: None,
            vab_staged: false,
            bank_bytes_loaded: false,
            duck_level: DUCK_LEVEL_REF,
            duck_target: DUCK_LEVEL_REF,
            reward_bank_base: None,
            voice_cues_dropped: 0,
            xa: Default::default(),
        }
    }
}

/// How a queued cue id leaves the scheduler - the routing half of retail's
/// cue dispatcher `FUN_8004FCC8` (`legaia_engine_audio::classify_cue`), applied
/// **at fire time**, exactly where the native director applies it.
///
/// The queue is a `u16` because the battle cue space is - the action SM's
/// cast cues run to `0x20E` - while the SFX descriptor table is `0x00..=0x63`.
/// Truncating with `as u8` did not make an out-of-band cue silent, it made it
/// play the **wrong** descriptor: `0x20C` became `0x0C`, `0x118` became `0x18`,
/// and every one of those is a populated entry.
///
/// Only the `Voice` band is re-routed. The `Ring` band's `id - 1` resolution
/// below `0x40` is retail's (`classify_cue`'s low leg) but the producers that
/// feed this queue hand it an art-record `HitCue::kind` / a menu descriptor id
/// the bank is already indexed by, so applying it would silently move every
/// cue that works today. The native director makes the same choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CueRoute {
    /// Key this descriptor id out of the bank its category names.
    Descriptor(u8),
    /// A streamed CD-XA voice trigger (`FUN_8003D53C`), not an SPU
    /// descriptor: `channel` is the clip slot after the `1/3/5` remap,
    /// `submode` the channel inside the file. The one producer feeding this
    /// queue such ids is the `FUN_801F3990` band, which the two measured
    /// retail casts never raised (`docs/subsystems/cast-module.md`), so it
    /// is declined on both hosts. The cast's own voice (the module head cue)
    /// never comes through here: it rides the `(clip, channel, dur)` channel
    /// into [`crate::play_xa`]'s lazy tier.
    Voice { channel: u8, submode: u8 },
}

/// Route one queued cue id ([`CueRoute`]).
pub(crate) fn route_cue(id: u16) -> CueRoute {
    match classify_cue(u32::from(id)) {
        CueDispatch::Voice {
            channel, submode, ..
        } => CueRoute::Voice { channel, submode },
        // A `Ring` id that is out of the descriptor space could only come
        // from a `0` cue wrapping to `0xFFFF`; keying descriptor `0xFF`
        // is a no-op in every bank, so the narrowing loses nothing.
        CueDispatch::Ring { .. } => CueRoute::Descriptor(id.min(0xFF) as u8),
    }
}

/// Sample end of an uploaded bank in SPU RAM - one past the highest
/// `addr + size` of its VAGs - or `fallback` for an empty / absent bank. The
/// transient reward bank is placed at the 16-byte-aligned address above this.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn vab_bank_used_end(bank: Option<&VabBank>, fallback: u32) -> u32 {
    bank.and_then(|b| b.samples.iter().flatten().map(|s| s.addr + s.size).max())
        .unwrap_or(fallback)
}

impl PlaySfx {
    /// The duck target as retail computes it from a percentage of the
    /// reference level (`BattleEvent::DuckAudioLevel`): `75` under a summon /
    /// magic capture, `100` when the Done band ramps it back. The ramp itself
    /// runs in [`Self::tick_duck`].
    pub fn set_duck_pct(&mut self, pct: u8) {
        let pct = u32::from(pct.min(100));
        self.duck_target = (u32::from(DUCK_LEVEL_REF) * pct / 100) as u8;
    }

    /// One frame of the duck ramp: step the live level one unit toward the
    /// target (retail `DAT_1F800393` per vsync, the `0x35` / `0x51` arms) and
    /// return the sequencer master volume to re-apply, `master_vol * level /
    /// ref` - the `FUN_800267A8` -> `SsSeqSetVol` re-apply, which halves the
    /// cell into the 0..127 volume domain the same way `master_vol` already
    /// is. `None` when the level already sits at its target (nothing to
    /// re-apply, and the native director skips the call too).
    pub fn tick_duck(&mut self) -> Option<u8> {
        if self.duck_level == self.duck_target {
            return None;
        }
        self.duck_level = if self.duck_level < self.duck_target {
            self.duck_level + 1
        } else {
            self.duck_level - 1
        };
        let vol = u32::from(crate::play_bgm::BGM_MASTER_VOL) * u32::from(self.duck_level)
            / u32::from(DUCK_LEVEL_REF);
        Some(vol.min(127) as u8)
    }
    /// The VAB slot a cue's descriptor names, resolved through its `+4`
    /// category. `None` for an id the disc table doesn't carry.
    pub fn slot_for_cue(&self, id: u8) -> Option<u8> {
        self.cue_slots.get(&id).copied()
    }

    /// The staged bank a cue must key, with the fallback the routing needs.
    ///
    /// **The fallback is the pre-routing behaviour on purpose.** This page
    /// stages slots `0` and `2`; categories `6` and `11` name banks it does not
    /// hold (PROT 0876 / 0889 - traced, but they do not fit beside the other
    /// two in the shared SPU region), so those - and an unknown cue id -
    /// resolve to [`FALLBACK_VAB_SLOT`] = the class-2 bank, exactly the bank
    /// this page staged for every cue before the routing existed. That keeps
    /// categories 6 / 11 sounding as they did while categories 0 / 2 become
    /// correct. Retail avoids the arithmetic because slot 6 *is* slot 2's
    /// region, refilled per game mode - see `docs/formats/sfx-table.md`.
    fn resolve_slot(&self, id: u8) -> u8 {
        let slot = self.slot_for_cue(id).unwrap_or(FALLBACK_VAB_SLOT);
        if self.bank_bytes.contains_key(&slot) {
            slot
        } else {
            FALLBACK_VAB_SLOT
        }
    }
}

impl LegaiaRuntime {
    /// Render one cue on a throwaway SPU with its own fresh upload of the
    /// program bank **its category names**, and report
    /// `(peak, active_samples)`: the loudest absolute sample, and how far in the
    /// cue was last non-zero.
    ///
    /// Deliberately does not touch the live SPU - rendering consumes ticks, and
    /// stealing them from the audio callback would glitch the music. Backs both
    /// [`Self::play_sfx_probe_peak`] and
    /// [`Self::play_sfx_probe_active_samples`].
    fn probe_render(&mut self, id: u32, max_samples: u32) -> (u32, u32) {
        use legaia_engine_audio::{
            Spu, VabBank,
            spu::ram::{SPU_RAM_BYTES, SpuAllocator},
        };
        if id > u8::MAX as u32 {
            return (0, 0);
        }
        self.load_sfx_bank_bytes();
        let slot = self.sfx.resolve_slot(id as u8);
        let Some(staged) = self.sfx.bank_bytes.get(&slot) else {
            return (0, 0);
        };
        let Ok(report) = legaia_vab::parse(&staged.bytes, staged.vab_offset) else {
            return (0, 0);
        };
        let mut spu = Spu::new();
        let mut alloc = SpuAllocator::new(
            SPU_RESERVED_BYTES,
            SPU_RAM_BYTES as u32 - SPU_RESERVED_BYTES,
        );
        let vab = VabBank::upload(
            &mut spu,
            &mut alloc,
            &report,
            &staged.bytes[staged.vab_offset..],
        );
        if self
            .sfx
            .bank
            .play_one_shot(id as u8, &mut spu, &vab)
            .is_none()
        {
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
                    .map(|(id, d)| (id, d.program, d.tone, d.note, d.voice_count())),
            );
            self.sfx.cue_slots = table.cue_slots().collect();
        }
    }

    /// Read each **pinned** slot's program bank off the loaded PROT and keep
    /// its bytes, keyed by slot: slot 0 = PROT 0868 (the shared UI cues), slot
    /// 2 = PROT 0869 (battle / duel), with the `DAT_8007BD11 == 4` alternate
    /// 0875 as slot 2's fallback. Each is tried at VAB offset `+4` (the entry
    /// is a chunk-header-prefixed stream) then `+0`. No-op once staged.
    pub(crate) fn load_sfx_bank_bytes(&mut self) {
        if self.sfx.bank_bytes_loaded {
            return;
        }
        let Some(host) = self.scene_host.as_ref() else {
            return;
        };
        self.sfx.bank_bytes_loaded = true;
        for (slot, prot) in PINNED_SLOT_BANKS.iter().copied() {
            // The class-2 slot has a documented alternate entry (`0875` when
            // `DAT_8007BD11 == 4`); the slot-0 system bank has no such swap, so
            // its second candidate is a repeat and the loop breaks on the first.
            let alt = if slot == FALLBACK_VAB_SLOT {
                crate::sfx_view::SFX_BANK_ALT_PROT_INDEX
            } else {
                prot
            };
            for idx in [prot, alt] {
                let Ok(bytes) = host.index.entry_bytes_extended(idx) else {
                    continue;
                };
                let Some(vab_offset) = [4usize, 0]
                    .into_iter()
                    .find(|o| legaia_vab::parse(&bytes, *o).is_ok())
                else {
                    continue;
                };
                self.sfx.bank_bytes.insert(
                    slot,
                    StagedBankBytes {
                        prot: idx,
                        bytes,
                        vab_offset,
                    },
                );
                break;
            }
        }
    }

    /// Queue a cue to fire `frames` sim ticks from now (`0` = this frame).
    ///
    /// The id is the full `u16` cue space (`FUN_8004FCC8`'s, which the
    /// battle's cast cues reach at `0x118..` / `0x20C..`); it is classified at
    /// fire time ([`route_cue`]), never truncated. `impl Into<u16>` so the
    /// existing `u8` descriptor-id callers keep compiling unchanged.
    pub(crate) fn enqueue_sfx(&mut self, id: impl Into<u16>, frames: u16) {
        self.sfx.queued += 1;
        self.sfx.sched.enqueue(PendingCue::new(id.into(), frames));
    }

    /// Queue a battle cue with its `(actor, target)` slots riding along -
    /// the native `AudioBgmDirector::enqueue_sfx` signature. The slots are
    /// HUD / trace context on both hosts: the native fire path pans nothing
    /// off them, so neither does this one.
    pub(crate) fn enqueue_battle_cue(&mut self, id: u16, frames: u16, actor: u8, target: u8) {
        self.sfx.queued += 1;
        self.sfx
            .sched
            .enqueue(PendingCue::new(id, frames).with_actors(actor, target));
    }

    /// Set the battle audio duck's target (`BattleEvent::DuckAudioLevel`).
    /// The ramp runs in [`Self::tick_duck`], once per sim tick.
    pub(crate) fn set_duck_pct(&mut self, pct: u8) {
        self.sfx.set_duck_pct(pct);
    }

    /// One frame of the duck ramp, re-applied to the live sequencer. Ticked
    /// from [`Self::tick_sfx`] so the ramp back to full after a battle keeps
    /// running once the scene mode has left `Battle`. No-op off wasm (no
    /// sequencer to re-apply to; the level still ramps, so the tests can read
    /// it back).
    pub(crate) fn tick_duck(&mut self) {
        let _vol = self.sfx.tick_duck();
        #[cfg(target_arch = "wasm32")]
        if let (Some(vol), Some(out)) = (_vol, self.audio_out.as_ref()) {
            out.set_sequencer_master_vol(vol);
        }
    }

    /// Drop every queued SFX cue and the transient reward bank - the scene
    /// transition / battle abort clear the native window runs as
    /// `bgm.clear_sfx()` on every `SceneEntered` edge (`boot.rs`). Cues
    /// queued against the departing scene's timing must not fire into the
    /// next one, and the reward bank borrowed room in a BGM region the new
    /// scene's VAB restage re-owns.
    pub(crate) fn on_scene_change_audio(&mut self) {
        self.sfx.sched.clear();
        self.drop_reward_bank();
    }

    /// Forget the transient reward bank (slot 11) wherever it is recorded.
    fn drop_reward_bank(&mut self) {
        self.sfx.reward_bank_base = None;
        self.sfx.bank_bytes.remove(&TRANSIENT_REWARD_SLOT);
        #[cfg(target_arch = "wasm32")]
        self.sfx_vabs.remove(&TRANSIENT_REWARD_SLOT);
    }

    /// Sample end of the staged BGM bank in SPU RAM (`LegaiaRuntime::bgm_bank`,
    /// whichever of the two upload sites last filled the region), the address
    /// the transient reward bank is placed above. An absent bank reads as the
    /// region floor. `None` off wasm, where there is no SPU and no bank: the
    /// reward bank then does not stage, and cue `0x50` takes the class-2
    /// fallback it always took.
    pub(crate) fn bgm_bank_used_end(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        {
            Some(vab_bank_used_end(
                self.bgm_bank.as_ref(),
                SPU_RESERVED_BYTES,
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            None
        }
    }

    /// Whether the reward bank is staged in slot 11.
    pub(crate) fn has_reward_bank(&self) -> bool {
        self.sfx.reward_bank_base.is_some()
    }

    /// Drop the reward bank if a BGM restage has since grown into its room.
    /// The two BGM upload sites (`stage_scene_bgm_bank` in the runtime and
    /// `WebBgmDirector::stage_owned`) re-own the region wholesale, so the
    /// bank is stale the moment the live bank's sample end passes its base -
    /// the native director drops it inside those two sites; this host checks
    /// at fire time instead, which needs no hook in either.
    fn reconcile_reward_bank(&mut self) {
        if let (Some(base), Some(used)) = (self.sfx.reward_bank_base, self.bgm_bank_used_end())
            && used > base
        {
            self.drop_reward_bank();
        }
    }

    /// Stage the battle-end reward bank (PROT 0889, cue `0x50`, category 11)
    /// **behind the resident BGM bank**, in the free tail of the BGM region -
    /// the port's version of retail's results-time load of PROT 0889 into
    /// slot 11 (`FUN_8001FC00(0x37B, 0xB, ..)` + `FUN_8001E54C(0xB, ..)` in
    /// `FUN_8004E568` phases 2 / 4), and the twin of the native
    /// `AudioBgmDirector::stage_transient_sfx_vab`. The SFX region is full
    /// (its two pinned banks leave ~2.5 KB and this VAB body is 19344 bytes),
    /// so the reward bank borrows BGM room instead, exactly as long as the
    /// current track leaves any. Idempotent while staged. Returns `false`
    /// when the BGM occupancy is unknown, the entry has no VAB header, or the
    /// tail is too small - cue `0x50` then keys the class-2 fallback.
    ///
    /// Off wasm there is no SPU: the bank is recorded as staged (so the
    /// routing is testable) but nothing is uploaded.
    // REF: FUN_8001E54C, FUN_8004E568
    pub(crate) fn stage_transient_reward_bank(&mut self) -> bool {
        self.reconcile_reward_bank();
        if self.has_reward_bank() {
            return true;
        }
        let Some(used_end) = self.bgm_bank_used_end() else {
            return false;
        };
        let Some(host) = self.scene_host.as_ref() else {
            return false;
        };
        let Ok(bytes) = host
            .index
            .entry_bytes_extended(SLOT11_REWARD_BANK_PROT_INDEX)
        else {
            return false;
        };
        let Some((report, vab_offset)) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        else {
            return false;
        };
        let region_end = legaia_engine_audio::spu::ram::SPU_RAM_BYTES as u32 - SFX_BANK_SPU_BYTES;
        let base = used_end.div_ceil(16) * 16;
        if base >= region_end {
            return false;
        }
        let body_total: u32 = report.vag_samples.iter().map(|v| v.size as u32).sum();
        if body_total > region_end - base {
            return false;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let Some(out) = self.audio_out.as_ref() else {
                return false;
            };
            let bank = out.with_spu(|spu| {
                let mut alloc =
                    legaia_engine_audio::spu::ram::SpuAllocator::new(base, region_end - base);
                VabBank::upload(spu, &mut alloc, &report, &bytes[vab_offset..])
            });
            self.sfx_vabs.insert(TRANSIENT_REWARD_SLOT, bank);
        }
        self.sfx.bank_bytes.insert(
            TRANSIENT_REWARD_SLOT,
            StagedBankBytes {
                prot: SLOT11_REWARD_BANK_PROT_INDEX,
                bytes,
                vab_offset,
            },
        );
        self.sfx.reward_bank_base = Some(base);
        true
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
            .and_then(|h| h.world.locomotion.player_anim.as_ref())
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
        // The battle audio duck ramps one retail unit per vsync, whatever
        // mode the scene is in (the ramp back to full outlives the battle).
        self.tick_duck();
        self.fire_matured_sfx();
    }

    /// Fire this frame's matured cues into the live SPU. Split out of
    /// [`Self::tick_sfx`] so the direct-play entry point can use it too.
    fn fire_matured_sfx(&mut self) {
        // Off wasm there is no live SPU to key into (`WebAudioOut` is the only
        // audio device this crate has), so the scheduler still advances - which
        // is what the disc-gated tests exercise - but nothing sounds.
        let batch = self.sfx.sched.tick_frame();
        if batch.is_empty() {
            return;
        }
        // Classify first, on every target: a voice-leg cue is declined the
        // same way whether or not there is a mixer, and the count is what the
        // off-wasm tests read.
        let mut descriptors: Vec<(u16, u8)> = Vec::with_capacity(batch.fired.len());
        for cue in &batch.fired {
            match route_cue(cue.id) {
                CueRoute::Descriptor(id) => descriptors.push((cue.id, id)),
                CueRoute::Voice { .. } => self.sfx.voice_cues_dropped += 1,
            }
        }
        #[cfg(target_arch = "wasm32")]
        if !descriptors.is_empty() {
            // A page that could not stage an SFX bank still has the scene's
            // own BGM VAB resident, and that is what the native director
            // keys against in the same situation (`tick_sfx_frame` gives up
            // only when `sfx_vabs` is empty **and** `bank` is `None`). This
            // host used to `return` here, so every cue on a boot where the
            // SFX slots did not fit was dropped rather than played out of
            // the wrong bank - silence where the native window is audible.
            let staged = self.stage_sfx_vab();
            if !staged && self.bgm_bank.is_none() {
                return;
            }
            // A BGM restage since the reward bank staged makes it stale.
            self.reconcile_reward_bank();
            let Some(out) = self.audio_out.as_ref() else {
                return;
            };
            let bank = &self.sfx.bank;
            let sfx = &self.sfx;
            let vabs = &self.sfx_vabs;
            let bgm_bank = self.bgm_bank.as_ref();
            let mut fired = Vec::new();
            out.with_spu(|spu| {
                for &(queued_id, id) in &descriptors {
                    // Each cue keys the bank its own `+4` category names. The
                    // second `get` covers a slot whose bytes read but whose
                    // upload failed - the cue keeps its old sound rather than
                    // dropping out - and the scene BGM bank is the last
                    // resort, the native fallback.
                    let Some(vab) = vabs
                        .get(&sfx.resolve_slot(id))
                        .or_else(|| vabs.get(&FALLBACK_VAB_SLOT))
                        .or(bgm_bank)
                    else {
                        continue;
                    };
                    if let Some(voice) = bank.play_one_shot(id, spu, vab) {
                        fired.push((queued_id, voice));
                    }
                }
            });
            self.sfx.fired += fired.len() as u32;
            if let Some(last) = fired.last() {
                self.sfx.last_fired = Some(*last);
            }
        }
    }

    /// Key one voice from an explicit
    /// [`VoiceAttr`](legaia_engine_audio::VoiceAttr) set - the door
    /// `AudioBgmDirector::key_on_voice_attr` is on the native window, and the
    /// minigames page's `LegaiaMinigames::muscle_tally_voice` is on its own.
    ///
    /// Some cues never name themselves by id: the Muscle Dome's between-leg
    /// tally roll resolves a whole attr set per lane (`FUN_801D1288`), so the
    /// id-keyed scheduler above could not sound it however it was fed. The
    /// tally was therefore silent on this host alone.
    ///
    /// `attr.vab_id` is a retail **VAB id**, resolved here as an SFX slot with
    /// the live scene BGM bank behind it - the same two-step the native
    /// director runs, and the same one [`Self::fire_matured_sfx`] uses.
    /// Returns whether a voice keyed on.
    pub(crate) fn key_on_voice_attr(&mut self, attr: legaia_engine_audio::VoiceAttr) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            let slot = u8::try_from(attr.vab_id).unwrap_or(0);
            if !self.sfx_vabs.contains_key(&slot) {
                self.stage_sfx_vab();
            }
            let Some(vab) = self.sfx_vabs.get(&slot).or(self.bgm_bank.as_ref()) else {
                return false;
            };
            let Some(out) = self.audio_out.as_ref() else {
                return false;
            };
            out.with_spu(|spu| legaia_engine_audio::key_on_voice_attr(&attr, spu, vab))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // No live SPU off wasm; the count is still what the tests read.
            let _ = attr;
            false
        }
    }

    /// Upload every pinned slot's program bank into the dedicated top region of
    /// SPU RAM. Idempotent; returns whether at least one bank is resident.
    /// Needs audio to be live, so this runs lazily on the first cue rather than
    /// at `load_disc`.
    ///
    /// The banks share **one** `SpuAllocator` over the region, so they pack
    /// end to end. Two allocators each starting at the region base would put
    /// slot 0's samples on top of slot 2's and every cue would play whichever
    /// bank uploaded last - the exact failure the routing exists to remove.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn stage_sfx_vab(&mut self) -> bool {
        if !self.sfx_vabs.is_empty() {
            return true;
        }
        self.load_sfx_bank_bytes();
        let Some(out) = self.audio_out.as_ref() else {
            return false;
        };
        if self.sfx.bank_bytes.is_empty() {
            return false;
        }
        let staged = out.with_spu(|spu| {
            use legaia_engine_audio::spu::ram::{SPU_RAM_BYTES, SpuAllocator};
            // Top region, below nothing - the BGM allocator is capped under it.
            let mut alloc = SpuAllocator::new(
                SPU_RAM_BYTES as u32 - SFX_BANK_SPU_BYTES,
                SFX_BANK_SPU_BYTES,
            );
            let mut out_map = BTreeMap::new();
            for (slot, b) in self.sfx.bank_bytes.iter() {
                // The reward bank lives behind the BGM, not in this region.
                if *slot == TRANSIENT_REWARD_SLOT {
                    continue;
                }
                let Ok(report) = legaia_vab::parse(&b.bytes, b.vab_offset) else {
                    continue;
                };
                let bank = legaia_engine_audio::VabBank::upload(
                    spu,
                    &mut alloc,
                    &report,
                    &b.bytes[b.vab_offset..],
                );
                out_map.insert(*slot, bank);
            }
            out_map
        });
        if staged.is_empty() {
            return false;
        }
        self.sfx_vabs = staged;
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
    ///
    /// `id` is the full `u16` dispatch space: a cast-voice id (`>= 0x100`)
    /// is accepted, classified at fire time and declined on the voice leg
    /// (counted in `voice_cues_dropped`), so the return is `false` for it -
    /// never a truncated descriptor keyed by mistake.
    pub fn play_sfx(&mut self, id: u32) -> bool {
        let Ok(id) = u16::try_from(id) else {
            return false;
        };
        let before = self.sfx.fired;
        self.enqueue_sfx(id, 0);
        self.fire_matured_sfx();
        self.sfx.fired > before
    }

    /// Is the SFX channel able to make a sound right now? True once the
    /// descriptor table decoded, the program banks staged into the live SPU,
    /// and audio is up.
    pub fn play_sfx_ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            !self.sfx.bank.is_empty() && !self.sfx_vabs.is_empty()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            !self.sfx.bank.is_empty()
        }
    }

    /// The channel's state for the page's readout:
    ///
    /// ```json
    /// { "descriptors": 100, "bank_prot": 869,
    ///   "banks": [ { "slot": 0, "prot": 868 }, { "slot": 2, "prot": 869 } ],
    ///   "vab_staged": true, "queued": 14, "fired": 12, "last_cue": 33,
    ///   "last_voice": 4, "idle_voices": 20 }
    /// ```
    ///
    /// `queued` counts what the cue *sources* produced and `fired` what the
    /// SPU took; the two differing is the readout that separates "no source
    /// fired" from "fired but inaudible". `banks` is the staged slot -> PROT
    /// map; `bank_prot` stays the class-2 entry specifically, because that is
    /// the bank an unstaged category still falls back to.
    pub fn play_sfx_state_json(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        let idle = self
            .audio_out
            .as_ref()
            .map(|o| o.with_spu(|spu| spu.idle_voice_count()))
            .unwrap_or(0);
        #[cfg(not(target_arch = "wasm32"))]
        let idle = 0usize;
        let banks: Vec<serde_json::Value> = self
            .sfx
            .bank_bytes
            .iter()
            .map(|(slot, b)| serde_json::json!({ "slot": slot, "prot": b.prot }))
            .collect();
        serde_json::json!({
            "descriptors": self.sfx.bank.len(),
            "bank_prot": self
                .sfx
                .bank_bytes
                .get(&FALLBACK_VAB_SLOT)
                .map(|b| b.prot)
                .unwrap_or(0),
            "banks": banks,
            "vab_staged": self.sfx.vab_staged,
            "cadence_steps": self.sfx.cadence_steps,
            "menu_cue_requests": self.sfx.menu_cue_requests,
            "queued": self.sfx.queued,
            "fired": self.sfx.fired,
            "last_cue": self.sfx.last_fired.map(|(id, _)| id),
            "last_voice": self.sfx.last_fired.map(|(_, v)| v),
            "idle_voices": idle,
            "pending": self.sfx.sched.pending_count(),
            "voice_cues_dropped": self.sfx.voice_cues_dropped,
            "duck_level": self.sfx.duck_level,
            "duck_target": self.sfx.duck_target,
            "reward_bank_staged": self.sfx.reward_bank_base.is_some(),
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
        if id > u8::MAX as u32 {
            return 0xFF;
        }
        self.sfx.slot_for_cue(id as u8).unwrap_or(0xFF) as u32
    }

    /// The PROT entry a cue **actually** sounds out of on this host, i.e. its
    /// slot after the unstaged-slot fallback ([`PlaySfx::resolve_slot`]). `0`
    /// when no bank could be read (a `PROT.DAT`-only load, or no scene staged).
    ///
    /// This is the observable the routing is measured by: two cues in different
    /// retail categories must report different entries, which is a fact about
    /// the page's own behaviour rather than about the descriptor table.
    pub fn play_sfx_cue_bank_prot(&mut self, id: u32) -> u32 {
        if id > u8::MAX as u32 {
            return 0;
        }
        self.load_sfx_bank_bytes();
        let slot = self.sfx.resolve_slot(id as u8);
        self.sfx.bank_bytes.get(&slot).map(|b| b.prot).unwrap_or(0)
    }

    /// Fire the cue mapped to a named event (see
    /// [`Self::play_sfx_events_json`]). Returns `false` for an unknown event, a
    /// row whose cue is withheld, or a cue that did not sound.
    ///
    /// A known-but-withheld row still counts the request
    /// ([`PlaySfx::menu_cue_requests`]), so the page's firing site stays
    /// measurable even for a row whose cue is `None`.
    ///
    /// Off wasm there is no `WebAudioOut` and so no live SPU, so this returns
    /// `false` there for a cue that *did* enqueue. The counters, not the return
    /// value, are what the disc-gated tests read.
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

    /// [`SFX_BANK_SPU_BYTES`] is squeezed between two hard measurements, and
    /// this is both of them. Widening it silences BGM; narrowing it drops a
    /// resident SFX bank. Either failure is silent in play - a track that
    /// stops loading its instruments and a cue that keys a sibling sample both
    /// sound like "the audio is a bit off", which is why the numbers are
    /// asserted rather than left in a comment.
    ///
    /// The four constants are disc measurements from `vab list`: the two
    /// pinned banks' VAG-body totals (PROT 0868 / 0869) and the two largest
    /// VAB sample bodies in `PROT.DAT` that a BGM path can stage (269632 in
    /// `1071_music_01`, 268496 in `1113_vab_01`). Every VAG in all four is
    /// already a multiple of the allocator's 16-byte ADPCM block, so the
    /// packed footprint equals the raw total exactly.
    #[test]
    fn sfx_bank_region_fits_both_pinned_banks() {
        use legaia_engine_audio::spu::ram::SPU_RAM_BYTES;
        const SLOT0_BODY_BYTES: u32 = 59_136; // PROT 0868
        const SLOT2_BODY_BYTES: u32 = 188_128; // PROT 0869
        const LARGEST_STAGED_BGM_BODY_BYTES: u32 = 269_632; // 1071_music_01
        const SECOND_LARGEST_BGM_BODY_BYTES: u32 = 268_496; // 1113_vab_01

        let both = SLOT0_BODY_BYTES + SLOT2_BODY_BYTES;
        assert!(
            both <= SFX_BANK_SPU_BYTES,
            "both pinned banks must fit one region: {both} > {SFX_BANK_SPU_BYTES}"
        );

        let bgm_budget = SPU_RAM_BYTES as u32 - SPU_RESERVED_BYTES - SFX_BANK_SPU_BYTES;
        for body in [LARGEST_STAGED_BGM_BODY_BYTES, SECOND_LARGEST_BGM_BODY_BYTES] {
            assert!(
                body <= bgm_budget,
                "a BGM VAB that fits today ({body}) must still fit: budget {bgm_budget}"
            );
        }
    }

    /// The duck is retail's own arithmetic: a 75% target lands at
    /// `0xD7 * 75 / 100`, the level steps one unit per tick toward it, and
    /// each step re-applies `master_vol * level / ref` - so the volume the
    /// sequencer is handed under a full summon duck is 75 of the 100 the
    /// track started at, on this host exactly as on the native one.
    #[test]
    fn duck_ramps_one_unit_per_tick_and_scales_the_shared_master_vol() {
        let mut sfx = PlaySfx::default();
        assert_eq!(sfx.duck_level, DUCK_LEVEL_REF);
        assert_eq!(sfx.tick_duck(), None, "at target: nothing to re-apply");
        sfx.set_duck_pct(75);
        let target = (u32::from(DUCK_LEVEL_REF) * 75 / 100) as u8;
        assert_eq!(sfx.duck_target, target);
        let first = sfx.tick_duck().expect("one step down");
        assert_eq!(sfx.duck_level, DUCK_LEVEL_REF - 1);
        assert_eq!(
            first,
            (100 * u32::from(DUCK_LEVEL_REF - 1) / u32::from(DUCK_LEVEL_REF)) as u8
        );
        let mut last = first;
        let mut steps = 1;
        while let Some(v) = sfx.tick_duck() {
            last = v;
            steps += 1;
        }
        assert_eq!(steps, u32::from(DUCK_LEVEL_REF - target));
        assert_eq!(sfx.duck_level, target);
        // Retail floors twice - `0xD7 * 75 / 100 = 161`, then
        // `100 * 161 / 0xD7 = 74` - so a "75%" duck hands the sequencer 74,
        // on this host exactly as the native director's `tick_duck` does.
        let expected = (100 * u32::from(target) / u32::from(DUCK_LEVEL_REF)) as u8;
        assert_eq!(expected, 74);
        assert_eq!(
            last, expected,
            "the duck floor is retail's, not a rounded 75"
        );
        // Back to full ramps up the same way and lands on the seed volume.
        sfx.set_duck_pct(100);
        let mut last = 0;
        while let Some(v) = sfx.tick_duck() {
            last = v;
        }
        assert_eq!(last, crate::play_bgm::BGM_MASTER_VOL);
        // Out-of-range percentages clamp rather than overflow.
        sfx.set_duck_pct(250);
        assert_eq!(sfx.duck_target, DUCK_LEVEL_REF);
    }

    /// A cast cue is a `FUN_8004FCC8` id (`0x118`, `0x20C`, ...), and the
    /// dispatcher sends it to the CD-XA voice leg. Truncating it to a `u8`
    /// would key descriptor `0x18` / `0x0C` - populated entries - so the
    /// route must say **voice**, never a descriptor, for every id at or
    /// above `0x100`; and every descriptor-space id must pass through
    /// unchanged.
    #[test]
    fn cast_cues_route_to_the_voice_leg_not_a_truncated_descriptor() {
        for id in [0x118u16, 0x119, 0x11A, 0x20C, 0x20D, 0x20E, 0x100, 0x1FF] {
            match route_cue(id) {
                CueRoute::Voice { .. } => {}
                other => panic!("{id:#x} must route to the voice leg, got {other:?}"),
            }
        }
        assert_eq!(
            route_cue(0x118),
            CueRoute::Voice {
                channel: 0x1B,
                submode: 0
            }
        );
        assert_eq!(
            route_cue(0x20E),
            CueRoute::Voice {
                channel: 0x21,
                submode: 6
            }
        );
        for id in [0x1Au16, 0x20, 0x21, 0x37, 0x50, 0x63, 0xFF] {
            assert_eq!(route_cue(id), CueRoute::Descriptor(id as u8));
        }
    }

    /// The reward bank goes above the highest staged VAG; an empty or
    /// absent bank puts it at the region floor.
    #[test]
    fn bank_used_end_is_the_highest_vag_end() {
        use legaia_engine_audio::UploadedVag;
        let bank = VabBank {
            master_vol: 127,
            samples: vec![
                Some(UploadedVag {
                    addr: 0x1000,
                    size: 0x200,
                }),
                None,
                Some(UploadedVag {
                    addr: 0x4000,
                    size: 0x10,
                }),
            ],
            programs: Vec::new(),
        };
        assert_eq!(vab_bank_used_end(Some(&bank), SPU_RESERVED_BYTES), 0x4010);
        let empty = VabBank {
            master_vol: 127,
            samples: Vec::new(),
            programs: Vec::new(),
        };
        assert_eq!(
            vab_bank_used_end(Some(&empty), SPU_RESERVED_BYTES),
            SPU_RESERVED_BYTES
        );
        assert_eq!(
            vab_bank_used_end(None, SPU_RESERVED_BYTES),
            SPU_RESERVED_BYTES
        );
    }

    /// The scene-change clear empties the scheduler and forgets the reward
    /// bank, so the next scene's cue `0x50` resolves to the fallback again
    /// until a results frame restages it.
    #[test]
    fn scene_change_clears_the_queue_and_the_reward_bank() {
        let mut rt = LegaiaRuntime::new();
        rt.enqueue_sfx(0x21u8, 3);
        rt.enqueue_battle_cue(0x118, 5, 0, 3);
        rt.sfx.reward_bank_base = Some(0x8000);
        rt.sfx.bank_bytes.insert(
            TRANSIENT_REWARD_SLOT,
            StagedBankBytes {
                prot: SLOT11_REWARD_BANK_PROT_INDEX,
                bytes: Vec::new(),
                vab_offset: 0,
            },
        );
        assert_eq!(rt.sfx.sched.pending_count(), 2);
        assert!(rt.has_reward_bank());
        rt.on_scene_change_audio();
        assert_eq!(rt.sfx.sched.pending_count(), 0);
        assert!(!rt.has_reward_bank());
        assert!(!rt.sfx.bank_bytes.contains_key(&TRANSIENT_REWARD_SLOT));
        // Without a BGM occupancy reading the bank refuses to stage: the
        // conservative answer, and the one the routing then falls back on.
        assert!(!rt.stage_transient_reward_bank());
    }

    /// A voice-leg cue that matures is counted as declined, on every
    /// target, and never reaches the descriptor path.
    #[test]
    fn a_matured_voice_cue_is_declined_and_counted() {
        let mut rt = LegaiaRuntime::new();
        rt.enqueue_battle_cue(0x20C, 0, 3, 0);
        rt.enqueue_sfx(0x118u16, 0);
        assert_eq!(rt.sfx.voice_cues_dropped, 0);
        rt.fire_matured_sfx();
        assert_eq!(rt.sfx.voice_cues_dropped, 2);
        assert_eq!(rt.sfx.queued, 2);
    }

    /// The fallback for an unstaged slot is the *previous* behaviour, and it
    /// has to stay that: categories 6 and 11 have no traced PROT entry, and
    /// routing them anywhere but the class-2 bank would change 31 descriptors'
    /// sound on a guess. See the bank-routing thread.
    #[test]
    fn unstaged_and_unknown_cues_fall_back_to_the_class2_bank() {
        let mut sfx = PlaySfx {
            cue_slots: BTreeMap::from([(0x21, 0), (0x09, 2), (0x2E, 6), (0x4D, 11)]),
            ..Default::default()
        };
        // Nothing staged yet: every cue resolves to the fallback slot.
        for id in [0x21u8, 0x09, 0x2E, 0x4D, 0xFE] {
            assert_eq!(sfx.resolve_slot(id), FALLBACK_VAB_SLOT);
        }
        for (slot, prot) in PINNED_SLOT_BANKS.iter().copied() {
            sfx.bank_bytes.insert(
                slot,
                StagedBankBytes {
                    prot,
                    bytes: Vec::new(),
                    vab_offset: 0,
                },
            );
        }
        assert_eq!(sfx.resolve_slot(0x21), 0, "category 0 -> slot 0");
        assert_eq!(sfx.resolve_slot(0x09), 2, "category 2 -> slot 2");
        assert_eq!(
            sfx.resolve_slot(0x2E),
            FALLBACK_VAB_SLOT,
            "slot 6 not staged"
        );
        assert_eq!(
            sfx.resolve_slot(0x4D),
            FALLBACK_VAB_SLOT,
            "slot 11 not staged"
        );
        assert_eq!(
            sfx.resolve_slot(0xFE),
            FALLBACK_VAB_SLOT,
            "not in the table"
        );
    }
}
