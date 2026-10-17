//! PROT 0916 (Aluru, `0x8E`) driven through the band seam: its director
//! carries the whole chain to `0xFF`, the arm-0 verdict picks the path, and
//! the bands draw on the arms that build them.

use super::*;
use legaia_engine_cast::cast_module_camera::{
    ALURU_BLAST_ARM, ALURU_ENTRY, ALURU_HIT_ARM, ALURU_SETTLE_ARM, ALURU_SPREAD_ARM,
};

const ALURU: u8 = 0x8E;

/// Engine slot 0 casting on monster slot 1, the creature in slot 9.
fn aluru_world(kill: Option<bool>) -> World {
    let mut world = module_code_world();
    world.actors[1].battle_monster_id = Some(1);
    world.actors[0].battle.active_target = 1;
    for i in 5..12 {
        world.actors[i].active = false;
        world.actors[i].battle.hp = 0;
    }
    world.casting.summon_actor_slot = Some(9);
    world.actors[9].active = true;
    world.actors[9].battle.hp = 100;
    world.casting.module_cam.kill = kill;
    world
}

type AluruRun = (Vec<u8>, Vec<Option<i16>>, Vec<u16>);

/// Run the module until it lets go, recording the phase each tick ran on,
/// the band spread each tick left, and the peak shatter word per slot.
fn run_aluru(world: &mut World) -> AluruRun {
    assert_eq!(world.cast_module_for(ALURU), Some(ALURU_ENTRY));
    let mut phases = Vec::new();
    let mut spreads = Vec::new();
    let mut peak = vec![0u16; world.actors.len()];
    for _ in 0..20_000 {
        let phase = world.casting.module_phase;
        let Some(run) = world.run_cast_module_code(ALURU, phase) else {
            break;
        };
        phases.push(phase);
        spreads.push(world.casting.module_cam.band_spread);
        for (p, a) in peak.iter_mut().zip(&world.actors) {
            *p = (*p).max(a.battle.shatter);
        }
        if !run.busy {
            break;
        }
    }
    (phases, spreads, peak)
}

/// The kill path zeroes the victim's HP on arm `0x11`'s passing tick,
/// breaks it up through arm `0x13`, owes the band no fold, and leaves
/// `0x13` for the done phase without visiting arm `0x14`.
#[test]
fn the_aluru_kill_path_zeroes_and_breaks_up_the_victim() {
    let mut world = aluru_world(Some(true));
    let (phases, spreads, peak) = run_aluru(&mut world);
    assert_eq!(world.casting.module_phase, 0xFF, "reaches the done phase");
    assert!(!phases.contains(&ALURU_SETTLE_ARM));
    assert_eq!(world.actors[1].battle.hp, 0);
    assert!(world.casting.module_skips_fold);
    // `2 * scalar` a tick over arm 0x13's 160 ticks.
    assert_eq!(peak[1], 2 * 8 * 160);
    // The creature is assembled (`0x800` down to `0`) before the blast.
    assert_eq!(peak[9], 0x800);
    assert_eq!(world.actors[9].battle.shatter, 0);
    // The bands draw on arm 0x11's pass and on every 0x12 / 0x13 pass.
    let drawn = phases
        .iter()
        .zip(&spreads)
        .filter(|(_, s)| s.is_some())
        .map(|(p, _)| *p)
        .collect::<Vec<_>>();
    assert_eq!(drawn.first(), Some(&ALURU_BLAST_ARM));
    assert!(
        drawn[1..]
            .iter()
            .all(|p| *p == ALURU_HIT_ARM || *p == ALURU_SPREAD_ARM)
    );
    assert_eq!(drawn.len(), 1 + 33 + 160);
}

