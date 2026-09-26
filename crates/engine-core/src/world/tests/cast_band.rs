//! The action SM's cast band as the engine drives it: a player Seru cast
//! routed through the magic + summon bands with the stager folding the
//! outcome at its strike, and a monster cast routed through the magic band
//! with the fold on the `0x29` exit.

use super::*;
use legaia_engine_vm::battle_action::ActionState;
use legaia_engine_vm::battle_target_group::RENDER_FLAG_HIDDEN;

/// A one-member party with Gimard (0x81) learned, one monster, the retail
/// Seru-magic catalog, the spell submenu open on the caster.
fn seru_cast_world() -> World {
    let mut world = World {
        party: crate::world::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    // A live session seats the creature above the eight battle slots.
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.battle.player_driven = true;
    world.mode = SceneMode::Battle;
    world.tables.spell_catalog = crate::retail_magic::retail_seru_magic_catalog();
    world.actors[0].active = true;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.mp = 50;
    world.actors[0].battle.liveness = 1;
    world.actors[0].move_state.world_x = 82;
    world.actors[0].move_state.world_z = -542;
    world.actors[0].battle.facing_angle = 0xFD9;
    world.set_battle_magic(0, 100);
    world.actors[1].active = true;
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.liveness = 1;
    world.actors[1].move_state.world_z = 543;
    let mut party = legaia_save::Party::zeroed(1);
    let mut list = party.members[0].spell_list();
    list.count = 1;
    list.ids[0] = 0x81;
    party.members[0].set_spell_list(list);
    world.party.roster = party;
    world.battle_ctx.active_actor = 0;
    world.battle.spell_menu = world.build_battle_spell_session(0);
    world
}

fn confirm_cast(world: &mut World) {
    use crate::input::PadButton;
    // Cross on the spell row opens the target cursor; Cross again confirms.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    take_commit_begin(world);
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    take_commit_begin(world);
    world.set_pad(0);
}

#[test]
fn a_seru_cast_runs_the_summon_band_and_the_stager_folds_once_at_its_strike() {
    let mut world = seru_cast_world();
    let cost = u16::from(world.tables.spell_catalog.get(0x81).unwrap().mp_cost);
    assert!(cost > 0);
    confirm_cast(&mut world);

    // The confirm armed the band, not the fold: nothing has landed yet.
    assert!(world.battle.spell_menu.is_none(), "spell menu closed");
    assert_eq!(
        world.actors[0].battle.mp, 50,
        "no MP charged at the confirm"
    );
    assert_eq!(world.actors[1].battle.hp, 300, "no damage at the confirm");
    assert_eq!(world.battle_ctx.action_state, ActionState::Begin.as_byte());
    assert_eq!(
        world.actors[0].battle.action_category,
        legaia_engine_vm::battle_action::ActionCategory::Magic.as_byte()
    );
    assert_eq!(world.actors[0].battle.params[0], 0x81);
    assert!(
        world.casting.pending_cast.is_some(),
        "the cast's outcome is owed"
    );

    let mut states = Vec::new();
    let mut spawn_requests = 0;
    let mut hidden_seen = false;
    let mut fade_delay_seen = false;
    let mut fade_white_seen = false;
    let mut folded_at = None;
    for tick in 0..0x400 {
        world.set_pad(0);
        let _ = world.tick();
        let s = world.battle_ctx.action_state;
        if states.last() != Some(&s) {
            states.push(s);
        }
        if let Some((id, origin)) = world.take_pending_summon_spawn() {
            spawn_requests += 1;
            assert_eq!(id, 0x81);
            // Seated behind the caster on the party side, the capture's law.
            assert_eq!(origin[0], 82);
            assert_eq!(origin[2], -542 - crate::world::battle::SUMMON_SPAWN_BEHIND);
            // A host seats it; the world adopts the seat.
            world.seat_summon_actor(9);
            assert_eq!(world.casting.summon_actor_slot, Some(9));
            assert_eq!(world.actors[9].move_state.world_z, origin[2]);
            // The band's 0x28 re-faced the caster at its target; the seat
            // wears that live facing (the capture's slot-7 `0xFD9` = slot 0's).
            assert_eq!(
                world.actors[9].battle.facing_angle, world.actors[0].battle.facing_angle,
                "the caster's facing"
            );
        }
        if world.actors[0].battle.render_flag == RENDER_FLAG_HIDDEN
            && world.actors[1].battle.render_flag == RENDER_FLAG_HIDDEN
        {
            hidden_seen = true;
            // The summon seat is never hidden by the band.
            assert_ne!(world.actors[9].battle.render_flag, RENDER_FLAG_HIDDEN);
        }
        if world.presentation.fade.is_some() && world.screen_fade_draw().is_none() {
            fade_delay_seen = true;
        }
        if let Some((rgb, abr, ot)) = world.screen_fade_draw()
            && rgb == 0xFF_FFFF
        {
            fade_white_seen = true;
            assert_eq!(abr, 1, "additive flash");
            assert_eq!(ot, 1, "the id the band stamps");
        }
        if folded_at.is_none() && world.casting.pending_cast.is_none() {
            folded_at = Some(tick);
        }
        if world.battle.command.is_some() || world.battle_ctx.active_actor != 0 {
            break;
        }
    }

    let folded_at = folded_at.expect("the stager folded the cast");
    assert_eq!(
        world.actors[0].battle.mp,
        50 - cost,
        "MP charged exactly once (0x28)"
    );
    assert!(world.actors[1].battle.hp < 300, "the strike landed");
    assert_eq!(spawn_requests, 1, "exactly one creature spawn per cast");
    assert!(
        hidden_seen,
        "party + monster hidden while the creature performs"
    );
    assert!(
        fade_delay_seen,
        "the flash-in's 0x14-frame start delay draws nothing"
    );
    assert!(fade_white_seen, "the flash-in lands on white");
    for s in [
        ActionState::MagicCastBegin,
        ActionState::MagicPreCastWait,
        ActionState::SummonInvoke,
        ActionState::SummonFadeIn,
        ActionState::SummonActorFreeze,
        ActionState::SummonSustain,
        ActionState::SummonReturn,
    ] {
        assert!(
            states.contains(&s.as_byte()),
            "band visited {s:?}: {states:02x?}"
        );
    }
    // The strike landed inside the band, after the creature was out.
    assert!(folded_at > 0x14, "not before the 0x29 wait: {folded_at}");
    // Restored + retired by the band's end.
    assert_ne!(world.actors[0].battle.render_flag, RENDER_FLAG_HIDDEN);
    assert_ne!(world.actors[1].battle.render_flag, RENDER_FLAG_HIDDEN);
    assert!(!world.actors[9].active, "the creature was despawned");
    assert!(world.casting.summon_stager.is_none());
    // The flash cue rode the SFX queue.
    assert!(
        world
            .audio
            .battle_sfx_cues
            .iter()
            .any(|c| c.kind == legaia_engine_vm::battle_action::SUMMON_FLASH_CUE),
        "the 0x33 arm's 0x63 cue"
    );
}

#[test]
fn a_seru_cast_with_no_host_seat_still_folds_and_ends() {
    let mut world = seru_cast_world();
    confirm_cast(&mut world);
    let mut folded = false;
    for _ in 0..0x400 {
        world.set_pad(0);
        let _ = world.tick();
        // Nobody seats the creature (a headless driver).
        let _ = world.take_pending_summon_spawn();
        if world.casting.pending_cast.is_none() {
            folded = true;
        }
        if folded && world.casting.summon_stager.is_none() {
            break;
        }
    }
    assert!(folded, "the unseated grace folded the cast");
    assert!(world.actors[1].battle.hp < 300);
    assert!(world.casting.summon_stager.is_none(), "the stager retired");
}

#[test]
fn a_monster_cast_runs_the_magic_band_and_folds_on_leaving_the_wait() {
    use crate::battle_events::BattleEvent;
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party: crate::world::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.tables.monster_catalog = vanilla_monster_catalog();
    world.actors[0].active = true;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    // Bandit Boss (id 5): [Flame 0x20, Thunder Bolt 0x23], 10 MP.
    world.actors[1].active = true;
    world.actors[1].battle.max_hp = 120;
    world.actors[1].battle.hp = 120;
    world.actors[1].battle.mp = 10;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(5);
    world.set_battle_magic(1, 40);
    world.rng_state = BANDIT_BOSS_FLAME_SEED;

    world.take_monster_turn(1);
    assert_eq!(world.actors[1].battle.params[0], 0x20, "picker chose Flame");
    assert_eq!(
        world.actors[1].battle.params[1], 0xFF,
        "no clip installed: terminator"
    );
    assert_eq!(world.actors[0].battle.hp, 200, "nothing landed at the pick");
    assert_eq!(world.actors[1].battle.mp, 10, "nothing charged at the pick");
    assert_eq!(world.battle_ctx.action_state, ActionState::Begin.as_byte());

    let mut states = Vec::new();
    let mut from_wait = None;
    for _ in 0..0x100 {
        world.set_pad(0);
        let _ = world.tick();
        let s = world.battle_ctx.action_state;
        if states.last() != Some(&s) {
            states.push(s);
        }
        if from_wait.is_none()
            && states.contains(&ActionState::MagicPreCastWait.as_byte())
            && s != ActionState::MagicPreCastWait.as_byte()
        {
            from_wait = Some((s, world.casting.pending_cast.is_none()));
        }
        if world.casting.pending_cast.is_none() {
            break;
        }
    }
    let (after, folded) = from_wait.expect("the band left the pre-cast wait");
    assert!(
        folded,
        "folded the frame the band left 0x29 (to {after:#04x})"
    );
    assert!(world.actors[0].battle.hp < 200, "the party took the hit");
    assert_eq!(
        world.actors[1].battle.mp,
        10 - u16::from(world.tables.spell_catalog.get(0x20).unwrap().mp_cost),
        "MP charged once, by the band's 0x28"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1, "one damage popup");
    assert_eq!(fx[0].target_slot, 0);
    // The monster-only spell-name label was raised at 0x28.
    assert!(
        world.pending_battle_events.iter().any(|e| matches!(
            e,
            BattleEvent::UiElement {
                effect_id: 0x4C,
                mode: 0
            }
        )),
        "the 0x4C label fires for a monster caster"
    );
}

#[test]
fn a_party_cast_raises_no_spell_name_label() {
    use crate::battle_events::BattleEvent;
    let mut world = seru_cast_world();
    confirm_cast(&mut world);
    for _ in 0..0x40 {
        world.set_pad(0);
        let _ = world.tick();
        if world.battle_ctx.action_state == ActionState::SummonInvoke.as_byte() {
            break;
        }
    }
    assert!(
        !world.pending_battle_events.iter().any(|e| matches!(
            e,
            BattleEvent::UiElement {
                effect_id: 0x4C,
                mode: 0
            }
        )),
        "0x801E43D0: the label block is skipped for a party id"
    );
}

// ---------------------------------------------------------------------------
// The band's PORT half: the slot-B module code kernels reached from the
// stager tick the action SM drives (`World::run_cast_module_code`).
// ---------------------------------------------------------------------------

/// A minimal battle world with the summon seat live, so the module kernels
/// that pose `actor_table[7]` have something to write.
fn module_code_world() -> World {
    let mut world = World {
        party: crate::world::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    for i in 0..8 {
        world.actors[i].active = true;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.anim_rate = legaia_engine_vm::battle_anim_rate::AnimRate(8);
    }
    world.battle_ctx.active_actor = 0;
    world
}

/// PROT 0909 (Viguro) is spell id `0x87` through the action-id dispatcher's
/// own arithmetic (`903 + (id - 0x81)`), so this resolves with no disc: the
/// stager's arm 0 poses the summon seat and advances the module phase.
#[test]
fn the_viguro_stager_runs_from_the_cast_band_seam() {
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x87), Some(909));
    world.casting.summon_actor_slot = Some(7);
    world.actors[7].battle.active_target = 2;
    world.actors[7].battle.render_flag = 0xFF;

    let run = world
        .run_cast_module_code(0x87, 0)
        .expect("PROT 0909 is a band entry");
    assert_eq!(run.prot_entry, 909);
    assert_eq!(
        world.actors[7].battle.active_target,
        legaia_engine_vm::cast_module_ticks::TARGET_CODE_ENEMY_ROW,
        "arm 0 retargets the summon seat to the enemy row"
    );
    assert_eq!(
        world.actors[7].battle.render_flag, 0,
        "and makes it visible"
    );
    assert_eq!(world.casting.module_phase, 1, "arm 0 advances ctx+0x279");
}

