//! PROT 0954 (Fatal Decision) on the world: the sixteen outcomes on a party
//! victim, one effect class at a time, and the whole body driven through
//! the band seam with the player stopping the wheel.

use super::*;
use legaia_engine_vm::cast_fatal_decision::{self as fd, FatalOutcome};
use legaia_engine_vm::status_effects::StatusKind;

const VICTIM: u8 = 0;
const CASTER: u8 = 1;

/// One party member (the victim) and one monster (the caster, acting and
/// aimed at the member).
fn fd_world() -> World {
    let mut world = World {
        party: crate::world::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    let v = &mut world.actors[VICTIM as usize];
    v.active = true;
    v.battle.hp = 301;
    v.battle.max_hp = 400;
    v.battle.liveness = 1;
    v.battle.mp = 41;
    v.battle.atk_working = 90;
    v.battle.atk_base = 90;
    v.move_state.world_z = -600;
    world.set_character_max_mp(VICTIM, 60);
    let c = &mut world.actors[CASTER as usize];
    c.active = true;
    c.battle.hp = 500;
    c.battle.liveness = 1;
    c.battle.active_target = VICTIM;
    c.move_state.world_z = 600;
    c.battle_monster_id = Some(0x79);
    world.battle_ctx.active_actor = CASTER;
    world
}

#[test]
fn hp_outcomes_land_on_the_victim_with_a_popup() {
    let mut w = fd_world();
    assert!(w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::HalveHp));
    let b = &w.actors[0].battle;
    assert_eq!((b.hp, b.max_hp, b.hp_bar_pending), (150, 200, 151));
    let fx = w.drain_battle_hit_fx();
    assert_eq!(
        (fx[0].target_slot, fx[0].amount, fx[0].is_heal),
        (0, 151, false)
    );

    let mut w = fd_world();
    w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::Death);
    assert_eq!(w.actors[0].battle.hp, 0);
}

#[test]
fn mp_outcomes_halve_the_seat_ceiling_too() {
    let mut w = fd_world();
    w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::HalveMp);
    assert_eq!(w.actors[0].battle.mp, 20);
    assert_eq!(w.tables.character_max_mp[0], 30);
    let mut w = fd_world();
    w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::DrainMp);
    assert_eq!(
        (w.actors[0].battle.mp, w.tables.character_max_mp[0]),
        (0, 0)
    );
}

#[test]
fn status_outcomes_install_through_the_tracker() {
    for (o, kind) in [
        (FatalOutcome::Venom, StatusKind::Venom),
        (FatalOutcome::Toxic, StatusKind::Toxic),
        (FatalOutcome::Curse, StatusKind::Curse),
        (FatalOutcome::Stone, StatusKind::Stone),
        (FatalOutcome::Numb, StatusKind::Numb),
    ] {
        let mut w = fd_world();
        w.apply_fatal_outcome(CASTER, VICTIM, o);
        assert!(w.battle.status_effects.has(VICTIM, kind), "{o:?}");
    }
    // Rot is all three limbs at once: `0x38` in the packed word.
    let mut w = fd_world();
    w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::Rot);
    assert_eq!(w.raw_status_word(VICTIM) & 0x38, 0x38);
}

#[test]
fn numb_and_stone_hand_a_queued_item_back_and_cancel_the_action() {
    use legaia_engine_vm::battle_action::ActionCategory;
    for o in [FatalOutcome::Numb, FatalOutcome::Stone] {
        let mut w = fd_world();
        w.actors[0].battle.action_category = ActionCategory::Item.as_byte();
        w.actors[0].battle.init_key = 5;
        w.actors[0].battle.params[0] = 0x0A;
        let reacts = w.apply_fatal_outcome(CASTER, VICTIM, o);
        assert!(!reacts, "{o:?} skips the reaction arm");
        assert_eq!(w.actors[0].battle.action_category, 0);
        assert_eq!(w.party.inventory.get(&0x0A).copied(), Some(1));
    }
}

#[test]
fn stat_outcomes_halve_the_attack_and_defence_pairs() {
    let mut w = fd_world();
    w.battle.defense_split[0] = Some((50, 3));
    w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::HalveAtk);
    w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::HalveDef);
    let b = &w.actors[0].battle;
    assert_eq!((b.atk_working, b.atk_base), (45, 45));
    assert_eq!(w.battle.defense_split[0], Some((25, 1)));
}

#[test]
fn the_full_heal_cures_and_fills() {
    let mut w = fd_world();
    w.battle.status_effects.apply(VICTIM, StatusKind::Venom);
    w.actors[0].battle.field_flags = 0x0380;
    assert!(!w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::FullHeal));
    assert_eq!(w.actors[0].battle.hp, 400);
    assert_eq!(w.raw_status_word(VICTIM), 0);
    let fx = w.drain_battle_hit_fx();
    assert_eq!((fx[0].amount, fx[0].is_heal), (99, true));
}

#[test]
fn the_steal_destroys_a_bag_item_and_the_tithe_takes_a_tenth() {
    let mut w = fd_world();
    let _ = w.party.inventory.add(0x0A, 3);
    // The world's RNG from its default seed: the gate passes and the draw
    // reaches the one occupied slot inside its `0x400` budget.
    assert!(w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::StealItem));
    assert_eq!(w.battle_ctx.message_id, 0x5B);
    assert_eq!(w.party.inventory.get(&0x0A).copied(), Some(2));
    assert!(w.battle.steal_caption.is_some());
    // An empty bag is the "no effect" arm, which skips the reaction.
    let mut w = fd_world();
    assert!(!w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::StealItem));

    let mut w = fd_world();
    w.party.money = 1005;
    assert!(w.apply_fatal_outcome(CASTER, VICTIM, FatalOutcome::GoldTithe));
    assert_eq!(w.party.money, 905);
}

