//! The field player's locomotion clip: the clip-base global `_DAT_8007BDD8`,
//! its writers, and the per-frame pick that turns it into the clip id the
//! actor's `+0x5C` carries.
//!
//! Retail never names a clip directly from the pad. Three layers stack:
//!
//! 1. **A writer stamps the base.** `_DAT_8007BDD8` is a small integer - the
//!    1-based slot inside one character's seven-record bank of the party
//!    locomotion bundle (PROT 0874 section 1). The pad controller
//!    `FUN_801D01B0` writes it every frame it runs
//!    (`0x801D0424..0x801D04A4`, see [`locomotion_clip_base`]); the hop phase
//!    machine `FUN_801D2298` writes `6` / `7` / `1`
//!    ([`crate::field_ledge_hop_arc::hop_phase`]); the walk-on dispatcher
//!    `FUN_801D1EC4` writes `2` under the scene flag `_DAT_8007B6A8`
//!    (`0x801D21AC`); scene entry seeds `2` (SCUS `0x8003B364`).
//! 2. **The settle tail picks the clip.** Every grounded frame that did not
//!    start a hop, `FUN_801D1BA0` ends in [`settle_clip_pick`]
//!    (`0x801D1D88..0x801D1EAC`): the base, strided by the party leader into
//!    that character's bank, becomes `+0x5C`.
//! 3. **The clip selector binds it.** `FUN_800204F8` resolves `+0x5C` to a
//!    record in one of three banks ([`clip_bank`]) and rewinds the playhead
//!    when the id changed.
//!
//! What the base values mean, read off the writers:
//!
//! | base | written by | bank slot | clip |
//! |---|---|---|---|
//! | `1` | pad walk (`0x801D0484`), hop tear-down | `0` | walk |
//! | `2` | pad idle (`0x801D045C`), scene entry, walk-on under `_DAT_8007B6A8` | `1` | idle |
//! | `3` | pad run (`0x801D046C`: every base step but the plain walk `8`) | `2` | run |
//! | `6` | hop take-off (`0x801D22FC`) | `5` | hop |
//! | `7` | hop landing crossing | `6` | land |
//! | `99` | pad move under `_DAT_8007B6A8` | - | scene-bank record `leader` |
//!
//! Slots `0` and `1` are capture-pinned (`docs/formats/anm.md`); the rest are
//! the arithmetic of these writers, not a capture.
//!
//! `see ghidra/scripts/funcs/overlay_0897_xxx_dat_801d1ba0.txt`; the
//! locomotion slice and the dispatcher arm were read from
//! `extracted/overlays/overlay_field_0897.bin` at base `0x801CE818`.

/// The base that routes the pick to the scene bank (`addiu v0, zero, 0x63`
/// at `0x801D1D90`).
pub const BASE_SCENE_SENTINEL: u16 = 99;

/// The pad-idle base (`0x801D045C`) - also scene entry's seed.
pub const BASE_IDLE: u16 = 2;

/// The pad-walk base (`0x801D0484`).
pub const BASE_WALK: u16 = 1;

/// The pad-run base (`0x801D046C`).
pub const BASE_RUN: u16 = 3;

/// The plain-walk base step the locomotion's selector falls back to; every
/// other base step (run `0xC`, forced slow `5`, debug turbo `0x18`) selects
/// [`BASE_RUN`].
pub const WALK_BASE_STEP: i32 = 8;

/// Records per character bank in the party locomotion bundle - the `* 7` of
/// the pick (`sll v0, a0, 3; subu v0, v0, a0` at `0x801D1E0C`).
pub const BANK_STRIDE: u16 = 7;

/// The actor flag bit that selects the party bank (`lui v1, 0x100`). Read by
/// `FUN_800204F8` at `0x8002053C`; raised by the locomotion every frame it
/// runs and cleared by the scene-sentinel arm of the pick.
pub const PARTY_BANK_FLAG: u32 = 0x0100_0000;

/// The playback-rate halfword the locomotion stamps into `+0x6A` whenever it
/// writes a base (`addiu v0, zero, 8; sh v0, 0x6a(s2)` at `0x801D0430`).
pub const LOCOMOTION_ANIM_RATE: i16 = 8;

