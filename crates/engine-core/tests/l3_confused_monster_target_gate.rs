//! Reach conversion: the **monster-AI target resolver** `FUN_801E7320`
//! (`world/battle/monster_ai.rs` `801e7320`), a GATED-(b) row whose gate is
//! "a monster whose `field_flags & 0x380` is set".
//!
//! That bitfield is retail's confuse class. The engine does not model
//! `+0x16E` bit-for-bit, so `World::maybe_confuse_retarget` bridges from
//! [`StatusKind::Confuse`] instead - which means the gate is seeded by
//! *landing Confuse on a monster*, and no pad ladder does: nothing in a
//! from-boot playthrough puts Confuse on the enemy side (the party learns no
//! confusing magic in the reachable spine, and a monster never confuses its
//! own ally).
//!
//! So the status is seeded directly, exactly as the Stone-gaze conversion
//! seeded Stone, and the fight is then driven through the ordinary live
//! battle loop (`World::tick`), not by calling the resolver.
//!
//! ## What the resolver has to be checked for
//!
//! "It ran" is not the observable. A confused monster must **act
//! uncontrollably against its own side**: the resolver rewrites the
//! `+0x1DD` target class the picker left behind into a living slot on the
//! *caster's* band. A retarget that silently picked another party member, or
//! that never fired, is indistinguishable from an unwired resolver unless the
//! test contrasts a confused monster against an unconfused one in the same
//! fight - which is what the assertions below do.
//!
//! ## What the first execution found
//!
//! Checking the *damage* rather than the target byte turned up a defect the
//! whole confuse mechanic sits on: `World::resolve_attack_target`
//! (`world/battle/loop_driver.rs`) clamps an armed target to the **opposing**
//! side, so a confused actor's rewritten byte fails the range test and the
//! swing falls back to `first_living_opponent_of`. Confusion therefore has no
//! effect on where damage lands, on either side. Written up on the ignored
//! repro `the_retarget_lands_the_damage_on_an_ally_not_on_the_party`.
//!
//! Disc-free: synthetic party + the vanilla monster / formation tables.

use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::status_effects::StatusKind;

const PARTY: u8 = 3;
const MONSTERS: u8 = 3;

/// A battle-ready world: three party members, three monsters, all alive, the
/// live loop on so `World::tick` actually cycles turns.
fn battle_world(seed: u32) -> World {
    let mut w = World::new();
    while w.actors.len() < (PARTY + MONSTERS) as usize {
        w.actors.push(Actor::default());
    }
    w.party_count = PARTY;
    w.load_party(legaia_save::Party::zeroed(PARTY as usize));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.enter_battle(PARTY, MONSTERS);
    for i in 0..(PARTY + MONSTERS) as usize {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 400;
        w.actors[i].battle.max_hp = 400;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 40);
    }
    w.mode = SceneMode::Battle;
    w.live_gameplay_loop = true;
    w.rng_state = seed;
    w
}

/// Every distinct `(action_category, active_target)` the actor at `slot`
/// carried, sampled once per frame.
///
/// The category is kept rather than filtered on, because the resolver has a
/// terminal arm that **zeroes** it: `class < 3` re-rolling onto the caster's
/// own slot takes retail's `clear_category_self` path (`sb zero, 0x1DE`,
/// target = self). Filtering on `category == 3` therefore drops exactly the
/// outcome that proves the arm ran, and reads as "the monster never acted".
fn armed_actions(w: &mut World, slot: u8, frames: usize) -> Vec<(u8, u8)> {
    let mut seen: Vec<(u8, u8)> = Vec::new();
    for _ in 0..frames {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
        let a = &w.actors[slot as usize];
        let rec = (a.battle.action_category, a.battle.active_target);
        // Skip the pre-turn idle: category 0 targeting slot 0 is the state a
        // fresh actor table starts in, not an armed action.
        if rec == (0, 0) || seen.contains(&rec) {
            continue;
        }
        seen.push(rec);
    }
    seen
}

/// The four seeds every case sweeps. A single seed measures one draw of a
/// re-roll loop, and the resolver's arms are chosen by that draw.
const SEEDS: [u32; 4] = [0x1234_5678, 0x0BAD_F00D, 0xDEAD_BEEF, 0x5EED_0001];

