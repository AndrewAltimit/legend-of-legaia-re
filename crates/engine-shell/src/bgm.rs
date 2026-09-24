//! Concrete [`legaia_engine_core::scene::BgmDirector`] adapter that drives a
//! cpal-backed [`legaia_engine_audio::AudioOut`].
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
use legaia_engine_audio::{
    ArtsShoutBank, AudioOut, PendingCue, SHOUT_CD_RESPONSE_DELAY, Sequencer, SfxBank, SfxScheduler,
    VabBank, XaClipBank,
};
use legaia_engine_core::scene::BgmDirector;
use legaia_engine_core::world::{SfxRingOp, SharedRegionBank, SideBandBank};
use legaia_seq::Seq;

/// The pause menu's cursor-step cue: `FUN_80032A44`'s ring write
/// `_li a2,0x21` at `0x80032b9c` (`ghidra/scripts/funcs/80032a44.txt`). Its
/// `sfx-table.md` descriptor is category `0`, so the director sounds it out
/// of the slot-0 system bank (PROT 0868). Provenance `disc`: traced to the
/// retail ring write, the same three ids the browser play page fires
/// (`web-viewer::play_sfx`), so the two hosts blip alike.
pub const RETAIL_MENU_CURSOR_CUE: u16 = 0x21;
/// The enabled-row confirm cue: `li a1,0x20` at `0x80032d24` in `FUN_80032A44`.
pub const RETAIL_MENU_CONFIRM_CUE: u16 = 0x20;
/// The cancel cue: `_li a2,0x37` at `0x80032d74` in `FUN_80032A44`.
pub const RETAIL_MENU_CANCEL_CUE: u16 = 0x37;

/// BGM director that routes [`BgmDirector`] events into a live
/// [`AudioOut`]. The director holds a clone of the audio handle (cpal stream
/// is reference-counted internally via `Arc`) plus the active VAB bank.
pub struct AudioBgmDirector {
    audio: Arc<AudioOut>,
    bank: Option<VabBank>,
    /// Master volume forwarded to every freshly-attached sequencer. Engines
    /// bump this when the user adjusts the music slider.
    pub master_vol: u8,
    /// Loop-to event index for newly-started sequencers. `None` plays once
    /// (sequencer reports `finished` when it runs off the end). Most field
    /// BGM loops to 0; cutscene SEQs typically don't.
    pub loop_to: Option<usize>,
    /// Whether playback is currently paused. `pause` / `resume` toggle
    /// without detaching the active sequencer.
    paused: bool,
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
    /// The current field scene's prescript bundle - the retail
    /// current-bundle slot `_DAT_8007B8D0` - whose record 0 is the runtime
    /// half (`>= 0x200`) of the SFX descriptor table. Mirrored from the world
    /// by [`Self::sync_field_sfx`].
    runtime_sfx_bundle: Vec<u8>,
    /// The side-band bank staged behind the BGM, `None` while none is.
    side_band: Option<SideBandBank>,
    /// The last `(request, bgm generation)` a side-band stage was attempted
    /// for, so a bank that does not fit is not re-read every frame; a BGM
    /// restage (which moves the free tail) makes it worth trying again.
    side_band_attempt: Option<(i32, u64)>,
    /// Bumped on every BGM-region restage ([`Self::set_bank`] /
    /// [`Self::stage_owned_vab`]).
    bgm_gen: u64,
    /// The bank the SPU region VAB slots `2` and `6` share holds - retail's
    /// per-mode refill of one region (`FUN_800265E8` gives both slots
    /// `0x33010`), driven by
    /// [`legaia_engine_core::world::World::sync_sfx_residency`]. `None` while
    /// neither slot is open.
    shared_region: Option<SharedRegionBank>,
    /// SPU address the shared region starts at: one past the slot-0 system
    /// bank's samples, inside the reserved SFX region.
    shared_region_base: u32,
}

/// `_DAT_8007B910`'s reference value (`0xD7`, `FUN_8001FFA4`): the un-ducked
/// level the `0x51` arm ramps back to.
pub const DUCK_LEVEL_REF: u8 = 0xD7;

/// VAB slot the battle-end reward bank (PROT 0889, cue `0x50`) is installed
/// in - retail streams it at results time (`FUN_8004E568` phase 4,
/// `FUN_8001E54C(0xB, ...)`), and the port stages it transiently the same
/// way ([`AudioBgmDirector::stage_transient_sfx_vab`]).
pub const TRANSIENT_REWARD_SLOT: u8 = 11;

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

