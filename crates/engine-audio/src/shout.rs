//! Battle Tactical-Arts **shout** bank - the per-character CD-XA voice clips
//! and the cue tables that pick a clip channel per art.
//!
//! REF: FUN_8004C140 - the arts-voice cue selector; the port of its channel
//! pick is [`ArtsShoutBank::pick_channel`], which carries the `PORT` tag. The
//! tag used to sit here at module scope, where the reach report's anchor
//! fallback resolved it to *the next function in the file* (`ArtsShoutBank::new`) -
//! so merely constructing a bank would have read as "the selector ran".
//!
//! When the retail
//! staged-animation materialiser (`FUN_8004AD80`) runs a party art it calls
//! `FUN_8004C140(char_id, action_constant, flag)`, which draws a random
//! channel from the art's **candidate-channel pool** (re-rolling an immediate
//! repeat of the party-wide last pick) and
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

/// First-enemy formation id that forces the shout onto channel `0xC`
/// (`li v0,0x4f` / `bne v1,v0` / `li a1,0xc` at `0x8004C3FC..0x8004C414`,
/// compared against the byte at `gp+0x9F4` = `0x8007BD0C`, formation slot 0).
pub const FORCED_CHANNEL_FORMATION_ID: u8 = 0x4F;

/// The channel [`FORCED_CHANNEL_FORMATION_ID`] forces.
pub const FORCED_CHANNEL: u8 = 0x0C;

/// Seed of the bank's BIOS-`Rand()` stand-in (the BIOS's own reset seed).
const RAND_SEED: u32 = 0x0000_0001;

/// Decoded arts-voice bank: per character slot, the per-channel clips plus the
/// per-art candidate-channel pools, and the last channel fired (the retail
/// no-immediate-repeat state, the byte `gp+0xA4A` = `0x8007BD62`).
#[derive(Debug, Clone)]
pub struct ArtsShoutBank {
    clips: [BTreeMap<u8, ShoutClip>; SHOUT_CSLOTS],
    pools: [BTreeMap<u8, Vec<u8>>; SHOUT_CSLOTS],
    /// `gp+0xA4A` - ONE byte for the whole party, not one per character:
    /// both pick loops (`0x8004C3E8` and `0x8004C554`) compare against and
    /// store to the same `gp`-relative byte whatever `char_id` is.
    last_channel: Option<u8>,
    /// `gp+0x9F4` - the first formation slot's monster id, which the
    /// selector reads for its one forced-channel exception.
    first_enemy_id: Option<u8>,
    /// State of the BIOS `Rand()` stand-in (`FUN_80056798` is the BIOS
    /// thunk `li t2,0xa0; jr t2; li t1,0x2f`).
    rand_state: u32,
}

impl Default for ArtsShoutBank {
    fn default() -> Self {
        Self {
            clips: Default::default(),
            pools: Default::default(),
            last_channel: None,
            first_enemy_id: None,
            rand_state: RAND_SEED,
        }
    }
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

    /// Install the battle's first formation id (`gp+0x9F4`, `0x8007BD0C`).
    /// `None` (the default) never matches [`FORCED_CHANNEL_FORMATION_ID`].
    /// Neither host installs it yet (the battle-start formation is known to
    /// `engine-core`'s battle session, not to the audio director that owns
    /// the bank), so the forced-channel exception is inert on both.
    pub fn set_first_enemy_id(&mut self, id: Option<u8>) {
        self.first_enemy_id = id;
    }

    /// The last channel the selector picked (`gp+0xA4A`), party-wide.
    pub fn last_channel(&self) -> Option<u8> {
        self.last_channel
    }

    /// Reseed the BIOS-`Rand()` stand-in (tests / deterministic replays).
    pub fn seed_rand(&mut self, seed: u32) {
        self.rand_state = seed;
    }

    /// The BIOS `Rand()` recurrence: `seed = seed * 0x41C64E6D + 0x3039`,
    /// result `(seed >> 16) & 0x7FFF`.
    fn next_rand(&mut self) -> u32 {
        self.rand_state = self
            .rand_state
            .wrapping_mul(0x41C6_4E6D)
            .wrapping_add(0x3039);
        (self.rand_state >> 16) & 0x7FFF
    }