/// What the pad controller writes to the clip base this frame, or `None`
/// when it writes nothing.
///
/// `current_clip` is the actor's `+0x5C` on entry; the whole write is gated
/// on it being positive (`lh v0, 0x5c(s2); blez` at `0x801D0424`).
/// `direction` is the resolved direction mask the controller steps on
/// (`s0 & 0xF000`); `base_step` is the selector's `$s4`
/// (`5` / `8` / `0xC` / `0x18`); `scene_flag` is `_DAT_8007B6A8`.
///
/// Out of `0x801D0448..0x801D04A4`: no direction stores [`BASE_IDLE`];
/// otherwise the plain walk step stores [`BASE_WALK`] and every other step
/// [`BASE_RUN`] - and under the scene flag the store is overwritten with
/// [`BASE_SCENE_SENTINEL`]. The flag also forces the base step to `5`, so
/// the walk arm and the flag never meet; the redundant `lbu` at
/// `0x801D0474` is why the walk arm tests it at all.
///
/// PORT: FUN_801D01B0 (the clip-base slice `0x801D0424..0x801D04A4`)
pub fn locomotion_clip_base(
    current_clip: i16,
    direction: u16,
    base_step: i32,
    scene_flag: bool,
) -> Option<u16> {
    if current_clip <= 0 {
        return None;
    }
    if direction & 0xF000 == 0 {
        return Some(BASE_IDLE);
    }
    if scene_flag {
        return Some(BASE_SCENE_SENTINEL);
    }
    Some(if base_step == WALK_BASE_STEP {
        BASE_WALK
    } else {
        BASE_RUN
    })
}

/// Which bank `FUN_800204F8` binds a clip id from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBank {
    /// `*(0x8007B75C)` - the party locomotion bundle (PROT 0874 section 1).
    Party,
    /// `*(0x8007B888)` - the scene's own ANM bundle.
    Scene,
    /// `*(0x8007B840)` - the bank ids at or above `0x400` bind from.
    High,
}

/// Resolve a clip id to `(bank, record index)`, the way `FUN_800204F8` does
/// (`0x80020524..0x800205A8`): the party bank when the actor carries
/// [`PARTY_BANK_FLAG`], otherwise the scene bank below `0x400` and the high
/// bank above. The record is word `clip & 0x3FF` of a
/// `[u32 count][u32 offsets...]` table, i.e. record `(clip & 0x3FF) - 1`.
///
/// `None` for a non-positive id: the selector binds nothing then
/// (`blez a0` at `0x8002052C`).
///
/// REF: FUN_800204F8
pub fn clip_bank(clip: i16, party_flag: bool) -> Option<(ClipBank, u16)> {
    if clip <= 0 {
        return None;
    }
    let bank = if party_flag {
        ClipBank::Party
    } else if clip < 0x400 {
        ClipBank::Scene
    } else {
        ClipBank::High
    };
    Some((bank, (clip as u16 & 0x3FF).wrapping_sub(1)))
}

/// What one settle-tail pick produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettleClipPick {
    /// The id stored into `+0x5C`.
    pub clip: u16,
    /// [`PARTY_BANK_FLAG`] after the call.
    pub party_flag: bool,
    /// Whether `FUN_800204F8` runs this frame. It is skipped under scratchpad
    /// `0x1F800394 & 0x400` and for a zero id.
    pub binds: bool,
    /// The flag bit **as the selector sees it**. The override arm clears it
    /// around the call and restores it after, so an override id binds from
    /// the scene bank even though the actor ends the frame flagged.
    pub bind_party_flag: bool,
}

impl SettleClipPick {
    /// The `(bank, record)` this pick binds, when it binds.
    pub fn bound(&self) -> Option<(ClipBank, u16)> {
        if !self.binds {
            return None;
        }
        clip_bank(self.clip as i16, self.bind_party_flag)
    }
}

/// The anim-clip tail of the field vertical settle: `FUN_801D1BA0` at
/// `0x801D1D88..0x801D1EAC`, run after `jal 0x801D1EC4` on every frame the
/// routine did not start a hop.
///
/// `base` is `_DAT_8007BDD8`, `leader` the party leader's character id
/// `_DAT_8007B8F8`, `clip_override` the op-`4C CE` word `_DAT_8007B6AC`,
/// `party_flag` the actor's [`PARTY_BANK_FLAG`] on entry and `bind_blocked`
/// scratchpad `0x1F800394 & 0x400`.
///
/// ```text
///   base == 99            clip = leader + 1, flag cleared
///   flag and override     clip = base + override - 1
///   flag                  clip = base + leader * 7
///   otherwise             clip = base
/// ```
///
/// The arithmetic is the same as the unreferenced helper `FUN_801E58A8`
/// ([`crate::menu_actor_seed::actor_clip_pick`], which stays the port of that
/// retail-unreachable copy); this is the copy retail actually runs, and it
/// adds the two bind gates (`0x801D1E2C..0x801D1E4C`)
/// and the override arm's clear-bind-restore of the flag
/// (`0x801D1E54..0x801D1EAC`).
///
/// PORT: FUN_801D1BA0 (the anim-clip tail `0x801D1D88..0x801D1EAC`)
pub fn settle_clip_pick(
    base: u16,
    leader: u16,
    clip_override: u32,
    party_flag: bool,
    bind_blocked: bool,
) -> SettleClipPick {
    let (clip, flag) = if base == BASE_SCENE_SENTINEL {
        (leader.wrapping_add(1), false)
    } else if !party_flag {
        (base, false)
    } else if clip_override != 0 {
        // The override arm: `addu; addiu -1` at `0x801D1DF4..0x801D1DFC`.
        (
            base.wrapping_add(clip_override as u16).wrapping_sub(1),
            true,
        )
    } else {
        // `(leader << 3) - leader` at `0x801D1E0C..0x801D1E10`.
        (base.wrapping_add(leader.wrapping_mul(BANK_STRIDE)), true)
    };
    SettleClipPick {
        clip,
        party_flag: flag,
        binds: !bind_blocked && clip as i16 != 0,
        bind_party_flag: flag && clip_override == 0,
    }
}