#[test]
fn an_unconfused_monster_only_ever_arms_a_party_target() {
    // The contrast half. Without it, "a confused monster targets its own
    // band" could be true of every monster and measure nothing.
    let mut armed = 0usize;
    for seed in SEEDS {
        let mut w = battle_world(seed);
        let actions = armed_actions(&mut w, PARTY, 600);
        assert!(
            !actions.is_empty(),
            "seed {seed:#x}: monster slot {PARTY} never armed an action"
        );
        for (cat, t) in &actions {
            assert_eq!(
                *cat, 3,
                "seed {seed:#x}: an unconfused monster's category must stay 3 \
                 - only the retarget's self arm clears it"
            );
            assert!(
                *t < PARTY,
                "seed {seed:#x}: an unconfused monster armed slot {t}, which \
                 is not on the party band"
            );
        }
        armed += actions.len();
    }
    assert!(armed > 0);
}

#[test]
fn a_confused_monster_swings_at_its_own_band() {
    // Seed the gate: Confuse on the monster in the first enemy slot.
    let mut any_ally_target = false;
    let mut any_self_clear = false;
    for seed in SEEDS {
        let mut w = battle_world(seed);
        w.status_effects.apply(PARTY, StatusKind::Confuse);
        assert!(
            w.status_effects
                .statuses(PARTY)
                .iter()
                .any(|s| s.kind == StatusKind::Confuse),
            "the fixture must actually land Confuse, or every assertion below \
             is vacuous"
        );

        let actions = armed_actions(&mut w, PARTY, 600);
        assert!(
            !actions.is_empty(),
            "seed {seed:#x}: the confused monster never armed an action"
        );
        for (cat, t) in &actions {
            assert!(
                *t >= PARTY,
                "seed {seed:#x}: a confused monster armed party slot {t} - the \
                 retarget is not rewriting every uncontrolled swing"
            );
            if *t == PARTY {
                // The re-roll landed on the caster itself: retail's arm clears
                // the category and keeps self as the target.
                assert_eq!(
                    *cat, 0,
                    "seed {seed:#x}: a self-target must clear the action \
                     category (retail `sb zero, 0x1DE`)"
                );
                any_self_clear = true;
            } else {
                assert_eq!(*cat, 3, "seed {seed:#x}: an ally swing stays armed");
                any_ally_target = true;
            }
        }
    }
    assert!(
        any_ally_target,
        "the confuse retarget never moved a swing onto another monster - the \
         resolver did not run, or every draw landed on self"
    );
    assert!(
        any_self_clear,
        "the self-target arm was never taken across four seeds - the branch \
         that clears the action category is unmeasured here"
    );
}

/// HP across the monster band / the party.
fn band_hp(w: &World) -> u32 {
    (PARTY as usize..(PARTY + MONSTERS) as usize)
        .map(|i| w.actors[i].battle.hp as u32)
        .sum()
}
fn party_hp(w: &World) -> u32 {
    (0..PARTY as usize)
        .map(|i| w.actors[i].battle.hp as u32)
        .sum()
}

/// Drive a fight in which **only monsters can act**: every party member is
/// asleep, so `actor_blocked_from_acting` skips its turn before either the
/// command menu or the auto-attack arm can run. Returns
/// `(monster-band HP lost, party HP lost)`.
///
/// Without that gate the party's own auto-attacks damage the band and "the
/// monster band took damage" is true whether or not anything was confused -
/// the vacuous shape this test exists to avoid.
fn damage_with_only_monsters_acting(seed: u32, confuse_first_monster: bool) -> (u32, u32) {
    let mut w = battle_world(seed);
    for slot in 0..PARTY {
        w.status_effects.apply(slot, StatusKind::Sleep);
        assert!(
            w.status_effects
                .statuses(slot)
                .iter()
                .any(|s| s.kind == StatusKind::Sleep && s.kind.blocks_actions()),
            "the fixture must actually park the party, or the contrast is lost"
        );
    }
    if confuse_first_monster {
        w.status_effects.apply(PARTY, StatusKind::Confuse);
    }
    let band_before = band_hp(&w);
    let party_before = party_hp(&w);
    for _ in 0..1200 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
        if band_hp(&w) < band_before || party_hp(&w) < party_before {
            break;
        }
    }
    (
        band_before.saturating_sub(band_hp(&w)),
        party_before.saturating_sub(party_hp(&w)),
    )
}

#[test]
fn a_confused_monster_still_swings_and_the_target_byte_is_the_only_thing_that_moved() {
    // What is measurable today, and the diagnosis behind the ignored repro
    // below: the retarget writes the monster band into `+0x1DD`, the swing
    // still lands, and it lands on the **party**.
    for seed in SEEDS {
        // Control: nothing confused. Monsters hit the party, never each other.
        let (band, party) = damage_with_only_monsters_acting(seed, false);
        assert_eq!(band, 0, "seed {seed:#x}: unconfused friendly fire");
        assert!(party > 0, "seed {seed:#x}: no monster swing landed at all");

        // Confused: the swing still lands, and still on the party.
        let (band, party) = damage_with_only_monsters_acting(seed, true);
        assert!(
            party > 0,
            "seed {seed:#x}: a confused monster stopped attacking entirely - \
             that would be a different (worse) defect than the one below"
        );
        assert_eq!(band, 0, "seed {seed:#x}: see the ignored repro below");
    }
}