impl AudioBgmDirector {
    pub fn new(audio: Arc<AudioOut>) -> Self {
        Self {
            audio,
            bank: None,
            master_vol: 100,
            loop_to: Some(0),
            paused: false,
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
            runtime_sfx_bundle: Vec::new(),
            side_band: None,
            side_band_attempt: None,
            bgm_gen: 0,
            shared_region: None,
            shared_region_base: crate::boot::SPU_RAM_BYTES - crate::boot::SFX_BANK_SPU_BYTES,
        }
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
    /// [`AudioOut::play_xa_shout`]). Returns the fired channel, or `None`
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
    /// ([`Self::set_xa_lazy_source`]) - the cast voices' path. Returns
    /// `false` when nothing is staged for the request.
    // REF: FUN_8003D53C
    pub fn play_xa_clip(&mut self, clip_slot: u32, channel: u32, duration_sectors: u32) -> bool {
        let (Ok(slot), Ok(ch)) = (u8::try_from(clip_slot), u8::try_from(channel)) else {
            return false;
        };
        let staged = self
            .xa_clip_bank
            .as_ref()
            .is_some_and(|b| b.is_staged(slot, ch));
        if !staged && !self.stage_xa_channel_lazily(slot, ch, duration_sectors) {
            return false;
        }
        let Some(bank) = self.xa_clip_bank.as_ref() else {
            return false;
        };
        let Some(clip) = bank.clip(slot, ch) else {
            return false;
        };
        let frames = bank
            .cut_frames(slot, ch, duration_sectors)
            .unwrap_or(0)
            .max(1);
        let take = if clip.stereo { frames * 2 } else { frames };
        let pcm = clip.pcm[..take.min(clip.pcm.len())].to_vec();
        if pcm.is_empty() {
            return false;
        }
        let channels = if clip.stereo {
            legaia_xa::Channels::Stereo
        } else {
            legaia_xa::Channels::Mono
        };
        self.audio.play_xa_shout(
            pcm,
            clip.sample_rate,
            channels,
            0x4000,
            SHOUT_CD_RESPONSE_DELAY,
        );
        true
    }

    /// Set the audio duck's target as a percentage of the reference level
    /// (`BattleEvent::DuckAudioLevel`): `75` under a summon / magic capture,
    /// `100` when the Done band ramps it back. The ramp itself runs in
    /// [`Self::tick_duck`].
    pub fn set_duck_pct(&mut self, pct: u8) {
        let pct = u32::from(pct.min(100));
        self.duck_target = (u32::from(DUCK_LEVEL_REF) * pct / 100) as u8;
    }

    /// One frame of the duck ramp: step the live level one unit toward the
    /// target (retail `DAT_1F800393` per vsync, the `0x35` / `0x51` arms)
    /// and re-apply it to the BGM as `master_vol * level / ref` - the
    /// `FUN_800267A8` -> `SsSeqSetVol` re-apply, which halves the cell into
    /// the 0..127 volume domain the same way `master_vol` already is.
    pub fn tick_duck(&mut self) {
        if self.duck_level == self.duck_target {
            return;
        }
        self.duck_level = if self.duck_level < self.duck_target {
            self.duck_level + 1
        } else {
            self.duck_level - 1
        };
        let vol =
            u32::from(self.master_vol) * u32::from(self.duck_level) / u32::from(DUCK_LEVEL_REF);
        self.audio.set_sequencer_master_vol(vol.min(127) as u8);
    }

    /// The live duck level in `_DAT_8007B910` units (for tests / traces).
    pub fn duck_level(&self) -> u8 {
        self.duck_level
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
    /// bank borrows BGM room instead, exactly as long as the current track
    /// leaves any: it is dropped again the moment a track restages
    /// ([`Self::stage_owned_vab`] / [`Self::set_bank`]). Returns `false` when
    /// the entry has no VAB header or the tail is too small.
    // REF: FUN_8001E54C, FUN_8004E568
    pub fn stage_transient_sfx_vab(&mut self, slot: u8, entry_bytes: &[u8]) -> bool {
        let Some((report, vab_off)) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(entry_bytes, o).ok().map(|r| (r, o)))
        else {
            return false;
        };
        // The BGM region runs from the reserved head up to the SFX region;
        // the resident bank's samples - and any other bank already borrowing
        // the tail - end where the free tail begins.
        let region_end = crate::boot::SPU_RAM_BYTES - crate::boot::SFX_BANK_SPU_BYTES;
        self.sfx_vabs.remove(&slot);
        let base = self.bgm_tail_used_end().div_ceil(16) * 16;
        if base >= region_end {
            return false;
        }
        let body_total: u32 = report.vag_samples.iter().map(|v| v.size as u32).sum();
        if body_total > region_end - base {
            return false;
        }
        let body = &entry_bytes[vab_off..];
        let bank = self.audio.with_spu(|spu| {
            let mut alloc = legaia_engine_audio::SpuAllocator::new(base, region_end - base);
            VabBank::upload(spu, &mut alloc, &report, body)
        });
        self.sfx_vabs.insert(slot, bank);
        true
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
            return self.bank.as_ref();
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
        let Some(vab) = self.sfx_vabs.get(&slot).or(self.bank.as_ref()) else {
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
    /// the SPU. Each cue resolves against the resident SFX bank **its own `+4`
    /// category names** ([`Self::sfx_vab_for_cue`]) - the retail path, where
    /// `FUN_80065034` repoints the current-bank globals at the cue's slot
    /// before the program lookup - and falls back to the active scene BGM bank
    /// ([`Self::bank`]) when nothing is staged at all (the disc-free boot).
    /// Returns the `(cue_id, voice)` pairs that keyed on. A cue is silently
    /// dropped when no bank is staged, its id isn't in the descriptor bank, its
    /// program / tone isn't resident, or no SPU voice is free (matching the
    /// retail "no voice / no program -> skip" behaviour). Call once per
    /// simulation tick so delayed cues advance even when none are enqueued that
    /// frame.
    pub fn tick_sfx_frame(&mut self) -> Vec<(u16, u8)> {
        let batch = self.sfx_sched.tick_frame();
        if batch.is_empty() {
            return Vec::new();
        }
        if self.sfx_vabs.is_empty() && self.bank.is_none() {
            return Vec::new();
        }
        let bank = &self.sfx_bank;
        let mut fired = Vec::new();
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
        self.audio.with_spu(|spu| {
            for (id, fire) in &ring {
                let voice = match fire {
                    RingFire::Static(small, vab) => bank.play_one_shot(*small, spu, vab),
                    RingFire::Runtime(row, vab) => SfxBank::play_descriptor(row, spu, vab),
                };
                if let Some(v) = voice {
                    fired.push((*id, v));
                }
            }
            for cue in &batch.fired {
                // The queue is a `u16` because the battle cue space is - the
                // action SM's cast cues run to `0x20E` - while the SFX
                // descriptor table is `0x00..=0x63`. Truncating with `as u8`
                // did not make an out-of-band cue silent, it made it play the
                // **wrong** descriptor: `0x20C` became `0x0C`, `0x118` became
                // `0x18`, and every one of those is a populated entry. Classify
                // instead, exactly as `FUN_8004FCC8` does.
                //
                // Only the `Voice` band is re-routed here. The `Ring` band's
                // `id - 1` resolution below `0x40` is retail's
                // (`route_sfx_cue`'s low leg) but the one live producer feeds
                // this queue an art-record `HitCue::kind` that the bank is
                // already indexed by, so applying it would silently move the
                // one cue that works today. Left as an open thread rather than
                // changed without an oracle.
                if let legaia_engine_audio::CueDispatch::Voice {
                    channel, submode, ..
                } = legaia_engine_audio::classify_cue(u32::from(cue.id))
                {
                    // A streamed CD-XA voice, not an SPU descriptor. The one
                    // producer feeding this queue such ids is the
                    // `FUN_801F3990` band, which the two measured retail casts
                    // never raised (`docs/subsystems/cast-module.md`), so it is
                    // declined rather than voiced. The cast's own voice - the
                    // module head cue - does not come through here: it rides
                    // the `(clip, channel, dur)` channel into `play_xa_clip`.
                    log::debug!(
                        "battle cue {:#06x} is a CD-XA voice (clip channel {channel:#04x} \
                         submode {submode}); the FUN_801F3990 band is declined",
                        cue.id
                    );
                    continue;
                }
                let Ok(id) = u8::try_from(cue.id) else {
                    continue;
                };
                // The cue's category picks its bank; a closed slot is silent,
                // and with nothing staged the scene BGM VAB stands in.
                let Some(vab) = self.sfx_vab_for_cue(id) else {
                    continue;
                };
                if let Some(voice) = bank.play_one_shot(id, spu, vab) {
                    fired.push((cue.id, voice));
                }
            }
        });
        fired
    }

    /// One past the highest sample the BGM region's occupants use: the BGM
    /// bank itself plus every bank borrowing its tail (the reward bank and a
    /// side-band bank). An empty region reads as its floor.
    fn bgm_tail_used_end(&self) -> u32 {
        let side = self.side_band.map(|b| b.slot);
        let ends = |b: &VabBank| b.samples.iter().flatten().map(|s| s.addr + s.size).max();
        let mut end = self
            .bank
            .as_ref()
            .and_then(ends)
            .unwrap_or(crate::boot::SPU_RESERVED_BYTES);
        for (slot, bank) in &self.sfx_vabs {
            if *slot == TRANSIENT_REWARD_SLOT || Some(*slot) == side {
                end = end.max(ends(bank).unwrap_or(0));
            }
        }
        end
    }

    /// Forget every bank borrowing the BGM region's tail - the region is
    /// being re-owned.
    fn drop_bgm_tail_banks(&mut self) {
        self.sfx_vabs.remove(&TRANSIENT_REWARD_SLOT);
        if let Some(b) = self.side_band.take() {
            self.sfx_vabs.remove(&b.slot);
        }
        self.bgm_gen = self.bgm_gen.wrapping_add(1);
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
        if self.shared_region == want {
            return want.is_none_or(|b| self.sfx_vabs.contains_key(&b.slot));
        }
        self.shared_region = want;
        let (a, b) = legaia_engine_core::world::SHARED_REGION_SLOTS;
        self.sfx_vabs.remove(&a);
        self.sfx_vabs.remove(&b);
        let Some(want) = want else {
            return true;
        };
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
        let room = crate::boot::SPU_RAM_BYTES.saturating_sub(base);
        let body_total: u32 = report.vag_samples.iter().map(|v| v.size as u32).sum();
        if body_total > room {
            log::debug!(
                "shared-region bank PROT {} ({body_total} B) does not fit in {room} B",
                want.prot_entry
            );
            return false;
        }
        let body = &bytes[vab_off..];
        let bank = self.audio.with_spu(|spu| {
            let mut alloc = legaia_engine_audio::SpuAllocator::new(base, room);
            VabBank::upload(spu, &mut alloc, &report, body)
        });
        self.sfx_vabs.insert(want.slot, bank);
        true
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
            }
        }
    }

    /// Mirror the field-side SFX sources the ring's cues resolve against:
    /// the scene's prescript bundle (the runtime descriptor rows, cue ids
    /// `>= 0x200`) and the side-band bank the scripts' op-`0x36` sub-`1`
    /// requests hold in VAB slot `3` (or `6`). `side_band` is
    /// [`legaia_engine_core::world::World::side_band_bank`] in a field-family
    /// mode and `None` elsewhere - retail has slot 3 open only in the field
    /// (`docs/formats/sfx-table.md`). `read_entry` reads an extraction-frame
    /// PROT entry. The bank is staged behind the BGM, in the free tail of its
    /// region, the way the reward bank is.
    // REF: FUN_800243F0, FUN_8001E54C
    pub fn sync_field_sfx(
        &mut self,
        bundle: &[u8],
        side_band: Option<SideBandBank>,
        read_entry: impl FnOnce(u32) -> Option<Vec<u8>>,
    ) {
        if self.runtime_sfx_bundle.as_slice() != bundle {
            self.runtime_sfx_bundle = bundle.to_vec();
        }
        let Some(want) = side_band else {
            if let Some(b) = self.side_band.take() {
                self.sfx_vabs.remove(&b.slot);
            }
            return;
        };
        if self.side_band == Some(want) {
            return;
        }
        if self.side_band_attempt == Some((want.request, self.bgm_gen)) {
            return;
        }
        self.side_band_attempt = Some((want.request, self.bgm_gen));
        if let Some(b) = self.side_band.take() {
            self.sfx_vabs.remove(&b.slot);
        }
        let Some(bytes) = read_entry(want.prot_entry) else {
            log::debug!("side-band bank PROT {} unreadable", want.prot_entry);
            return;
        };
        if self.stage_transient_sfx_vab(want.slot, &bytes) {
            self.side_band = Some(want);
            log::debug!(
                "side-band bank {} (PROT {}) staged in slot {} behind the BGM",
                want.request,
                want.prot_entry,
                want.slot
            );
        } else {
            log::debug!(
                "side-band bank {} (PROT {}) does not fit behind the BGM",
                want.request,
                want.prot_entry
            );
        }
    }

    /// The side-band bank currently staged, if any.
    pub fn side_band(&self) -> Option<SideBandBank> {
        self.side_band
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
        // The BGM region is re-owned wholesale; a transient reward bank in
        // its tail is gone with it, and so is a side-band bank.
        self.drop_bgm_tail_banks();
        self.bank = Some(bank);
    }

    /// Borrow the active bank - useful for tests / inspection.
    pub fn bank(&self) -> Option<&VabBank> {
        self.bank.as_ref()
    }

    /// `true` if a sequencer is currently attached to the audio output.
    pub fn is_playing(&self) -> bool {
        self.audio.sequencer_progress().is_some() && !self.paused
    }

    /// Split a raw `music_01` bank entry (`[chunk][pBAV VAB][pQES SEQ]`),
    /// upload the entry's **own** VAB into the SPU BGM region (capped below
    /// the resident SFX bank, exactly like `stage_scene_vab`), stash it as the
    /// active bank, and return the SEQ bytes. `None` when the pair is absent
    /// or the VAB header doesn't parse. This is the global-pool half of BGM
    /// playback - the track brings its own instruments, unlike the scene-local
    /// path that reuses the pre-staged scene VAB.
    fn stage_owned_vab(&mut self, entry_bytes: &[u8]) -> Option<Vec<u8>> {
        let vab_off = entry_bytes.windows(4).position(|w| w == b"pBAV")?;
        let seq_rel = entry_bytes[vab_off..]
            .windows(4)
            .position(|w| w == b"pQES")?;
        let report = legaia_vab::parse(entry_bytes, vab_off).ok()?;
        let body = &entry_bytes[vab_off..];
        let bank = self.audio.with_spu(|spu| {
            let mut alloc = legaia_engine_audio::SpuAllocator::new(
                crate::boot::SPU_RESERVED_BYTES,
                crate::boot::SPU_RAM_BYTES
                    - crate::boot::SPU_RESERVED_BYTES
                    - crate::boot::SFX_BANK_SPU_BYTES,
            );
            VabBank::upload(spu, &mut alloc, &report, body)
        });
        // A restaged track reclaims the whole BGM region, transient tail
        // included.
        self.drop_bgm_tail_banks();
        self.bank = Some(bank);
        Some(entry_bytes[vab_off + seq_rel..].to_vec())
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
        if self.audio.sequencer_progress().is_some() && !self.paused {
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
        self.paused = false;
        self.last_started = Some(bgm_id);
        log::info!("AudioBgmDirector: BGM {bgm_id} started");
        Ok(())
    }
}

impl BgmDirector for AudioBgmDirector {
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        // Suppress duplicate starts for the same BGM id - the field VM's
        // op 0x35 occasionally re-emits without a state change (we'd lose
        // the playhead by re-attaching).
        if self.last_started == Some(bgm_id)
            && !self.paused
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
            && !self.paused
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

    fn pause(&mut self) {
        self.paused = true;
        self.audio.set_sequencer_paused(true);
    }

    fn resume(&mut self) {
        self.paused = false;
        self.audio.set_sequencer_paused(false);
    }

    /// Detach the track **and reopen the gate**.
    ///
    /// The pause state is one quantity with two representations here - this
    /// director's own `paused` latch and the output's `sequencer_paused`
    /// gate - and every other arm writes both (`pause`, `resume`,
    /// `unhalt_pause`, `start_inner`). This one wrote only the latch, so a
    /// stop issued while paused left the two disagreeing: latch clear, gate
    /// closed. Nothing audible followed, because `start_inner` happens to
    /// reopen the gate unconditionally - but "the defect is masked by the
    /// next call" is not a body, and the browser play page's `stop` (which
    /// documents itself as doing what this one does) always wrote both.
    fn stop(&mut self) {
        self.audio.detach_sequencer();
        self.audio.set_sequencer_paused(false);
        self.paused = false;
        self.last_started = None;
    }

    /// Sub-op `0xA` - the unhalt-pause swap-commit (retail `0x801E0264`).
    /// When the pause latch is still set no start intervened, so the
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
        if self.paused {
            self.audio.detach_sequencer();
            self.paused = false;
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
/// (an [`AudioBgmDirector`] needs a live [`AudioOut`]).
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
        assert_director::<AudioBgmDirector>();
        let _ = empty_bank(); // touch path so unused-import lint stays clean
    }
}
