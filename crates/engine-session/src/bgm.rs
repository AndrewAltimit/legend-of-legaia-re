//! The [`legaia_engine_core::scene::BgmDirector`] both hosts run: one director
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
use legaia_asset::sfx_table::FALLBACK_VAB_SLOT;
use legaia_engine_audio::bgm_tail::{BgmTail, TailBorrow};
use legaia_engine_audio::{
    ArtsShoutBank, AudioSink, PendingCue, SHOUT_CD_RESPONSE_DELAY, Sequencer, SfxBank,
    SfxFireBatch, SfxScheduler, VabBank, XaClipBank,
};
use legaia_engine_core::scene::BgmDirector;
use legaia_engine_core::world::{SfxRingOp, SharedRegionBank, SideBandBank};
use legaia_seq::Seq;

/// The pause menu's cursor-step cue, re-exported from the one engine-side
/// table both hosts fire from ([`legaia_engine_core::menu_cues`], provenance
/// there: `FUN_80032A44`'s ring writes). Category `0`, so the director sounds
/// it out of the slot-0 system bank (PROT 0868).
pub const RETAIL_MENU_CURSOR_CUE: u16 = legaia_engine_core::menu_cues::MENU_CURSOR_CUE as u16;
/// The enabled-row confirm cue ([`legaia_engine_core::menu_cues::MENU_CONFIRM_CUE`]).
pub const RETAIL_MENU_CONFIRM_CUE: u16 = legaia_engine_core::menu_cues::MENU_CONFIRM_CUE as u16;
/// The cancel cue ([`legaia_engine_core::menu_cues::MENU_CANCEL_CUE`]).
pub const RETAIL_MENU_CANCEL_CUE: u16 = legaia_engine_core::menu_cues::MENU_CANCEL_CUE as u16;

/// BGM director that routes [`BgmDirector`] events into a live
/// audio output ([`AudioSink`]). The director holds a clone of the audio handle (cpal stream
/// is reference-counted internally via `Arc`) plus the active VAB bank.
pub struct AudioBgmDirector<S: AudioSink> {
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
    /// ([`legaia_asset::sfx_table::SfxTable::cue_slots`]): a cue's `+4`
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
    /// seventeen files (`docs/subsystems/cast-module.md`); none is decoded
    /// until a cast names its channel, and then only the read span the
    /// starter would have covered. `None` on a disc-free boot.
    xa_lazy: Option<XaLazySource>,
    /// The battle audio duck, in retail's own units: `_DAT_8007B910` is the
    /// live level (seeded `0xD7` = [`DUCK_LEVEL_REF`] by the cold reset
    /// `FUN_8001FFA4`), ramped one unit per vsync toward a target the action
    /// SM sets - `ref * 75 / 100` under a summon, back to `ref` in the Done
    /// band's `0x51` arm - and applied to the BGM through `SsSeqSetVol`.
    /// `duck_level` mirrors the cell; `duck_target` the arm's clamp.
    duck_level: u8,
    duck_target: u8,
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
}

/// `_DAT_8007B910`'s reference value (`0xD7`, `FUN_8001FFA4`): the un-ducked
/// level the `0x51` arm ramps back to.
pub const DUCK_LEVEL_REF: u8 = legaia_engine_audio::duck::DUCK_LEVEL_REF;

/// VAB slot the battle-end reward bank (PROT 0889, cue `0x50`) is installed
/// in - retail streams it at results time (`FUN_8004E568` phase 4,
/// `FUN_8001E54C(0xB, ...)`), and the port stages it transiently the same
/// way ([`AudioBgmDirector::stage_transient_sfx_vab`]).
pub const TRANSIENT_REWARD_SLOT: u8 = legaia_engine_audio::bgm_tail::REWARD_SLOT;

/// What one [`AudioBgmDirector::tick_sfx_frame`] / [`AudioBgmDirector::fire_now`]
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

