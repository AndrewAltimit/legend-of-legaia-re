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
//! hosts stage identical banks from identical sectors; this module only
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
//! window's `AudioDirector`), and every request - a battle's clips and
//! shouts, a field line, a minigame announcer - reaches it through the
//! session steps the native window runs (`BootSession::route_battle_audio`,
//! its field SFX routing, `sound_muscle_hub`, `sound_baka_announcer`), so
//! the modelled CD-response start delay, the one-deep back-to-back queue and
//! the mix point **before** the page's post-mixer gain are the director's on
//! both hosts.
//!
//! What this module keeps is the page's side of **staging**. The native
//! director reads a clip the bank lacks straight off the disc image; the
//! page's disc bytes live on the JavaScript side, so its director is told
//! where each `XA<n>.XA` sits (`AudioDirector::set_xa_file_extents`) and
//! *defers* such a clip: it lists the span it wants
//! ([`LegaiaRuntime::play_xa_stage_requests_json`]), the page slices those
//! sectors and hands them back ([`LegaiaRuntime::play_xa_install_span`]),
//! and the director plays the waiting request. The banks installed before a
//! director exists live here until one takes them.
//!
//! REF: FUN_8004C140 (arts-voice cue selector), FUN_8003D53C (CD-XA clip
//! starter both banks stand in for).

use crate::runtime::LegaiaRuntime;
use legaia_art::arts_voice::ArtsVoiceTable;
#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::AudioSink;
use legaia_engine_audio::{ArtsShoutBank, XaClipBank};
use legaia_engine_session::audio_director::XaLaneStats;
use legaia_engine_session::xa_banks::{
    XaBankFile, bank_file, clip_slot_of, file_key, wanted_files,
};
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

/// The page's side of the XA lane: the banks installed before a director
/// exists, and what was installed.
#[derive(Default)]
pub struct PlayXa {
    /// Arts-voice shout bank; `None` until a shout file installs, and again
    /// once a director has taken it.
    pub shout_bank: Option<ArtsShoutBank>,
    /// Battle one-shot clip bank; `None` until a clip file installs, and
    /// again once a director has taken it.
    pub clip_bank: Option<XaClipBank>,
    /// Installed file -> decoded channel count.
    pub installed: BTreeMap<String, u32>,
}

impl LegaiaRuntime {
    /// The `SCUS_942.54` arts-voice cue tables, parsed fresh from the
    /// executable the runtime kept at `load_disc`. `None` on a
    /// `PROT.DAT`-only load, where every art is unvoiced.
    fn arts_voice_table(&self) -> Option<ArtsVoiceTable> {
        ArtsVoiceTable::parse_from_scus(self.scus.as_deref()?)
    }

    /// Where each `XA<n>.XA` sits on the loaded disc, as the director's
    /// `(clip slot, first sector, sector count)` rows. Empty before a disc
    /// is loaded.
    pub(crate) fn xa_file_extents(&self) -> Vec<(u8, u32, u32)> {
        self.disc_files
            .iter()
            .filter_map(|f| Some((clip_slot_of(&f.path)?, f.lba, f.size.div_ceil(2048))))
            .collect()
    }

    /// Hand the director the loaded disc's XA file extents, so a clip its
    /// bank lacks is deferred to the page rather than dropped. Called when a
    /// director is built and when a disc loads under an existing one.
    pub(crate) fn sync_xa_file_extents(&mut self) {
        let extents = self.xa_file_extents();
        if let Some(d) = self.scene_host.director_mut() {
            d.set_xa_file_extents(extents);
        }
    }