/// PROT 0922 (Puera, spell `0x94`) writes one byte and nothing else, and only
/// on arm 0 - the `bnez a1` at the routine's head.
#[test]
fn the_puera_stager_writes_ctx_278_only_on_arm_zero() {
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x94), Some(922));
    world.run_cast_module_code(0x94, 1).unwrap();
    assert_eq!(world.casting.module_ctx_278, 0);
    world.run_cast_module_code(0x94, 0).unwrap();
    assert_eq!(world.casting.module_ctx_278, 3);
}

/// PROT 0927 (Juggernaut, spell `0x99`) sweeps the enemy row with the
/// never-kill clamp: the party is untouched and no monster drops below 1 HP.
#[test]
fn the_juggernaut_sweep_spares_the_party_and_never_kills() {
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x99), Some(927));
    for i in 3..8 {
        world.actors[i].battle.hp = 40;
    }
    world.set_battle_attack(0, 400);
    let run = world
        .run_cast_module_aoe(0x99, 0)
        .expect("PROT 0927 has a never-kill damage shape");
    assert_eq!(run.prot_entry, 927);
    assert!(
        !run.aoe_hits.is_empty(),
        "the sweep reached at least one seat"
    );
    assert!(
        run.aoe_hits.iter().all(|h| h.seat >= 3),
        "the party row is not swept"
    );
    for i in 0..3 {
        assert_eq!(world.actors[i].battle.hp, 100, "party seat {i} untouched");
    }
    for h in &run.aoe_hits {
        assert!(
            world.actors[h.seat as usize].battle.hp >= 1,
            "seat {} was killed by a HP-1 clamp",
            h.seat
        );
    }
    // A spell whose module has no never-kill shape takes the ordinary fold.
    assert!(world.run_cast_module_aoe(0x87, 0).is_none());
}

