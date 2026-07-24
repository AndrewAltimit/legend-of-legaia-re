//! The per-frame **cast census** - the head of the battle cast tick
//! `FUN_801E09F8` (battle overlay `0898`, `see
//! ghidra/scripts/funcs/overlay_battle_action_801e09f8.txt`).
//!
//! Every frame, before it drives any effect child, the cast tick rebuilds four
//! context bytes from scratch. Three of them are the gates the magic band's
//! exit states wait on, and the fourth pair is the auto-target latch the
//! item / magic retarget arms read:
//!
//! | ctx byte | what it counts | who reads it |
//! |---|---|---|
//! | `+0x249` | visible actors mid-animation | state `0x2E` (`MagicExit`) |
//! | `+0x24D` | occupied effect-child slots | state `0x2D` (`MagicRecovery`) |
//! | `+0x24A` | the *sole* living party slot, 1-based (`0` = not exactly one) | state `0x28` category `8` retarget |
//! | `+0x24B` | the *sole* living monster slot, 0-based (`0` = not exactly one) | state `0x28` category `9` retarget |
//!
//! Because the census is rebuilt from zero each frame, an exit gate is a
//! *measurement*, not a latch: a state that waits on `+0x249 == 0` is waiting
//! for every visible actor's animation to land, and an actor left with a
//! non-zero render word `+0x4` and a stuck `+0x1D9` holds that state open
//! indefinitely. That is the magic-band sibling of the HP-bar settle park in
//! [`crate::battle_hp_bar`], and worth knowing when a cast never finishes.
//!
//! Transcribed from the DISASSEMBLY at `0x801E0A44..0x801E0BF0`, not the C.
//! The rest of `FUN_801E09F8` - the per-slot effect-child driver that runs
//! once the census finds work - is **not** ported here.

/// The per-slot inputs the census reads. Retail walks the actor-pointer table
/// `&DAT_801C9370` and reads three fields per entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CensusSlot {
    /// `actor[+0x4]` - the mesh colour/tint word. Zero means the actor is not
    /// being drawn, and the animation census skips it entirely.
    pub render_word: u32,
    /// `actor[+0x1D9]` - the **current** animation id.
    pub current_anim: u8,
    /// `actor[+0x14C]` - live HP, doubling as the liveness flag.
    pub liveness: u16,
}

/// The four bytes the census produces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CastCensus {
    /// `ctx[+0x249]`.
    pub anim_outstanding: u8,
    /// `ctx[+0x24D]`.
    pub effect_children: u8,
    /// `ctx[+0x24A]` - 1-based, `0` when the living party count is not one.
    pub sole_party_target: u8,
    /// `ctx[+0x24B]` - 0-based, `0` when the living monster count is not one.
    pub sole_monster_target: u8,
}

/// Retail's census loop bound: slots `0..=6`. Slot 7 is not walked - the same
/// seven-slot window [`crate::battle_action::clear_pool_flag_words`] uses.
pub const CENSUS_SLOTS: usize = 7;

/// The `ctx[+0x24E..=+0x251]` / `ctx[+0x252..=+0x255]` effect-child arrays are
/// four entries each.
pub const EFFECT_CHILD_SLOTS: usize = 4;

/// The animation id a **party** slot holds while down. A party slot sitting in
/// it is subtracted back out of the animation census, so a knocked-out member
/// does not hold a cast open forever (`0x801E0AB0`: `li v0,0x8`).
pub const DOWNED_ANIM_ID: u8 = 8;