/// One ring cue resolved to what it keys, for [`AudioBgmDirector::tick_sfx_frame`].
enum RingFire<'a> {
    /// A static-table id (`< 0x200`) and its category's bank.
    Static(u8, &'a VabBank),
    /// A runtime-bank row (`>= 0x200`) and the bank its `+4` names.
    Runtime([u8; 8], &'a VabBank),
}

/// The disc side of lazy CD-XA staging: the image path and every
/// `XA<n>.XA`'s `(lba, sectors)` keyed by clip slot `n - 1`.
struct XaLazySource {
    disc: std::path::PathBuf,
    files: BTreeMap<u8, (u32, u32)>,
}

impl XaLazySource {
    /// Walk the disc's ISO once for the `XA<n>.XA` files. `None` when the
    /// image does not open or carries none.
    fn open(disc: &std::path::Path) -> Option<Self> {
        let mut raw = legaia_iso::raw::RawDisc::open(disc).ok()?;
        let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
        let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
        let mut map = BTreeMap::new();
        for (path, rec) in &files {
            let base = path.rsplit('/').next().unwrap_or(path);
            let base = base.split(';').next().unwrap_or(base).to_ascii_uppercase();
            let Some(n) = base
                .strip_prefix("XA")
                .and_then(|s| s.strip_suffix(".XA"))
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(slot) = u8::try_from(n.checked_sub(1)?) else {
                continue;
            };
            let sectors = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
            map.insert(slot, (rec.lba, sectors));
        }
        (!map.is_empty()).then_some(Self {
            disc: disc.to_path_buf(),
            files: map,
        })
    }

    /// `count` raw 2352-byte sectors from `lba`, concatenated.
    fn read_sectors(&self, lba: u32, count: u32) -> Option<Vec<u8>> {
        let mut raw = legaia_iso::raw::RawDisc::open(&self.disc).ok()?;
        let mut out = Vec::with_capacity(
            count as usize * legaia_engine_audio::xa_clip_bank::RAW_SECTOR_BYTES,
        );
        for s in 0..count {
            let sector = raw.read_raw_sector(lba + s).ok()?;
            out.extend_from_slice(&sector[..]);
        }
        Some(out)
    }

    /// One channel of `XA<slot + 1>.XA` over the starter's read span for
    /// `duration_sectors`, decoded: `(clip, interleave width)`.
    fn channel_span(
        &self,
        slot: u8,
        channel: u8,
        duration_sectors: u32,
    ) -> Option<(legaia_engine_audio::XaClip, u8)> {
        let &(lba, file_sectors) = self.files.get(&slot)?;
        let span = legaia_engine_audio::xa_clip_bank::read_span_sectors(duration_sectors)
            .min(file_sectors);
        let raw = self.read_sectors(lba, span)?;
        legaia_engine_audio::xa_clip_bank::decode_channel_span(&raw, channel)
    }
}

/// Read one cast-voice clip straight off a disc image: channel `channel` of
/// `XA<clip_slot + 1>.XA`, from the file's first sector to the clip
/// starter's stop point for `duration_sectors`
/// (`legaia_engine_audio::xa_clip_bank::read_span_sectors`). This is the
/// read the director performs the first time a cast names a clip; exposed
/// so the disc-gated oracle can pin it without an audio device. `None` when
/// the disc, the file or the channel is absent.
// REF: FUN_8003D53C
pub fn read_xa_channel_span(
    disc: &std::path::Path,
    clip_slot: u8,
    channel: u8,
    duration_sectors: u32,
) -> Option<(legaia_engine_audio::XaClip, u8)> {
    XaLazySource::open(disc)?.channel_span(clip_slot, channel, duration_sectors)
}

impl<S: AudioSink> AudioBgmDirector<S> {
    pub fn new(audio: Arc<S>) -> Self {
        Self {
            audio,
            bank: None,
            master_vol: 100,
            loop_to: Some(0),
            last_started: None,
            sfx_bank: SfxBank::new(),
            sfx_cue_slots: BTreeMap::new(),
            sfx_vabs: BTreeMap::new(),
            sfx_sched: SfxScheduler::new(),
            shout_bank: None,
            xa_clip_bank: None,
            xa_lazy: None,
            duck_level: DUCK_LEVEL_REF,
            duck_target: DUCK_LEVEL_REF,
            duck_ref: DUCK_LEVEL_REF,
            duck_pct: 100,
            runtime_sfx_bundle: Vec::new(),
            tail: BgmTail::default(),
            shared_region: None,
            shared_retry_gen: None,
            shared_region_base: legaia_engine_audio::spu_layout::SFX_REGION_BASE,
            resident_slot0: None,
            sfx_evicted: false,
        }
    }

    /// The audio output the director keys into.
    pub fn audio(&self) -> &Arc<S> {
        &self.audio
    }

    /// Install the arts-voice shout bank (demuxed + decoded from the user's
    /// disc at boot; see [`crate::boot::read_arts_shout_bank`]).
    pub fn set_shout_bank(&mut self, bank: ArtsShoutBank) {
        self.shout_bank = Some(bank);
    }

    /// Whether the arts-voice shout bank was staged.
    pub fn has_shout_bank(&self) -> bool {
        self.shout_bank.is_some()
    }

    /// Fire the Tactical-Arts shout for `(cslot, action_constant)` through
    /// the XA mixing path. Resolves the cue against the bank's channel pools
    /// (retail `FUN_8004C140` selection, no immediate repeat) and stages the
    /// clip with the modeled CD-response start delay
    /// ([`SHOUT_CD_RESPONSE_DELAY`]), so the shout starts *after* the art
    /// animation that requested it - never before. A second shout while one
    /// is sounding queues behind it (the back-to-back no-drop path in
    /// [`AudioSink::play_xa_shout`]). Returns the fired channel, or `None`
    /// when the bank is absent or the art is unvoiced.
    pub fn play_art_shout(&mut self, cslot: u8, action: u8) -> Option<u8> {
        let bank = self.shout_bank.as_mut()?;
        let (channel, clip) = bank.shout(cslot, action)?;
        self.audio.play_xa_shout(
            clip.pcm.clone(),
            clip.sample_rate,
            legaia_xa::Channels::Mono,
            0x4000,
            SHOUT_CD_RESPONSE_DELAY,
        );
        Some(channel)
    }

    /// Install the generic CD-XA clip bank (demuxed + decoded from the disc
    /// at boot; see [`crate::boot::read_battle_xa_clip_bank`]).
    pub fn set_xa_clip_bank(&mut self, bank: XaClipBank) {
        self.xa_clip_bank = Some(bank);
    }

    /// `true` once a CD-XA clip bank is staged.
    pub fn has_xa_clip_bank(&self) -> bool {
        self.xa_clip_bank.is_some()
    }

    /// Point lazy staging at the disc image: walk its ISO once for every
    /// `XA<n>.XA` and keep `(slot, lba, sectors)`, the way the boot filler
    /// `FUN_801CFA78` builds the clip table at `0x801C6ED8` (slot `n - 1`
    /// for `XA<n>.XA`; `docs/subsystems/audio.md`). Returns how many files
    /// resolved; `0` leaves lazy staging off.
    pub fn set_xa_lazy_source(&mut self, disc: &std::path::Path) -> usize {
        let Some(src) = XaLazySource::open(disc) else {
            return 0;
        };
        let n = src.files.len();
        self.xa_lazy = Some(src);
        n
    }

    /// `true` once lazy staging has a disc to read from.
    pub fn has_xa_lazy_source(&self) -> bool {
        self.xa_lazy.is_some()
    }

    /// Stage `(slot, channel)` from the disc for a request the bank does
    /// not hold: read the file's sectors from its start to the starter's
    /// stop point (`read_span_sectors(dur)`, capped at the file), decode that
    /// one channel and stage it lazily. Returns whether the clip is staged
    /// afterwards.
    fn stage_xa_channel_lazily(&mut self, slot: u8, channel: u8, duration_sectors: u32) -> bool {
        let Some(src) = self.xa_lazy.as_ref() else {
            return false;
        };
        let Some((clip, width)) = src.channel_span(slot, channel, duration_sectors) else {
            return false;
        };
        log::debug!(
            "XA lazy stage: XA{}.XA channel {channel} over {} sectors -> {} frames",
            u32::from(slot) + 1,
            legaia_engine_audio::xa_clip_bank::read_span_sectors(duration_sectors),
            clip.frames()
        );
        self.xa_clip_bank
            .get_or_insert_with(XaClipBank::new)
            .insert_lazy(slot, channel, clip, width);
        true
    }

    /// Play one CD-XA clip request - the engine's `FUN_8003D53C(clip,
    /// channel, dur)`: the staged `(slot, channel)` PCM, cut at the retail
    /// read span (`XaClipBank::cut_frames`), through the same XA mixing
    /// path the arts shouts take, with the same modelled CD-response start
    /// delay. A request while a clip is sounding queues behind it (the
    /// mixer's back-to-back path). A `(slot, channel)` the bank does not
    /// hold is staged from the disc first when a lazy source is set
    /// ([`Self::set_xa_lazy_source`]) - the cast voices' path. Returns the
    /// frames played, `None` when nothing is staged for the request.
    // REF: FUN_8003D53C
    pub fn play_xa_clip(
        &mut self,
        clip_slot: u32,
        channel: u32,
        duration_sectors: u32,
    ) -> Option<u32> {
        let (Ok(slot), Ok(ch)) = (u8::try_from(clip_slot), u8::try_from(channel)) else {
            return None;
        };
        let staged = self
            .xa_clip_bank
            .as_ref()
            .is_some_and(|b| b.is_staged(slot, ch));
        if !staged && !self.stage_xa_channel_lazily(slot, ch, duration_sectors) {
            return None;
        }
        let clip = self
            .xa_clip_bank
            .as_ref()?
            .cut(slot, ch, duration_sectors)?;
        let frames = (if clip.stereo {
            clip.pcm.len() / 2
        } else {
            clip.pcm.len()
        }) as u32;
        let channels = if clip.stereo {
            legaia_xa::Channels::Stereo
        } else {
            legaia_xa::Channels::Mono
        };
        self.audio.play_xa_shout(
            clip.pcm,
            clip.sample_rate,
            channels,
            0x4000,
            SHOUT_CD_RESPONSE_DELAY,
        );
        Some(frames)
    }

    /// The arts-voice shout bank, once staged.
    pub fn shout_bank(&self) -> Option<&ArtsShoutBank> {
        self.shout_bank.as_ref()
    }

    /// The arts-voice shout bank, created empty on first use - for a host
    /// that installs it file by file.
    pub fn shout_bank_mut(&mut self) -> &mut ArtsShoutBank {
        self.shout_bank.get_or_insert_with(ArtsShoutBank::new)
    }

    /// The CD-XA clip bank, once staged.
    pub fn xa_clip_bank(&self) -> Option<&XaClipBank> {
        self.xa_clip_bank.as_ref()
    }

    /// The CD-XA clip bank, created empty on first use - for a host that
    /// installs it file by file or span by span.
    pub fn xa_clip_bank_mut(&mut self) -> &mut XaClipBank {
        self.xa_clip_bank.get_or_insert_with(XaClipBank::new)
    }

    /// Set the audio duck's target as a percentage of the reference level
    /// (`BattleEvent::DuckAudioLevel`): `75` under a summon / magic capture,
    /// `100` when the Done band ramps it back. The ramp itself runs in
    /// [`Self::tick_duck`].
    pub fn set_duck_pct(&mut self, pct: u8) {
        self.duck_pct = pct;
        self.duck_target = legaia_engine_audio::duck::duck_target_for_pct_of(self.duck_ref, pct);
    }

    /// Install the configured audio level the duck is a percentage of and
    /// rests on (`World::audio.levels.configured_level`, a loaded save's
    /// `_DAT_8008457C`). Called per frame before [`Self::tick_duck`]; a
    /// change re-targets the ramp, so a save carrying a lower level brings
    /// the BGM down to it the way retail's next MAN load does.
    pub fn set_duck_reference(&mut self, configured_level: i32) {
        let reference = legaia_engine_audio::duck::reference_level(configured_level);
        if reference != self.duck_ref {
            self.duck_ref = reference;
            self.set_duck_pct(self.duck_pct);
        }
    }

    /// One frame of the duck ramp: step the live level one unit toward the
    /// target (retail `DAT_1F800393` per vsync, the `0x35` / `0x51` arms)
    /// and re-apply it to the BGM as `master_vol * level / ref` - the
    /// `FUN_800267A8` -> `SsSeqSetVol` re-apply, which halves the cell into
    /// the 0..127 volume domain the same way `master_vol` already is.
    ///
    /// A level resting below the reference is re-applied every frame too
    /// ([`legaia_engine_audio::duck::duck_apply`]), so a track started under
    /// a settled duck comes down on its first frame - the kernel the page's
    /// `tick_duck` calls.
    pub fn tick_duck(&mut self) {
        use legaia_engine_audio::duck;
        let moved = duck::step_duck(&mut self.duck_level, self.duck_target);
        if let Some(vol) = duck::duck_apply(moved, self.master_vol, self.duck_level) {
            self.audio.set_sequencer_master_vol(vol);
        }
    }

    /// The live duck level in `_DAT_8007B910` units (for tests / traces).
    pub fn duck_level(&self) -> u8 {
        self.duck_level
    }

    /// The level the duck ramps toward (`duck_level` rests here).
    pub fn duck_target(&self) -> u8 {
        self.duck_target
    }

    /// Cues waiting in the scheduler (queued, not yet matured).
    pub fn sfx_pending(&self) -> usize {
        self.sfx_sched.pending_count()
    }

    /// The VAB slot cue `id`'s `+4` category names, before the closed-slot
    /// rule ([`Self::sfx_slot_for_cue`]); `None` for an id the installed
    /// table does not carry.
    pub fn cue_slot_raw(&self, id: u8) -> Option<u8> {
        self.sfx_cue_slots.get(&id).copied()
    }

    /// The extraction-frame PROT entry the bank staged in `slot` came from:
    /// the slot-0 system bank, the shared slot-2 / slot-6 region's bank, the
    /// reward bank, a side-band bank, or `monster.snd` for the battle's
    /// slots `7` / `8`. `None` for an empty slot.
    pub fn prot_for_slot(&self, slot: u8) -> Option<u32> {
        use legaia_asset::sfx_table::{
            SLOT0_SYSTEM_BANK_PROT_INDEX, SLOT11_REWARD_BANK_PROT_INDEX,
        };
        if !self.sfx_vabs.contains_key(&slot) {
            return None;
        }
        if slot == 0 && self.resident_slot0.is_some() {
            return Some(SLOT0_SYSTEM_BANK_PROT_INDEX);
        }
        if let Some(b) = self.shared_region.filter(|b| b.slot == slot) {
            return Some(b.prot_entry);
        }
        if slot == TRANSIENT_REWARD_SLOT && self.tail.reward().is_some() {
            return Some(SLOT11_REWARD_BANK_PROT_INDEX);
        }
        if let Some((b, _)) = self.tail.side_band().filter(|(b, _)| b.slot == slot) {
            return Some(b.prot_entry);
        }
        self.tail
            .monsters()
            .is_some_and(|(_, borrows)| borrows.iter().any(|t| t.slot == slot))
            .then_some(legaia_asset::vab_multi_bank::MONSTER_SND_PROT_INDEX as u32)
    }

    /// Whether the battle-end reward bank is parked in its slot.
    pub fn has_reward_bank(&self) -> bool {
        self.tail.reward().is_some()
    }

    /// Stage the reward bank (PROT 0889) behind the BGM unless it is already
    /// parked - the results frame's `LEVEL_UP_CUE` request, which both hosts
    /// make. `read_entry` reads an extraction-frame PROT entry. Returns
    /// whether the bank is parked afterwards.
    pub fn ensure_reward_bank(&mut self, read_entry: impl FnOnce(u32) -> Option<Vec<u8>>) -> bool {
        self.observe_track();
        if self.has_reward_bank() {
            return true;
        }
        let Some(bytes) = read_entry(legaia_asset::sfx_table::SLOT11_REWARD_BANK_PROT_INDEX) else {
            return false;
        };
        self.stage_transient_sfx_vab(TRANSIENT_REWARD_SLOT, &bytes)
    }

    /// Whether a VAB is staged in `slot`.
    pub fn has_sfx_vab_slot(&self, slot: u8) -> bool {
        self.sfx_vabs.contains_key(&slot)
    }

    /// Stage a VAB into `slot` **behind the resident BGM bank**, in the free
    /// tail of the BGM region - the port's version of retail's results-time
    /// load of PROT 0889 into slot 11 (`FUN_8001FC00(0x37B, 0xB, ..)` +
    /// `FUN_8001E54C(0xB, ..)` in `FUN_8004E568` phases 2 / 4). The SFX
    /// region is full (its two pinned banks leave ~2.5 KB), so the reward
    /// bank borrows BGM room instead. It stays parked until a track overruns
    /// its base or the field init closes it - the residency rule of
    /// [`legaia_engine_audio::bgm_tail`], which the browser page drives too.
    /// Returns `false` when the entry has no VAB header or the tail is too
    /// small.
    // REF: FUN_8001E54C, FUN_8004E568
    pub fn stage_transient_sfx_vab(&mut self, slot: u8, entry_bytes: &[u8]) -> bool {
        let Some(borrow) = self.park_tail_bank(slot, entry_bytes) else {
            return false;
        };
        if slot == TRANSIENT_REWARD_SLOT {
            self.tail.commit_reward(borrow);
        }
        true
    }

    /// Upload `entry_bytes`' VAB into `slot` above the track and every other
    /// tail borrower ([`BgmTail::place`]). `None` when the entry has no VAB
    /// header or the tail is too small; the slot is then empty.
    fn park_tail_bank(&mut self, slot: u8, entry_bytes: &[u8]) -> Option<TailBorrow> {
        let (report, vab_off) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(entry_bytes, o).ok().map(|r| (r, o)))?;
        self.observe_track();
        self.sfx_vabs.remove(&slot);
        let base = self.tail.place_report(slot, &report)?;
        let body = &entry_bytes[vab_off..];
        let bank = self
            .audio
            .with_spu(|spu| legaia_engine_audio::bgm_tail::upload_at(spu, base, &report, body));
        let end = legaia_engine_audio::spu_layout::bank_used_end(&bank).unwrap_or(base);
        self.sfx_vabs.insert(slot, bank);
        Some(TailBorrow { slot, base, end })
    }

    /// Install the sound-effect descriptor bank (decoded from the user's
    /// `SCUS_942.54` `DAT_8006F198` table at boot). Replaces any prior bank.
    pub fn set_sfx_bank(&mut self, bank: SfxBank) {
        self.sfx_bank = bank;
    }

    /// Install the cue id -> VAB slot routing decoded from the same
    /// descriptor table as [`Self::set_sfx_bank`]
    /// (`legaia_asset::sfx_table::SfxTable::cue_slots`). Without it every cue
    /// falls back to the class-2 bank, which is what a single-bank host did.
    pub fn set_sfx_cue_slots<I: IntoIterator<Item = (u8, u8)>>(&mut self, slots: I) {
        self.sfx_cue_slots = slots.into_iter().collect();
    }

    /// Install one resident SFX program bank at its VAB `slot` (0 = the
    /// PROT 0868 system bank, 2 = the PROT 0869 class-2 bank), uploaded into
    /// the shared SPU RAM region at boot. Cues fire against the bank their own
    /// category names so their programs are always resident; see
    /// [`Self::sfx_vabs`].
    pub fn set_sfx_vab(&mut self, slot: u8, bank: VabBank) {
        self.sfx_vabs.insert(slot, bank);
    }

    /// Whether any resident SFX bank was staged.
    pub fn has_sfx_vab(&self) -> bool {
        !self.sfx_vabs.is_empty()
    }

    /// The VAB slots that have a resident bank, ascending.
    pub fn staged_sfx_slots(&self) -> Vec<u8> {
        self.sfx_vabs.keys().copied().collect()
    }

    /// Borrow the active SFX bank - useful for tests / inspection.
    pub fn sfx_bank(&self) -> &SfxBank {
        &self.sfx_bank
    }

    /// The VAB slot cue `id` resolves to on this director, `None` when its
    /// slot is closed. See [`resolve_sfx_slot`].
    pub fn sfx_slot_for_cue(&self, id: u8) -> Option<u8> {
        resolve_sfx_slot(&self.sfx_cue_slots, &self.sfx_vabs, id)
    }

    /// The bank cue `id` keys: its slot's resident bank, or - only while no
    /// SFX bank staged at all (a disc-free boot) - the scene BGM bank.
    fn sfx_vab_for_cue(&self, id: u8) -> Option<&VabBank> {
        if self.sfx_vabs.is_empty() {
            // Not while a track holds the SFX region: that bank is the
            // ending theme's, and its programs are not the cue's.
            return (!self.sfx_evicted).then_some(self.bank.as_ref()).flatten();
        }
        self.sfx_vabs.get(&self.sfx_slot_for_cue(id)?)
    }

    /// Key one voice from an explicit
    /// [`VoiceAttr`](legaia_engine_audio::VoiceAttr) set - the shape retail's
    /// `FUN_80065034` takes, and the one a caller that already holds the
    /// program / tone / note / volume needs. The catalog path
    /// ([`Self::enqueue_sfx`]) is for a cue that names itself by id.
    ///
    /// The Muscle Dome's between-leg tally roll is the live caller: its per-
    /// lane cue (`FUN_801D1288`) resolves a full attr set rather than a cue
    /// id, so nothing in the id-keyed path could sound it.
    ///
    /// `vab_id` picks the bank the way retail's does - it is a **VAB id**, and
    /// this director resolves it as an SFX slot, falling back to the active
    /// scene BGM bank when that slot has nothing staged (the same fallback
    /// [`Self::tick_sfx_frame`] uses on a disc-free boot). Returns whether a
    /// voice keyed on.
    pub fn key_on_voice_attr(&mut self, attr: legaia_engine_audio::VoiceAttr) -> bool {
        let slot = u8::try_from(attr.vab_id).unwrap_or(0);
        let Some(vab) = self
            .sfx_vabs
            .get(&slot)
            .or_else(|| self.bank.as_ref().filter(|_| !self.sfx_evicted))
        else {
            return false;
        };
        self.audio
            .with_spu(|spu| legaia_engine_audio::key_on_voice_attr(&attr, spu, vab))
    }

    /// Queue a one-shot sound cue to fire `frames` after this call (the
    /// strike's `timing_frames`). `id` is the [`SfxBank`] descriptor id
    /// directly (the art-record `HitCue::kind`), played without
    /// `classify_cue`. `actor` / `target` ride along for HUD context.
    pub fn enqueue_sfx(&mut self, id: u16, frames: u16, actor: u8, target: u8) {
        self.sfx_sched
            .enqueue(PendingCue::new(id, frames).with_actors(actor, target));
    }

    /// Advance the SFX scheduler one frame and fire any matured cue through
    /// the SPU ([`Self::fire_batch`]). Call once per simulation tick so
    /// delayed cues advance even when none are enqueued that frame.
    pub fn tick_sfx_frame(&mut self) -> SfxFrameReport {
        let batch = self.sfx_sched.tick_frame();
        self.fire_batch(&batch)
    }

    /// Fire one cue **now**, without ticking the scheduler: a UI blip sounded
    /// on the frame of the press must not age every other queued cue a frame
    /// per blip ([`SfxFireBatch::immediate`]).
    pub fn fire_now(&mut self, id: u16) -> SfxFrameReport {
        self.fire_batch(&SfxFireBatch::immediate(id))
    }

    /// Fire one batch through the SPU. Each cue resolves against the
    /// resident SFX bank **its own `+4` category names**
    /// ([`Self::sfx_vab_for_cue`]) - the retail path, where `FUN_80065034`
    /// repoints the current-bank globals at the cue's slot before the program
    /// lookup - and falls back to the active scene BGM bank ([`Self::bank`])
    /// when nothing is staged at all (the disc-free boot). A cue is silently
    /// dropped when no bank is staged, its id isn't in the descriptor bank, its
    /// program / tone isn't resident, or no SPU voice is free (matching the
    /// retail "no voice / no program -> skip" behaviour). The report counts the
    /// ring cues that came due and the voice-leg cues declined whether or not
    /// anything keyed, so a host can tell a silent source from an absent one.
    fn fire_batch(&mut self, batch: &SfxFireBatch) -> SfxFrameReport {
        let mut report = SfxFrameReport {
            ring_due: batch.ring.clone(),
            ..SfxFrameReport::default()
        };
        // The queue is a `u16` because the battle cue space is - the action
        // SM's cast cues run to `0x20E` - while the SFX descriptor table is
        // `0x00..=0x63`. Truncating with `as u8` did not make an out-of-band
        // cue silent, it made it play the **wrong** descriptor: `0x20C`
        // became `0x0C`, `0x118` became `0x18`, and every one of those is a
        // populated entry. Classify instead, exactly as `FUN_8004FCC8` does.
        //
        // Only the `Voice` band is re-routed here. The `Ring` band's `id - 1`
        // resolution below `0x40` is retail's (`route_sfx_cue`'s low leg) but
        // the producers feed this queue an art-record `HitCue::kind` / a menu
        // descriptor id the bank is already indexed by, so applying it would
        // silently move every cue that works today. Left as an open thread
        // rather than changed without an oracle.
        let mut descriptors: Vec<(u16, u8)> = Vec::with_capacity(batch.fired.len());
        for cue in &batch.fired {
            match legaia_engine_audio::classify_cue(u32::from(cue.id)) {
                legaia_engine_audio::CueDispatch::Voice {
                    channel, submode, ..
                } => {
                    // A streamed CD-XA voice, not an SPU descriptor. No
                    // producer feeds this queue such ids: the `FUN_801F3990`
                    // cast-cue band (the item-use voice) and the cast
                    // module's head cue both resolve at the world into the
                    // `(clip, channel, dur)` channel `play_xa_clip` plays, so
                    // a voice id here is a stray and is declined.
                    log::debug!(
                        "battle cue {:#06x} is a CD-XA voice (clip channel {channel:#04x} \
                         submode {submode}) on the SFX queue; declined",
                        cue.id
                    );
                    report.voice_declined += 1;
                }
                // A `Ring` id out of the descriptor space could only come
                // from a `0` cue wrapping to `0xFFFF`; keying descriptor
                // `0xFF` is a no-op in every bank.
                legaia_engine_audio::CueDispatch::Ring { .. } => {
                    descriptors.push((cue.id, cue.id.min(0xFF) as u8))
                }
            }
        }
        if (batch.ring.is_empty() && descriptors.is_empty())
            || (self.sfx_vabs.is_empty() && self.bank.is_none())
        {
            return report;
        }
        let bank = &self.sfx_bank;
        // Resolve the ring's ids before borrowing the SPU: below `0x200` a
        // static-table id keyed through its category's bank, at or above it a
        // runtime row keyed through the bank its own `+4` category names.
        let ring: Vec<(u16, RingFire<'_>)> = batch
            .ring
            .iter()
            .filter_map(|&id| {
                let fire = match u8::try_from(id) {
                    Ok(small) => RingFire::Static(small, self.sfx_vab_for_cue(small)?),
                    Err(_) => {
                        let row = legaia_engine_core::world::runtime_sfx_descriptor_in(
                            &self.runtime_sfx_bundle,
                            id,
                        )?;
                        // No fallback: a runtime row's program indexes the
                        // bank its category names, and no other bank.
                        RingFire::Runtime(row, self.sfx_vabs.get(&row[4])?)
                    }
                };
                Some((id as u16, fire))
            })
            .collect();
        // The cue's category picks its bank; a closed slot is silent, and
        // with nothing staged the scene BGM VAB stands in.
        let queued: Vec<(u16, u8, &VabBank)> = descriptors
            .iter()
            .filter_map(|&(queued_id, id)| Some((queued_id, id, self.sfx_vab_for_cue(id)?)))
            .collect();
        let fired = &mut report.fired;
        self.audio.with_spu(|spu| {
            for (id, fire) in &ring {
                let voice = match fire {
                    RingFire::Static(small, vab) => bank.play_one_shot(*small, spu, vab),
                    RingFire::Runtime(row, vab) => bank.play_descriptor(row, spu, vab),
                };
                if let Some(v) = voice {
                    fired.push((*id, v));
                }
            }
            for &(queued_id, id, vab) in &queued {
                if let Some(voice) = bank.play_one_shot(id, spu, vab) {
                    fired.push((queued_id, voice));
                }
            }
        });
        report
    }

    /// Re-check the tail borrowers against the live track's sample end and
    /// forget the ones it overran ([`BgmTail::observe_bgm_end`]). Runs after
    /// every track upload; idempotent.
    fn observe_track(&mut self) {
        let end = legaia_engine_audio::bgm_tail::track_end(self.bank.as_ref());
        for slot in self.tail.observe_bgm_end(end) {
            self.sfx_vabs.remove(&slot);
        }
    }

    /// Set where the shared slot-2 / slot-6 region starts - one past the
    /// slot-0 system bank's samples.
    pub fn set_shared_region_base(&mut self, base: u32) {
        self.shared_region_base = base.div_ceil(16) * 16;
    }

    /// The bank the shared slot-2 / slot-6 region holds, if any.
    pub fn shared_region(&self) -> Option<SharedRegionBank> {
        self.shared_region
    }

    /// Refill the region VAB slots `2` and `6` share with `want` - the bank
    /// the world's residency says the current mode holds there - the way each
    /// mode's initialiser refills it in retail (`FUN_801D6704` loads PROT 0876
    /// into slot 6, `FUN_800520F0` PROT 0869 into slot 2). Whatever the region
    /// held is dropped first, under both slot keys: the two are one region.
    /// `read_entry` reads an extraction-frame PROT entry. Returns whether the
    /// wanted bank is now resident. A bank that does not fit (the dance's
    /// PROT 1231 is 234 400 bytes of samples against the region's
    /// 190 720) leaves both slots closed, which is what its cues then are.
    // REF: FUN_801D6704, FUN_800520F0, FUN_8001E54C
    pub fn sync_shared_region(
        &mut self,
        want: Option<SharedRegionBank>,
        read_entry: impl FnOnce(u32) -> Option<Vec<u8>>,
    ) -> bool {
        if self.sfx_evicted {
            // The region holds a track's samples; the refill waits for the
            // re-stage (`reclaim_sfx_region`), which re-arms this sync.
            return false;
        }
        if self.shared_region == want {
            let resident = want.is_none_or(|b| self.sfx_vabs.contains_key(&b.slot));
            // A bank that did not fit is retried once the tail has moved -
            // the dance's track replacing the field's, or a spill the new
            // track overran.
            if resident || self.shared_retry_gen == Some(self.tail.generation()) {
                return resident;
            }
        }
        self.shared_region = want;
        self.shared_retry_gen = None;
        let (a, b) = legaia_engine_core::world::SHARED_REGION_SLOTS;
        self.sfx_vabs.remove(&a);
        self.sfx_vabs.remove(&b);
        self.tail.drop_shared_spill();
        let Some(want) = want else {
            return true;
        };
        let staged = self.stage_shared_region(want, read_entry);
        if !staged {
            self.shared_retry_gen = Some(self.tail.generation());
        }
        staged
    }

    /// Read and upload `want` into the shared region - spilling into the BGM
    /// tail when its bodies do not fit above slot 0
    /// ([`legaia_engine_audio::spu_layout::upload_shared_region_spilled`]).
    fn stage_shared_region(
        &mut self,
        want: SharedRegionBank,
        read_entry: impl FnOnce(u32) -> Option<Vec<u8>>,
    ) -> bool {
        let Some(bytes) = read_entry(want.prot_entry) else {
            log::debug!("shared-region bank PROT {} unreadable", want.prot_entry);
            return false;
        };
        let Some((report, vab_off)) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        else {
            return false;
        };
        let base = self.shared_region_base;
        let room = legaia_engine_audio::spu_layout::SPU_RAM_BYTES.saturating_sub(base);
        let body_total: u32 = report.vag_samples.iter().map(|v| v.size as u32).sum();
        if body_total > room {
            return self.stage_shared_spilled(want, &report, &bytes[vab_off..]);
        }
        let body = &bytes[vab_off..];
        let bank = match self.sfx_vabs.get(&0) {
            // Above the resident slot-0 bank, through the kernel the boot
            // staging and the page share.
            Some(slot0) => self.audio.with_spu(|spu| {
                legaia_engine_audio::spu_layout::upload_shared_region(spu, slot0, &report, body)
            }),
            None => Some(self.audio.with_spu(|spu| {
                let mut alloc = legaia_engine_audio::SpuAllocator::new(base, room);
                VabBank::upload(spu, &mut alloc, &report, body)
            })),
        };
        let Some(bank) = bank else {
            return false;
        };
        self.sfx_vabs.insert(want.slot, bank);
        true
    }

    /// Stage an oversize shared-region bank across the region and the BGM
    /// tail, parking the tail half as a [`BgmTail`] borrower so a track that
    /// overruns it drops the bank (and the next sync re-stages it).
    fn stage_shared_spilled(
        &mut self,
        want: SharedRegionBank,
        report: &legaia_vab::VabReport,
        body: &[u8],
    ) -> bool {
        self.observe_track();
        let Some(tail_base) = self.tail.spill_base() else {
            return false;
        };
        let Some(slot0) = self.sfx_vabs.get(&0) else {
            return false;
        };
        let up = self.audio.with_spu(|spu| {
            legaia_engine_audio::spu_layout::upload_shared_region_spilled(
                spu, slot0, tail_base, report, body,
            )
        });
        let Some(up) = up else {
            log::debug!(
                "shared-region bank PROT {} does not fit above slot 0 and behind the BGM",
                want.prot_entry
            );
            return false;
        };
        if let Some((base, end)) = up.spill {
            self.tail
                .commit_shared_spill(legaia_engine_audio::bgm_tail::TailBorrow {
                    slot: want.slot,
                    base,
                    end,
                });
        }
        self.sfx_vabs.insert(want.slot, up.bank);
        true
    }

    /// Stage the slot-0 system bank (PROT 0868's entry `bytes`) at the
    /// bottom of the SFX region through the shared layout kernel
    /// ([`legaia_engine_audio::spu_layout::upload_resident_sfx`]), and keep
    /// the bytes so [`Self::reclaim_sfx_region`] can re-stage it. The shared
    /// region is then filled above it by [`Self::sync_shared_region`].
    /// Returns `false` when the entry carries no VAB at `+4` or `+0`.
    // REF: FUN_8001E54C
    pub fn stage_resident_slot0(&mut self, bytes: Vec<u8>) -> bool {
        let Some((report, vab_off)) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        else {
            return false;
        };
        let staged = self.audio.with_spu(|spu| {
            legaia_engine_audio::spu_layout::upload_resident_sfx(
                spu,
                (&report, &bytes[vab_off..]),
                None,
            )
        });
        self.set_shared_region_base(legaia_engine_audio::spu_layout::shared_region_base(
            &staged.slot0,
        ));
        self.sfx_vabs.insert(0, staged.slot0);
        self.resident_slot0 = Some(bytes);
        true
    }

    /// Whether a track bank currently holds the SFX region (the resident
    /// SFX banks are dropped until it lets go).
    pub fn sfx_evicted(&self) -> bool {
        self.sfx_evicted
    }

    /// Keep the resident SFX banks consistent with the BGM bank just
    /// staged. A bank reaching into the SFX region (the ending theme's)
    /// overwrote them: drop every one, so no cue keys a stale address. A
    /// bank that fits the BGM region after such a track hands the region
    /// back: re-stage slot 0 from the kept bytes and re-arm the shared
    /// region, which the next [`Self::sync_shared_region`] refills for the
    /// current mode.
    fn reclaim_sfx_region(&mut self) {
        use legaia_engine_audio::spu_layout::sfx_region_free;
        if !sfx_region_free(self.bank.as_ref()) {
            self.sfx_evicted = true;
            self.sfx_vabs.clear();
            self.tail.clear();
            self.shared_region = None;
            return;
        }
        if !self.sfx_evicted {
            return;
        }
        self.sfx_evicted = false;
        self.shared_region = None;
        if let Some(bytes) = self.resident_slot0.take()
            && !self.stage_resident_slot0(bytes)
        {
            log::warn!("slot-0 system bank did not re-stage after the SFX region was freed");
        }
    }

    /// Key off SPU voices a field-VM op stopped (`FUN_800653C8`, the side-band
    /// teardown's top two).
    // REF: FUN_800653C8
    pub fn stop_sfx_voices(&mut self, voices: &[u8]) {
        let mask = voices
            .iter()
            .filter(|&&v| v < 24)
            .fold(0u32, |m, &v| m | (1 << v));
        if mask != 0 {
            self.audio.with_spu(|spu| spu.key_off_mask(mask));
        }
    }

    /// Replay the world's SFX ring producer calls onto the retail ring half
    /// of the scheduler, and install the step the ring ages by per
    /// [`Self::tick_sfx_frame`] - the vsyncs one call spans (retail ages by
    /// `DAT_1F800393` once per game tick of that many vsyncs; a host that
    /// ticks per vsync ages by 1). Call once per sim tick, after the world tick and
    /// before [`Self::tick_sfx_frame`] - the order retail's frame runs the
    /// producers and the drainer in.
    // REF: FUN_80035B50, FUN_80035BAC, FUN_80035BD0
    pub fn apply_sfx_ring_ops(&mut self, ops: &[SfxRingOp], frame_step: u8) {
        self.sfx_sched.set_frame_step(frame_step);
        for op in ops {
            match *op {
                SfxRingOp::Push(id) => {
                    self.sfx_sched.push_ring_cue(id);
                }
                SfxRingOp::SetLastDelay(d) => self.sfx_sched.set_ring_cue_delay(d),
                SfxRingOp::ReplaceLast(id) => self.sfx_sched.replace_ring_cue(id),
                SfxRingOp::WriteSlot(slot, id) => self.sfx_sched.write_ring_slot(slot.into(), id),
                SfxRingOp::ArmSlot(slot, id, delay) => {
                    self.sfx_sched
                        .arm_ring_slot(slot.into(), id, i32::from(delay))
                }
            }
        }
    }

    /// Mirror the field-side SFX sources the ring's cues resolve against:
    /// the scene's prescript bundle (the runtime descriptor rows, cue ids
    /// `>= 0x200`) and the side-band bank the scripts' op-`0x36` sub-`1`
    /// requests hold in VAB slot `3`. `side_band` is
    /// [`legaia_engine_core::world::World::side_band_bank`] in a field-family
    /// mode and `None` elsewhere - retail has slot 3 open only in the field
    /// (`docs/formats/sfx-table.md`). `field_family` says the world is in such
    /// a mode, where the field init `FUN_801D6704` has closed slot `11`
    /// (`0x801D68B4`), so a parked reward bank is dropped. `read_entry` reads
    /// an extraction-frame PROT entry. The bank is staged behind the BGM, in
    /// the free tail of its region, the way the reward bank is; the
    /// residency rule and the retry memo are [`BgmTail`]'s.
    // REF: FUN_800243F0, FUN_8001E54C, FUN_801D6704
    pub fn sync_field_sfx(
        &mut self,
        bundle: &[u8],
        field_family: bool,
        side_band: Option<SideBandBank>,
        read_entry: impl FnOnce(u32) -> Option<Vec<u8>>,
    ) {
        if self.runtime_sfx_bundle.as_slice() != bundle {
            self.runtime_sfx_bundle = bundle.to_vec();
        }
        // The drainer rolls one-shots over voices 23..=22 in the field and
        // 23..=20 elsewhere (`FUN_80016B6C`).
        self.sfx_bank.set_field_family(field_family);
        if field_family && let Some(slot) = self.tail.drop_reward() {
            self.sfx_vabs.remove(&slot);
        }
        self.observe_track();
        let Some(want) = side_band else {
            if let Some(slot) = self.tail.drop_side_band() {
                self.sfx_vabs.remove(&slot);
            }
            return;
        };
        if !self.tail.begin_side_band_attempt(want) {
            return;
        }
        if let Some(slot) = self.tail.drop_side_band() {
            self.sfx_vabs.remove(&slot);
        }
        let Some(bytes) = read_entry(want.prot_entry) else {
            log::debug!("side-band bank PROT {} unreadable", want.prot_entry);
            return;
        };
        match self.park_tail_bank(want.slot, &bytes) {
            Some(borrow) => {
                self.tail.commit_side_band(want, borrow);
                log::debug!(
                    "side-band bank {} (PROT {}) staged in slot {} behind the BGM",
                    want.request,
                    want.prot_entry,
                    want.slot
                );
            }
            None => log::debug!(
                "side-band bank {} (PROT {}) does not fit behind the BGM",
                want.request,
                want.prot_entry
            ),
        }
    }

    /// Keep the battle's `monster.snd` banks staged - `want` is
    /// [`legaia_engine_core::world::World::battle_monster_sound_banks`]
    /// (`(VAB slot, bank index)`, empty outside battle), `read_archive` reads
    /// PROT 891. Retail's battle scene loader `FUN_800520F0` streams them into
    /// slots `7` / `8` at their own SPU bases; the port parks them behind the
    /// BGM like the reward bank, and drops them when the battle ends. A
    /// request that does not fit is not re-read until the free tail moves.
    // REF: FUN_800520F0, FUN_8003E104
    pub fn sync_battle_monster_banks(
        &mut self,
        want: &[(u8, u16)],
        read_archive: impl FnOnce() -> Option<Vec<u8>>,
    ) {
        self.observe_track();
        let parked = self.tail.monsters().map(|(k, _)| k.clone());
        if parked.as_deref() == Some(want) {
            return;
        }
        if parked.is_some() {
            for slot in self.tail.drop_monsters() {
                self.sfx_vabs.remove(&slot);
            }
        }
        let key: legaia_engine_audio::bgm_tail::MonsterBankKey = want.to_vec();
        if key.is_empty() || !self.tail.begin_monster_attempt(&key) {
            return;
        }
        let Some(archive) = read_archive() else {
            log::debug!("monster.snd (PROT 891) unreadable");
            return;
        };
        for &(slot, bank) in want {
            let Some(bytes) = legaia_asset::vab_multi_bank::bank_bytes(&archive, usize::from(bank))
            else {
                log::debug!("monster.snd bank {bank} absent");
                continue;
            };
            match self.park_tail_bank(slot, bytes) {
                Some(borrow) => {
                    self.tail.commit_monster(&key, borrow);
                    log::debug!("monster.snd bank {bank} staged in slot {slot} behind the BGM");
                }
                None => log::debug!("monster.snd bank {bank} does not fit behind the BGM"),
            }
        }
    }

    /// One tick of the world's field-side SFX sources, in the order both
    /// hosts run them after the world tick: the ring producer calls replayed
    /// onto the scheduler ([`Self::apply_sfx_ring_ops`]), the scene's runtime
    /// descriptor rows and side-band bank ([`Self::sync_field_sfx`]), the
    /// shared slot-2 / slot-6 region for the current mode (every tick -
    /// [`World::sync_sfx_residency`] is the residency's one writer), the
    /// battle's `monster.snd` banks, then the voice stops before the direct
    /// voice keys, so a release and a re-key of one voice in the same tick
    /// end keyed. The field's CD-XA cues are not drained here: each host
    /// plays them on its own XA lane, first.
    ///
    /// [`World::sync_sfx_residency`]: legaia_engine_core::world::World::sync_sfx_residency
    // REF: FUN_80035B50, FUN_80035BAC, FUN_80035BD0, FUN_800653C8
    pub fn route_world_sfx(
        &mut self,
        world: &mut legaia_engine_core::world::World,
        index: &legaia_engine_core::scene::ProtIndex,
    ) {
        let ops = world.take_sfx_ring_ops();
        // One `World::tick` is one vsync, and the scheduler ticks once per
        // `World::tick`, so the ring ages by the vsyncs one tick spans
        // (`display_frame_step`, always 1) - not by the game-tick cadence
        // `frame_step`, which retail applies once per *game tick* of that
        // many vsyncs. The two schedules are the same in wall time.
        self.apply_sfx_ring_ops(&ops, world.clock.display_frame_step.clamp(1, 255) as u8);
        let field_family = matches!(
            world.mode,
            legaia_engine_core::world::SceneMode::Field
                | legaia_engine_core::world::SceneMode::WorldMap
        );
        // A slot-6 side-band bank is not a tail borrower: retail streams it
        // over the field bank in the shared region, and the residency below
        // carries it.
        let side_band = world.tail_side_band_bank().filter(|b| b.slot != 6);
        self.sync_field_sfx(world.runtime_sfx_bundle(), field_family, side_band, |e| {
            index.entry_bytes_extended(e).ok()
        });
        // The slot-2 / slot-6 region follows the mode: the field bank in the
        // field, the class-2 bank in battle, a minigame's own in its mode.
        let shared = world.sync_sfx_residency();
        self.sync_shared_region(shared, |e| index.entry_bytes_extended(e).ok());
        // The battle's two monster.snd banks (VAB slots 7 / 8).
        let monster_banks = world.battle_monster_sound_banks();
        self.sync_battle_monster_banks(&monster_banks, || {
            index
                .entry_bytes_extended(legaia_asset::vab_multi_bank::MONSTER_SND_PROT_INDEX as u32)
                .ok()
        });
        self.stop_sfx_voices(&world.take_sfx_voice_stops());
        // A minigame's directly keyed voices (the slot machine's reel motor).
        for k in world.take_sfx_voice_keys() {
            let keyed = self.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                k.voice,
                k.vab_program_tone,
                k.note_and_fine,
                k.volume,
            ));
            log::debug!(
                "direct voice {:#04x} {:?} keyed: {keyed}",
                k.voice,
                k.vab_program_tone
            );
        }
    }

    /// Queue one tick's battle strike / cast cues at their strike-relative
    /// timing, `(actor, target)` riding along. A results-frame
    /// [`LEVEL_UP_CUE`](legaia_engine_core::world::LEVEL_UP_CUE) first parks
    /// the reward bank behind the BGM ([`Self::ensure_reward_bank`]), as
    /// retail loads PROT 0889 at results time.
    // REF: FUN_8004E568
    pub fn enqueue_battle_cues(
        &mut self,
        cues: &[legaia_engine_core::battle_events::BattleSfxCue],
        index: &legaia_engine_core::scene::ProtIndex,
    ) {
        if cues
            .iter()
            .any(|c| c.kind == legaia_engine_core::world::LEVEL_UP_CUE)
            && !self.ensure_reward_bank(|e| index.entry_bytes_extended(e).ok())
        {
            log::info!("level-up jingle bank (PROT 0889) did not stage behind the BGM");
        }
        for cue in cues {
            self.enqueue_sfx(cue.kind, cue.timing_frames, cue.actor_slot, cue.target_slot);
        }
    }

    /// The per-tick tail of the audio frame: rest the duck on the world's
    /// configured level (a loaded save's `_DAT_8008457C`), ramp it one unit,
    /// then advance the scheduler and fire what matured. The duck runs in
    /// every mode - the ramp back to full outlives the battle.
    pub fn tick_audio_frame(&mut self, configured_level: i32) -> SfxFrameReport {
        self.set_duck_reference(configured_level);
        self.tick_duck();
        self.tick_sfx_frame()
    }

    /// The side-band bank currently staged, if any.
    pub fn side_band(&self) -> Option<SideBandBank> {
        self.tail.side_band().map(|(k, _)| k)
    }

    /// The tail borrowers' residency model (for tests / traces).
    pub fn bgm_tail(&self) -> &BgmTail<SideBandBank> {
        &self.tail
    }

    /// Drop every queued SFX cue (scene transition / battle abort).
    pub fn clear_sfx(&mut self) {
        self.sfx_sched.clear();
    }

    /// Replace the active VAB bank. Engines call this once per scene after
    /// resolving the scene's primary VAB entry through
    /// [`legaia_engine_core::scene::SceneHost::scene_vab_bytes`]; the bank
    /// is uploaded into the SPU and stored here for subsequent SEQ starts.
    pub fn set_bank(&mut self, bank: VabBank) {
        // A tail borrower the new bank's samples reach is gone with it; one
        // above them stays (retail's resolver closes neither slot).
        self.bank = Some(bank);
        self.reclaim_sfx_region();
        self.observe_track();
    }

    /// Borrow the active bank - useful for tests / inspection.
    pub fn bank(&self) -> Option<&VabBank> {
        self.bank.as_ref()
    }

    /// `true` if a sequencer is attached, paused or not - the track whose
    /// samples the BGM region holds.
    pub fn is_attached(&self) -> bool {
        self.audio.sequencer_progress().is_some()
    }

    /// `true` if a sequencer is currently attached to the audio output.
    pub fn is_playing(&self) -> bool {
        self.audio.sequencer_progress().is_some() && !self.audio.sequencer_paused()
    }

    /// Split a raw `music_01` bank entry (`[chunk][pBAV VAB][pQES SEQ]`),
    /// upload the entry's **own** VAB into the SPU BGM region (capped below
    /// the resident SFX region, or across it for a bank too large for the
    /// BGM region - `legaia_engine_audio::spu_layout`), stash it as the
    /// active bank, and return the SEQ bytes. `None` when the pair is absent
    /// or the VAB header doesn't parse. Every track the field VM starts comes
    /// through here - a scene-local id plays retail's fallback track - so
    /// the track always brings its own instruments.
    fn stage_owned_vab(&mut self, entry_bytes: &[u8]) -> Option<Vec<u8>> {
        // The installer walk's split (type-0 bank, type-2 score), not a
        // magic hunt - see `chunk_install::owned_bank_offsets`.
        let split = legaia_engine_core::chunk_install::owned_bank_offsets(entry_bytes)?;
        let vab_off = split.vab;
        let report = legaia_vab::parse(entry_bytes, vab_off).ok()?;
        // Both halves are validated before the SPU is touched - the page's
        // `stage_owned` keeps the same order - so a score that does not parse
        // leaves the playing track's samples where they were.
        let seq_bytes = entry_bytes.get(split.seq..)?;
        Seq::parse(seq_bytes).ok()?;
        let body = &entry_bytes[vab_off..];
        // Into the BGM region, or - for a bank that does not fit it, the
        // ending theme's - from the same base across the SFX region, as
        // retail opens VAB 10 at slot 0's base (`spu_layout`).
        let staged = self
            .audio
            .with_spu(|spu| legaia_engine_audio::spu_layout::upload_owned_bank(spu, &report, body));
        // A tail borrower the new track's samples reach is gone with them.
        self.bank = Some(staged.bank);
        self.reclaim_sfx_region();
        self.observe_track();
        Some(seq_bytes.to_vec())
    }

    fn start_inner(&mut self, bgm_id: u16, seq_bytes: &[u8]) -> Result<()> {
        let Some(bank) = self.bank.clone() else {
            log::warn!("AudioBgmDirector::start({bgm_id}) ignored - no VAB bank loaded for scene");
            return Ok(());
        };
        let seq = Seq::parse(seq_bytes).context("parse SEQ for BGM start")?;
        let mut sequencer = Sequencer::new(seq, bank);
        sequencer.set_master_vol(self.master_vol);
        if let Some(loop_to) = self.loop_to {
            sequencer.set_loop_to(loop_to);
        }
        // Retail BGM changes are hard cuts (or short `SsSeqSetVol` ramps), not
        // a serial cross-fade that fades the old track out to silence before
        // the new one is even installed - that swallows the incoming track's
        // intro. Swap immediately so the new track sounds from its first event,
        // with only a brief click-guard fade-in on the SPU master. If nothing
        // is playing, attach directly at full volume.
        //
        // ~2 frames at 60 Hz (44100 / 60 * 2). Long enough to avoid an onset
        // pop, far too short to hide an intro (the old fade held it silent for
        // 22050 samples = 0.5 s).
        const TRANSITION_FADE_IN_SAMPLES: u32 = 1_470;
        if self.audio.sequencer_progress().is_some() && !self.audio.sequencer_paused() {
            self.audio.swap_bgm(sequencer, TRANSITION_FADE_IN_SAMPLES);
        } else {
            self.audio.attach_sequencer(sequencer);
        }
        // Retail's start arm (op 0x35 sub-op 1, `0x801E0104`) clears the
        // pause bit alongside the track select, so a start issued while
        // paused must reopen the sequencer gate too - attaching behind a
        // closed gate leaves the new track silent until an explicit
        // resume/unhalt.
        self.audio.set_sequencer_paused(false);
        self.last_started = Some(bgm_id);
        log::info!("AudioBgmDirector: BGM {bgm_id} started");
        Ok(())
    }
}