/// DEFECT REPRO (ignored: it fails today, and the fix is outside this lane's
/// fence). **Confusion currently changes nothing about where damage lands.**
///
/// `World::resolve_monster_target` (`FUN_801E7320`) does its job - the four
/// live tests above pin the rewritten `+0x1DD` byte on the caster's own band -
/// but the strike resolver throws it away.
/// `World::resolve_attack_target` (`crates/engine-core/src/world/battle/
/// loop_driver.rs`) clamps the armed target to the *opposing* side:
///
/// ```text
/// let (lo, hi) = if attacker < pc { (pc, n) } else { (0, pc) };
/// if (lo..hi).contains(&t) && alive { return Some(t) }
/// self.first_living_opponent_of(attacker)
/// ```
///
/// A confused monster's target is `>= pc`, which is outside `0..pc`, so every
/// friendly-fire swing silently falls back to `first_living_opponent_of` - a
/// party member. The same holds mirrored for a confused party member. So the
/// whole confuse mechanic is inert at the point it would be felt, and no
/// assertion on the target byte can see it.
///
/// Retail has no such clamp: `FUN_801EC3E4` resolves against the target the
/// action SM left in `+0x1DD`, and the side range is a port-side safety net.
/// The fix is to treat the armed target as authoritative when it names a
/// living actor, and keep the opposing-side fallback only for an unset or
/// dead one.
#[test]
#[ignore = "defect: resolve_attack_target clamps the armed target to the opposing side, discarding the confuse retarget"]
fn the_retarget_lands_the_damage_on_an_ally_not_on_the_party() {
    let mut confused_hits = 0usize;
    for seed in SEEDS {
        let (band, _) = damage_with_only_monsters_acting(seed, true);
        if band > 0 {
            confused_hits += 1;
        }
    }
    assert!(
        confused_hits > 0,
        "a confused monster must be able to hit its own band"
    );
}

#[test]
fn a_confused_party_member_is_flipped_the_other_way() {
    // The same resolver, entered from the party side: a confused member is
    // never handed the command menu - it auto-swings and the class-3..6 arm
    // flips it onto the party band. Retail's `+0x1DD` class space is one
    // space for both sides, so a port that only handled the monster branch
    // would pass every assertion above and fail here.
    //
    // All three members are confused: a *player-driven* battle parks on the
    // first unconfused member's command menu and never reaches anyone else's
    // turn, so a single confused member is unobservable through this host.
    let mut any = false;
    for seed in SEEDS {
        let mut w = battle_world(seed);
        w.battle_player_driven = true;
        for slot in 0..PARTY {
            w.status_effects.apply(slot, StatusKind::Confuse);
        }
        // The contrast: with no confusion the same world parks on a command
        // session instead of arming anything.
        let mut control = battle_world(seed);
        control.battle_player_driven = true;
        for _ in 0..600 {
            control.tick();
            if control.battle_command.is_some() {
                break;
            }
        }
        assert!(
            control.battle_command.is_some(),
            "seed {seed:#x}: an unconfused player-driven party must open the \
             command menu - otherwise the auto-act below is not the \
             confusion's doing"
        );

        for slot in 0..PARTY {
            let actions = armed_actions(&mut w, slot, 600);
            for (cat, t) in &actions {
                assert!(
                    *t < PARTY,
                    "seed {seed:#x}: confused party slot {slot} armed slot {t} \
                     - the retarget did not flip it onto the party band"
                );
                // The asymmetry the two branches really have, read off
                // `FUN_801E7320`'s instruction stream: the class-`0..2` arm
                // ends with `bne v0,v1` against `ctx[+0x13]` and clears
                // `+0x1DE` on a self-hit (`0x801E73E0..0x801E73FC`); the
                // class-`3..6` arm at `0x801E7484` jumps straight to the
                // epilogue with **no** self test. So a confused party member
                // that rolls itself keeps an armed swing at itself, and a
                // port that "fixed" that symmetry would be wrong.
                assert_eq!(
                    *cat, 3,
                    "seed {seed:#x}: the party-band arm must never clear the \
                     action category, self-target or not"
                );
                any = true;
            }
        }
        assert!(
            w.battle_command.is_none(),
            "seed {seed:#x}: a fully confused party must never be handed the \
             command menu"
        );
    }
    assert!(
        any,
        "no confused party member ever armed an action across four seeds"
    );
}
