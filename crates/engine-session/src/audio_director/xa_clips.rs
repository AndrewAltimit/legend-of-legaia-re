//! CD-XA voice on the director: the arts-shout bank, the one-shot clip bank,
//! and lazy per-channel staging - read straight off a disc image, or asked of
//! a host that slices the sectors itself.

use super::*;

/// Unity XA gain (Q1.14): the level of a stream the CD input passes at full
/// volume (a movie's audio - the STR overlay sets libsnd's serial volume to
/// `127`, register `0x7FFE`).
pub const XA_GAIN_UNITY: u16 = 0x4000;

/// The SPU CD-input volume retail plays every one-shot CD-XA cue at, as the
/// Q1.14 gain the mixer takes: register `0x75EA` (Q15) `= 0x3AF5` (Q1.14),
/// about `0.921` (-0.71 dB).
///
/// The one-shot cue player `FUN_8003D53C` mutes the CD input
/// (`FUN_8003EE7C(0)`) before it seeks; its CD-ready state machine
/// `FUN_8003D764` raises it with `li a0,0x75` / `FUN_8003EE7C` at
/// `0x8003D92C` once the drive reports a read in progress (status bit `0x20`),
/// and mutes it again when the read passes the clip's end. `FUN_8003EE7C(v)`
/// is libsnd `SsSetSerialVol(0, v, v)` (`FUN_800643C4`, register `= v *
/// 0x102`), so `117` lands as `0x75EA`, the value the battle save states hold
/// while a voice is up. Arts shouts, battle grunts, cast voices, field voice
/// lines and the minigame announcers all play through that player.
pub const XA_CUE_GAIN: u16 = 0x3AF5;

/// Where the director reads a CD-XA channel it has not staged: one channel
/// of `XA<slot + 1>.XA` over the clip starter's read span for
/// `duration_sectors`, decoded, as `(clip, interleave width)`. The native
/// host's source reads the disc image
/// (`crate::native::XaDiscSource`).
pub trait XaChannelSource {
    fn channel_span(
        &self,
        slot: u8,
        channel: u8,
        duration_sectors: u32,
    ) -> Option<(legaia_engine_audio::XaClip, u8)>;
}

/// One channel span the director is waiting for: the raw sectors a host
/// that cannot be read synchronously (the browser page holds the disc bytes
/// on the JavaScript side) slices and hands to
/// [`AudioDirector::install_xa_span`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XaStageRequest {
    /// Clip slot: the file is `XA<slot + 1>.XA`.
    pub slot: u8,
    pub channel: u8,
    /// First raw sector of the file.
    pub lba: u32,
    /// Sectors to slice from `lba`: the clip starter's read span, capped at
    /// the file.
    pub sectors: u32,
    /// The span of the request that asked (the latest live one).
    pub duration_sectors: u32,
    /// Whether a clip request is waiting on this span (it plays once the
    /// span installs), or the span was only staged ahead of its request
    /// ([`AudioDirector::prestage_xa_clip`]).
    pub replay: bool,
}

/// The XA lane's request counters, for a host's readout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XaLaneStats {
    /// Shouts that resolved to a clip and were handed to the mixer.
    pub shouts_fired: u32,
    /// Shout requests that resolved to nothing (bank absent, art unvoiced,
    /// channel not decoded) - retail's silent-art degradation.
    pub shouts_unvoiced: u32,
    /// Clip requests handed to the mixer.
    pub clips_fired: u32,
    /// Clip requests queued for the host to slice, which play on install.
    pub clips_deferred: u32,
    /// Clip requests for a `(slot, channel)` nothing can stage.
    pub clips_unstaged: u32,
    /// Channel spans a host installed for a request.
    pub lazy_installed: u32,
    /// The most recent shout: `(cslot, action, channel)`.
    pub last_shout: Option<(u8, u8, u8)>,
    /// The most recent clip: `(slot, channel, frames cut)`.
    pub last_clip: Option<(u8, u8, u32)>,
}

impl<S: AudioSink> AudioDirector<S> {
    /// Hand the director the source lazy staging reads from.
    pub fn set_xa_channel_source(&mut self, source: Box<dyn XaChannelSource + Send + Sync>) {
        self.xa_lazy = Some(source);
    }

    /// Tell the director where each `XA<n>.XA` sits on the disc, as
    /// `(clip slot, first sector, sector count)` - the table the boot filler
    /// `FUN_801CFA78` builds at `0x801C6ED8`. With no synchronous source
    /// ([`Self::set_xa_channel_source`]) a clip the bank does not hold is
    /// then **deferred**: queued on [`Self::xa_stage_requests`] for the host
    /// to slice and install ([`Self::install_xa_span`]).
    pub fn set_xa_file_extents(&mut self, extents: impl IntoIterator<Item = (u8, u32, u32)>) {
        self.xa_extents = extents
            .into_iter()
            .map(|(slot, lba, sectors)| (slot, (lba, sectors)))
            .collect();
    }

