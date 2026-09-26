//! Monster **action-tag** lookups - the battle action SM's calls into the
//! first-byte tag search `FUN_80050E2C`
//! ([`legaia_asset::monster_archive::find_action_by_tag`]).
//!
//! A monster's staged anim id is an index into its archive record's
//! `+0x4C` action-record array, and the byte at `record[0]` of each entry is a
//! semantic **tag**, not that index. So when the SM wants "the walk" or "the
//! close-in", it cannot stage a literal id the way the party arms do (a party
//! file is identity-ordered, so entry `1` *is* tag `1`); it searches the
//! monster's table for the tag and stages the index it finds, or retail's
//! `0xFF` sentinel when the monster carries none.
//!
//! The SM's call sites, read off `overlay_battle_action_801e295c.txt` and
//! `overlay_battle_action_801e7824.txt`:
//!
//! | site | tag | what it stages |
//! |---|---|---|
//! | `0x801E3268` | [`APPROACH_TRANSITION_TAG`] | state `0x14`'s monster arm: the pre-approach clip; a hit routes to `0x15` |
//! | `0x801E32A4` | [`WALK_TAG`] | the same arm's fallback when the monster has no `0x20` entry; routes to `0x19` |
//! | `0x801E3340` | [`WALK_TAG`] | state `0x15`, once the pre-approach clip has committed; routes to `0x16` |
//! | `0x801E33D0` | [`CLOSE_IN_TAG`] | state `0x16` on arrival; routes to `0x17` |
//! | `0x801E55D8` | [`KO_TAUNT_TAG`] | the end-of-attack arm, when a monster's target is down and a party member still stands |
//! | `0x801E7858` | [`WALK_TAG`] | the capture takedown `FUN_801E7824` |
//!
//! The roster census over PROT 0867 is what makes the routing matter: of the
//! 186 populated monster records, 180 carry no tag-`0x20` entry and so take
//! the `0x19` walk exactly as a party member does; every record carries a
//! tag-`1` entry; and the six that do carry `0x20` all carry `0x21` too, at
//! indices well past `1` (monster 73's pre-approach is entry `10`).

use super::BattleActionHost;
use legaia_asset::monster_archive::{NO_ACTION_ENTRY, find_action_by_tag};

/// The pre-approach transition clip (`li a1,0x20` at `0x801E3260`).
pub const APPROACH_TRANSITION_TAG: u8 = 0x20;
/// The walk / Move clip (`li a1,0x1` at `0x801E329C`, `0x801E3338`,
/// `0x801E7850`).
pub const WALK_TAG: u8 = 0x01;
/// The close-in clip staged on arrival (`li a1,0x21` at `0x801E33C8`).
pub const CLOSE_IN_TAG: u8 = 0x21;
/// The taunt a monster plays after downing its target (`li a1,0x22` at
/// `0x801E55D0`).
pub const KO_TAUNT_TAG: u8 = 0x22;

/// Retail's `FUN_80050E2C(record + 0x4C, tag, record[0x4A])` for the actor in
/// `slot`: the index of its first action entry tagged `tag`, or
/// [`NO_ACTION_ENTRY`] when it carries none.
///
/// `None` only when the host resolves no action table for the slot at all
/// ([`BattleActionHost::monster_action_tags`]); callers then leave the queued
/// anim untouched.
///
/// REF: FUN_80050E2C (call sites `0x801E3268` / `0x801E32A4` / `0x801E3340` /
/// `0x801E33D0` / `0x801E55D8` in `FUN_801E295C`, `0x801E7858` in
/// `FUN_801E7824`)
pub fn monster_action_by_tag<H: BattleActionHost + ?Sized>(
    host: &H,
    slot: u8,
    tag: u8,
) -> Option<u8> {
    let tags = host.monster_action_tags(slot)?;
    // The count is `lbu a2,0x4a(a0)` - a byte - so the scan never looks past
    // entry 255 whatever the host hands over.
    let tags = &tags[..tags.len().min(usize::from(u8::MAX))];
    Some(find_action_by_tag(tags, tag).unwrap_or(NO_ACTION_ENTRY))
}