/// Rebuild the four census bytes.
///
/// `kind_slots` is `ctx[+0x24E..=+0x251]` and `child_slots` is
/// `ctx[+0x252..=+0x255]`. Retail's early-out at `0x801E0BA8` skips the whole
/// rest of the tick - **including the `+0x24D` count** - when every
/// `kind_slots` entry is zero, so an empty kind array leaves
/// [`CastCensus::effect_children`] at zero regardless of what `child_slots`
/// holds. That ordering is load-bearing and is reproduced here.
///
/// PORT: FUN_801E09F8 (the census head, `0x801E0A44..0x801E0BF0`; the per-slot
/// effect-child driver that follows it is not ported)
pub fn cast_census(
    slots: &[CensusSlot],
    kind_slots: [u8; EFFECT_CHILD_SLOTS],
    child_slots: [u8; EFFECT_CHILD_SLOTS],
) -> CastCensus {
    let mut out = CastCensus::default();
    let mut living_party = 0u8;
    let mut living_monsters = 0u8;

    for i in 0..CENSUS_SLOTS {
        let Some(s) = slots.get(i) else { continue };
        // Animation census - gated on the actor being drawn at all.
        if s.render_word != 0 {
            if s.current_anim != 0 {
                out.anim_outstanding = out.anim_outstanding.wrapping_add(1);
            }
            if i < 3 && s.current_anim == DOWNED_ANIM_ID {
                out.anim_outstanding = out.anim_outstanding.wrapping_sub(1);
            }
        }
        // Liveness census - runs for every slot, drawn or not (retail's
        // `+0x4 == 0` branch rejoins *above* this block, at `0x801E0AD4`).
        if s.liveness != 0 {
            if i < 3 {
                living_party = living_party.wrapping_add(1);
                out.sole_party_target = i as u8 + 1;
            } else {
                living_monsters = living_monsters.wrapping_add(1);
                out.sole_monster_target = i as u8;
            }
        }
    }
    // The latches only survive when exactly one candidate remains; retail
    // branches *over* the clear on `== 1` (`0x801E0B38` / `0x801E0B50`).
    if living_party != 1 {
        out.sole_party_target = 0;
    }
    if living_monsters != 1 {
        out.sole_monster_target = 0;
    }

    if kind_slots.iter().all(|&k| k == 0) {
        return out;
    }
    out.effect_children = child_slots.iter().filter(|&&c| c != 0).count() as u8;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(render: u32, anim: u8, hp: u16) -> CensusSlot {
        CensusSlot {
            render_word: render,
            current_anim: anim,
            liveness: hp,
        }
    }

    #[test]
    fn animation_census_skips_undrawn_actors() {
        let slots = [
            slot(0, 5, 10), // not drawn - not counted
            slot(1, 5, 10), // drawn, animating - counted
            slot(1, 0, 10), // drawn, idle - not counted
            slot(1, 5, 10), // monster, drawn, animating - counted
        ];
        assert_eq!(cast_census(&slots, [0; 4], [0; 4]).anim_outstanding, 2);
    }

    #[test]
    fn a_downed_party_slot_cancels_its_own_animation_count() {
        // Anim 8 on a party slot is added by the `!= 0` test and taken back
        // out by the `== 8` test, netting zero. On a monster slot it stays.
        let party = [slot(1, DOWNED_ANIM_ID, 0)];
        assert_eq!(cast_census(&party, [0; 4], [0; 4]).anim_outstanding, 0);
        let monsters = [
            slot(0, 0, 0),
            slot(0, 0, 0),
            slot(0, 0, 0),
            slot(1, DOWNED_ANIM_ID, 5),
        ];
        assert_eq!(cast_census(&monsters, [0; 4], [0; 4]).anim_outstanding, 1);
    }

    #[test]
    fn the_census_walks_seven_slots_not_eight() {
        let mut slots = [slot(1, 1, 1); 8];
        slots[7] = slot(1, 1, 1);
        assert_eq!(cast_census(&slots, [0; 4], [0; 4]).anim_outstanding, 7);
    }

    #[test]
    fn sole_survivor_latches_need_exactly_one() {
        // Two living monsters -> no latch.
        let two = [
            slot(1, 0, 10),
            slot(0, 0, 0),
            slot(0, 0, 0),
            slot(1, 0, 5),
            slot(1, 0, 5),
        ];
        let c = cast_census(&two, [0; 4], [0; 4]);
        assert_eq!(c.sole_monster_target, 0);
        assert_eq!(c.sole_party_target, 1, "slot 0, 1-based");
        // Down to one monster -> the latch appears, 0-based.
        let one = [
            slot(1, 0, 10),
            slot(0, 0, 0),
            slot(0, 0, 0),
            slot(1, 0, 0),
            slot(1, 0, 5),
        ];
        assert_eq!(cast_census(&one, [0; 4], [0; 4]).sole_monster_target, 4);
        // No survivors at all also clears (count 0 != 1).
        let none = [slot(1, 0, 0); 7];
        let c = cast_census(&none, [0; 4], [0; 4]);
        assert_eq!((c.sole_party_target, c.sole_monster_target), (0, 0));
    }

    #[test]
    fn effect_child_count_is_gated_on_the_kind_array() {
        // Occupied child slots but an empty kind array: retail returns before
        // the `+0x24D` count, so the recovery gate reads clear.
        assert_eq!(
            cast_census(&[], [0, 0, 0, 0], [1, 1, 0, 0]).effect_children,
            0
        );
        // One non-zero kind entry is enough to reach the count.
        assert_eq!(
            cast_census(&[], [0, 3, 0, 0], [1, 1, 0, 0]).effect_children,
            2
        );
    }
}
