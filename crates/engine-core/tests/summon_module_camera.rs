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
