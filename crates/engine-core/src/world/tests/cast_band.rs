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
        party_count: 1,
        ..World::default()
    };
    // A live session seats the creature above the eight battle slots.
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.spell_catalog = crate::retail_magic::retail_seru_magic_catalog();
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
    world.roster = party;
    world.battle_ctx.active_actor = 0;
    world.battle_spell_menu = world.build_battle_spell_session(0);
    world
}

fn confirm_cast(world: &mut World) {
    use crate::input::PadButton;
    // Cross on the spell row opens the target cursor; Cross again confirms.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    world.set_pad(0);
}

#[test]
fn a_seru_cast_runs_the_summon_band_and_the_stager_folds_once_at_its_strike() {
    let mut world = seru_cast_world();
    let cost = u16::from(world.spell_catalog.get(0x81).unwrap().mp_cost);
    assert!(cost > 0);
    confirm_cast(&mut world);

    // The confirm armed the band, not the fold: nothing has landed yet.
    assert!(world.battle_spell_menu.is_none(), "spell menu closed");
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
    assert!(world.pending_cast.is_some(), "the cast's outcome is owed");

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
            assert_eq!(world.summon_actor_slot, Some(9));
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
        if world.screen_fade.is_some() && world.screen_fade_draw().is_none() {
            fade_delay_seen = true;
        }
        if let Some((rgb, abr, ot)) = world.screen_fade_draw()
            && rgb == 0xFF_FFFF
        {
            fade_white_seen = true;
            assert_eq!(abr, 1, "additive flash");
            assert_eq!(ot, 1, "the id the band stamps");
        }
        if folded_at.is_none() && world.pending_cast.is_none() {
            folded_at = Some(tick);
        }
        if world.battle_command.is_some() || world.battle_ctx.active_actor != 0 {
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
    assert!(world.summon_stager.is_none());
    // The flash cue rode the SFX queue.
    assert!(
        world
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
        if world.pending_cast.is_none() {
            folded = true;
        }
        if folded && world.summon_stager.is_none() {
            break;
        }
    }
    assert!(folded, "the unseated grace folded the cast");
    assert!(world.actors[1].battle.hp < 300);
    assert!(world.summon_stager.is_none(), "the stager retired");
}

#[test]
fn a_monster_cast_runs_the_magic_band_and_folds_on_leaving_the_wait() {
    use crate::battle_events::BattleEvent;
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.monster_catalog = vanilla_monster_catalog();
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
    world.rng_state = 0;

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
            from_wait = Some((s, world.pending_cast.is_none()));
        }
        if world.pending_cast.is_none() {
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
        10 - u16::from(world.spell_catalog.get(0x20).unwrap().mp_cost),
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
