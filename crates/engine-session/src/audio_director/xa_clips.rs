//! CD-XA voice on the director: the arts-shout bank, the one-shot clip bank,
//! and lazy per-channel staging straight off the disc image.

use super::*;

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

impl<S: AudioSink> AudioDirector<S> {
    /// Hand the director the source lazy staging reads from.
    pub fn set_xa_channel_source(&mut self, source: Box<dyn XaChannelSource + Send + Sync>) {
        self.xa_lazy = Some(source);
    }

    /// Install the arts-voice shout bank (demuxed + decoded from the user's
    /// disc at boot; see [`crate::native::read_arts_shout_bank`]).
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
}