/// The damage path leaves the victim's HP to the band's fold and runs on
/// to arm `0x14`, which lets go once the victim has settled.
#[test]
fn the_aluru_damage_path_leaves_the_hit_to_the_fold() {
    let mut world = aluru_world(Some(false));
    let (phases, _, peak) = run_aluru(&mut world);
    assert_eq!(world.casting.module_phase, 0xFF);
    assert!(phases.contains(&ALURU_SETTLE_ARM));
    assert_eq!(world.actors[1].battle.hp, 100);
    assert!(!world.casting.module_skips_fold);
    assert_eq!(peak[1], 0, "a damaged victim is not broken up");
}

/// The verdict is drawn once, off the world's `rand()` stream; a scripted
/// fight against a `+0x20` record skips the draw and never kills.
#[test]
fn the_aluru_verdict_skips_the_roll_on_a_scripted_boss() {
    let mut world = aluru_world(None);
    let mut def = legaia_engine_battle::monster_catalog::MonsterDef::new(1, "Boss", 300, 20);
    def.wide_texture_page = 1;
    world.tables.monster_catalog.insert(def);
    world.battle_ctx.scripted_fight = 4;
    let before = world.rng_state;
    let _ = world.run_cast_module_code(ALURU, 0);
    assert_eq!(world.casting.module_cam.kill, Some(false));
    assert_eq!(world.rng_state, before, "no draw");
    let mut world = aluru_world(None);
    let before = world.rng_state;
    let _ = world.run_cast_module_code(ALURU, 0);
    assert!(world.casting.module_cam.kill.is_some());
    assert_ne!(world.rng_state, before, "one draw");
    let held = world.rng_state;
    let _ = world.run_cast_module_code(ALURU, world.casting.module_phase);
    assert_eq!(world.rng_state, held, "drawn once per cast");
}

const JEDO: u8 = 0x9D;

/// Run PROT 0931 from its opening to the end over three living monsters
/// (engine slots 1..4), returning the peak shatter word per slot.
fn run_jedo(world: &mut World) -> Vec<u16> {
    assert_eq!(world.cast_module_for(JEDO), Some(931));
    let mut peak = vec![0u16; world.actors.len()];
    for _ in 0..20_000 {
        let phase = world.casting.module_phase;
        let Some(run) = world.run_cast_module_code(JEDO, phase) else {
            break;
        };
        for (p, a) in peak.iter_mut().zip(&world.actors) {
            *p = (*p).max(a.battle.shatter);
        }
        if !run.busy {
            break;
        }
    }
    peak
}

fn jedo_world() -> World {
    let mut world = aluru_world(Some(false));
    for i in 1..4 {
        world.actors[i].battle_monster_id = Some(1);
    }
    world.actors[4].battle_monster_id = None;
    world
}

/// On a fight that is not scripted, Jedo's arm 30 breaks every living
/// monster up and its exit kills them; the band's fold is waived.
#[test]
fn jedo_kills_the_living_row_on_an_ordinary_fight() {
    let mut world = jedo_world();
    let peak = run_jedo(&mut world);
    assert_eq!(world.casting.module_phase, 0xFF);
    for (i, p) in peak.iter().enumerate().take(4).skip(1) {
        assert_eq!(world.actors[i].battle.hp, 0, "slot {i}");
        assert!(*p > 0, "slot {i} comes apart");
    }
    assert_eq!(world.actors[0].battle.hp, 100, "the caster is spared");
    assert!(world.casting.module_skips_fold);
}

/// On a scripted fight the kill never runs: the hit is the band's fold.
#[test]
fn jedo_leaves_a_scripted_fight_to_the_fold() {
    let mut world = jedo_world();
    world.battle_ctx.scripted_fight = 1;
    let peak = run_jedo(&mut world);
    assert_eq!(world.casting.module_phase, 0xFF);
    for (i, p) in peak.iter().enumerate().take(4).skip(1) {
        assert_eq!(world.actors[i].battle.hp, 100, "slot {i}");
        assert_eq!(*p, 0);
    }
    assert!(!world.casting.module_skips_fold);
}

