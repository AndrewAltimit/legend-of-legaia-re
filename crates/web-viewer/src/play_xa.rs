//! The play page's **CD-XA lane**: the arts-voice shouts and the battle's
//! one-shot XA clips - the browser twin of the native boot's
//! `read_arts_shout_bank` / `read_battle_xa_clip_bank` staging plus the
//! director's `play_art_shout` / `play_xa_clip`.
//!
//! # Why the page needs its own staging
//!
//! The native window demuxes the clip files off the disc image at boot
//! through `legaia_iso::raw::RawDisc` - channel demux needs the raw
//! 2352-byte sectors, because the CD-XA subheader that carries the channel
//! number sits outside the 2048-byte ISO view. The browser has no
//! filesystem, but it still holds the visitor's disc bytes in the tab, and
//! the runtime already reports every ISO file's `(lba, size)`
//! (`LegaiaRuntime::disc_file_extent_json`). So the page slices the raw
//! sectors of each wanted file out of the bytes it has and hands them to
//! [`LegaiaRuntime::play_xa_install`], which demuxes + decodes them here and
//! keeps **only the decoded clips** - the sectors are borrowed for the call
//! and dropped, so a 30 MB `XA27.XA` costs the page its decoded channels,
//! not its raw bytes.
//!
//! # What is staged, and from where
//!
//! * **Shouts** - `XA2.XA` Vahn / `XA4.XA` Noa / `XA6.XA` Gala
//!   (`legaia_art::arts_voice::clip_file`), 16-channel short-mono banks,
//!   decoded per channel and trimmed of trailing silence exactly as the
//!   native reader does, into a [`ArtsShoutBank`] whose per-art candidate
//!   pools come from the `SCUS_942.54` cue tables (`FUN_8004C140`'s data,
//!   `legaia_art::arts_voice::ArtsVoiceTable`).
//! * **Clips** - [`BATTLE_XA_CLIP_SLOTS`]: `XA27.XA` (slot 26, the eight
//!   stereo attack stings) and `XA30.XA` (slot `0x1D`, the ten mono
//!   per-character grunts), every 4-bit channel decoded into a
//!   [`XaClipBank`] keyed `(slot, channel)` - the raw space the retail clip
//!   starter `FUN_8003D53C(clip_slot, channel, duration_sectors)` takes.
//!
//! Both play through `WebAudioOut::play_xa_shout`, the same
//! `StreamResampler` staging the native cpal path uses: the modelled
//! CD-response start delay ([`SHOUT_CD_RESPONSE_DELAY`]) so the voice trails
//! the animation, the one-deep back-to-back queue, and a mix point **before**
//! the page's post-mixer gain, so a shout sits against the music at the same
//! ratio as under the native window.
//!
//! REF: FUN_8004C140 (arts-voice cue selector), FUN_8003D53C (CD-XA clip
//! starter both banks stand in for).

use crate::runtime::LegaiaRuntime;
use legaia_art::arts_voice::{ArtsVoiceTable, clip_file};
use legaia_engine_audio::{ArtsShoutBank, SHOUT_CD_RESPONSE_DELAY, ShoutClip, XaClip, XaClipBank};
use legaia_xa::demux::{
    AUDIO_BYTES_PER_SECTOR, SUBHEADER_OFFSET, USER_DATA_OFFSET, parse_subheader,
};
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

/// One raw Mode 2 sector as the page slices it out of the disc bytes.
pub(crate) const RAW_SECTOR_BYTES: usize = 2352;

/// Clip slots the battle's one-shot CD-XA cues address, as `(slot, file)`:
/// `26` = `XA27.XA` (the eight stereo attack stings the melee kernel's
/// `0x10C` cue resolves to through the sound funnel's voice leg) and `0x1D`
/// = `XA30.XA` (the ten mono per-character grunts the same kernel fires
/// directly). Slot `i` is `XA<i+1>.XA` by the boot-built clip table's own
/// construction (`docs/subsystems/audio.md`). The same table the native
/// boot's `BATTLE_XA_CLIP_SLOTS` carries; `engine-shell` is not a dependency
/// of this crate, so it is restated here and pinned by a test.
pub(crate) const BATTLE_XA_CLIP_SLOTS: &[(u8, &str)] = &[(26, "XA27.XA"), (0x1D, "XA30.XA")];

/// Unity XA gain (Q1.14), what both native XA players pass.
const XA_GAIN_UNITY: u16 = 0x4000;

/// Trailing samples under this magnitude are channel-padding silence, trimmed
/// off a shout so its audible end matches the retail read-span cutoff closely
/// enough for the back-to-back promotion queue (the native reader's constant).
const SHOUT_TAIL_SILENCE: u16 = 8;