/// The tick seam the action SM already drives: arming the stager zeroes the
/// module phase (retail's `0x801E4B1C`) and each tick re-enters the module.
#[test]
fn arming_the_stager_zeroes_the_module_phase() {
    let mut world = module_code_world();
    world.casting.module_phase = 9;
    world.casting.module_ctx_278 = 7;
    world.arm_summon_stager(0, 0x87);
    assert_eq!(world.casting.module_phase, 0);
    assert_eq!(world.casting.module_ctx_278, 0);
    world.casting.summon_actor_slot = Some(7);
    assert!(
        world.summon_stager_tick(),
        "the stager is busy from tick one"
    );
    assert_eq!(
        world.casting.module_phase, 1,
        "the stager tick re-entered PROT 0909's module code"
    );
}

/// PROT 0905's restore arm must not be a **second** HP owner.
///
/// The engine folds a cast's HP outcome exactly once, and that fold already
/// routes this module's own magnitude in (`World::seru_tick_heal_amount`).
/// The driven seam re-enters the module every frame, so an arm that also
/// stored would restore twice per cast and more if the phase lingered. The
/// arm keeps its cure sweep - that half has no second owner.
#[test]
fn the_vera_restore_arm_leaves_hp_to_the_fold_and_still_cures() {
    use legaia_engine_vm::cast_seru_ticks_a::{
        VERA_CURE_MASKS, VERA_CURE_MIN_LEVEL, VERA_RESTORE_ARM,
    };
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x83), Some(905));
    world.casting.summon_actor_slot = Some(7);
    // A hurt, status-carrying ally in the victim seat.
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.hp = 40;
    world.actors[0].battle.max_hp = 1000;
    world.actors[0].battle.field_flags = 0x0003;
    // A cure tier the arm will act on, and a caster whose record carries the
    // spell at a level past the cure floor (the arm reads the record, not the
    // actor).
    world.battle_ctx.follow_up_pending = 1;
    let mut member = legaia_save::CharacterRecord::parse(&[0u8; 0x414]).expect("blank record");
    let mut list = member.spell_list();
    list.count = 1;
    list.ids[0] = 0x83;
    list.levels[0] = VERA_CURE_MIN_LEVEL;
    member.set_spell_list(list);
    world.party.roster.members = vec![member];

    let mut ran_restore = false;
    for arm in 0..=VERA_RESTORE_ARM {
        world.casting.module_phase = arm;
        if world.run_cast_module_code(0x83, arm).is_none() {
            break;
        }
        ran_restore |= arm == VERA_RESTORE_ARM;
    }
    assert!(ran_restore, "the restore arm has to have executed");
    assert_eq!(
        world.actors[0].battle.hp, 40,
        "the module tick is not an HP owner - the fold is"
    );
    assert_eq!(
        world.actors[0].battle.field_flags,
        0x0003 & VERA_CURE_MASKS[0],
        "the cure half still runs"
    );
}

