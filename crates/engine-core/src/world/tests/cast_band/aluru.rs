//! PROT 0916 (Aluru, `0x8E`) driven through the band seam: its director
//! carries the whole chain to `0xFF`, the arm-0 verdict picks the path, and
//! the bands draw on the arms that build them.

use super::*;
use legaia_engine_battle_vm::cast_module_camera::{
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