    /// Install the arts-voice shout bank (demuxed + decoded from the user's
    /// disc at boot; see [`crate::native::read_arts_shout_bank`]).
    pub fn set_shout_bank(&mut self, bank: ArtsShoutBank) {
        self.shout_bank = Some(bank);
    }

    /// Tell the shout selector the battle's first formation id - the byte
    /// `gp+0x9F4` (`0x8007BD0C`, formation slot 0) `FUN_8004C140` compares
    /// with `0x4F` before it plays (`0x8004C404..0x8004C414`); a match
    /// replaces the drawn channel with `0xC`. The session sets it from the
    /// world's active formation each battle-event step
    /// (`BootSession::route_battle_audio`), ahead of the tick's shouts. A
    /// no-op while no bank is staged.
    pub fn set_shout_first_enemy_id(&mut self, id: Option<u8>) {
        if let Some(bank) = self.shout_bank.as_mut() {
            bank.set_first_enemy_id(id);
        }
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
        self.commands
            .push(super::AudioCommand::Shout(cslot, action));
        let picked = self
            .shout_bank
            .as_mut()
            .and_then(|bank| bank.shout(cslot, action))
            .map(|(channel, clip)| (channel, clip.pcm.clone(), clip.sample_rate));
        let Some((channel, pcm, sample_rate)) = picked else {
            self.xa_stats.shouts_unvoiced += 1;
            return None;
        };
        self.audio.play_xa_shout(
            pcm,
            sample_rate,
            legaia_xa::Channels::Mono,
            XA_CUE_GAIN,
            SHOUT_CD_RESPONSE_DELAY,
        );
        self.xa_stats.shouts_fired += 1;
        self.xa_stats.last_shout = Some((cslot, action, channel));
        Some(channel)
    }

    /// Install the generic CD-XA clip bank (demuxed + decoded from the disc
    /// at boot; see [`crate::native::read_battle_xa_clip_bank`]).
    pub fn set_xa_clip_bank(&mut self, bank: XaClipBank) {
        self.xa_clip_bank = Some(bank);
    }

    /// `true` once a CD-XA clip bank is staged.
    pub fn has_xa_clip_bank(&self) -> bool {
        self.xa_clip_bank.is_some()
    }

    /// `true` once lazy staging has a disc to read from.
    pub fn has_xa_lazy_source(&self) -> bool {
        self.xa_lazy.is_some()
    }