/// `(character slot, clip file)` for the three voiced characters.
pub(crate) fn shout_files() -> impl Iterator<Item = (u8, &'static str)> {
    (0u8..3).filter_map(|c| clip_file(c as usize).map(|f| (c, f)))
}

/// Every XA file the lane wants installed, shouts first then clips.
pub(crate) fn wanted_files() -> Vec<&'static str> {
    shout_files()
        .map(|(_, f)| f)
        .chain(BATTLE_XA_CLIP_SLOTS.iter().map(|(_, f)| *f))
        .collect()
}

/// Live state of the page's XA lane.
#[derive(Default)]
pub struct PlayXa {
    /// Arts-voice shout bank; `None` until a shout file installs.
    pub shout_bank: Option<ArtsShoutBank>,
    /// Battle one-shot clip bank; `None` until a clip file installs.
    pub clip_bank: Option<XaClipBank>,
    /// Installed file -> decoded channel count.
    pub installed: BTreeMap<String, u32>,
    /// Shouts that resolved to a clip and were handed to the mixer.
    pub shouts_fired: u32,
    /// Shout requests that resolved to nothing (bank absent, art unvoiced,
    /// channel not decoded) - retail's silent-art degradation.
    pub shouts_unvoiced: u32,
    /// Clip requests handed to the mixer.
    pub clips_fired: u32,
    /// Clip requests for a `(slot, channel)` this lane has not staged.
    pub clips_unstaged: u32,
    /// The most recent shout: `(cslot, action, channel)`.
    pub last_shout: Option<(u8, u8, u8)>,
    /// The most recent clip: `(slot, channel, frames cut)`.
    pub last_clip: Option<(u8, u8, u32)>,
    /// Clip requests waiting for the page to slice their sectors - the
    /// **lazy** tier the cast voices ride ([`LegaiaRuntime::play_xa_stage_requests_json`]).
    /// One per `(slot, channel)`; the request is replayed when its span
    /// installs.
    pub pending_stage: Vec<PendingXaStage>,
    /// Clip requests that went to the lazy tier instead of playing at once.
    pub clips_deferred: u32,
    /// Channel spans the page installed lazily.
    pub lazy_installed: u32,
}

/// One lazy staging request: which raw sectors of which file the page
/// should hand to [`LegaiaRuntime::play_xa_install_span`], and the clip
/// request that wants them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingXaStage {
    /// The ISO path as `disc_file_extent_json` resolves it.
    pub path: String,
    /// First raw sector of the file.
    pub lba: u32,
    /// Sectors to slice from `lba`: the clip starter's read span, capped at
    /// the file.
    pub sectors: u32,
    pub slot: u8,
    pub channel: u8,
    pub duration_sectors: u32,
}

/// One CD-XA channel demuxed out of a run of raw sectors: the concatenated
/// 18 x 128-byte sound groups of every Form 2 audio sector carrying its
/// `(file_no, ch_no)` (grouped on both, keyed here by channel), plus the
/// coding parameters from the subheader.
#[derive(Debug, Clone)]
pub(crate) struct DemuxedChannel {
    pub ch_no: u8,
    pub sample_rate: u32,
    pub stereo: bool,
    pub bits_per_sample: u8,
    pub audio: Vec<u8>,
}

/// Demux a run of raw 2352-byte Mode 2 sectors per `(file_no, ch_no)` - the
/// in-memory twin of `legaia_xa::demux::demux_disc_range`. Sectors whose
/// subheader copies disagree, or that are not Form 2 audio, are skipped; a
/// trailing partial sector is ignored.
pub(crate) fn demux_raw_sectors(sectors: &[u8]) -> Vec<DemuxedChannel> {
    let mut by_key: BTreeMap<(u8, u8), DemuxedChannel> = BTreeMap::new();
    for raw in sectors.as_chunks::<RAW_SECTOR_BYTES>().0 {
        let mut sub_bytes = [0u8; 8];
        sub_bytes.copy_from_slice(&raw[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 8]);
        let (sub, ok) = parse_subheader(&sub_bytes);
        if !ok || !sub.is_audio() || !sub.is_form2() {
            continue;
        }
        let stream = by_key
            .entry((sub.file_no, sub.ch_no))
            .or_insert_with(|| DemuxedChannel {
                ch_no: sub.ch_no,
                sample_rate: sub.sample_rate(),
                stereo: sub.is_stereo(),
                bits_per_sample: sub.bits_per_sample(),
                audio: Vec::new(),
            });
        stream
            .audio
            .extend_from_slice(&raw[USER_DATA_OFFSET..USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR]);
    }
    by_key.into_values().collect()
}