// ---------------------------------------------------------------------------
// PROT 0907 (Nighto): the kill / confuse fork, driven at the band
// ---------------------------------------------------------------------------

/// Run PROT 0907's phase chain far enough that the fork arm has executed.
fn run_nighto_to_the_fork(world: &mut World) {
    use legaia_engine_vm::cast_seru_ticks_a::NIGHTO_CONFUSE_ARM;
    for _ in 0..=usize::from(NIGHTO_CONFUSE_ARM) + 2 {
        if world.run_cast_module_code(0x85, 0).is_none() {
            break;
        }
    }
}

/// The band draws Nighto's verdict once and holds it - retail's arm 0 parks
/// both rolls in the module words `0x801F8534` / `0x801F853C` and arm 13 only
/// reads them, so a verdict re-rolled per frame could flicker a resist into a
/// kill mid-cast.
#[test]
fn the_nighto_verdict_is_drawn_once_and_held_for_the_whole_cast() {
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x85), Some(907));
    world.casting.summon_actor_slot = Some(7);
    world.actors[0].battle.active_target = 3;
    assert_eq!(world.casting.module_nighto_outcome, None);
    world.run_cast_module_code(0x85, 0).expect("PROT 0907 runs");
    let first = world
        .casting
        .module_nighto_outcome
        .expect("the band drew the verdict on the first tick");
    let cursor = world.rng_state;
    for _ in 0..8 {
        world.run_cast_module_code(0x85, 0);
        assert_eq!(
            world.casting.module_nighto_outcome,
            Some(first),
            "the verdict changed mid-cast"
        );
    }
    assert_eq!(
        world.rng_state, cursor,
        "a later tick drew from the RNG again"
    );
}

