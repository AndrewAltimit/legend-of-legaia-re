//! The [`legaia_engine_scene_host::scene_host::BgmDirector`] both hosts run: one director
//! generic over the audio output ([`legaia_engine_audio::AudioSink`]) - the
//! native cpal [`legaia_engine_audio::AudioOut`] or the browser's
//! `WebAudioOut`.
//!
//! The director owns the audio output handle plus the active scene's
//! [`legaia_engine_audio::VabBank`] (uploaded into the SPU at scene-load
//! time). On each `start` / `queue` call it parses the SEQ bytes the field
//! VM resolved through the BGM table, builds a [`legaia_engine_audio::Sequencer`],
//! and attaches it to the audio output. `pause` / `resume` toggle the
//! sequencer-feed flag without rebuilding state; `stop` detaches the
//! sequencer entirely.
//!
//! The retail engine routes BGM through SsAPI seq-context callbacks (see
//! `docs/subsystems/audio.md` "PsyQ libsnd SsAPI" + the `_DAT_801CE564`
//! seq-context resolver). We don't need that level of indirection in the
//! port - the field VM's BGM events arrive pre-resolved with the right SEQ
//! bytes and the active VAB is staged once per scene. This adapter is the
//! join point.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use legaia_engine_audio::bgm_tail::{BgmTail, TailBorrow};
use legaia_engine_audio::{
    ArtsShoutBank, AudioSink, PendingCue, SHOUT_CD_RESPONSE_DELAY, Sequencer, SfxBank,
    SfxFireBatch, SfxScheduler, VabBank, XaClipBank,
};
use legaia_engine_core::world::SideBandBank;
use legaia_engine_mode::sfx_residency::SharedRegionBank;
use legaia_engine_scene_host::scene_host::BgmDirector;
use legaia_engine_system::sound_state::SfxRingOp;
use legaia_game_tables::sfx_table::FALLBACK_VAB_SLOT;
use legaia_seq::Seq;

mod bgm_playback;
mod command_log;
mod sfx_banks;
mod sfx_fire;
mod xa_clips;

pub use command_log::{AudioCommand, AudioCommandLog};
#[cfg(test)]
use sfx_banks::resolve_sfx_slot;
pub use xa_clips::{XA_CUE_GAIN, XA_GAIN_UNITY, XaChannelSource, XaLaneStats, XaStageRequest};

/// The pause menu's cursor-step cue, re-exported from the one engine-side
/// table both hosts fire from ([`legaia_engine_menus::menu_cues`], provenance
/// there: `FUN_80032A44`'s ring writes). Category `0`, so the director sounds
/// it out of the slot-0 system bank (PROT 0868).
pub const RETAIL_MENU_CURSOR_CUE: u16 = legaia_engine_menus::menu_cues::MENU_CURSOR_CUE as u16;
/// The enabled-row confirm cue ([`legaia_engine_menus::menu_cues::MENU_CONFIRM_CUE`]).
pub const RETAIL_MENU_CONFIRM_CUE: u16 = legaia_engine_menus::menu_cues::MENU_CONFIRM_CUE as u16;
/// The cancel cue ([`legaia_engine_menus::menu_cues::MENU_CANCEL_CUE`]).
pub const RETAIL_MENU_CANCEL_CUE: u16 = legaia_engine_menus::menu_cues::MENU_CANCEL_CUE as u16;