/// Every summon-creature module runs to its done phase through the band
/// seam with no host feed beyond the test world's.
#[test]
fn every_summon_creature_module_runs_to_done() {
    for id in [
        0x8Cu8, 0x8D, 0x8E, 0x8F, 0x93, 0x95, 0x9A, 0x9B, 0x9C, 0x9D, 0x9E, 0x9F, 0xA0,
    ] {
        let mut world = aluru_world(Some(false));
        let entry = world.cast_module_for(id).expect("a module");
        let mut ticks = 0;
        for _ in 0..20_000 {
            let phase = world.casting.module_phase;
            let run = world.run_cast_module_code(id, phase).expect("runs");
            ticks += 1;
            if !run.busy {
                break;
            }
        }
        assert_eq!(world.casting.module_phase, 0xFF, "PROT {entry} id {id:#x}");
        assert!(ticks > 200, "PROT {entry} lasts {ticks} ticks");
    }
}

/// `ctx[+1]` counts the fight's seated monsters, not the active slots: a
/// slot the last fight left active (no monster, the resting colour word no
/// defeat fade drains) is no monster seat, and the next fight's entry clears
/// it. Long unbroken sessions hung Theeder's and Gizam's arm-14 row poll on
/// such a phantom seat.
#[test]
fn a_stale_active_row_is_neither_seated_nor_counted() {
    let mut world = World::default();
    world.enter_battle(3, 3);
    for i in 3..6 {
        world.actors[i].battle_monster_id = Some(1);
        world.actors[i].battle.hp = 0;
        world.actors[i].battle.render_color =
            legaia_engine_battle_vm::battle_formulas::TINT_NEUTRAL;
    }
    world.enter_battle(3, 2);
    assert!(
        !world.actors[5].active,
        "the row past the layout starts empty"
    );
    world.actors[3].battle_monster_id = Some(1);
    world.actors[4].battle_monster_id = Some(1);
    assert_eq!(world.cast_module_ctx().monster_count, 2);
    // A slot some other path leaves active still counts for nothing.
    world.actors[5].active = true;
    world.actors[5].battle.render_color = legaia_engine_battle_vm::battle_formulas::TINT_NEUTRAL;
    assert_eq!(world.cast_module_ctx().monster_count, 2);
    // A downed monster keeps its seat in the count (retail's escape divisor
    // keeps it too).
    world.actors[4].battle.hp = 0;
    world.actors[4].active = false;
    assert_eq!(world.cast_module_ctx().monster_count, 2);
}

/// The band does not stand forever on a module held by a poll the engine
/// cannot answer: PROT 0912's arm `0x13` waits for every dead monster's
/// colour word to drain, and a dead seat whose defeat fade never runs holds
/// it. The Deene fight's `0x8A` stood in the Linger phase at module phase
/// `0x13` for good; the stager lets go after
/// [`crate::world::battle::cast_band::SUMMON_LINGER_STALL_FRAMES`].
#[test]
fn the_band_lets_go_of_a_module_held_on_an_undrainable_seat() {
    let mut world = module_code_world();
    world.actors[1].battle_monster_id = Some(1);
    world.actors[0].battle.active_target = 1;
    world.casting.summon_actor_slot = Some(9);
    world.actors[9].active = true;
    world.actors[9].battle.hp = 100;
    world.arm_summon_stager(0, 0x8A);
    assert_eq!(world.cast_module_for(0x8A), Some(912));
    // The seat dies with its resting colour word and never enters the
    // defeat fade.
    world.actors[1].battle.hp = 0;
    world.actors[1].battle.render_color = legaia_engine_battle_vm::battle_formulas::TINT_NEUTRAL;
    let mut ticks = 0u32;
    while world.summon_stager_tick() {
        ticks += 1;
        // Keep the seat undrained: nothing in the engine fades it.
        world.actors[1].battle.hp = 0;
        world.actors[1].battle.render_color =
            legaia_engine_battle_vm::battle_formulas::TINT_NEUTRAL;
        assert!(ticks < 20_000, "the band never let go");
    }
    assert_eq!(
        world.casting.module_phase,
        legaia_engine_cast::seru_magic_ticks::FREED_SETTLE_PHASE,
        "held on the row poll until the stager let go"
    );
}

