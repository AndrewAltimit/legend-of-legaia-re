//! Disc-gated: a player summon's **module** owns the camera and the length
//! of the summon band's return state.
//!
//! Drives PROT 0903 (Gimard, spell `0x81`) through the stager seam the action
//! SM calls in `0x35` / `0x36` (`World::summon_stager_tick`) with the shared
//! battle camera stepping beside it, and checks the three things the module
//! director (`legaia_engine_vm::cast_module_camera`) is for:
//!
//! - the band holds for as long as the module's countdown-paced arms do -
//!   hundreds of frames, not the engine stager's own walk-in;
//! - the camera lands on the module's own cuts (arm 4's low shot on the
//!   creature: pitch `-0x1C0`, TR y `0x380`);
//! - the chain ends, and the stager with it.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::cast_module_ticks::SUMMON_SEAT;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() && p.join("SCUS_942.54").is_file() {
            return Some(p);
        }
    }
    None
}

fn battle_world(dir: &std::path::Path) -> World {
    let mut world = World {
        party: legaia_engine_core::world::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    for slot in [0usize, 3, SUMMON_SEAT as usize] {
        world.actors[slot].active = true;
        world.actors[slot].battle.max_hp = 400;
        world.actors[slot].battle.hp = 400;
        world.actors[slot].battle.liveness = 1;
    }
    world.actors[0].move_state.world_z = -542;
    world.actors[0].move_state.world_x = 82;
    world.actors[3].move_state.world_z = 800;
    world.actors[0].battle.active_target = 3;
    world.battle_ctx.active_actor = 0;

    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut pool = legaia_asset::cast_effect_pool::CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let Some(entry) = archive.entries.get(idx as usize).cloned() else {
            continue;
        };
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_ok() {
            pool.insert(idx, &bytes);
        }
    }
    world.casting.effect_pool = Some(std::sync::Arc::new(pool));
    world
}

#[test]
fn gimard_paces_the_band_and_frames_its_creature() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut world = battle_world(&dir);
    assert_eq!(world.cast_module_for(0x81), Some(903));
    world.arm_summon_stager(0, 0x81);
    world.casting.summon_actor_slot = Some(SUMMON_SEAT);
    // The module owns the camera from the band's sustain on.
    world.battle_ctx.action_state = 0x35;

    let mut ticks = 0u32;
    let mut saw_creature_cut = false;
    while ticks < 4000 {
        let busy = world.summon_stager_tick();
        world.clock.display_frames += 1;
        world.tick_battle_camera();
        let pose = world.battle_cam_pose();
        if pose.pitch == -448.0 && pose.tr[1] == 896.0 {
            saw_creature_cut = true;
        }
        ticks += 1;
        if !busy {
            break;
        }
    }
    eprintln!("[ran] Gimard band held {ticks} ticks");
    assert!(
        ticks > 400,
        "the module's countdown paces the band: {ticks} ticks"
    );
    assert!(ticks < 4000, "the chain ends");
    assert!(saw_creature_cut, "arm 4's low cut on the creature landed");
    assert!(world.casting.summon_stager.is_none(), "the stager retired");
}

/// PROT 0903 seats its records on the arms that make the spawn calls: three
/// on the creature in arm 3, the camera-relative fire tunnel in arm 6, and
/// the battle overlay's breath prototype in arm 8 - not the whole record set
/// on the stager's first tick, where every program had run out long before
/// the creature's attack.
#[test]
fn gimard_seats_its_tunnel_and_breath_on_their_arms() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut world = battle_world(&dir);
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries[898].clone();
    let mut b898 = Vec::new();
    archive.read_entry(&entry, &mut b898).expect("read 0898");
    world.tables.move_power_overlay = Some(std::sync::Arc::from(b898.as_slice()));
    world.arm_summon_stager(0, 0x81);
    world.casting.summon_actor_slot = Some(SUMMON_SEAT);
    world.battle_ctx.action_state = 0x35;

    // On first entering each arm: (part count, any camera-relative part,
    // any part on the breath's library mesh).
    let mut at_arm = std::collections::BTreeMap::new();
    for _ in 0..4000 {
        let busy = world.summon_stager_tick();
        world.tick_summon(legaia_engine_core::world::EFFECT_SCENE_GRAPH_STEP);
        world.clock.display_frames += 1;
        world.tick_battle_camera();
        let seen = world
            .casting
            .active_summon
            .as_ref()
            .map_or((0, false, false), |s| {
                (
                    s.parts.len(),
                    s.parts.iter().any(|p| p.state.field_52 & 0x780 != 0),
                    s.parts
                        .iter()
                        .any(|p| p.model_sel == GIMARD_BREATH_MODEL_SEL),
                )
            });
        at_arm.entry(world.casting.module_phase).or_insert(seen);
        if !busy {
            break;
        }
    }
    eprintln!("[ran] (parts, camera-relative, breath) on entering each arm: {at_arm:?}");
    assert_eq!(
        at_arm.get(&3).map(|s| s.0),
        Some(0),
        "nothing seated before arm 3"
    );
    assert_eq!(at_arm.get(&4).map(|s| s.0), Some(3), "arm 3 seats three");
    assert!(
        !at_arm.get(&6).is_some_and(|s| s.1),
        "no camera-relative part before arm 6"
    );
    assert!(
        at_arm.get(&7).is_some_and(|s| s.1),
        "arm 6 seats the tunnel"
    );
    assert!(
        !at_arm.get(&8).is_some_and(|s| s.2),
        "no breath before arm 8"
    );
    assert!(
        at_arm.get(&9).is_some_and(|s| s.2),
        "arm 8 seats the breath"
    );
}

/// The library mesh selector of the effect prototype `*(0x801F63A8)` PROT
/// 0903's arm 8 spawns (`record[+0] = 0x18`).
const GIMARD_BREATH_MODEL_SEL: i16 = 0x18;
