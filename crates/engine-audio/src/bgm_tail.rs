//! The banks that borrow the BGM region's free tail, and when they stop
//! being resident - one model both play hosts drive.
//!
//! The port's SPU map ([`crate::spu_layout`]) has no room for two of
//! retail's variable banks, so both hosts park them behind the current
//! track, in the BGM region above its samples:
//!
//! - the battle-end **reward** bank (PROT 0889, cue `0x50`, VAB slot `11`),
//!   staged when the results frame queues its cue;
//! - the field **side-band** bank a script selects (op `0x36` sub `1`, VAB
//!   slot `3`), staged while the world is in a field-family mode;
//! - the battle's two **monster** banks (`monster.snd`, VAB slots `7` / `8`),
//!   staged while a battle is on screen - retail's battle scene loader
//!   `FUN_800520F0` streams them per battle (`FUN_8003E104`) at slot 7's and
//!   slot 8's own bases (`0x65010` / `0x6C810`).
//!
//! # The residency rule
//!
//! Retail gives both banks their own SPU base (`FUN_800265E8` seeds slot 3
//! at `0x60010` and slot 11 at `0x6F010`), and the BGM stream arm of
//! `FUN_800243F0` loads the next track into slot `1` (`jal 0x8001FC00` with
//! `a1 = 1` at `0x80024678`, then `FUN_8001E54C(1, ..)` at `0x80024780`)
//! without closing either: the resolver makes no call to the VAB closer
//! `FUN_8001FF58`. A track change therefore leaves a side-band or reward bank
//! open, and a bank stops sounding only when its own close runs (the battle
//! mode init closes `3`, the field init closes `11`) or when a larger track
//! overruns its base - retail's gaps are allocation, not enforcement.
//!
//! The port's version of "overruns its base" is a track whose samples end
//! past the borrower's base: [`BgmTail::observe_bgm_end`] drops exactly those
//! borrowers, and keeps every one the new track leaves intact. A host calls
//! it with the live track's sample end wherever it may have changed - after
//! its own upload (the native director) or before it reads the tail (the
//! browser page, whose upload site does not see the SFX channel). The call
//! is idempotent, so the two call timings reach the same state.
//!
//! # The retry memo
//!
//! A side-band bank that does not fit is not re-read every frame: the attempt
//! is remembered against the tail's [`BgmTail::generation`], which moves
//! exactly when the free tail can have changed - the track's sample end
//! moved, or a borrower was dropped. A scene change alone moves nothing
//! (retail's side-band request survives a door; it is the scripts' to
//! change), so it neither clears nor re-arms the memo.
//!
//! REF: FUN_800243F0, FUN_800265E8, FUN_8001FF58

use crate::spu::Spu;
use crate::spu::ram::SpuAllocator;
use crate::spu_layout::{SFX_REGION_BASE, SPU_RESERVED_BYTES, bank_used_end, vab_body_bytes};
use crate::vab_bind::VabBank;
use legaia_vab::VabReport;

/// VAB slot the battle-end reward bank (PROT 0889, cue `0x50`) is opened in
/// (`FUN_8004E568`: `li a1,0xb` at `0x8004ED7C` for the loader,
/// `li a0,0xb` at `0x8004EDB8` for the open).
pub const REWARD_SLOT: u8 = 11;

/// A battle's monster-bank request: `(VAB slot, monster.snd bank index)`
/// pairs, slot `7` first - the shape
/// `legaia_engine_core::world::World::battle_monster_sound_banks` returns.
pub type MonsterBankKey = Vec<(u8, u16)>;

/// One bank parked in the BGM region's tail: its VAB slot and the SPU span
/// its samples occupy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TailBorrow {
    pub slot: u8,
    /// First byte of the span (16-byte aligned).
    pub base: u32,
    /// One past the last sample byte; `base` for a bank with no samples.
    pub end: u32,
}

/// The tail's occupants, the track end they were last checked against, and
/// the side-band retry memo. `K` is the host's side-band request key (the
/// engine's `SideBandBank`).
#[derive(Debug, Clone)]
pub struct BgmTail<K> {
    bgm_end: u32,
    generation: u64,
    reward: Option<TailBorrow>,
    side_band: Option<(K, TailBorrow)>,
    side_band_attempt: Option<(K, u64)>,
    monsters: Option<(MonsterBankKey, Vec<TailBorrow>)>,
    monster_attempt: Option<(MonsterBankKey, u64)>,
}

impl<K> Default for BgmTail<K> {
    fn default() -> Self {
        Self {
            bgm_end: SPU_RESERVED_BYTES,
            generation: 0,
            reward: None,
            side_band: None,
            side_band_attempt: None,
            monsters: None,
            monster_attempt: None,
        }
    }
}

/// The sample end a staged track bank reaches, the floor for an absent or
/// silent bank - the argument [`BgmTail::observe_bgm_end`] takes.
pub fn track_end(bank: Option<&VabBank>) -> u32 {
    bank.and_then(bank_used_end).unwrap_or(SPU_RESERVED_BYTES)
}