/// Drive the body through the band seam, the player pressing confirm a few
/// ticks into the spin.
#[test]
fn the_whole_body_runs_and_the_player_stops_the_wheel() {
    let mut w = fd_world();
    let (rate_c, rate_v) = (
        w.actors[CASTER as usize].battle.anim_rate.get(),
        w.actors[VICTIM as usize].battle.anim_rate.get(),
    );
    let mut phases = Vec::new();
    let mut six = 0;
    let mut halved = false;
    let mut hidden = false;
    let mut shots = 0;
    for _ in 0..20_000 {
        let phase = w.casting.module_phase;
        let press = phase == 6 && six == 12;
        if phase == 6 {
            six += 1;
        }
        w.input.set_pad(if press {
            crate::input::PadButton::Cross.mask()
        } else {
            0
        });
        let run = w.run_fatal_decision(fd::FATAL_DECISION_ENTRY);
        shots += usize::from(run.camera_shot.is_some());
        if phase == 0 {
            halved = w.actors[CASTER as usize].battle.anim_rate.get() == rate_c >> 1;
            hidden = w.actors[VICTIM as usize].battle.render_flag == fd::HIDDEN_RENDER_FLAG;
        }
        if phase == 5 && run.phase == 6 {
            assert!(
                w.battle.steal_caption.is_none(),
                "disc-free: no prompt text"
            );
        }
        phases.push(phase);
        if !run.busy {
            break;
        }
    }
    assert!(halved && hidden);
    phases.dedup();
    let st = w.casting.fatal_decision.expect("state kept");
    let landed = FatalOutcome::from_slot(st.slots[st.landed]).unwrap();
    // The reaction arm runs exactly when the landed outcome falls to it.
    let reacts = !matches!(
        landed,
        FatalOutcome::Nothing | FatalOutcome::Numb | FatalOutcome::Stone | FatalOutcome::FullHeal
    );
    assert_eq!(phases.contains(&11), reacts, "{landed:?} {phases:?}");
    assert_eq!(*phases.last().unwrap(), 0xFF);
    // Six shots: arms 0, 1, 2, 3, 4 and 8.
    assert_eq!(shots, 6);
    // The rates are back, and the caster is shown again.
    assert_eq!(w.actors[CASTER as usize].battle.anim_rate.get(), rate_c);
    assert_eq!(w.actors[VICTIM as usize].battle.anim_rate.get(), rate_v);
    assert_eq!(w.actors[CASTER as usize].battle.render_flag, 0);
    // The wheel stopped on the press, not on the countdown.
    assert_eq!(six, 13);
}

fn extracted_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = std::path::PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    None
}

/// Disc-gated: with the band's real images installed, the body seats its own
/// records - none at the pager seam - and the eight icons it seats are
/// sprite-arm nodes the hosts draw, each placed on the ring the arms
/// compute; the prompt and the landed outcome's name come off the image.
#[test]
fn the_wheel_draws_its_icons_off_the_disc_records() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    use legaia_asset::cast_effect_pool::{
        CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool,
    };
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut pool = CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let entry = archive.entries[idx as usize].clone();
        let mut bytes = Vec::new();
        archive.read_entry(&entry, &mut bytes).expect("read entry");
        assert!(pool.insert(idx, &bytes));
    }
    let mut w = fd_world();
    w.install_cast_effect_pool(std::sync::Arc::new(pool));
    let prompt = w
        .fatal_decision_text_for_test(fd::PROMPT_TEXT)
        .expect("prompt string");
    assert!(
        prompt.contains('\u{E0CE}'),
        "the prompt carries a button escape"
    );
    // Run to the spin (arm 6) and hold there.
    for _ in 0..2_000 {
        w.input.set_pad(0);
        w.run_fatal_decision(fd::FATAL_DECISION_ENTRY);
        w.tick_summon(1);
        if w.casting.module_phase == 6 {
            break;
        }
    }
    assert_eq!(w.casting.module_phase, 6);
    assert!(w.battle.steal_caption.is_some(), "the stop prompt is up");
    w.run_fatal_decision(fd::FATAL_DECISION_ENTRY);
    w.tick_summon(1);
    let scene = w.casting.active_summon.as_ref().expect("records seated");
    let icons: Vec<_> = scene
        .parts
        .iter()
        .filter(|p| {
            p.tag
                .is_some_and(|t| (0x0954_0100..0x0954_0108).contains(&t))
        })
        .collect();
    assert_eq!(icons.len(), 8, "one icon per slot");
    let st = w.casting.fatal_decision.unwrap();
    for p in &icons {
        let i = (p.tag.unwrap() - 0x0954_0100) as i32;
        let [x, y] = fd::ring_offset(st.rot + i * fd::WHEEL_STEP);
        assert_eq!((p.state.world_x, p.state.world_y), (x, y));
    }
    assert!(
        scene.sprite_arm_draws().len() >= 8,
        "the icons are sprite-arm nodes"
    );
    // Stop it, and walk to the banner.
    w.input.set_pad(crate::input::PadButton::Cross.mask());
    for _ in 0..2_000 {
        w.run_fatal_decision(fd::FATAL_DECISION_ENTRY);
        w.input.set_pad(0);
        if w.casting.fatal_banner.is_some() {
            break;
        }
    }
    assert!(
        w.battle.steal_caption.is_none(),
        "the prompt closed on the stop"
    );
    let st = w.casting.fatal_decision.unwrap();
    let landed = FatalOutcome::from_slot(st.slots[st.landed]).unwrap();
    let banner = w.casting.fatal_banner.clone().expect("the banner is up");
    assert!(!banner.is_empty(), "{landed:?} names itself");
}