    /// The lane's request counters: the director's, zero before one exists.
    fn xa_stats(&self) -> XaLaneStats {
        self.scene_host
            .director()
            .map(|d| d.xa_stats().clone())
            .unwrap_or_default()
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

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The lazy staging requests the page should serve now, as a JSON
    /// array of `{ "path", "lba", "sectors", "slot", "channel" }` - the
    /// director's own list (`AudioDirector::xa_stage_requests`), each row
    /// named by the ISO path `disc_file_extent_json` resolves. For each,
    /// the page slices `bytes.subarray(lba * 2352, (lba + sectors) * 2352)`
    /// out of the disc bytes it holds and calls
    /// [`Self::play_xa_install_span`] with the same `path` and `channel`.
    /// A request stays listed until served, so a page that polls every
    /// frame sees each one until it installs. Empty before audio is up.
    pub fn play_xa_stage_requests_json(&self) -> String {
        let reqs: Vec<serde_json::Value> = self
            .scene_host
            .director()
            .map(|d| d.xa_stage_requests())
            .unwrap_or_default()
            .iter()
            .map(|p| {
                let path = self
                    .disc_files
                    .iter()
                    .find(|f| clip_slot_of(&f.path) == Some(p.slot))
                    .map(|f| f.path.clone())
                    .unwrap_or_else(|| format!("XA{}.XA", u32::from(p.slot) + 1));
                serde_json::json!({
                    "path": path,
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
    /// sliced for a [`Self::play_xa_stage_requests_json`] request
    /// (`AudioDirector::install_xa_span`): demux + decode that channel only,
    /// stage it lazily under the bank's cap, and play the clip request that
    /// asked for it (through the same XA path, with the modelled CD-response
    /// delay). Returns whether the channel decoded. The sectors are borrowed
    /// for the call only.
    pub fn play_xa_install_span(&mut self, path: &str, sectors: &[u8], channel: u32) -> bool {
        let (Some(slot), Ok(ch)) = (clip_slot_of(path), u8::try_from(channel)) else {
            return false;
        };
        self.scene_host
            .director_mut()
            .is_some_and(|d| d.install_xa_span(slot, ch, sectors))
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The XA files this module wants, as a JSON string array in install
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
        let xa = self.xa_stats();
        let pending = self
            .scene_host
            .director()
            .map_or(0, |d| d.xa_stage_requests().len());
        serde_json::json!({
            "wanted": wanted_files(),
            "installed": self.sfx.xa.installed,
            "shout_bank": self.xa_shout_bank().is_some_and(|b| b.has_clips()),
            "clip_bank": self.xa_clip_bank().is_some_and(|b| b.has_clips()),
            "voice_tables": self.arts_voice_table().is_some(),
            "shouts_fired": xa.shouts_fired,
            "shouts_unvoiced": xa.shouts_unvoiced,
            "clips_fired": xa.clips_fired,
            "clips_unstaged": xa.clips_unstaged,
            "clips_deferred": xa.clips_deferred,
            "lazy_installed": xa.lazy_installed,
            "pending_stage": pending,
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
    use legaia_xa::demux::{
        AUDIO_BYTES_PER_SECTOR, RAW_SECTOR_BYTES, SUBHEADER_OFFSET, USER_DATA_OFFSET,
    };

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

    /// A runtime whose disc carries `XA7.XA` and whose director knows it.
    fn runtime_with_xa7() -> LegaiaRuntime {
        let mut rt = LegaiaRuntime::new();
        rt.disc_files.push(legaia_iso::image::FileEntry {
            path: "XA/XA7.XA;1".into(),
            lba: 1000,
            size: 8 * 2048 * 300,
        });
        assert!(
            rt.audio_director().is_some(),
            "off wasm: built on first use"
        );
        rt
    }

    /// An 8-channel interleave with real audio on channel 4.
    fn xa7_run() -> Vec<u8> {
        let mut run = Vec::new();
        for _ in 0..4 {
            for ch in 0..8u8 {
                run.extend(sector(1, ch, 0x00, if ch == 4 { 0x77 } else { 0x00 }));
            }
        }
        run
    }

    /// A clip whose file the disc carries is deferred by the director: the
    /// page lists one request per `(slot, channel)` naming the starter's
    /// read span under the file's ISO path, until it serves it; installing
    /// the span decodes only that channel and plays the request. The cast
    /// voices' path.
    #[test]
    fn a_clip_the_disc_carries_is_listed_then_played_when_its_span_installs() {
        let mut rt = runtime_with_xa7();
        // Gimard: slot 6 channel 4, span 686 vsyncs -> 1717 sectors.
        let d = rt.audio_director().unwrap();
        assert!(
            d.play_xa_clip(6, 4, 686).is_none(),
            "nothing plays this call"
        );
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
        // A slot the disc has no file for is unstaged, not deferred.
        let d = rt.audio_director().unwrap();
        assert!(d.play_xa_clip(0x21, 0, 60).is_none());
        assert_eq!(d.xa_stats().clips_unstaged, 1);
        assert_eq!(d.xa_stage_requests().len(), 1);

        // The page serves the request.
        let run = xa7_run();
        assert!(rt.play_xa_install_span("XA/XA7.XA;1", &run, 4));
        let v: serde_json::Value = serde_json::from_str(&rt.play_xa_state_json()).unwrap();
        assert_eq!(v["pending_stage"], 0, "served");
        assert_eq!(v["lazy_installed"], 1);
        assert_eq!(v["clips_fired"], 1, "the deferred request played");
        assert_eq!(v["last_clip"][0], 6);
        assert_eq!(v["last_clip"][1], 4);
        let bank = rt.xa_clip_bank().unwrap();
        assert!(bank.is_staged(6, 4));
        assert_eq!(bank.channel_count(6), 8);
        // An install nobody asked for is refused.
        assert!(!rt.play_xa_install_span("XA/XA7.XA;1", &run, 5));
        assert!(!rt.play_xa_install_span("SCUS_942.54;1", &run, 4));
    }

    /// The director learns the disc's XA extents when it is built, and
    /// again when a disc loads under it.
    #[test]
    fn the_director_is_told_the_disc_xa_extents() {
        let mut rt = LegaiaRuntime::new();
        assert!(rt.xa_file_extents().is_empty());
        // No extent: a request is unstaged, never deferred.
        let d = rt.audio_director().unwrap();
        assert!(d.play_xa_clip(6, 4, 686).is_none());
        assert!(d.xa_stage_requests().is_empty());
        rt.disc_files.push(legaia_iso::image::FileEntry {
            path: "XA/XA7.XA;1".into(),
            lba: 1000,
            size: 2048 * 10,
        });
        assert_eq!(rt.xa_file_extents(), vec![(6, 1000, 10)]);
        rt.sync_xa_file_extents();
        let d = rt.audio_director().unwrap();
        assert!(d.play_xa_clip(6, 4, 686).is_none());
        // Capped at the file.
        assert_eq!(d.xa_stage_requests()[0].sectors, 10);
    }

    #[test]
    fn requests_without_banks_are_counted_not_dropped_silently() {
        let mut rt = LegaiaRuntime::new();
        let d = rt.audio_director().unwrap();
        assert!(d.play_art_shout(0, 0x27).is_none());
        assert!(d.play_xa_clip(26, 0, 60).is_none());
        let v: serde_json::Value = serde_json::from_str(&rt.play_xa_state_json()).unwrap();
        assert_eq!(v["shouts_unvoiced"], 1);
        assert_eq!(v["clips_unstaged"], 1);
        assert_eq!(v["shout_bank"], false);
        assert!(!rt.play_xa_install("XA99.XA", &[]), "unwanted file");
        assert!(!rt.play_xa_install("XA2.XA", &[]), "no sectors, no channel");
    }
}