impl<S: AudioSink> BgmDirector for AudioBgmDirector<S> {
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        // Suppress duplicate starts for the same BGM id - the field VM's
        // op 0x35 occasionally re-emits without a state change (we'd lose
        // the playhead by re-attaching).
        if self.last_started == Some(bgm_id)
            && !self.audio.sequencer_paused()
            && self.audio.sequencer_progress().is_some()
        {
            return;
        }
        if let Err(e) = self.start_inner(bgm_id, seq_bytes) {
            log::warn!("AudioBgmDirector::start({bgm_id}) failed: {e:#}");
        }
    }

    fn start_owned_vab(&mut self, bgm_id: u16, entry_bytes: &[u8]) {
        // Suppress a redundant re-emit of the same global track (the field VM
        // occasionally re-fires op 0x35): re-uploading the VAB + restarting
        // would drop the playhead.
        if self.last_started == Some(bgm_id)
            && !self.audio.sequencer_paused()
            && self.audio.sequencer_progress().is_some()
        {
            return;
        }
        let Some(seq) = self.stage_owned_vab(entry_bytes) else {
            log::warn!("AudioBgmDirector::start_owned_vab({bgm_id}) - no [VAB][SEQ] pair in entry");
            return;
        };
        if let Err(e) = self.start_inner(bgm_id, &seq) {
            log::warn!("AudioBgmDirector::start_owned_vab({bgm_id}) failed: {e:#}");
        }
    }

    /// The pause state is the output's sequencer gate and nothing else - the
    /// browser twin reads the same bit - so a writer that bypasses the
    /// director (a movie ducking the score, `legaia_engine_core::movie_audio`)
    /// cannot leave a second latch disagreeing with it.
    fn pause(&mut self) {
        self.audio.set_sequencer_paused(true);
    }

    /// Sub-op `4`: retail's re-attach replays the slot's sequence from its
    /// start (`FUN_800628F0` resets the read cursor before it plays), so the
    /// track rewinds, then the gate opens.
    fn resume(&mut self) {
        self.audio.rewind_sequencer();
        self.audio.set_sequencer_paused(false);
    }

    /// Detach the track **and reopen the gate**, so a stop issued while
    /// paused leaves nothing closed behind it (the browser play page's `stop`
    /// does the same).
    fn stop(&mut self) {
        self.audio.detach_sequencer();
        self.audio.set_sequencer_paused(false);
        self.last_started = None;
    }

    /// Sub-op `0xA` - the unhalt-pause swap-commit (retail `0x801E0264`).
    /// When the pause gate is still closed no start intervened, so the
    /// director is holding the track sub-op 2 paused: release it the way
    /// retail's `FUN_800266E0` + `FUN_80026520` pair detaches and closes
    /// the slot. When a start already landed (the paired sub-op 9 precedes
    /// the commit in script order), the swap is done and the occupant is
    /// the incoming track - leave it alone. Either way the pause gate is
    /// cleared unconditionally, mirroring retail clearing `_DAT_8007B750`
    /// bit 1 on every pass through the arm - without this, a start issued
    /// while paused attaches its sequencer behind a still-closed gate and
    /// the score stays silent after the cutscene.
    fn unhalt_pause(&mut self) {
        if self.audio.sequencer_paused() {
            self.audio.detach_sequencer();
            self.last_started = None;
        }
        self.audio.set_sequencer_paused(false);
    }
}