    /// Pick a channel from the art's candidate pool the way retail's
    /// selector does (`0x8004C3D8..0x8004C414`, and its second-half twin at
    /// `0x8004C544..0x8004C56C`):
    ///
    /// 1. `jal 0x80056798` (BIOS `Rand()`), `div v0,s0; mfhi` - a uniform
    ///    draw modulo the pool length;
    /// 2. **re-roll** while the drawn member equals the party-wide last pick
    ///    `gp+0xA4A` (`beq v0,a1,<loop head>`);
    /// 3. store the pick to `gp+0xA4A`;
    /// 4. then, and only for what is *played* - the stored last pick stays
    ///    the drawn member - if the first formation id `gp+0x9F4` is
    ///    [`FORCED_CHANNEL_FORMATION_ID`], replace the channel with
    ///    [`FORCED_CHANNEL`].
    ///
    /// Two port deviations, both about hazards rather than behaviour: the
    /// draw comes from a bank-private `Rand()` recurrence rather than the one
    /// BIOS generator every other retail caller also advances (same
    /// distribution, different sequence), and a pool whose every member is
    /// the last pick - retail's re-roll loop never exits on one - repeats the
    /// channel instead of hanging. The retail loop has no such bound.
    ///
    /// PORT: FUN_8004C140
    pub fn pick_channel(&mut self, cslot: u8, action: u8) -> Option<u8> {
        let len = {
            let pool = self.pool(cslot, action)?;
            if pool.is_empty() {
                return None;
            }
            pool.len()
        };
        let all_repeat = {
            let pool = self.pool(cslot, action)?;
            pool.iter().all(|&c| Some(c) == self.last_channel)
        };
        let pick = loop {
            let idx = self.next_rand() as usize % len;
            let c = self.pool(cslot, action)?[idx];
            if all_repeat || Some(c) != self.last_channel {
                break c;
            }
        };
        self.last_channel = Some(pick);
        if self.first_enemy_id == Some(FORCED_CHANNEL_FORMATION_ID) {
            return Some(FORCED_CHANNEL);
        }
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
    fn immediate_repeat_is_rerolled_against_one_party_wide_byte() {
        let mut b = bank_with(1, &[2, 5], 0x40, &[2, 5]);
        let mut prev = b.pick_channel(1, 0x40).unwrap();
        for _ in 0..32 {
            let next = b.pick_channel(1, 0x40).unwrap();
            assert_ne!(prev, next, "the re-roll never repeats the last pick");
            prev = next;
        }
        // The last-pick byte is shared: Vahn's pool {5, 9} re-rolls against
        // Noa's last pick too.
        b.set_pool(0, 0x22, vec![5, 9]);
        let noa = b.last_channel().unwrap();
        for _ in 0..16 {
            let v = b.pick_channel(0, 0x22).unwrap();
            if noa == 5 {
                assert_eq!(v, 9);
            }
            b.last_channel = Some(noa);
        }
        // Single-member pool: the retail loop would spin; the port repeats.
        b.set_pool(1, 0x41, vec![7]);
        assert_eq!(b.pick_channel(1, 0x41), Some(7));
        assert_eq!(b.pick_channel(1, 0x41), Some(7));
    }

    #[test]
    fn the_draw_reaches_every_pool_member() {
        let pool: Vec<u8> = (0..9).collect();
        let mut b = bank_with(0, &pool, 0x2B, &pool);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            seen.insert(b.pick_channel(0, 0x2B).unwrap());
        }
        assert_eq!(seen.len(), 9, "a nine-channel pool is heard in full");
    }

    #[test]
    fn formation_0x4f_forces_channel_twelve_but_keeps_the_drawn_last_pick() {
        let mut b = bank_with(2, &[3, 4, 12], 0x30, &[3, 4]);
        b.set_first_enemy_id(Some(FORCED_CHANNEL_FORMATION_ID));
        let played = b.pick_channel(2, 0x30).unwrap();
        assert_eq!(played, FORCED_CHANNEL);
        let last = b.last_channel().unwrap();
        assert!(last == 3 || last == 4, "gp+0xA4A holds the drawn member");
        b.set_first_enemy_id(Some(0x4E));
        assert_ne!(b.pick_channel(2, 0x30), Some(FORCED_CHANNEL));
    }
}