    /// Whether the clip bank holds `(slot, channel)`.
    fn xa_channel_staged(&self, slot: u8, channel: u8) -> bool {
        self.xa_clip_bank
            .as_ref()
            .is_some_and(|b| b.is_staged(slot, channel))
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

    /// Queue a stage request for `(slot, channel)` when the disc carries
    /// `XA<slot + 1>.XA`: the file's first `read_span_sectors(dur)` sectors
    /// (capped at the file), the span the clip starter would have read. One
    /// request per `(slot, channel)` at a time. `false` when no extent is
    /// known for the slot.
    fn defer_xa_channel(
        &mut self,
        slot: u8,
        channel: u8,
        duration_sectors: u32,
        replay: bool,
    ) -> bool {
        let Some(&(lba, file_sectors)) = self.xa_extents.get(&slot) else {
            return false;
        };
        let sectors = legaia_engine_audio::xa_clip_bank::read_span_sectors(duration_sectors)
            .min(file_sectors)
            .max(1);
        if let Some(p) = self
            .xa_pending
            .iter_mut()
            .find(|p| p.slot == slot && p.channel == channel)
        {
            // Re-requested before the host served it: keep the wider span
            // and the latest request.
            p.sectors = p.sectors.max(sectors);
            if replay {
                // A live request takes over a prestage one.
                p.duration_sectors = duration_sectors;
                p.replay = true;
            }
        } else {
            self.xa_pending.push(XaStageRequest {
                slot,
                channel,
                lba,
                sectors,
                duration_sectors,
                replay,
            });
        }
        true
    }

    /// Play one CD-XA clip request - the engine's `FUN_8003D53C(clip,
    /// channel, dur)`: the staged `(slot, channel)` PCM, cut at the retail
    /// read span (`XaClipBank::cut_frames`), through the same XA mixing
    /// path the arts shouts take, with the same modelled CD-response start
    /// delay. A request while a clip is sounding queues behind it (the
    /// mixer's back-to-back path).
    ///
    /// A `(slot, channel)` the bank does not hold is staged first: read off
    /// the disc when a synchronous source is set
    /// ([`Self::set_xa_channel_source`]), otherwise **deferred** when the
    /// file's extent is known ([`Self::set_xa_file_extents`]) - queued for
    /// the host to slice, and played when its span installs
    /// ([`Self::install_xa_span`]). Returns the frames played, `None` when
    /// nothing plays this call.
    // REF: FUN_8003D53C
    pub fn play_xa_clip(
        &mut self,
        clip_slot: u32,
        channel: u32,
        duration_sectors: u32,
    ) -> Option<u32> {
        self.commands.push(super::AudioCommand::XaClip(
            clip_slot,
            channel,
            duration_sectors,
        ));
        let (Ok(slot), Ok(ch)) = (u8::try_from(clip_slot), u8::try_from(channel)) else {
            self.xa_stats.clips_unstaged += 1;
            return None;
        };
        if !self.xa_channel_staged(slot, ch)
            && !self.stage_xa_channel_lazily(slot, ch, duration_sectors)
        {
            if self.defer_xa_channel(slot, ch, duration_sectors, true) {
                self.xa_stats.clips_deferred += 1;
            } else {
                self.xa_stats.clips_unstaged += 1;
            }
            return None;
        }
        let Some(clip) = self
            .xa_clip_bank
            .as_ref()
            .and_then(|b| b.cut(slot, ch, duration_sectors))
        else {
            self.xa_stats.clips_unstaged += 1;
            return None;
        };
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
            XA_CUE_GAIN,
            SHOUT_CD_RESPONSE_DELAY,
        );
        self.xa_stats.clips_fired += 1;
        self.xa_stats.last_clip = Some((slot, ch, frames));
        Some(frames)
    }

    /// Stage `(clip_slot, channel)` ahead of the request that will ask for
    /// it. The engine lists a round's cast voices at the round's start
    /// (`World::drain_battle_xa_prestage`) and a scene's lines at its load
    /// (`World::drain_field_xa_prestage`), so a host that slices sectors
    /// asynchronously has the span decoded by the time the clip is raised
    /// and [`Self::play_xa_clip`] cuts it on the same call.
    ///
    /// Queues a stage request that installs **without** playing. A director
    /// with a synchronous source reads a span on first use and stages
    /// nothing ahead; so does one that already holds the channel.
    pub fn prestage_xa_clip(&mut self, clip_slot: u32, channel: u32, duration_sectors: u32) {
        let (Ok(slot), Ok(ch)) = (u8::try_from(clip_slot), u8::try_from(channel)) else {
            return;
        };
        if self.xa_lazy.is_some() || self.xa_channel_staged(slot, ch) {
            return;
        }
        self.defer_xa_channel(slot, ch, duration_sectors, false);
    }

    /// The stage requests waiting on the host, oldest first. A request stays
    /// listed until [`Self::install_xa_span`] serves it.
    pub fn xa_stage_requests(&self) -> &[XaStageRequest] {
        &self.xa_pending
    }

    /// Install one channel of `XA<slot + 1>.XA` from the raw 2352-byte
    /// sectors a host sliced for an [`Self::xa_stage_requests`] entry:
    /// decode that channel only, stage it lazily under the bank's cap, and
    /// play the clip request that was waiting on it. Returns whether the
    /// channel decoded; `false` too for a span nobody asked for.
    pub fn install_xa_span(&mut self, slot: u8, channel: u8, raw_sectors: &[u8]) -> bool {
        let Some(pos) = self
            .xa_pending
            .iter()
            .position(|p| p.slot == slot && p.channel == channel)
        else {
            return false;
        };
        let req = self.xa_pending.remove(pos);
        let Some((clip, width)) =
            legaia_engine_audio::xa_clip_bank::decode_channel_span(raw_sectors, channel)
        else {
            self.xa_stats.clips_unstaged += 1;
            return false;
        };
        self.xa_clip_bank
            .get_or_insert_with(XaClipBank::new)
            .insert_lazy(slot, channel, clip, width);
        self.xa_stats.lazy_installed += 1;
        if req.replay {
            let _ = self.play_xa_clip(u32::from(slot), u32::from(channel), req.duration_sectors);
        }
        true
    }