/// The `+0x20` immunity is the byte AND the scripted-fight flag: retail's
/// resist force reads `ctx[+0x287] != 0 && record[+0x20] != 0`, so an "immune"
/// monster in a random encounter is not immune at all.
#[test]
fn a_wide_texture_page_monster_resists_nighto_only_in_a_scripted_fight() {
    use legaia_engine_vm::cast_seru_ticks_a::NightoOutcome;
    let verdict = |wide: u8, scripted: u8, seed: u32| -> NightoOutcome {
        let mut world = module_code_world();
        world.rng_state = seed;
        let mut def = crate::monster_catalog::MonsterDef::new(77, "Big", 300, 20);
        def.wide_texture_page = wide;
        world.tables.monster_catalog.insert(def);
        world.actors[3].battle_monster_id = Some(77);
        world.battle_ctx.scripted_fight = scripted;
        world.casting.summon_actor_slot = Some(7);
        world.actors[0].battle.active_target = 3;
        run_nighto_to_the_fork(&mut world);
        world.casting.module_nighto_outcome.expect("verdict drawn")
    };
    // `resisted()`, not a variant compare: the resist is one of two
    // independent bits, and the kill word still picks which leg the cast
    // takes - so a forced resist is `KillResisted` or `ConfuseResisted`
    // depending on a roll this test does not pin.
    // A stream on which the plain throw does not resist, so a resist below
    // can only be the forced one. Scanned rather than pinned: the resist
    // throw's draw index depends on how many draws the band takes first, and
    // roughly half of all streams pass.
    let seed = (0..64u32)
        .map(|i| i.wrapping_mul(0x9E37_79B9).wrapping_add(1))
        .find(|&s| !verdict(0, 0, s).resisted())
        .expect("some stream throws no natural resist");
    assert!(
        verdict(1, 4, seed).resisted(),
        "the byte plus the scripted flag forces the resist"
    );
    // Same monster, same rolls, random encounter: the gate is open, so the
    // fork runs. Whichever side it lands on, it is not the forced resist.
    assert!(
        !verdict(1, 0, seed).resisted(),
        "a random encounter must not force the resist"
    );
    assert!(
        !verdict(0, 4, seed).resisted(),
        "a monster without the byte must not force the resist"
    );
}

/// The fork actually reaches the victim: a killed victim's HP is zeroed and a
/// confused one takes `+0x16E |= 0x380`. Before the verdict was driven, the
/// tick always took the resisted branch and wrote neither.
#[test]
fn the_nighto_fork_writes_the_victim_on_both_of_its_branches() {
    use legaia_engine_vm::cast_seru_ticks_a::{NIGHTO_CONFUSE_BITS, NightoOutcome};
    let mut kills = 0usize;
    let mut confuses = 0usize;
    for seed in 0..512u32 {
        let mut world = module_code_world();
        world.rng_state = seed.wrapping_mul(0x9E37_79B9).wrapping_add(1);
        world.casting.summon_actor_slot = Some(7);
        world.actors[0].battle.active_target = 3;
        run_nighto_to_the_fork(&mut world);
        match world.casting.module_nighto_outcome {
            Some(NightoOutcome::Kill) => {
                kills += 1;
                assert_eq!(world.actors[3].battle.hp, 0, "the kill branch zeroes HP");
            }
            Some(NightoOutcome::Confuse) => {
                confuses += 1;
                assert_eq!(
                    world.actors[3].battle.field_flags & NIGHTO_CONFUSE_BITS,
                    NIGHTO_CONFUSE_BITS,
                    "the confuse branch sets the +0x16E bits"
                );
            }
            _ => {}
        }
    }
    // 512 seeds, not 64: `World::next_rng` is an LCG mod 2^32, whose low
    // three bits have period 8 and correlate with the next draw's parity, so
    // a short consecutive-seed sample can miss the `kill % 8 == 0` branch
    // entirely (64 seeds yields none).
    assert!(kills > 0, "no seed took the kill branch");
    assert!(confuses > 0, "no seed took the confuse branch");
}