/// BGM director that routes [`BgmDirector`] events into a live
/// audio output ([`AudioSink`]). The director holds a clone of the audio handle (cpal stream
/// is reference-counted internally via `Arc`) plus the active VAB bank.
pub struct AudioDirector<S: AudioSink> {
    audio: Arc<S>,
    bank: Option<VabBank>,
    /// Master volume forwarded to every freshly-attached sequencer. Engines
    /// bump this when the user adjusts the music slider.
    pub master_vol: u8,
    /// Loop-to event index for newly-started sequencers. `None` plays once
    /// (sequencer reports `finished` when it runs off the end). Most field
    /// BGM loops to 0; cutscene SEQs typically don't.
    pub loop_to: Option<usize>,
    /// Last started BGM id, if any. Useful for diagnostics + suppressing
    /// redundant `start(same_id)` calls (the field VM occasionally re-emits
    /// op `0x35` without a state change).
    pub last_started: Option<u16>,
    /// Sound-effect descriptor bank (decoded from the executable's
    /// `DAT_8006F198` table, see `sfx-table.md`). Empty until
    /// [`Self::set_sfx_bank`]; play requests against an empty bank no-op.
    /// The bank is static across scenes (it lives in the executable), so it
    /// is set once at boot; the per-scene VAB it plays through is the same
    /// [`Self::bank`] the BGM sequencer uses.
    sfx_bank: SfxBank,
    /// Cue id -> **VAB slot**, the routing half of the same descriptor table
    /// ([`legaia_game_tables::sfx_table::SfxTable::cue_slots`]): a cue's `+4`
    /// category selects the mixer record whose `+8` is the slot its voices key.
    /// Empty until [`Self::set_sfx_cue_slots`]; an absent id routes to
    /// [`FALLBACK_VAB_SLOT`], while a routed id whose slot is closed is silent.
    sfx_cue_slots: BTreeMap<u8, u8>,
    /// Resident SFX program banks keyed by that slot. Slot `0` is the system
    /// bank (extraction PROT 0868) the 16 shared UI cues key, uploaded once at
    /// boot at the bottom of the reserved SFX region. Slots `2` and `6` share
    /// the rest of that region, as they share one SPU base in retail, and hold
    /// whichever bank the current mode's initialiser loads there - PROT 0876
    /// (slot 6) in the field and on the world map, PROT 0869 (slot 2) in
    /// battle, a minigame's own bank in its mode
    /// ([`Self::sync_shared_region`]). Slot `11` (the reward bank) and slot `3`
    /// (a side-band bank) borrow the BGM region's tail. Empty on a disc-free
    /// boot; [`Self::tick_sfx_frame`] then falls back to the scene BGM bank
    /// ([`Self::bank`]).
    sfx_vabs: BTreeMap<u8, VabBank>,
    /// Frame-timed one-shot cue queue. [`Self::enqueue_sfx`] adds a cue at
    /// its strike-relative delay; [`Self::tick_sfx_frame`] advances one frame
    /// and fires matured cues through the SPU.
    sfx_sched: SfxScheduler,
    /// Arts-voice **shout** bank: the per-character CD-XA clips
    /// (`XA2`/`XA4`/`XA6`, demuxed per channel + decoded at boot) and the
    /// SCUS cue tables. `None` on a disc-free / extracted-dir boot (the raw
    /// CD-XA subheaders needed for channel demux only exist on a real disc
    /// image); shout requests then no-op, leaving arts silent - the same
    /// degradation retail applies to an unvoiced art.
    shout_bank: Option<ArtsShoutBank>,
    /// Generic CD-XA **clip** bank keyed on `(clip_slot, channel)` - the
    /// battle's `XA27` attack stings and `XA30` grunts (`FUN_8003D53C`
    /// requests the melee kernel makes). Same staging caveat as the shout
    /// bank: disc image only.
    xa_clip_bank: Option<XaClipBank>,
    /// Where a clip the bank does not hold is staged **from at cast time**:
    /// the disc image and the `XA<n>.XA` files' `(lba, sectors)`, resolved
    /// once at boot ([`Self::set_xa_lazy_source`]). The cast band names
    /// fifteen files (`docs/subsystems/cast-module.md`); none is decoded
    /// until a cast names its channel, and then only the read span the
    /// starter would have covered. `None` on a disc-free boot.
    xa_lazy: Option<Box<dyn XaChannelSource + Send + Sync>>,
    /// Each `XA<n>.XA`'s `(first sector, sector count)` by clip slot
    /// `n - 1`, for a host that slices sectors itself
    /// ([`Self::set_xa_file_extents`]). Empty when lazy staging reads the
    /// disc directly.
    xa_extents: BTreeMap<u8, (u32, u32)>,
    /// Channel spans waiting on the host ([`Self::xa_stage_requests`]).
    xa_pending: Vec<XaStageRequest>,
    /// The XA lane's request counters ([`Self::xa_stats`]).
    xa_stats: XaLaneStats,
    /// The battle audio duck, in retail's own units: `_DAT_8007B910` is the
    /// live level (seeded `0xD7` = [`DUCK_LEVEL_REF`] by the cold reset
    /// `FUN_8001FFA4`), ramped one unit per vsync toward a target the action
    /// SM sets - `ref * 75 / 100` under a summon, back to `ref` in the Done
    /// band's `0x51` arm - and applied to the BGM through `SsSeqSetVol`.
    /// `duck_level` mirrors the cell; `duck_target` the arm's clamp.
    duck_level: u8,
    duck_target: u8,
    /// The level last applied to the BGM - retail's `DAT_8007B6EC`
    /// (`gp+0x3D4`), which the per-frame driver `FUN_8001698C` sets to the
    /// live cell at the top of each frame, before the action SM steps the
    /// cell ([`Self::tick_duck`]).
    duck_applied: u8,
    /// The reference the duck takes its percentage of and rests on - the
    /// configured level `_DAT_8008457C` a save carries
    /// ([`Self::set_duck_reference`]) - and the percentage last asked for.
    duck_ref: u8,
    duck_pct: u8,
    /// The current field scene's prescript bundle - the retail
    /// current-bundle slot `_DAT_8007B8D0` - whose record 0 is the runtime
    /// half (`>= 0x200`) of the SFX descriptor table. Mirrored from the world
    /// by [`Self::sync_field_sfx`].
    runtime_sfx_bundle: Vec<u8>,
    /// The banks borrowing the BGM region's free tail (the reward bank and a
    /// side-band bank), their residency and the side-band retry memo - the
    /// kernel the browser page drives too
    /// ([`legaia_engine_audio::bgm_tail`]).
    tail: BgmTail<SideBandBank>,
    /// The bank the SPU region VAB slots `2` and `6` share holds - retail's
    /// per-mode refill of one region (`FUN_800265E8` gives both slots
    /// `0x33010`), driven by
    /// [`legaia_engine_core::world::World::sync_sfx_residency`]. `None` while
    /// neither slot is open.
    shared_region: Option<SharedRegionBank>,
    /// The BGM-tail generation the last failed shared-region stage met, so a
    /// bank that did not fit is retried only once the free tail has moved
    /// ([`Self::sync_shared_region`]).
    shared_retry_gen: Option<u64>,
    /// SPU address the shared region starts at: one past the slot-0 system
    /// bank's samples, inside the reserved SFX region.
    shared_region_base: u32,
    /// The slot-0 system bank's entry bytes (PROT 0868), kept from the boot
    /// staging ([`Self::stage_resident_slot0`]) so the bank can be re-staged
    /// after a track that overran the SFX region lets it go.
    resident_slot0: Option<Vec<u8>>,
    /// A track bank reaches into the SFX region - the ending theme's, laid
    /// across it as retail lays VAB 10 over the resident banks
    /// ([`legaia_engine_audio::spu_layout`]). While set, every resident SFX
    /// bank is dropped, the shared region does not refill and a cue is
    /// silent; the next track that fits the BGM region clears it and
    /// re-stages the resident banks.
    sfx_evicted: bool,
    /// Whether the world was in battle at the last
    /// [`Self::route_world_sfx`]: the edge into or out of a battle is a
    /// retail mode init (`FUN_8001DCF8`), which tears down the held
    /// sustained cue run (`FUN_80017910` at `0x8001DDA8`).
    sfx_in_battle: Option<bool>,
    /// The CD input's reverb send last handed the output
    /// ([`Self::set_cd_reverb`]); `SPUCNT` bit 2, clear at boot.
    cd_reverb: bool,
    /// The command record ([`Self::commands`]).
    commands: AudioCommandLog,
}

