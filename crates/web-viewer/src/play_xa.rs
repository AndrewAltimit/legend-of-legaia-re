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
//! The file list and the per-file staging are the engine's,
//! `legaia_engine_session::xa_banks` - the same calls the native boot's
//! `read_arts_shout_bank` / `read_battle_xa_clip_bank` make, so the two
//! hosts stage identical banks from identical sectors; this lane only
//! demuxes the page's slice (`legaia_xa::demux::demux_raw_sectors`) and
//! hands the channels over.
//!
//! * **Shouts** - `XA2.XA` Vahn / `XA4.XA` Noa / `XA6.XA` Gala, 16-channel
//!   short-mono banks, into an [`ArtsShoutBank`] whose per-art candidate
//!   pools come from the `SCUS_942.54` cue tables (`FUN_8004C140`'s data,
//!   `legaia_art::arts_voice::ArtsVoiceTable`).
//! * **Clips** - `xa_banks::BATTLE_XA_CLIP_SLOTS`: the per-character voice
//!   banks the animation cue tracks address (`XA1` / `XA3` / `XA5` and
//!   `XA27` / `XA28` / `XA29` - Vahn's Spirit is `XA1` channel 0) and
//!   `XA30.XA` (slot `0x1D`, the ten mono block grunts), into an
//!   [`XaClipBank`] keyed `(slot, channel)` - the raw space the retail clip
//!   starter `FUN_8003D53C(clip_slot, channel, duration_sectors)` takes.
//!
//! Both banks belong to the page's audio director once it exists (the native
//! window's `AudioBgmDirector`), and both play through its `play_art_shout` /
//! `play_xa_clip` - the native calls, over the page's output: the modelled
//! CD-response start delay so the voice trails the animation, the one-deep
//! back-to-back queue, and a mix point **before** the page's post-mixer gain,
//! so a shout sits against the music at the same ratio as under the native
//! window. What this lane keeps is the page's side: installing the files the
//! page slices off its disc bytes (the banks live here until a director
//! takes them), the deferred span staging a cast voice rides, and the
//! readout's counters.
//!
//! REF: FUN_8004C140 (arts-voice cue selector), FUN_8003D53C (CD-XA clip
//! starter both banks stand in for).

use crate::runtime::LegaiaRuntime;
use legaia_art::arts_voice::ArtsVoiceTable;
#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::AudioSink;
use legaia_engine_audio::{ArtsShoutBank, XaClipBank};
use legaia_engine_session::xa_banks::{XaBankFile, bank_file, file_key, wanted_files};
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

/// One raw Mode 2 sector as the page slices it out of the disc bytes.
pub(crate) const RAW_SECTOR_BYTES: usize = legaia_xa::demux::RAW_SECTOR_BYTES;