/// Decode one demuxed 4-bit channel to PCM at its subheader rate.
fn decode_channel(ch: &DemuxedChannel) -> Option<Vec<i16>> {
    if ch.bits_per_sample != 4 {
        return None;
    }
    let channels = if ch.stereo {
        legaia_xa::Channels::Stereo
    } else {
        legaia_xa::Channels::Mono
    };
    let (pcm, _) = legaia_xa::decode(
        &ch.audio,
        legaia_xa::DecodeOptions {
            channels,
            sample_rate: ch.sample_rate,
            bits: legaia_xa::BitsPerSample::Four,
        },
    )
    .ok()?;
    Some(pcm)
}

/// Trim the trailing channel-padding silence off a decoded shout.
pub(crate) fn trim_trailing_silence(pcm: &mut Vec<i16>) {
    let mut end = pcm.len();
    while end > 0 && pcm[end - 1].unsigned_abs() < SHOUT_TAIL_SILENCE {
        end -= 1;
    }
    pcm.truncate(end);
}

/// Build one character's shout clips from its file's raw sectors: every
/// 4-bit **mono** channel (a stereo or 8-bit stream here would be a
/// mis-identified file), decoded and tail-trimmed, keyed by channel. Empty
/// channels are dropped. The native `read_arts_shout_bank` per-file body.
pub(crate) fn build_shout_channels(sectors: &[u8]) -> Vec<(u8, ShoutClip)> {
    demux_raw_sectors(sectors)
        .iter()
        .filter(|s| !s.stereo && s.bits_per_sample == 4)
        .filter_map(|s| {
            let mut pcm = decode_channel(s)?;
            trim_trailing_silence(&mut pcm);
            (!pcm.is_empty()).then_some((
                s.ch_no,
                ShoutClip {
                    pcm,
                    sample_rate: s.sample_rate,
                },
            ))
        })
        .collect()
}

/// Build one clip slot's channels from its file's raw sectors: every 4-bit
/// channel, mono or stereo at its subheader rate, plus the interleave width
/// (`widest channel + 1`, so the retail read span can be divided by it even
/// when a channel was skipped). The native `read_battle_xa_clip_bank`
/// per-file body.
pub(crate) fn build_clip_channels(sectors: &[u8]) -> (u8, Vec<(u8, XaClip)>) {
    let streams = demux_raw_sectors(sectors);
    let widest = streams.iter().map(|s| s.ch_no).max().unwrap_or(0);
    let clips = streams
        .iter()
        .filter_map(|s| {
            let pcm = decode_channel(s)?;
            (!pcm.is_empty()).then_some((
                s.ch_no,
                XaClip {
                    pcm,
                    sample_rate: s.sample_rate,
                    stereo: s.stereo,
                },
            ))
        })
        .collect();
    (widest.saturating_add(1), clips)
}

/// Normalise a page-supplied path (`XA/XA2.XA;1`, `xa2.xa`) to the bare
/// upper-case file name the tables are keyed by.
fn file_key(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let base = base.split(';').next().unwrap_or(base);
    base.trim().to_ascii_uppercase()
}

impl LegaiaRuntime {
    /// The `SCUS_942.54` arts-voice cue tables, parsed fresh from the
    /// executable the runtime kept at `load_disc`. `None` on a
    /// `PROT.DAT`-only load, where every art is unvoiced.
    fn arts_voice_table(&self) -> Option<ArtsVoiceTable> {
        ArtsVoiceTable::parse_from_scus(self.scus.as_deref()?)
    }