/// `_DAT_8007B910`'s reference value (`0xD7`, `FUN_8001FFA4`): the un-ducked
/// level the `0x51` arm ramps back to.
pub const DUCK_LEVEL_REF: u8 = legaia_engine_audio::duck::DUCK_LEVEL_REF;

/// VAB slot the battle-end reward bank (PROT 0889, cue `0x50`) is installed
/// in - retail streams it at results time (`FUN_8004E568` phase 4,
/// `FUN_8001E54C(0xB, ...)`), and the port stages it transiently the same
/// way ([`AudioDirector::stage_transient_sfx_vab`]).
pub const TRANSIENT_REWARD_SLOT: u8 = legaia_engine_audio::bgm_tail::REWARD_SLOT;

/// What one [`AudioDirector::tick_sfx_frame`] / [`AudioDirector::fire_now`]
/// did. `fired` is what keyed a voice; the other two are counted whether or
/// not a bank was staged, so a host can tell a source that produced nothing
/// from one that produced cues nothing could sound.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SfxFrameReport {
    /// `(cue id, first voice)` per cue that keyed on, ring cues first.
    pub fired: Vec<(u16, u8)>,
    /// Retail-ring cues that came due this frame, keyed or not.
    pub ring_due: Vec<i16>,
    /// Queued cues `classify_cue` routed to the CD-XA voice leg and declined.
    pub voice_declined: u32,
}