/// Upload a tail bank at `base` (from [`BgmTail::place`]), bounded by the SFX
/// region.
pub fn upload_at(spu: &mut Spu, base: u32, report: &VabReport, bank_buf: &[u8]) -> VabBank {
    let mut alloc = SpuAllocator::new(base, SFX_REGION_BASE.saturating_sub(base));
    VabBank::upload(spu, &mut alloc, report, bank_buf)
}

impl<K: Copy + PartialEq> BgmTail<K> {
    /// Bumped whenever the free tail can have changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The track sample end the tail was last checked against.
    pub fn bgm_end(&self) -> u32 {
        self.bgm_end
    }

    /// The reward bank's span, while it is parked.
    pub fn reward(&self) -> Option<TailBorrow> {
        self.reward
    }

    /// The side-band bank's request key and span, while it is parked.
    pub fn side_band(&self) -> Option<(K, TailBorrow)> {
        self.side_band
    }

    /// The live track now ends at `bgm_end`. Drops every borrower whose base
    /// the track's samples reached and returns their slots (the host forgets
    /// the matching banks); keeps every other. Idempotent.
    pub fn observe_bgm_end(&mut self, bgm_end: u32) -> Vec<u8> {
        let mut dropped = Vec::new();
        if bgm_end != self.bgm_end {
            self.bgm_end = bgm_end;
            self.generation = self.generation.wrapping_add(1);
        }
        if let Some(r) = self.reward
            && bgm_end > r.base
        {
            dropped.extend(self.drop_reward());
        }
        if let Some((_, b)) = self.side_band
            && bgm_end > b.base
        {
            dropped.extend(self.drop_side_band());
        }
        if self
            .monsters
            .as_ref()
            .is_some_and(|(_, bs)| bs.iter().any(|b| bgm_end > b.base))
        {
            dropped.extend(self.drop_monsters());
        }
        dropped
    }

    /// The monster banks' request key and spans, while they are parked.
    pub fn monsters(&self) -> Option<&(MonsterBankKey, Vec<TailBorrow>)> {
        self.monsters.as_ref()
    }

    /// Record one monster bank as parked under request `key` (appended to
    /// the set already parked under the same key, replacing any other).
    pub fn commit_monster(&mut self, key: &MonsterBankKey, borrow: TailBorrow) {
        match self.monsters.as_mut() {
            Some((k, bs)) if k == key => bs.push(borrow),
            _ => self.monsters = Some((key.clone(), vec![borrow])),
        }
    }

    /// Forget the monster banks; their slots.
    pub fn drop_monsters(&mut self) -> Vec<u8> {
        let Some((_, bs)) = self.monsters.take() else {
            return Vec::new();
        };
        self.generation = self.generation.wrapping_add(1);
        bs.iter().map(|b| b.slot).collect()
    }

    /// Whether to attempt staging monster request `key` now - the
    /// [`Self::begin_side_band_attempt`] memo for the battle banks.
    pub fn begin_monster_attempt(&mut self, key: &MonsterBankKey) -> bool {
        if self.monsters.as_ref().is_some_and(|(k, _)| k == key) {
            return false;
        }
        if self
            .monster_attempt
            .as_ref()
            .is_some_and(|(k, g)| k == key && *g == self.generation)
        {
            return false;
        }
        self.monster_attempt = Some((key.clone(), self.generation));
        true
    }

    /// Where a borrower of `body_bytes` for `slot` goes: above the track and
    /// every other borrower, 16-byte aligned. `None` when the rest of the BGM
    /// region cannot hold it.
    pub fn place(&self, slot: u8, body_bytes: u32) -> Option<u32> {
        let others = self
            .reward
            .iter()
            .chain(self.side_band.iter().map(|(_, b)| b))
            .chain(self.monsters.iter().flat_map(|(_, bs)| bs.iter()))
            .filter(|b| b.slot != slot)
            .map(|b| b.end);
        let base = others.fold(self.bgm_end, u32::max).div_ceil(16) * 16;
        (base < SFX_REGION_BASE && body_bytes <= SFX_REGION_BASE - base).then_some(base)
    }

    /// [`Self::place`] for a parsed bank.
    pub fn place_report(&self, slot: u8, report: &VabReport) -> Option<u32> {
        self.place(slot, vab_body_bytes(report))
    }

    /// Record the reward bank as parked in `borrow`.
    pub fn commit_reward(&mut self, borrow: TailBorrow) {
        self.reward = Some(borrow);
    }

    /// Record the side-band bank `key` as parked in `borrow`.
    pub fn commit_side_band(&mut self, key: K, borrow: TailBorrow) {
        self.side_band = Some((key, borrow));
    }

    /// Forget the reward bank; its slot when one was parked.
    pub fn drop_reward(&mut self) -> Option<u8> {
        let b = self.reward.take()?;
        self.generation = self.generation.wrapping_add(1);
        Some(b.slot)
    }

