//! The cast band end to end: the summon band, the monster magic band and the
//! party cast label.

use super::*;
use legaia_engine_battle_vm::battle_action::ActionState;
use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
use legaia_engine_mode::mode::SceneMode;

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
        legaia_engine_battle_vm::battle_action::ActionCategory::Magic.as_byte()
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
            // The request carries the stager's spawn point behind the
            // caster (where an undirected module's parts are staged), on
            // the caster's live pair - which the action seed's formation
            // recentre (`FUN_801EFE44`) has moved off its fixture seat.
            assert_eq!(origin[0], world.actors[0].move_state.world_x);
            assert_eq!(
                origin[2],
                world.actors[0].move_state.world_z - crate::world::battle::SUMMON_SPAWN_BEHIND
            );
            // A host seats it; the world adopts the seat. PROT 0903's band is
            // director-paced, so the creature stands where `FUN_801F19EC`
            // puts it - on the caster - until its arm 3 moves it.
            world.seat_summon_actor(9);
            assert_eq!(world.casting.summon_actor_slot, Some(9));
            assert_eq!(
                world.actors[9].move_state.world_z,
                world.actors[0].move_state.world_z
            );
            // The installer's quarter rate (`+0x21D = 2`).
            assert_eq!(world.actors[9].battle.anim_rate.get(), 2);
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
    // The flash cue went through the dispatcher `FUN_8004FCC8`: `0x63` is
    // past `0x40`, so the ring carries it unchanged.
    assert!(
        world
            .take_sfx_ring_ops()
            .contains(&legaia_engine_system::sound_state::SfxRingOp::Push(
                legaia_engine_battle_vm::battle_action::SUMMON_FLASH_CUE as i16
            )),
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
    use legaia_engine_battle::battle_events::BattleEvent;
    use legaia_engine_battle::monster_catalog::vanilla_monster_catalog;
    use legaia_engine_battle::spells::SpellCatalog;

    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
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
    use legaia_engine_battle::battle_events::BattleEvent;
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
