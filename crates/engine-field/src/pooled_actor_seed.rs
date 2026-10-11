//! Two small field-overlay helpers: `FUN_801E5834`, which seeds a pooled
//! actor off the descriptor `0x801F2978`, and `FUN_801E58A8`, an actor
//! **anim-clip pick** (not a list row count - see below). Both live in PROT
//! 0897 at base `0x801CE818`.
//!
//! ## `FUN_801E5834` - pooled menu-actor spawn
//!
//! Allocates one entry from the actor pool `_DAT_8007C34C` for the fixed
//! descriptor `0x801F2978` through `FUN_80020DE0`, and on a non-null result
//! writes five halfwords: `+0x54 = 0` (the phase byte every handler in this
//! band dispatches on) and the four arguments into `+0x50`, `+0x14`, `+0x16`
//! and `+0x9C`. A null allocation is silently dropped - there is no retry and
//! no error path.
//!
//! ## `FUN_801E58A8` - actor anim-clip pick
//!
//! Writes `+0x5E = -2`, derives a clip index into `+0x5C` from three globals
//! and one actor flag bit, and hands the actor to the clip selector
//! `FUN_800204F8`. Read out of the disassembly (`0x801E58A8..0x801E59A4`):
//!
//! ```text
//!   base     = word @ 0x8007BDD8   ; the clip base the field tick picks
//!   leader   = u16  @ 0x8007B8F8   ; the party leader's character id
//!   override = word @ 0x8007B6AC   ; op 4C CE's value
//!
//!   if base == 99:                     clip = leader + 1        ; clear bit
//!   else if actor[+0x10] & 0x01000000:
//!       if override != 0:              clip = base + override - 1 ; clear, tick, re-set
//!       else:                          clip = base + leader*7
//!   else:                              clip = base
//!   FUN_800204F8(actor)
//! ```
//!
//! The `leader * 7` term is `(leader << 3) - leader` in the body. The flag bit
//! `0x01000000` is cleared **around** the `FUN_800204F8` tick in the
//! `override != 0` arm and restored immediately after, which is the only
//! reason that arm returns early instead of falling into the shared tick -
//! both paths tick the actor exactly once.
//!
//! An earlier reading named this a "list row-count seed" and the leader word
//! a page count. The identity comes from its two neighbours in the bytes: the
//! value lands in `+0x5C`, the word `FUN_800204F8` reads as the clip to bind,
//! and the **same arithmetic** runs inline as the tail of the field vertical
//! settle `FUN_801D1BA0` (`0x801D1D88..0x801D1EAC`) - there on the player,
//! every grounded frame that did not hop, with `_DAT_8007BDD8` written by
//! `FUN_801D1EC4` just before. `leader * 7` is the per-character stride into
//! the party locomotion clip bank.
//!
//! REF: FUN_80020de0, FUN_800204f8  -- callees, not ported here
//!
//! `see ghidra/scripts/funcs/801e5834.txt`,
//! `see ghidra/scripts/funcs/801e58a8.txt`
//!
//! ## Neither one runs in retail
//!
//! Both are real entries - `locate-entry-image.py` resolves each in PROT 0897
//! with its own frame - and neither is reached. A five-form sweep of
//! `SCUS_942.54`, all 31 based overlay images and the raw bytes of every
//! extracted `PROT.DAT` entry finds no literal address word, no `jal`, no `j`,
//! no in-image branch and no `lui`+`addiu` materialisation for either
//! (`docs/tooling/address-reference-scan.md`).
//!
//! `FUN_801E58A8` needs the image qualifier. Its one scan hit is a branch in
//! the **battle_action** overlay, which shares the slot-A load base and holds
//! unrelated code at that VA; a branch is PC-relative and cannot leave its own
//! image, so it is not a reference to the field-overlay routine.
//! `--home field` marks it.
//!
//! That is why the two notes below read `REPLACED-BY:` and not `NOT WIRED:`.
//! Neither can run in the port because neither runs in retail. The spawn's
//! write set is held on the side `SubmodeScreen` struct; the clip pick's live
//! twin is `FUN_801D1BA0`'s tail, whose job the engine's field clip player
//! does (`legaia_engine_core::field_anim`).

/// The `base == 99` special case in the clip pick.
pub const BASE_SENTINEL: u16 = 99;

/// The actor flag bit the clip pick tests and toggles (the per-character
/// clip-bank class).
pub const CLIP_FLAG_BIT: u32 = 0x0100_0000;

/// The value written to `actor[+0x5E]` on every call.
pub const CLIP_SENTINEL: i16 = -2;

/// The five fields `FUN_801E5834` writes into a freshly-allocated pool entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuActorSeed {
    /// `+0x54` - phase, always zeroed on spawn.
    pub phase: u16,
    /// `+0x50` - sub-handler id (the first argument).
    pub handler: u16,
    /// `+0x14` - screen X (the second argument).
    pub x: u16,
    /// `+0x16` - screen Y (the third argument).
    pub y: u16,
    /// `+0x9C` - the dwell / parameter halfword (the fourth argument).
    pub param: u16,
}