    /// Forget the side-band bank; its slot when one was parked.
    pub fn drop_side_band(&mut self) -> Option<u8> {
        let (_, b) = self.side_band.take()?;
        self.generation = self.generation.wrapping_add(1);
        Some(b.slot)
    }

    /// Forget every borrower (a track laid across the SFX region overwrote
    /// them all); their slots.
    pub fn clear(&mut self) -> Vec<u8> {
        self.drop_reward()
            .into_iter()
            .chain(self.drop_side_band())
            .chain(self.drop_monsters())
            .collect()
    }

    /// Whether to attempt staging side-band `key` now: `false` while it is
    /// already parked, or while the last attempt at it met the same free
    /// tail. A `true` answer records the attempt.
    pub fn begin_side_band_attempt(&mut self, key: K) -> bool {
        if self.side_band.is_some_and(|(k, _)| k == key) {
            return false;
        }
        if self.side_band_attempt == Some((key, self.generation)) {
            return false;
        }
        self.side_band_attempt = Some((key, self.generation));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monster_banks_stack_and_drop_with_the_track() {
        let mut t = BgmTail::<i32>::default();
        t.observe_bgm_end(0x30000);
        let key: MonsterBankKey = vec![(7, 11), (8, 40)];
        assert!(t.begin_monster_attempt(&key));
        let a = t.place(7, 0x1000).unwrap();
        t.commit_monster(&key, b(7, a, a + 0x1000));
        let c = t.place(8, 0x800).unwrap();
        assert_eq!(c, a + 0x1000, "the second bank stacks above the first");
        t.commit_monster(&key, b(8, c, c + 0x800));
        assert!(!t.begin_monster_attempt(&key), "already parked");
        // A track reaching the first bank overwrites both.
        assert_eq!(t.observe_bgm_end(a + 0x10), vec![7, 8]);
        assert!(t.monsters().is_none());
    }

    fn b(slot: u8, base: u32, end: u32) -> TailBorrow {
        TailBorrow { slot, base, end }
    }

    #[test]
    fn a_track_change_keeps_a_borrower_it_leaves_intact() {
        let mut t = BgmTail::<i32>::default();
        t.observe_bgm_end(0x30000);
        let base = t.place(REWARD_SLOT, 0x4000).unwrap();
        assert_eq!(base, 0x30000);
        t.commit_reward(b(REWARD_SLOT, base, base + 0x4000));
        // A smaller track: retail's resolver closes nothing, and the port's
        // samples above the new end are untouched.
        assert!(t.observe_bgm_end(0x20000).is_empty());
        assert!(t.reward().is_some());
        // A track reaching past the base overwrote it.
        assert_eq!(t.observe_bgm_end(0x30010), vec![REWARD_SLOT]);
        assert!(t.reward().is_none());
    }

    #[test]
    fn borrowers_stack_above_each_other() {
        let mut t = BgmTail::<i32>::default();
        t.observe_bgm_end(0x20001);
        let r = t.place(REWARD_SLOT, 0x100).unwrap();
        assert_eq!(r, 0x20010);
        t.commit_reward(b(REWARD_SLOT, r, r + 0x105));
        let s = t.place(3, 0x100).unwrap();
        assert_eq!(s, (r + 0x105).div_ceil(16) * 16);
        t.commit_side_band(7, b(3, s, s + 0x100));
        // Re-placing the reward skips its own old span but not the other.
        assert_eq!(t.place(REWARD_SLOT, 0x10).unwrap(), s + 0x100);
        // A borrower that does not fit is refused.
        assert!(t.place(3, SFX_REGION_BASE).is_none());
    }

    #[test]
    fn the_memo_rearms_only_when_the_free_tail_moves() {
        let mut t = BgmTail::<i32>::default();
        t.observe_bgm_end(0x40000);
        assert!(t.begin_side_band_attempt(2002));
        assert!(!t.begin_side_band_attempt(2002), "same tail, same request");
        // Re-observing the same track (a scene change with no restage).
        t.observe_bgm_end(0x40000);
        assert!(!t.begin_side_band_attempt(2002));
        // A different request is its own attempt.
        assert!(t.begin_side_band_attempt(2003));
        // The track moved: worth trying again.
        t.observe_bgm_end(0x38000);
        assert!(t.begin_side_band_attempt(2003));
        // A parked bank is never re-attempted.
        t.commit_side_band(2003, b(3, 0x38000, 0x39000));
        t.observe_bgm_end(0x30000);
        assert!(!t.begin_side_band_attempt(2003));
    }

    #[test]
    fn dropping_a_borrower_rearms_the_memo() {
        let mut t = BgmTail::<i32>::default();
        t.observe_bgm_end(0x40000);
        t.commit_reward(b(REWARD_SLOT, 0x40000, 0x44000));
        assert!(t.begin_side_band_attempt(2002));
        assert_eq!(t.drop_reward(), Some(REWARD_SLOT));
        assert!(t.begin_side_band_attempt(2002));
        assert_eq!(t.clear(), Vec::<u8>::new());
    }
}
