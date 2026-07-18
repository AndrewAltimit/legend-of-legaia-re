//! Battle Tactical-Arts **shout** bank - the per-character CD-XA voice clips
//! and the cue tables that pick a clip channel per art.
//!
//! PORT: FUN_8004C140 - the arts-voice cue selector. When the retail
//! staged-animation materialiser (`FUN_8004AD80`) runs a party art it calls
//! `FUN_8004C140(char_id, action_constant, flag)`, which picks a channel from
//! the art's **candidate-channel pool** (avoiding an immediate repeat) and
//! fires the CD-XA clip player `FUN_8003D53C(clip_slot, channel, dur)`.
//! The clip files are per character: Vahn=`XA2.XA`, Noa=`XA4.XA`,
//! Gala=`XA6.XA` - 16-channel short-mono shout banks (see
//! `crates/art::arts_voice` for the SCUS cue-table parser and
//! `docs/subsystems/audio.md` for the full path).
//!
//! REF: FUN_8003D53C (CD-XA clip play - the engine equivalent is
//! [`crate::AudioOut::play_xa_shout`] / [`crate::OfflineMixer`], which mix the
//! decoded PCM into the SPU output the way the PSX CD-input path does).
//!
//! This module is **data-only and device-free**: the host (engine-shell boot)
//! demuxes the XA files per channel, decodes them to PCM, parses the SCUS cue
//! tables, and feeds both in. Keeping the disc/table I/O out of this crate
//! keeps `legaia-engine-audio` free of parser-crate dependencies.

use std::collections::BTreeMap;

/// Number of playable-character shout banks (Vahn / Noa / Gala; Terra has no
/// clip file).
pub const SHOUT_CSLOTS: usize = 3;

/// Modeled CD-controller response delay between the shout request (issued on
/// the art's animation-start frame) and the first audible XA sample, in SPU
/// samples (44.1 kHz). ~150 ms - the seek + first-sector-read latency of the
/// retail CD path. This is what keeps the shout **trailing** the art
/// animation instead of leading it (the recomp-verified retail contract:
/// XA audio arrives after the animation begins).
pub const SHOUT_CD_RESPONSE_DELAY: u32 = 6_615;

/// One decoded mono shout clip (a single XA channel of a character's bank).
#[derive(Debug, Clone, Default)]
pub struct ShoutClip {
    /// Decoded mono PCM.
    pub pcm: Vec<i16>,
    /// Source sample rate (18 900 Hz for the retail mono shout banks).
    pub sample_rate: u32,
}

/// Decoded arts-voice bank: per character slot, the per-channel clips plus the
/// per-art candidate-channel pools, and the last channel fired (the retail
/// no-immediate-repeat state, `FUN_8004C140`'s static).
#[derive(Debug, Clone, Default)]
pub struct ArtsShoutBank {
    clips: [BTreeMap<u8, ShoutClip>; SHOUT_CSLOTS],
    pools: [BTreeMap<u8, Vec<u8>>; SHOUT_CSLOTS],
    last_channel: [Option<u8>; SHOUT_CSLOTS],
}

impl ArtsShoutBank {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` once at least one decoded clip is staged.
    pub fn has_clips(&self) -> bool {
        self.clips.iter().any(|c| !c.is_empty())
    }

    /// Stage one decoded XA channel clip for a character slot.
    pub fn insert_clip(&mut self, cslot: u8, channel: u8, clip: ShoutClip) {
        if let Some(c) = self.clips.get_mut(cslot as usize) {
            c.insert(channel, clip);
        }
    }

    /// Stage the candidate-channel pool for `(cslot, action_constant)`.
    pub fn set_pool(&mut self, cslot: u8, action: u8, channels: Vec<u8>) {
        if let Some(p) = self.pools.get_mut(cslot as usize)
            && !channels.is_empty()
        {
            p.insert(action, channels);
        }
    }

    /// The candidate-channel pool for `(cslot, action_constant)`; `None` when
    /// the art has no arts-voice entry (an art retail plays silent).
    pub fn pool(&self, cslot: u8, action: u8) -> Option<&[u8]> {
        self.pools
            .get(cslot as usize)?
            .get(&action)
            .map(Vec::as_slice)
    }

    /// Pick a channel from the art's candidate pool, avoiding an immediate
    /// repeat of the character's previously fired channel when the pool has
    /// an alternative (the retail selector re-rolls on a repeat; the engine
    /// pick is deterministic - keyed on the action constant - stepping to the
    /// next pool member instead of re-rolling).
    pub fn pick_channel(&mut self, cslot: u8, action: u8) -> Option<u8> {
        let pool = self.pool(cslot, action)?;
        if pool.is_empty() {
            return None;
        }
        let base = action as usize % pool.len();
        let mut pick = pool[base];
        if self
            .last_channel
            .get(cslot as usize)?
            .is_some_and(|l| l == pick)
            && pool.len() > 1
        {
            pick = pool[(base + 1) % pool.len()];
        }
        self.last_channel[cslot as usize] = Some(pick);
        Some(pick)
    }

    /// Resolve the shout for `(cslot, action_constant)`: picks a channel from
    /// the pool (updating the no-repeat state) and returns it with the staged
    /// clip. `None` when the art is unvoiced or the clip channel wasn't
    /// decoded.
    pub fn shout(&mut self, cslot: u8, action: u8) -> Option<(u8, &ShoutClip)> {
        let channel = self.pick_channel(cslot, action)?;
        let clip = self.clips.get(cslot as usize)?.get(&channel)?;
        Some((channel, clip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank_with(cslot: u8, channels: &[u8], pool_action: u8, pool: &[u8]) -> ArtsShoutBank {
        let mut b = ArtsShoutBank::new();
        for &ch in channels {
            b.insert_clip(
                cslot,
                ch,
                ShoutClip {
                    pcm: vec![ch as i16 + 1; 8],
                    sample_rate: 18_900,
                },
            );
        }
        b.set_pool(cslot, pool_action, pool.to_vec());
        b
    }

    #[test]
    fn shout_resolves_pool_channel_to_clip() {
        let mut b = bank_with(0, &[0, 6], 0x27, &[0, 6]);
        let (ch, clip) = b.shout(0, 0x27).expect("voiced art resolves");
        assert!(ch == 0 || ch == 6);
        assert_eq!(clip.pcm.len(), 8);
    }

    #[test]
    fn unvoiced_art_and_missing_clip_stay_silent() {
        let mut b = bank_with(0, &[0], 0x27, &[0]);
        assert!(b.shout(0, 0x28).is_none(), "no pool entry -> silent");
        // Pool names channel 9 but no clip decoded for it.
        b.set_pool(0, 0x30, vec![9]);
        assert!(b.shout(0, 0x30).is_none(), "missing clip -> silent");
        assert!(b.shout(3, 0x27).is_none(), "Terra has no bank");
    }

    #[test]
    fn immediate_repeat_steps_to_next_pool_member() {
        let mut b = bank_with(1, &[2, 5], 0x40, &[2, 5]);
        let first = b.pick_channel(1, 0x40).unwrap();
        let second = b.pick_channel(1, 0x40).unwrap();
        assert_ne!(first, second, "same art twice must not repeat the channel");
        // Single-member pool: repeat is allowed (nothing to avoid onto).
        b.set_pool(1, 0x41, vec![7]);
        assert_eq!(b.pick_channel(1, 0x41), Some(7));
        assert_eq!(b.pick_channel(1, 0x41), Some(7));
    }
}