/// Build the field set a pooled menu-actor spawn installs.
///
/// The allocation itself (`FUN_80020DE0` against the pool `_DAT_8007C34C`
/// and the descriptor `0x801F2978`) is host plumbing; this is the write set
/// that follows a successful one. Retail drops the whole write on a null
/// allocation, which the caller expresses by not calling this.
///
/// PORT: FUN_801e5834
///
/// REPLACED-BY: `World::open_field_submode_screen`'s side `SubmodeScreen`
/// struct, which holds the handler id, cursor and panel pen this seed would
/// write onto the pooled actor.
///
/// Additionally **retail-unreachable** - nothing on the disc reaches
/// `FUN_801E5834` in any reference form, so no wiring pass could close this
/// row even if the fields existed (see the module's "Neither one runs in
/// retail"). What follows is why the port could not run it *either*, kept
/// because it names a real gap in the engine's pooled `Actor`.
///
/// The engine has the pool but not the write set. Its one
/// spawner of a handler actor is
/// `legaia_engine_core::world::World::open_field_submode_screen`, which
/// takes a free slot through `spawn_handler_actor`, clears the phase byte
/// `+0x54`, and then keeps the handler id, the cursor and the panel pen on
/// a **side** `SubmodeScreen` struct instead of on the actor - so no pooled
/// actor carries `+0x50` / `+0x14` / `+0x16` / `+0x9C` for this seed to
/// fill, and `MenuActorSeed` has nowhere to land. The prerequisite is
/// those four fields on the engine's pooled `Actor`, not a new subsystem;
/// the descriptor itself (`0x801F2978`) is additionally a second
/// pooled-actor family that no engine path spawns at all - the engine only
/// ever seats `ActorHandler::SubmodeDriver`.
pub fn menu_actor_seed(handler: u16, x: u16, y: u16, param: u16) -> MenuActorSeed {
    MenuActorSeed {
        phase: 0,
        handler,
        x,
        y,
        param,
    }
}

/// What one clip pick produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorClipPick {
    /// The clip index written to `actor[+0x5C]`.
    pub clip: u16,
    /// The value of the `0x01000000` flag bit after the call.
    pub flag_set: bool,
}

/// Derive the actor's clip index.
///
/// `base` is `_DAT_8007BDD8`, `leader` is `_DAT_8007B8F8`, `clip_override` is
/// `_DAT_8007B6AC`, and `flag_set` is `actor[+0x10] & 0x01000000 != 0` on
/// entry.
///
/// PORT: FUN_801e58a8
///
/// REPLACED-BY: the engine field clip player (`legaia_engine_core::field_anim`,
/// the idle / walk slot of the party locomotion bank), which picks the
/// player's clip where `FUN_801D1BA0`'s inline twin of this arithmetic does.
///
/// Additionally **retail-unreachable** - `FUN_801E58A8`'s only scan hit is a
/// branch from another overlay at the same VA, which cannot reach it (see
/// the module's "Neither one runs in retail"). The live copy of the same
/// arithmetic is the tail of `FUN_801D1BA0`, and what the port lacks there is
/// the clip base `_DAT_8007BDD8`, which `FUN_801D1EC4` writes on four arms
/// the port does not model.
pub fn actor_clip_pick(
    base: u16,
    leader: u16,
    clip_override: u32,
    flag_set: bool,
) -> ActorClipPick {
    if base == BASE_SENTINEL {
        return ActorClipPick {
            clip: leader.wrapping_add(1),
            flag_set: false,
        };
    }
    if !flag_set {
        return ActorClipPick {
            clip: base,
            flag_set: false,
        };
    }
    if clip_override != 0 {
        // The bit is cleared around the tick and restored, so it ends set.
        return ActorClipPick {
            clip: base.wrapping_add(clip_override as u16).wrapping_sub(1),
            flag_set: true,
        };
    }
    ActorClipPick {
        clip: base.wrapping_add(leader.wrapping_mul(7)),
        flag_set: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_zeroes_the_phase_and_keeps_the_four_arguments() {
        let s = menu_actor_seed(0x22, 0x50, 0x30, 9);
        assert_eq!(s.phase, 0);
        assert_eq!((s.handler, s.x, s.y, s.param), (0x22, 0x50, 0x30, 9));
    }

    #[test]
    fn sentinel_base_uses_leader_plus_one_and_clears_the_flag() {
        let r = actor_clip_pick(BASE_SENTINEL, 4, 77, true);
        assert_eq!(r.clip, 5);
        assert!(!r.flag_set);
    }

    #[test]
    fn sentinel_base_wins_over_the_flag_being_clear() {
        let r = actor_clip_pick(BASE_SENTINEL, 0, 0, false);
        assert_eq!(r.clip, 1);
    }

    #[test]
    fn flag_clear_passes_the_base_through() {
        let r = actor_clip_pick(12, 4, 77, false);
        assert_eq!(r.clip, 12);
        assert!(!r.flag_set);
    }

    #[test]
    fn flag_set_with_override_adds_override_minus_one_and_restores_the_flag() {
        let r = actor_clip_pick(12, 4, 5, true);
        assert_eq!(r.clip, 16);
        assert!(r.flag_set);
    }

    #[test]
    fn flag_set_without_override_scales_leader_by_seven() {
        // (leader << 3) - leader, not (leader << 3).
        let r = actor_clip_pick(12, 4, 0, true);
        assert_eq!(r.clip, 12 + 28);
        assert!(r.flag_set);
    }
}