/// Which VAB slot a cue resolves to, given the installed cue -> slot routing
/// and the set of slots that actually staged. `None` means the cue is silent.
///
/// A routed cue keys the slot its `+4` category names **or nothing**: retail's
/// drainer `FUN_80016B6C` skips a cue whose mixer record's `+0xB` enable byte
/// is zero (`0x80016CE4..0x80016CEC`), and a closed slot has it zero - so a
/// category-6 cue in battle (slot 6 closed by `FUN_8001DCF8`) or a category-2
/// cue in the field (slot 2 closed by `FUN_801D6704`) is silent there, not
/// rerouted to whichever bank is open. The hosts restage the slot-2 / slot-6
/// region per mode ([`AudioBgmDirector::sync_shared_region`]), so each
/// category's own bank is resident exactly where retail's is.
///
/// Only an id the routing does not carry (no descriptor table installed, or an
/// id past it) still falls back to [`FALLBACK_VAB_SLOT`] when that is staged.
///
/// Free function rather than a method so it is testable without a cpal device
/// (an [`AudioBgmDirector`] needs a live audio output).
pub(crate) fn resolve_sfx_slot<T>(
    cue_slots: &BTreeMap<u8, u8>,
    staged: &BTreeMap<u8, T>,
    id: u8,
) -> Option<u8> {
    let slot = cue_slots.get(&id).copied().unwrap_or(FALLBACK_VAB_SLOT);
    staged.contains_key(&slot).then_some(slot)
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
    fn headless() -> AudioBgmDirector<legaia_engine_audio::TestAudioSink> {
        AudioBgmDirector::new(Arc::new(legaia_engine_audio::TestAudioSink::new(
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
        assert_director::<AudioBgmDirector<legaia_engine_audio::AudioOut>>();
        let _ = empty_bank(); // touch path so unused-import lint stays clean
    }
}