/// Theeder's arm-14 row poll walks the seated monster row only: a phantom
/// active slot past it (no monster, a colour word nothing drains) does not
/// hold the module.
#[test]
fn theeder_finishes_beside_a_phantom_active_seat() {
    let mut world = module_code_world();
    world.party.party_count = 3;
    for i in 3..5 {
        world.actors[i].battle_monster_id = Some(1);
    }
    world.actors[0].battle.active_target = 3;
    world.actors[5].battle.hp = 0;
    world.actors[5].battle.render_color = legaia_engine_battle_vm::battle_formulas::TINT_NEUTRAL;
    for i in 6..8 {
        world.actors[i].active = false;
        world.actors[i].battle.hp = 0;
    }
    world.casting.summon_actor_slot = Some(9);
    world.actors[9].active = true;
    world.actors[9].battle.hp = 100;
    assert_eq!(world.cast_module_for(0x82), Some(904));
    assert_eq!(world.cast_module_ctx().monster_count, 2);
    // From the arm-14 row poll on.
    world.casting.module_phase = 14;
    let mut done = false;
    for _ in 0..8000 {
        let phase = world.casting.module_phase;
        match world.run_cast_module_code(0x82, phase) {
            Some(r) if r.busy => {}
            _ => {
                done = true;
                break;
            }
        }
    }
    assert!(done, "held at phase {:#x}", world.casting.module_phase);
}

/// A monster standing dead and idle outside the defeat fade (a hit on a
/// frozen seat, a seat seeded dead) enters the fade, and its lanes drain -
/// what every row poll in the band waits on. The seeded `gilium_summon_mid_cast`
/// run held Gilium's arm `0x16` on such a seat until the stall guard let go.
#[test]
fn a_dead_idle_monster_enters_the_defeat_fade_and_drains() {
    let mut world = module_code_world();
    world.actors[1].battle_monster_id = Some(1);
    world.actors[1].battle.hp = 0;
    world.actors[1].battle.render_flag = 0;
    world.actors[1].battle.render_color = legaia_engine_battle_vm::battle_formulas::TINT_NEUTRAL;
    for _ in 0..600 {
        world.tick_battle_animations();
    }
    assert_eq!(
        world.actors[1].battle.render_flag,
        legaia_engine_battle_vm::battle_formulas::STATE_DEFEAT_FADE
    );
    assert_eq!(
        world.actors[1].battle.render_color & 0x00FF_FFFF,
        0,
        "the lanes drained"
    );
    // A living monster is left alone.
    assert_eq!(world.actors[2].battle.render_flag, 0);
}

/// Horn hides the party for its rise (arm 8), shows it again and hides the
/// creature on the heal (arm `0xA`).
#[test]
fn horn_hides_and_shows_the_party_around_the_heal() {
    use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
    let mut world = aluru_world(Some(false));
    assert_eq!(world.cast_module_for(0x9C), Some(930));
    let run_arm = |world: &mut World, arm: u8| {
        world.casting.module_phase = arm;
        world.casting.module_cam.countdown.0 = 0;
        for _ in 0..4 {
            let phase = world.casting.module_phase;
            world.run_cast_module_code(0x9C, phase);
            if world.casting.module_phase != arm {
                break;
            }
        }
        assert_ne!(world.casting.module_phase, arm, "arm {arm} passed");
    };
    run_arm(&mut world, 8);
    assert_eq!(world.actors[0].battle.render_flag, RENDER_FLAG_HIDDEN);
    assert!(world.battle_body_hidden(0));
    run_arm(&mut world, 0xA);
    assert_eq!(world.actors[0].battle.render_flag, 0);
    assert!(!world.battle_body_hidden(0));
    assert!(world.battle_body_hidden(9), "the creature leaves");
}