/// `ctx[+0]` is the **party** count, the bound of every `0..ctx[+0]` sweep in
/// the band - Evil Seru Magic's whole-row hit (`FUN_801F8D64`), Orb's heal,
/// Element Change's hide. Seeding it from the actor table made all three run
/// over the monster row as well, which retail's separate `ctx[+1]` sweep is
/// what covers.
#[test]
fn the_module_context_takes_ctx0_from_the_party_row() {
    use legaia_engine_vm::cast_module_ticks::FIRST_MONSTER_SEAT;
    let mut world = module_code_world();
    // Eight seated combat actors, three of them party.
    world.party.party_count = 3;
    let ctx = world.cast_module_ctx();
    assert_eq!(ctx.party_count, 3, "ctx[+0] followed the actor table");
    assert!(ctx.party_count <= FIRST_MONSTER_SEAT);
    assert_eq!(
        ctx.monster_count, 5,
        "ctx[+1] still counts the live monster row"
    );
    // A lone party member narrows `ctx[+0]` and leaves `ctx[+1]` alone.
    world.party.party_count = 1;
    let ctx = world.cast_module_ctx();
    assert_eq!(ctx.party_count, 1);
    assert_eq!(ctx.monster_count, 5);
}

/// PROT 0904's ring sweep hits a seat when the bearing difference is within
/// `+-0x30` **modulo a turn** - retail's `(|ref - seat| - 0x30)` compared
/// unsigned against `0xFB1`, whose underflow is what makes the near end of
/// the cone count. A cone that only tested one side would drop every seat on
/// the wrapping edge.
#[test]
fn the_ring_cone_wraps_at_both_ends() {
    use legaia_engine_vm::cast_seru_ticks_a::THEEDER_CONE_HALF_WIDTH;
    let mut world = module_code_world();
    // Seat three monsters around a centre at the origin: due +Z (bearing 0),
    // just inside the cone's far edge, and well outside it.
    let centre = (0i16, 0i16);
    let place = |w: &mut World, slot: usize, x: i16, z: i16| {
        w.actors[slot].battle.seat = Some((x, z));
    };
    place(&mut world, 3, 0, 1000); // bearing 0x000
    place(&mut world, 4, 1000, 0); // bearing 0x400 - a quarter turn out
    place(&mut world, 5, 0, -1000); // bearing 0x800 - half a turn out
    // Seat 6 is in the swept range too; park it off the ray rather than at
    // the centre, where a zero displacement would read as bearing 0 and sit
    // inside every cone.
    place(&mut world, 6, -1000, 0); // bearing 0xC00
    // A ray pointing at seat 3 takes it and nothing else.
    let hit = world.seats_in_cone(centre, 0x000, THEEDER_CONE_HALF_WIDTH, 3..7);
    assert_eq!(hit, vec![3]);
    // The same ray one unit the OTHER side of the wrap still takes seat 3:
    // `0xFFF` is one step below a full turn, i.e. one step before bearing 0.
    let hit = world.seats_in_cone(centre, 0x0FFF, THEEDER_CONE_HALF_WIDTH, 3..7);
    assert_eq!(hit, vec![3], "the cone did not wrap at 0x1000");
    // A ray a whole quarter turn away takes seat 4 instead.
    let hit = world.seats_in_cone(centre, 0x0400, THEEDER_CONE_HALF_WIDTH, 3..7);
    assert_eq!(hit, vec![4]);
    // A ray between the seats takes nobody.
    let hit = world.seats_in_cone(centre, 0x0200, THEEDER_CONE_HALF_WIDTH, 3..7);
    assert!(hit.is_empty(), "the cone reached {hit:?} from a gap");
}