/// Unity XA gain (Q1.14), what both native XA players pass.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) const XA_GAIN_UNITY: u16 = 0x4000;

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
    /// Whether a clip request is waiting on this span (play it once the span
    /// installs), or the span was only staged ahead of a cast
    /// ([`LegaiaRuntime::prestage_xa_clip`]).
    pub replay: bool,
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
        // Through the director once one exists (it keys the mixer); before
        // that the lane's own bank resolves the pick and nothing sounds.
        let fired = match self.scene_host.director_mut() {
            Some(d) => d.play_art_shout(cslot, action),
            None => self
                .sfx
                .xa
                .shout_bank
                .as_mut()
                .and_then(|b| b.shout(cslot, action))
                .map(|(channel, _)| channel),
        };
        match fired {
            Some(channel) => {
                self.sfx.xa.shouts_fired += 1;
                self.sfx.xa.last_shout = Some((cslot, action, channel));
            }
            None => self.sfx.xa.shouts_unvoiced += 1,
        }
        fired
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
        let Some(cut) = self
            .xa_clip_bank()
            .and_then(|b| b.cut(slot, ch, duration_sectors))
        else {
            if self.defer_xa_clip(slot, ch, duration_sectors, true) {
                self.sfx.xa.clips_deferred += 1;
            } else {
                self.sfx.xa.clips_unstaged += 1;
            }
            return false;
        };
        let frames = (if cut.stereo {
            cut.pcm.len() / 2
        } else {
            cut.pcm.len()
        }) as u32;
        // The director cuts and plays the same span; before one exists the
        // request is counted and nothing sounds.
        if let Some(d) = self.scene_host.director_mut() {
            let _ = d.play_xa_clip(clip_slot, channel, duration_sectors);
        }
        self.sfx.xa.clips_fired += 1;
        self.sfx.xa.last_clip = Some((slot, ch, frames));
        true
    }

    /// The shout bank: the director's once one exists, the lane's own before.
    pub(crate) fn xa_shout_bank(&self) -> Option<&ArtsShoutBank> {
        match self.scene_host.director() {
            Some(d) => d.shout_bank(),
            None => self.sfx.xa.shout_bank.as_ref(),
        }
    }

    /// The shout bank, created empty on first use.
    fn xa_shout_bank_mut(&mut self) -> &mut ArtsShoutBank {
        match self.scene_host.director_mut() {
            Some(d) => d.shout_bank_mut(),
            None => self
                .sfx
                .xa
                .shout_bank
                .get_or_insert_with(ArtsShoutBank::new),
        }
    }

    /// The clip bank: the director's once one exists, the lane's own before.
    pub(crate) fn xa_clip_bank(&self) -> Option<&XaClipBank> {
        match self.scene_host.director() {
            Some(d) => d.xa_clip_bank(),
            None => self.sfx.xa.clip_bank.as_ref(),
        }
    }

    /// The clip bank, created empty on first use.
    fn xa_clip_bank_mut(&mut self) -> &mut XaClipBank {
        match self.scene_host.director_mut() {
            Some(d) => d.xa_clip_bank_mut(),
            None => self.sfx.xa.clip_bank.get_or_insert_with(XaClipBank::new),
        }
    }
}

impl LegaiaRuntime {
    /// Stage `(clip_slot, channel)` ahead of the cast that will ask for it:
    /// queue a lazy staging request that installs the span **without**
    /// playing it, unless the bank already holds the channel. The engine
    /// lists the round's candidates at the round's start
    /// (`World::drain_battle_xa_prestage`), so by the time the cast raises its
    /// clip the page has sliced and decoded it and [`Self::play_xa_clip`] cuts
    /// it on the same call, as the native window does.
    pub(crate) fn prestage_xa_clip(&mut self, clip_slot: u32, channel: u32, duration_sectors: u32) {
        let (Ok(slot), Ok(ch)) = (u8::try_from(clip_slot), u8::try_from(channel)) else {
            return;
        };
        let held = self
            .xa_clip_bank()
            .is_some_and(|b| b.cut(slot, ch, duration_sectors).is_some());
        if !held {
            self.defer_xa_clip(slot, ch, duration_sectors, false);
        }
    }