    /// Fire the Tactical-Arts shout for `(cslot, action_constant)` through
    /// the XA mixing path - the twin of the native
    /// `AudioBgmDirector::play_art_shout`. Resolves the cue against the
    /// bank's channel pools (retail `FUN_8004C140`'s pick, no immediate
    /// repeat) and stages the clip with the modelled CD-response delay so the
    /// shout starts *after* the art animation that requested it. A second
    /// shout while one is sounding queues behind it. Returns the fired
    /// channel, or `None` when the bank is absent or the art is unvoiced.
    /// Off wasm the resolution runs and is counted; nothing sounds.
    pub(crate) fn play_art_shout(&mut self, cslot: u8, action: u8) -> Option<u8> {
        let Some(bank) = self.sfx.xa.shout_bank.as_mut() else {
            self.sfx.xa.shouts_unvoiced += 1;
            return None;
        };
        let Some((channel, clip)) = bank.shout(cslot, action) else {
            self.sfx.xa.shouts_unvoiced += 1;
            return None;
        };
        #[cfg(target_arch = "wasm32")]
        if let Some(out) = self.audio_out.as_ref() {
            out.play_xa_shout(
                clip.pcm.clone(),
                clip.sample_rate,
                legaia_xa::Channels::Mono,
                XA_GAIN_UNITY,
                SHOUT_CD_RESPONSE_DELAY,
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (clip, XA_GAIN_UNITY, SHOUT_CD_RESPONSE_DELAY);
        self.sfx.xa.shouts_fired += 1;
        self.sfx.xa.last_shout = Some((cslot, action, channel));
        Some(channel)
    }

    /// Play one CD-XA clip request - the engine's `FUN_8003D53C(clip,
    /// channel, dur)`, the twin of the native `AudioBgmDirector::play_xa_clip`:
    /// the staged `(slot, channel)` PCM cut at the retail read span
    /// (`XaClipBank::cut_frames`), through the same XA path as the shouts
    /// with the same start delay.
    ///
    /// A `(slot, channel)` the bank does not hold whose file is on the disc
    /// is **deferred**: the request is queued for the page to slice the
    /// file's read span out of its disc bytes
    /// ([`Self::play_xa_stage_requests_json`]) and replays when the span
    /// installs ([`Self::play_xa_install_span`]) - the cast voices' path,
    /// the native window's disc read done by the page instead. Returns
    /// `false` when nothing plays this call (deferred, or no such file).
    // REF: FUN_8003D53C
    pub(crate) fn play_xa_clip(
        &mut self,
        clip_slot: u32,
        channel: u32,
        duration_sectors: u32,
    ) -> bool {
        let (Ok(slot), Ok(ch)) = (u8::try_from(clip_slot), u8::try_from(channel)) else {
            self.sfx.xa.clips_unstaged += 1;
            return false;
        };
        let Some(pcm) = self
            .sfx
            .xa
            .clip_bank
            .as_ref()
            .and_then(|b| cut_clip(b, slot, ch, duration_sectors))
        else {
            if self.defer_xa_clip(slot, ch, duration_sectors) {
                self.sfx.xa.clips_deferred += 1;
            } else {
                self.sfx.xa.clips_unstaged += 1;
            }
            return false;
        };
        let frames = (if pcm.stereo {
            pcm.pcm.len() / 2
        } else {
            pcm.pcm.len()
        }) as u32;
        #[cfg(target_arch = "wasm32")]
        if let Some(out) = self.audio_out.as_ref() {
            let channels = if pcm.stereo {
                legaia_xa::Channels::Stereo
            } else {
                legaia_xa::Channels::Mono
            };
            out.play_xa_shout(
                pcm.pcm,
                pcm.sample_rate,
                channels,
                XA_GAIN_UNITY,
                SHOUT_CD_RESPONSE_DELAY,
            );
        }
        self.sfx.xa.clips_fired += 1;
        self.sfx.xa.last_clip = Some((slot, ch, frames));
        true
    }
}

impl LegaiaRuntime {
    /// Queue a lazy staging request for `(slot, ch)` when the disc carries
    /// `XA<slot + 1>.XA`: the file's first `read_span_sectors(dur)` sectors
    /// (capped at the file), the span the clip starter would have read.
    /// One request per `(slot, ch)` at a time. `false` when the disc has no
    /// such file (or no disc is loaded).
    fn defer_xa_clip(&mut self, slot: u8, ch: u8, duration_sectors: u32) -> bool {
        let name = format!("XA{}.XA", u32::from(slot) + 1);
        let Some(f) = self.disc_files.iter().find(|f| file_key(&f.path) == name) else {
            return false;
        };
        let file_sectors = f.size.div_ceil(2048);
        let sectors = legaia_engine_audio::xa_clip_bank::read_span_sectors(duration_sectors)
            .min(file_sectors)
            .max(1);
        let req = PendingXaStage {
            path: f.path.clone(),
            lba: f.lba,
            sectors,
            slot,
            channel: ch,
            duration_sectors,
        };
        let pending = &mut self.sfx.xa.pending_stage;
        if let Some(p) = pending
            .iter_mut()
            .find(|p| p.slot == slot && p.channel == ch)
        {
            // Re-requested before the page served it: keep the wider span
            // and the latest request.
            p.sectors = p.sectors.max(sectors);
            p.duration_sectors = duration_sectors;
        } else {
            pending.push(req);
        }
        true
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The lazy staging requests the page should serve now, as a JSON
    /// array of `{ "path", "lba", "sectors", "slot", "channel" }`. For each,
    /// the page slices `bytes.subarray(lba * 2352, (lba + sectors) * 2352)`
    /// out of the disc bytes it holds and calls
    /// [`Self::play_xa_install_span`] with the same `path` and `channel`.
    /// A request stays listed until served, so a page that polls every
    /// frame sees each one until it installs.
    pub fn play_xa_stage_requests_json(&self) -> String {
        let reqs: Vec<serde_json::Value> = self
            .sfx
            .xa
            .pending_stage
            .iter()
            .map(|p| {
                serde_json::json!({
                    "path": p.path,
                    "lba": p.lba,
                    "sectors": p.sectors,
                    "slot": p.slot,
                    "channel": p.channel,
                })
            })
            .collect();
        serde_json::json!(reqs).to_string()
    }

    /// Install one channel of one file from the raw sectors the page
    /// sliced for a [`Self::play_xa_stage_requests_json`] request: demux +
    /// decode that channel only, stage it lazily under the bank's cap, and
    /// replay the clip request that asked for it (through the same XA path,
    /// with the modelled CD-response delay). Returns whether the channel
    /// decoded. The sectors are borrowed for the call only.
    pub fn play_xa_install_span(&mut self, path: &str, sectors: &[u8], channel: u32) -> bool {
        let key = file_key(path);
        let Ok(ch) = u8::try_from(channel) else {
            return false;
        };
        let Some(pos) = self
            .sfx
            .xa
            .pending_stage
            .iter()
            .position(|p| file_key(&p.path) == key && p.channel == ch)
        else {
            return false;
        };
        let req = self.sfx.xa.pending_stage.remove(pos);
        let Some((clip, width)) =
            legaia_engine_audio::xa_clip_bank::decode_channel_span(sectors, ch)
        else {
            self.sfx.xa.clips_unstaged += 1;
            return false;
        };
        self.sfx
            .xa
            .clip_bank
            .get_or_insert_with(XaClipBank::new)
            .insert_lazy(req.slot, ch, clip, width);
        self.sfx.xa.lazy_installed += 1;
        self.play_xa_clip(u32::from(req.slot), u32::from(ch), req.duration_sectors);
        true
    }
}

/// The staged clip cut at the retail read span, or `None` when it is not
/// in the bank / cuts to nothing.
fn cut_clip(bank: &XaClipBank, slot: u8, ch: u8, duration_sectors: u32) -> Option<XaClip> {
    let clip = bank.clip(slot, ch)?;
    let frames = bank
        .cut_frames(slot, ch, duration_sectors)
        .unwrap_or(0)
        .max(1);
    let take = if clip.stereo { frames * 2 } else { frames };
    let pcm = clip.pcm[..take.min(clip.pcm.len())].to_vec();
    (!pcm.is_empty()).then_some(XaClip {
        pcm,
        sample_rate: clip.sample_rate,
        stereo: clip.stereo,
    })
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The XA files this lane wants, as a JSON string array in install
    /// order - `["XA/XA2.XA", ..., "XA/XA30.XA"]` once a disc is loaded,
    /// each spelled as the ISO path `disc_file_extent_json` resolves (the
    /// files sit in the disc's `XA/` directory; the bare names are returned
    /// before a disc is loaded). The page resolves each through
    /// `disc_file_extent_json`, slices its disc bytes at `lba * 2352` for
    /// `ceil(size / 2048) * 2352` bytes, and hands the slice to
    /// [`Self::play_xa_install`] under the same path. Derived from the same
    /// tables the native boot reads (`legaia_art::arts_voice::clip_file` and
    /// [`BATTLE_XA_CLIP_SLOTS`]), never hand-listed.
    pub fn play_xa_wanted_files_json(&self) -> String {
        let resolved: Vec<String> = wanted_files()
            .into_iter()
            .map(|name| {
                self.disc_files
                    .iter()
                    .find(|f| file_key(&f.path) == name)
                    .map(|f| f.path.clone())
                    .unwrap_or_else(|| name.to_string())
            })
            .collect();
        serde_json::json!(resolved).to_string()
    }

    /// Install one XA file from its raw 2352-byte sectors: demux per
    /// channel, decode to PCM, and stage into the shout bank (for
    /// `XA2` / `XA4` / `XA6`, with the executable's cue pools) or the clip
    /// bank (for the [`BATTLE_XA_CLIP_SLOTS`] files). The sectors are
    /// borrowed for the call only; what stays resident is the decoded PCM.
    /// Returns `true` when at least one channel decoded; `false` for a file
    /// the lane does not want, or one that yields no audio channel.
    /// Installing a file twice replaces its channels.
    pub fn play_xa_install(&mut self, path: &str, sectors: &[u8]) -> bool {
        let key = file_key(path);
        if let Some((cslot, _)) = shout_files().find(|(_, f)| *f == key) {
            let clips = build_shout_channels(sectors);
            if clips.is_empty() {
                return false;
            }
            let pools: Vec<(u8, Vec<u8>)> = self
                .arts_voice_table()
                .map(|t| {
                    t.pools(cslot as usize)
                        .map(|(a, p)| (a, p.to_vec()))
                        .collect()
                })
                .unwrap_or_default();
            let bank = self
                .sfx
                .xa
                .shout_bank
                .get_or_insert_with(ArtsShoutBank::new);
            let n = clips.len() as u32;
            for (ch, clip) in clips {
                bank.insert_clip(cslot, ch, clip);
            }
            for (action, pool) in pools {
                bank.set_pool(cslot, action, pool);
            }
            self.sfx.xa.installed.insert(key, n);
            return true;
        }
        if let Some((slot, _)) = BATTLE_XA_CLIP_SLOTS.iter().find(|(_, f)| *f == key) {
            let (width, clips) = build_clip_channels(sectors);
            if clips.is_empty() {
                return false;
            }
            let bank = self.sfx.xa.clip_bank.get_or_insert_with(XaClipBank::new);
            bank.set_channel_count(*slot, width);
            let n = clips.len() as u32;
            for (ch, clip) in clips {
                bank.insert(*slot, ch, clip);
            }
            self.sfx.xa.installed.insert(key, n);
            return true;
        }
        false
    }

    /// The lane's state for the page's readout:
    ///
    /// ```json
    /// { "wanted": ["XA2.XA", ...], "installed": { "XA2.XA": 16 },
    ///   "shout_bank": true, "clip_bank": false, "voice_tables": true,
    ///   "shouts_fired": 3, "shouts_unvoiced": 0, "clips_fired": 5,
    ///   "clips_unstaged": 0, "last_shout": [0, 39, 6],
    ///   "last_clip": [29, 3, 24192], "xa_active": false }
    /// ```
    ///
    /// `voice_tables` says whether the executable's cue pools decoded (a
    /// shout bank without them is all clips and no way to pick one);
    /// `xa_active` is `null` when audio is not up.
    pub fn play_xa_state_json(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        let active = self.audio_out.as_ref().map(|o| o.xa_active());
        #[cfg(not(target_arch = "wasm32"))]
        let active: Option<bool> = None;
        let xa = &self.sfx.xa;
        serde_json::json!({
            "wanted": wanted_files(),
            "installed": xa.installed,
            "shout_bank": xa.shout_bank.as_ref().is_some_and(|b| b.has_clips()),
            "clip_bank": xa.clip_bank.as_ref().is_some_and(|b| b.has_clips()),
            "voice_tables": self.arts_voice_table().is_some(),
            "shouts_fired": xa.shouts_fired,
            "shouts_unvoiced": xa.shouts_unvoiced,
            "clips_fired": xa.clips_fired,
            "clips_unstaged": xa.clips_unstaged,
            "clips_deferred": xa.clips_deferred,
            "lazy_installed": xa.lazy_installed,
            "pending_stage": xa.pending_stage.len(),
            "last_shout": xa.last_shout.map(|(c, a, ch)| [c, a, ch]),
            "last_clip": xa.last_clip.map(|(s, c, f)| [u32::from(s), u32::from(c), f]),
            "xa_active": active,
        })
        .to_string()
    }

    /// **Diagnostic**: the peak absolute sample of the shout clip
    /// `(cslot, action_constant)` resolves to, `0` when the art is unvoiced
    /// or the bank is absent. Resolves on a copy of the bank so the
    /// no-immediate-repeat state of the live one is untouched. Non-zero is
    /// the evidence that an installed file decoded to real audio rather
    /// than to silence.
    pub fn play_xa_probe_shout_peak(&self, cslot: u32, action: u32) -> u32 {
        let (Ok(cslot), Ok(action)) = (u8::try_from(cslot), u8::try_from(action)) else {
            return 0;
        };
        let Some(mut bank) = self.sfx.xa.shout_bank.clone() else {
            return 0;
        };
        bank.shout(cslot, action)
            .map(|(_, clip)| {
                clip.pcm
                    .iter()
                    .map(|s| u32::from(s.unsigned_abs()))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0)
    }

    /// **Diagnostic** sibling for the clip bank: the frames a
    /// `(slot, channel, duration_sectors)` request would play after the
    /// retail read-span cut, `0` when the clip is not staged.
    pub fn play_xa_probe_clip_frames(&self, slot: u32, channel: u32, duration_sectors: u32) -> u32 {
        let (Ok(slot), Ok(ch)) = (u8::try_from(slot), u8::try_from(channel)) else {
            return 0;
        };
        self.sfx
            .xa
            .clip_bank
            .as_ref()
            .and_then(|b| cut_clip(b, slot, ch, duration_sectors))
            .map(|c| {
                (if c.stereo {
                    c.pcm.len() / 2
                } else {
                    c.pcm.len()
                }) as u32
            })
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One synthetic raw Mode 2 Form 2 audio sector for `(file, ch)` with
    /// the given coding byte and a constant audio fill.
    fn sector(file_no: u8, ch_no: u8, coding: u8, fill: u8) -> Vec<u8> {
        let mut s = vec![0u8; RAW_SECTOR_BYTES];
        // submode: audio (0x04) | form 2 (0x20) | real-time (0x40).
        let sub = [file_no, ch_no, 0x64, coding];
        s[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 4].copy_from_slice(&sub);
        s[SUBHEADER_OFFSET + 4..SUBHEADER_OFFSET + 8].copy_from_slice(&sub);
        for b in &mut s[USER_DATA_OFFSET..USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR] {
            *b = fill;
        }
        s
    }

    /// The wanted list is the native boot's staging set, in order, and every
    /// clip slot names `XA<slot+1>.XA` - the clip table's own construction.
    #[test]
    fn wanted_files_are_the_native_staging_set() {
        assert_eq!(
            wanted_files(),
            vec!["XA2.XA", "XA4.XA", "XA6.XA", "XA27.XA", "XA30.XA"]
        );
        for &(slot, file) in BATTLE_XA_CLIP_SLOTS {
            assert_eq!(file, format!("XA{}.XA", u32::from(slot) + 1));
        }
        assert_eq!(BATTLE_XA_CLIP_SLOTS, &[(26, "XA27.XA"), (0x1D, "XA30.XA")]);
    }

    /// The in-memory demux groups sectors per channel, honours the
    /// subheader's coding parameters, and skips what is not Form 2 audio or
    /// whose redundant subheader copy disagrees.
    #[test]
    fn demux_groups_per_channel_and_skips_non_audio() {
        let mut run = Vec::new();
        run.extend(sector(1, 0, 0x00, 0x11)); // 37.8 kHz mono 4-bit
        run.extend(sector(1, 3, 0x05, 0x22)); // 18.9 kHz stereo 4-bit
        run.extend(sector(1, 0, 0x00, 0x33)); // second sector of channel 0
        // Not audio: submode without the audio bit.
        let mut data = sector(1, 5, 0x00, 0x44);
        data[SUBHEADER_OFFSET + 2] = 0x08;
        data[SUBHEADER_OFFSET + 6] = 0x08;
        run.extend(data);
        // Redundant copy mismatch.
        let mut bad = sector(1, 6, 0x00, 0x55);
        bad[SUBHEADER_OFFSET + 5] = 7;
        run.extend(bad);
        // Trailing partial sector is ignored.
        run.extend([0u8; 100]);

        let streams = demux_raw_sectors(&run);
        assert_eq!(streams.len(), 2);
        let ch0 = streams.iter().find(|s| s.ch_no == 0).unwrap();
        assert_eq!(ch0.audio.len(), 2 * AUDIO_BYTES_PER_SECTOR);
        assert_eq!(ch0.sample_rate, 37_800);
        assert!(!ch0.stereo);
        assert_eq!(ch0.audio[0], 0x11);
        assert_eq!(ch0.audio[AUDIO_BYTES_PER_SECTOR], 0x33);
        let ch3 = streams.iter().find(|s| s.ch_no == 3).unwrap();
        assert_eq!(ch3.sample_rate, 18_900);
        assert!(ch3.stereo);
        assert_eq!(ch3.bits_per_sample, 4);
    }

    /// A shout channel that decodes to silence is dropped rather than
    /// staged as an empty clip; a stereo channel is not a shout.
    #[test]
    fn silent_and_stereo_channels_never_become_shouts() {
        let mut run = Vec::new();
        run.extend(sector(1, 0, 0x00, 0x00)); // all-zero groups -> silence
        run.extend(sector(1, 1, 0x01, 0x00)); // stereo
        assert!(build_shout_channels(&run).is_empty());
        let mut pcm = vec![100, 3, -2, 0, 0];
        trim_trailing_silence(&mut pcm);
        assert_eq!(pcm, vec![100]);
    }

    /// The clip builder records the interleave width from the widest channel
    /// seen, and - unlike the shout builder - keeps a silent clip: the
    /// native `read_battle_xa_clip_bank` trims nothing, so the retail
    /// read-span cut stays measured against the full channel length.
    #[test]
    fn clip_builder_reports_interleave_width_and_keeps_silent_clips() {
        let mut run = Vec::new();
        run.extend(sector(1, 0, 0x00, 0x00));
        run.extend(sector(1, 9, 0x00, 0x00));
        let (width, clips) = build_clip_channels(&run);
        assert_eq!(width, 10);
        assert_eq!(clips.len(), 2);
        assert!(clips.iter().all(|(_, c)| !c.pcm.is_empty() && !c.stereo));
        // An 8-bit channel is refused (the decoder is 4-bit only) but still
        // widens the interleave.
        let mut eight = sector(1, 12, 0x10, 0x00);
        eight[SUBHEADER_OFFSET + 3] = 0x10;
        run.extend(eight);
        let (width, clips) = build_clip_channels(&run);
        assert_eq!(width, 13);
        assert_eq!(clips.len(), 2);
    }

    /// Paths arrive in whatever shape the page has them.
    #[test]
    fn file_keys_normalise_page_paths() {
        assert_eq!(file_key("XA/XA2.XA;1"), "XA2.XA");
        assert_eq!(file_key("xa27.xa"), "XA27.XA");
        assert_eq!(file_key("/XA30.XA"), "XA30.XA");
    }

    /// A clip whose file the disc carries is deferred to the lazy tier:
    /// one request per `(slot, channel)` naming the starter's read span,
    /// listed until the page serves it; installing the span decodes only
    /// that channel and replays the request. The cast voices' path.
    #[test]
    fn a_clip_the_disc_carries_is_deferred_then_replayed_when_its_span_installs() {
        let mut rt = LegaiaRuntime::new();
        rt.disc_files.push(crate::disc::FileEntry {
            path: "XA/XA7.XA;1".into(),
            lba: 1000,
            size: 8 * 2048 * 300,
        });
        // Gimard: slot 6 channel 4, span 686 vsyncs -> 1717 sectors.
        assert!(!rt.play_xa_clip(6, 4, 686), "nothing plays this call");
        let v: serde_json::Value = serde_json::from_str(&rt.play_xa_state_json()).unwrap();
        assert_eq!(v["clips_deferred"], 1);
        assert_eq!(v["clips_unstaged"], 0);
        assert_eq!(v["pending_stage"], 1);
        let reqs: serde_json::Value =
            serde_json::from_str(&rt.play_xa_stage_requests_json()).unwrap();
        assert_eq!(reqs[0]["path"], "XA/XA7.XA;1");
        assert_eq!(reqs[0]["lba"], 1000);
        assert_eq!(reqs[0]["sectors"], (686 * 150 + 149) / 60);
        assert_eq!(reqs[0]["slot"], 6);
        assert_eq!(reqs[0]["channel"], 4);
        // Re-requesting before the page serves it does not duplicate.
        assert!(!rt.play_xa_clip(6, 4, 700));
        assert_eq!(rt.sfx.xa.pending_stage.len(), 1);
        assert_eq!(rt.sfx.xa.pending_stage[0].duration_sectors, 700);
        // A slot the disc has no file for is unstaged, not deferred.
        assert!(!rt.play_xa_clip(0x21, 0, 60));
        assert_eq!(rt.sfx.xa.clips_unstaged, 1);
        assert_eq!(rt.sfx.xa.pending_stage.len(), 1);

        // The page serves the request: an 8-channel interleave with real
        // audio on channel 4.
        let mut run = Vec::new();
        for _ in 0..4 {
            for ch in 0..8u8 {
                run.extend(sector(1, ch, 0x00, if ch == 4 { 0x77 } else { 0x00 }));
            }
        }
        assert!(rt.play_xa_install_span("XA/XA7.XA;1", &run, 4));
        assert!(rt.sfx.xa.pending_stage.is_empty(), "served");
        assert_eq!(rt.sfx.xa.lazy_installed, 1);
        assert_eq!(rt.sfx.xa.clips_fired, 1, "the deferred request replayed");
        assert_eq!(rt.sfx.xa.last_clip.map(|(s, c, _)| (s, c)), Some((6, 4)));
        let bank = rt.sfx.xa.clip_bank.as_ref().unwrap();
        assert!(bank.is_staged(6, 4));
        assert_eq!(bank.channel_count(6), 8);
        // Staged now: the next request plays at once.
        assert!(rt.play_xa_clip(6, 4, 686));
        // An install nobody asked for is refused.
        assert!(!rt.play_xa_install_span("XA/XA7.XA;1", &run, 5));
    }

    /// The request counters are the readout: a shout with no bank is
    /// counted as unvoiced, a clip with no bank as unstaged, and neither
    /// panics without audio.
    #[test]
    fn requests_without_banks_are_counted_not_dropped_silently() {
        let mut rt = LegaiaRuntime::new();
        assert!(rt.play_art_shout(0, 0x27).is_none());
        assert!(!rt.play_xa_clip(26, 0, 60));
        let v: serde_json::Value = serde_json::from_str(&rt.play_xa_state_json()).unwrap();
        assert_eq!(v["shouts_unvoiced"], 1);
        assert_eq!(v["clips_unstaged"], 1);
        assert_eq!(v["shout_bank"], false);
        assert!(!rt.play_xa_install("XA99.XA", &[]), "unwanted file");
        assert!(!rt.play_xa_install("XA2.XA", &[]), "no sectors, no channel");
    }
}