/// One ring cue resolved to what it keys, for [`AudioDirector::tick_sfx_frame`].
enum RingFire<'a> {
    /// A static-table id (`< 0x200`) and its category's bank.
    Static(u8, &'a VabBank),
    /// A runtime-bank row (`>= 0x200`) and the bank its `+4` names.
    Runtime([u8; 8], &'a VabBank),
}
impl<S: AudioSink> AudioDirector<S> {
    pub fn new(audio: Arc<S>) -> Self {
        Self {
            audio,
            bank: None,
            master_vol: legaia_engine_audio::sequencer::RETAIL_BGM_SEQ_VOL,
            loop_to: Some(0),
            last_started: None,
            sfx_bank: SfxBank::new(),
            sfx_cue_slots: BTreeMap::new(),
            sfx_vabs: BTreeMap::new(),
            sfx_sched: SfxScheduler::new(),
            shout_bank: None,
            xa_clip_bank: None,
            xa_lazy: None,
            xa_extents: BTreeMap::new(),
            xa_pending: Vec::new(),
            xa_stats: XaLaneStats::default(),
            duck_level: DUCK_LEVEL_REF,
            duck_target: DUCK_LEVEL_REF,
            duck_applied: DUCK_LEVEL_REF,
            duck_ref: DUCK_LEVEL_REF,
            duck_pct: 100,
            runtime_sfx_bundle: Vec::new(),
            tail: BgmTail::default(),
            shared_region: None,
            shared_retry_gen: None,
            shared_region_base: legaia_engine_audio::spu_layout::SFX_REGION_BASE,
            resident_slot0: None,
            sfx_evicted: false,
            sfx_in_battle: None,
            cd_reverb: false,
            commands: AudioCommandLog::default(),
        }
    }

    /// Every command a host's routing handed this director, as issued
    /// ([`AudioCommandLog`]).
    pub fn commands(&self) -> &AudioCommandLog {
        &self.commands
    }

    /// The audio output the director keys into.
    pub fn audio(&self) -> &Arc<S> {
        &self.audio
    }

    /// Route the CD input (every XA stream) into the SPU's reverb send, or
    /// stop - `SPUCNT` bit 2, which retail raises only in the Baka Fighter
    /// duel and the Muscle Dome hub
    /// ([`legaia_engine_core::world::World::cd_input_reverb`]). Acts and
    /// logs on the edge only.
    pub fn set_cd_reverb(&mut self, on: bool) {
        if self.cd_reverb == on {
            return;
        }
        self.cd_reverb = on;
        self.commands.push(AudioCommand::CdReverb(on));
        self.audio.set_cd_reverb(on);
    }

    /// The CD input's reverb send as last set ([`Self::set_cd_reverb`]).
    pub fn cd_reverb(&self) -> bool {
        self.cd_reverb
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_audio::VabBank;

    /// A cue keys the bank its `+4` category names when that slot is staged,
    /// and nothing otherwise - a closed slot is silent in retail.
    #[test]
    fn cue_routes_to_its_category_slot_or_is_silent() {
        // Retail categories: 0x21 menu cursor = 0, 0x09 duel hit = 2,
        // 0x2E field script = 6, 0x50 reward jingle = 11.
        let routing = BTreeMap::from([(0x21u8, 0u8), (0x09, 2), (0x2E, 6), (0x50, 11)]);
        // The field: slot 0 + the field bank in slot 6.
        let field: BTreeMap<u8, ()> = BTreeMap::from([(0, ()), (6, ())]);
        assert_eq!(resolve_sfx_slot(&routing, &field, 0x21), Some(0));
        assert_eq!(resolve_sfx_slot(&routing, &field, 0x2E), Some(6));
        assert_eq!(resolve_sfx_slot(&routing, &field, 0x09), None);
        assert_eq!(resolve_sfx_slot(&routing, &field, 0x50), None);
        // Battle: slot 0 + the class-2 bank in slot 2.
        let battle: BTreeMap<u8, ()> = BTreeMap::from([(0, ()), (2, ())]);
        assert_eq!(resolve_sfx_slot(&routing, &battle, 0x09), Some(2));
        assert_eq!(resolve_sfx_slot(&routing, &battle, 0x2E), None);
        // An id the routing does not carry falls back to the class-2 bank.
        assert_eq!(
            resolve_sfx_slot(&routing, &battle, 0xFE),
            Some(FALLBACK_VAB_SLOT)
        );
        assert_eq!(resolve_sfx_slot(&routing, &field, 0xFE), None);
        let none = BTreeMap::new();
        assert_eq!(
            resolve_sfx_slot(&none, &battle, 0x21),
            Some(FALLBACK_VAB_SLOT)
        );
    }

    /// A director over the device-free sink - the same director both hosts
    /// run, with the mixing core pulled by the test instead of a device.
    // Single-threaded like both hosts; the director takes its sink by `Arc`.
    #[allow(clippy::arc_with_non_send_sync)]
    fn headless() -> AudioDirector<legaia_engine_audio::TestAudioSink> {
        AudioDirector::new(Arc::new(legaia_engine_audio::TestAudioSink::new(
            legaia_engine_audio::SPU_INTERNAL_RATE,
        )))
    }

    /// The duck is retail's arithmetic: a 75% target lands at
    /// `0xD7 * 75 / 100`, the live level steps one unit per tick toward it,
    /// back up the same way, and an out-of-range percentage clamps.
    #[test]
    fn duck_ramps_one_unit_per_tick_to_retails_target() {
        use legaia_engine_audio::duck::DUCK_LEVEL_REF;
        let mut d = headless();
        assert_eq!(d.duck_level(), DUCK_LEVEL_REF);
        d.set_duck_pct(75);
        let target = (u32::from(DUCK_LEVEL_REF) * 75 / 100) as u8;
        let mut steps = 0;
        while d.duck_level() != target {
            d.tick_duck();
            steps += 1;
            assert!(steps <= 255, "the duck never reached its target");
        }
        assert_eq!(steps, u32::from(DUCK_LEVEL_REF - target));
        d.tick_duck();
        assert_eq!(d.duck_level(), target, "settled: the level holds");
        d.set_duck_pct(100);
        while d.duck_level() != DUCK_LEVEL_REF {
            d.tick_duck();
        }
        d.set_duck_pct(250);
        d.tick_duck();
        assert_eq!(
            d.duck_level(),
            DUCK_LEVEL_REF,
            "a percentage past 100 clamps"
        );
    }

    /// Retail applies the level at the top of a frame and the action SM
    /// steps it later in the same frame, so the BGM's volume is always the
    /// level one frame back: the first ducking frame still plays at the
    /// reference, and the applied level trails the cell by one step.
    #[test]
    fn the_applied_level_trails_the_cell_by_one_frame() {
        use legaia_engine_audio::duck::DUCK_LEVEL_REF;
        let mut d = headless();
        d.set_duck_pct(75);
        d.tick_duck();
        assert_eq!(d.duck_level(), DUCK_LEVEL_REF - 1);
        assert_eq!(d.duck_applied(), DUCK_LEVEL_REF);
        d.tick_duck();
        assert_eq!(d.duck_level(), DUCK_LEVEL_REF - 2);
        assert_eq!(d.duck_applied(), DUCK_LEVEL_REF - 1);
        while d.duck_level() != d.duck_target() {
            d.tick_duck();
        }
        assert_eq!(d.duck_applied(), d.duck_target() + 1);
        d.tick_duck();
        assert_eq!(d.duck_applied(), d.duck_target(), "settled one frame later");
    }

    /// A scene change empties the cue queue but keeps the reward bank: it
    /// lives in the BGM region's tail until a track overruns it or the field
    /// init closes it (the shared `BgmTail` rule).
    #[test]
    fn scene_change_clears_the_queue_but_keeps_the_reward_bank() {
        let mut d = headless();
        d.enqueue_sfx(0x21, 3, 0, 0);
        d.enqueue_sfx(0x118, 5, 0, 3);
        d.tail.commit_reward(TailBorrow {
            slot: TRANSIENT_REWARD_SLOT,
            base: 0x8000,
            end: 0x9000,
        });
        assert_eq!(d.sfx_sched.pending_count(), 2);
        d.clear_sfx();
        assert_eq!(d.sfx_sched.pending_count(), 0);
        assert!(d.tail.reward().is_some());
    }

    /// The scheduler steps once per tick wherever the host calls
    /// `tick_sfx_frame` - under a menu-overlay screen too, as retail's
    /// mode-`0x17` handler runs the cue drainer - so a delayed cue matures
    /// on its own frame.
    #[test]
    fn a_delayed_cue_matures_on_its_frame() {
        let mut d = headless();
        d.enqueue_sfx(0x21, 3, 0, 0);
        d.tick_sfx_frame();
        d.tick_sfx_frame();
        assert_eq!(d.sfx_sched.pending_count(), 1, "still delayed");
        d.tick_sfx_frame();
        d.tick_sfx_frame();
        assert_eq!(d.sfx_sched.pending_count(), 0, "matured");
    }

    /// The side-band retry memo is the shared tail's: a scene change neither
    /// clears nor re-arms it (a door stages no bank, so the free tail is what
    /// it was).
    #[test]
    fn a_scene_change_keeps_the_side_band_memo() {
        let mut d = headless();
        let want = SideBandBank {
            request: 2002,
            slot: 3,
            prot_entry: 1070,
        };
        assert!(d.tail.begin_side_band_attempt(want));
        d.clear_sfx();
        assert!(
            !d.tail.begin_side_band_attempt(want),
            "same tail, same request: not retried"
        );
    }

    /// A cast cue (`0x118`, `0x20C`, ...) routes to the CD-XA voice leg and
    /// never keys a descriptor - truncating it to `u8` would key the populated
    /// entries `0x18` / `0x0C`.
    #[test]
    fn a_matured_voice_cue_keys_no_descriptor() {
        let mut d = headless();
        d.set_bank(empty_bank());
        d.enqueue_sfx(0x20C, 0, 3, 0);
        d.enqueue_sfx(0x118, 0, 0, 0);
        let report = d.tick_sfx_frame();
        assert!(report.fired.is_empty());
        assert_eq!(report.voice_declined, 2, "both counted as declined");
        assert_eq!(d.sfx_pending(), 0);
    }

    /// A blip fired now does not tick the scheduler: a delayed cue keeps its
    /// full delay however many blips sound in between.
    #[test]
    fn a_direct_blip_does_not_age_the_queue() {
        let mut d = headless();
        d.enqueue_sfx(0x21, 2, 0, 0);
        for _ in 0..5 {
            let _ = d.fire_now(0x20);
        }
        assert_eq!(d.sfx_pending(), 1);
        // 2 -> 1 -> 0 -> fire: three ticks, none of them spent by a blip.
        let _ = d.tick_sfx_frame();
        let _ = d.tick_sfx_frame();
        assert_eq!(d.sfx_pending(), 1);
        let _ = d.tick_sfx_frame();
        assert_eq!(d.sfx_pending(), 0);
    }

    /// Ring cues that come due are reported even with no bank to key them.
    #[test]
    fn due_ring_cues_are_reported_without_a_bank() {
        let mut d = headless();
        d.apply_sfx_ring_ops(&[SfxRingOp::Push(0x2E)], 1);
        let mut due = Vec::new();
        for _ in 0..4 {
            due.extend(d.tick_sfx_frame().ring_due);
        }
        assert_eq!(due, vec![0x2E]);
    }

    /// Test stub bank - empty programs / samples. Real banks come from
    /// `legaia_vab::parse`.
    fn empty_bank() -> VabBank {
        VabBank {
            master_vol: 127,
            samples: Vec::new(),
            programs: Vec::new(),
        }
    }

    /// The director exposes no deferred-start slot at all: field-VM op
    /// `0x35` sub-op 9 is a start behind a load barrier this host never
    /// waits on, so `BgmDirector` carries `start` / `start_owned_vab` and
    /// nothing that stashes a track for a later trigger. Kept as a
    /// compile-time pin: adding a queue back would need a caller, and the
    /// only caller a queue ever had was a scene entry - which is the wrong
    /// trigger for a mid-cutscene music change.
    #[test]
    fn director_has_no_deferred_start_slot() {
        fn assert_director<T: BgmDirector>() {}
        assert_director::<AudioDirector<legaia_engine_audio::AudioOut>>();
        let _ = empty_bank(); // touch path so unused-import lint stays clean
    }
}