    /// Queue a lazy staging request for `(slot, ch)` when the disc carries
    /// `XA<slot + 1>.XA`: the file's first `read_span_sectors(dur)` sectors
    /// (capped at the file), the span the clip starter would have read.
    /// One request per `(slot, ch)` at a time. `false` when the disc has no
    /// such file (or no disc is loaded).
    fn defer_xa_clip(&mut self, slot: u8, ch: u8, duration_sectors: u32, replay: bool) -> bool {
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
            replay,
        };
        let pending = &mut self.sfx.xa.pending_stage;
        if let Some(p) = pending
            .iter_mut()
            .find(|p| p.slot == slot && p.channel == ch)
        {
            // Re-requested before the page served it: keep the wider span
            // and the latest request.
            p.sectors = p.sectors.max(sectors);
            if replay {
                // A live request takes over a prestage one.
                p.duration_sectors = duration_sectors;
                p.replay = true;
            }
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
        self.xa_clip_bank_mut()
            .insert_lazy(req.slot, ch, clip, width);
        self.sfx.xa.lazy_installed += 1;
        if req.replay {
            self.play_xa_clip(u32::from(req.slot), u32::from(ch), req.duration_sectors);
        }
        true
    }
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
    /// tables the native boot reads (`legaia_engine_session::xa_banks`),
    /// never hand-listed.
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
    /// bank (for the `xa_banks::BATTLE_XA_CLIP_SLOTS` files). The sectors are
    /// borrowed for the call only; what stays resident is the decoded PCM.
    /// Returns `true` when at least one channel decoded; `false` for a file
    /// the lane does not want, or one that yields no audio channel.
    /// Installing a file twice replaces its channels.
    pub fn play_xa_install(&mut self, path: &str, sectors: &[u8]) -> bool {
        let Some(file) = bank_file(path) else {
            return false;
        };
        // The staging is the engine's (`legaia_engine_session::xa_banks`),
        // the call the native boot's readers make over the same channels.
        let streams = legaia_xa::demux::demux_raw_sectors(sectors);
        let n = match file {
            XaBankFile::Shout(cslot) => {
                let table = self.arts_voice_table();
                legaia_engine_session::xa_banks::install_shout_file(
                    self.xa_shout_bank_mut(),
                    cslot,
                    &streams,
                    table.as_ref(),
                )
            }
            XaBankFile::Clip(slot) => legaia_engine_session::xa_banks::install_clip_file(
                self.xa_clip_bank_mut(),
                slot,
                &streams,
            ),
        };
        if n == 0 {
            return false;
        }
        self.sfx.xa.installed.insert(file_key(path), n);
        true
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
            "shout_bank": self.xa_shout_bank().is_some_and(|b| b.has_clips()),
            "clip_bank": self.xa_clip_bank().is_some_and(|b| b.has_clips()),
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
        let Some(mut bank) = self.xa_shout_bank().cloned() else {
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
        self.xa_clip_bank()
            .and_then(|b| b.cut(slot, ch, duration_sectors))
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
    use legaia_xa::demux::{AUDIO_BYTES_PER_SECTOR, SUBHEADER_OFFSET, USER_DATA_OFFSET};

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
        let bank = rt.xa_clip_bank().unwrap();
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
    /// A cast voice staged at the round's start installs without playing,
    /// and the cast's own request then plays on the call that raises it -
    /// the native window's timing. Without the prestage the request was
    /// deferred and sounded only after the page served the slice.
    #[test]
    fn a_prestaged_cast_voice_installs_silently_and_plays_on_its_cast() {
        let mut rt = LegaiaRuntime::new();
        rt.disc_files.push(crate::disc::FileEntry {
            path: "XA/XA7.XA;1".into(),
            lba: 1000,
            size: 8 * 2048 * 300,
        });
        rt.prestage_xa_clip(6, 4, 686);
        assert_eq!(rt.sfx.xa.pending_stage.len(), 1);
        assert!(!rt.sfx.xa.pending_stage[0].replay);
        let mut run = Vec::new();
        for _ in 0..4 {
            for ch in 0..8u8 {
                run.extend(sector(1, ch, 0x00, if ch == 4 { 0x77 } else { 0x00 }));
            }
        }
        assert!(rt.play_xa_install_span("XA/XA7.XA;1", &run, 4));
        assert_eq!(rt.sfx.xa.clips_fired, 0, "a prestage plays nothing");
        assert!(
            rt.play_xa_clip(6, 4, 686),
            "the cast's request plays at once"
        );
        assert_eq!(rt.sfx.xa.clips_deferred, 0);
        // Prestaging a held channel queues nothing.
        rt.prestage_xa_clip(6, 4, 686);
        assert!(rt.sfx.xa.pending_stage.is_empty());
    }

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