/// Bank slot (0-based record inside one character's seven-record bank) a
/// party-bank pick lands on, given the leader it was strided by. `None` when
/// the pick is not a party-bank bind or lands outside the leader's bank.
pub fn party_bank_slot(pick: &SettleClipPick, leader: u16) -> Option<usize> {
    let (bank, record) = pick.bound()?;
    if bank != ClipBank::Party {
        return None;
    }
    let first = leader.checked_mul(BANK_STRIDE)?;
    let slot = record.checked_sub(first)?;
    (slot < BANK_STRIDE).then_some(slot as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pad_writes_idle_walk_and_run() {
        assert_eq!(locomotion_clip_base(2, 0, 8, false), Some(BASE_IDLE));
        assert_eq!(locomotion_clip_base(2, 0x1000, 8, false), Some(BASE_WALK));
        assert_eq!(locomotion_clip_base(2, 0x2000, 0xC, false), Some(BASE_RUN));
        // The debug turbo and the forced-slow step are "not the walk step".
        assert_eq!(locomotion_clip_base(2, 0x2000, 0x18, false), Some(BASE_RUN));
        assert_eq!(
            locomotion_clip_base(2, 0x2000, 5, true),
            Some(BASE_SCENE_SENTINEL)
        );
        // Idle wins over the scene flag: the no-direction store jumps past it.
        assert_eq!(locomotion_clip_base(2, 0, 5, true), Some(BASE_IDLE));
    }

    #[test]
    fn a_zero_clip_actor_gets_no_base_write() {
        assert_eq!(locomotion_clip_base(0, 0x1000, 8, false), None);
        assert_eq!(locomotion_clip_base(-2, 0x1000, 8, false), None);
    }

    #[test]
    fn the_pick_strides_into_the_leaders_bank() {
        // Noa (leader 1) walking: base 1 -> clip 8 -> record 7 = her slot 0.
        let p = settle_clip_pick(BASE_WALK, 1, 0, true, false);
        assert_eq!(p.clip, 8);
        assert_eq!(p.bound(), Some((ClipBank::Party, 7)));
        assert_eq!(party_bank_slot(&p, 1), Some(0));
        // Vahn idle: record 1, slot 1 (the capture-pinned idle).
        let p = settle_clip_pick(BASE_IDLE, 0, 0, true, false);
        assert_eq!(party_bank_slot(&p, 0), Some(1));
        // Gala running: slot 2.
        let p = settle_clip_pick(BASE_RUN, 2, 0, true, false);
        assert_eq!(party_bank_slot(&p, 2), Some(2));
    }

    #[test]
    fn the_scene_sentinel_binds_the_scene_bank_and_drops_the_flag() {
        let p = settle_clip_pick(BASE_SCENE_SENTINEL, 2, 0, true, false);
        assert_eq!(p.clip, 3);
        assert!(!p.party_flag);
        assert_eq!(p.bound(), Some((ClipBank::Scene, 2)));
        assert_eq!(party_bank_slot(&p, 2), None);
    }

    #[test]
    fn the_override_binds_from_the_scene_bank_but_keeps_the_flag() {
        let p = settle_clip_pick(BASE_IDLE, 0, 0x24, true, false);
        assert_eq!(p.clip, 2 + 0x24 - 1);
        assert!(p.party_flag, "restored after the bind");
        assert_eq!(p.bound(), Some((ClipBank::Scene, 0x24)));
    }

    #[test]
    fn the_bind_gates() {
        assert!(!settle_clip_pick(BASE_IDLE, 0, 0, true, true).binds);
        // An unflagged zero base picks clip 0, which binds nothing.
        let p = settle_clip_pick(0, 0, 0, false, false);
        assert_eq!(p.clip, 0);
        assert!(!p.binds);
        assert_eq!(p.bound(), None);
    }

    #[test]
    fn clip_bank_is_one_based() {
        assert_eq!(clip_bank(1, true), Some((ClipBank::Party, 0)));
        assert_eq!(clip_bank(0x3FF, false), Some((ClipBank::Scene, 0x3FE)));
        assert_eq!(clip_bank(0x401, false), Some((ClipBank::High, 0)));
        assert_eq!(clip_bank(0, true), None);
    }
}