    /// The XA lane's request counters.
    pub fn xa_stats(&self) -> &XaLaneStats {
        &self.xa_stats
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_audio::TestAudioSink;
    use legaia_xa::demux::{
        AUDIO_BYTES_PER_SECTOR, RAW_SECTOR_BYTES, SUBHEADER_OFFSET, USER_DATA_OFFSET,
    };

    fn director() -> AudioDirector<TestAudioSink> {
        #[allow(clippy::arc_with_non_send_sync)]
        let sink = Arc::new(TestAudioSink::new(legaia_engine_audio::SPU_INTERNAL_RATE));
        AudioDirector::new(sink)
    }

    /// An 8-channel interleave of raw Mode 2 Form 2 audio sectors with real
    /// audio on channel 4.
    fn eight_channel_run() -> Vec<u8> {
        let mut run = Vec::new();
        for _ in 0..4 {
            for ch in 0..8u8 {
                let mut s = vec![0u8; RAW_SECTOR_BYTES];
                // submode: audio (0x04) | form 2 (0x20) | real-time (0x40).
                let sub = [1, ch, 0x64, 0x00];
                s[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 4].copy_from_slice(&sub);
                s[SUBHEADER_OFFSET + 4..SUBHEADER_OFFSET + 8].copy_from_slice(&sub);
                let fill = if ch == 4 { 0x77 } else { 0x00 };
                s[USER_DATA_OFFSET..USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR].fill(fill);
                run.extend(s);
            }
        }
        run
    }

    /// With the files' extents and no synchronous source, a clip the bank
    /// lacks is deferred: one request per `(slot, channel)` naming the
    /// starter's read span, which plays when its span installs.
    #[test]
    fn a_clip_is_deferred_then_played_when_its_span_installs() {
        let mut d = director();
        // No extent yet: nothing can stage it.
        assert!(d.play_xa_clip(6, 4, 686).is_none());
        assert_eq!(d.xa_stats().clips_unstaged, 1);
        assert!(d.xa_stage_requests().is_empty());

        d.set_xa_file_extents([(6, 1000, 8 * 300)]);
        assert!(
            d.play_xa_clip(6, 4, 686).is_none(),
            "nothing plays this call"
        );
        assert_eq!(d.xa_stats().clips_deferred, 1);
        let req = d.xa_stage_requests()[0].clone();
        assert_eq!((req.slot, req.channel, req.lba), (6, 4, 1000));
        assert_eq!(req.sectors, (686 * 150 + 149) / 60);
        assert!(req.replay);
        // Re-requested before it is served: one request, the latest span.
        assert!(d.play_xa_clip(6, 4, 700).is_none());
        assert_eq!(d.xa_stage_requests().len(), 1);
        assert_eq!(d.xa_stage_requests()[0].duration_sectors, 700);

        let run = eight_channel_run();
        assert!(!d.install_xa_span(6, 5, &run), "nobody asked for channel 5");
        assert!(d.install_xa_span(6, 4, &run));
        assert!(d.xa_stage_requests().is_empty(), "served");
        assert_eq!(d.xa_stats().lazy_installed, 1);
        assert_eq!(d.xa_stats().clips_fired, 1, "the waiting request played");
        assert_eq!(d.xa_stats().last_clip.map(|(s, c, _)| (s, c)), Some((6, 4)));
        // Staged now: the next request plays at once.
        assert!(d.play_xa_clip(6, 4, 686).is_some());
    }

    /// A prestage installs without playing, so the request that follows
    /// plays on its own call - and a held channel queues nothing.
    #[test]
    fn a_prestaged_clip_installs_silently_and_plays_on_its_request() {
        let mut d = director();
        d.set_xa_file_extents([(6, 1000, 8 * 300)]);
        d.prestage_xa_clip(6, 4, 686);
        assert_eq!(d.xa_stage_requests().len(), 1);
        assert!(!d.xa_stage_requests()[0].replay);
        assert!(d.install_xa_span(6, 4, &eight_channel_run()));
        assert_eq!(d.xa_stats().clips_fired, 0, "a prestage plays nothing");
        assert!(d.play_xa_clip(6, 4, 686).is_some());
        assert_eq!(d.xa_stats().clips_deferred, 0);
        d.prestage_xa_clip(6, 4, 686);
        assert!(d.xa_stage_requests().is_empty());
    }

    /// A live request takes over a prestage one for the same channel.
    #[test]
    fn a_live_request_takes_over_a_pending_prestage() {
        let mut d = director();
        d.set_xa_file_extents([(6, 1000, 8 * 300)]);
        d.prestage_xa_clip(6, 4, 100);
        assert!(d.play_xa_clip(6, 4, 686).is_none());
        let reqs = d.xa_stage_requests();
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].replay);
        assert_eq!(reqs[0].sectors, (686 * 150 + 149) / 60);
    }
}
