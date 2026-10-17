//! Disc-gated ladder for the **player Seru-magic tick bodies** of action ids
//! `0x87..=0x8B` (PROT 0909..0913), ported in
//! `legaia_engine_cast::seru_magic_ticks`.
//!
//! Why disc-gated when `World::cast_module_for` is arithmetic. Without
//! `SCUS_942.54` installed the resolver never consults the spell table's
//! class byte, so every id falls through to the action-id band by default and
//! the ladder would prove only that `903 + (id - 0x81)` is addition. With the
//! disc's own table installed it proves the five ids are *not* capture-class
//! and really do seat PROT 0909..0913, which is the claim the port rests on.
//!
//! What it drives. Each id is seated and then stepped frame by frame through
//! `World::run_cast_module_code` until the body reports `Done` or a bound is
//! hit, asserting that a ported tick body entered, that the module phase
//! walked, and that each body's own signature write landed. PROT 0909's two
//! **rendezvous** phases (`3` and `8`) are released only by its stager,
//! which the records its arms 2 and 7 spawn call through the move VM's op
//! `0x20`. This walk seats no parts, so the band keeps those records' clocks
//! in their place (`cast_module_camera::VIGURO_STAGER_CALLERS`) and each
//! rendezvous holds for as long as the record's script would make it.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_battle_vm::battle_anim_rate::AnimRate;
use legaia_engine_cast::seru_magic_ticks as seru;
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_overlay_images::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use std::path::PathBuf;

/// The summon seat every body in the band poses (`actor_table[7]`).
const SUMMON_SLOT: usize = 7;
/// Frames stepped per id before the ladder gives up. Every chain here is at
/// most 21 arms plus a settle arm plus the terminal; PROT 0910 also runs its
/// own arm timers (`seru_magic_ticks::swordie_tick`), several hundred frames
/// at a frame step of one, and PROT 0909's countdown arms and two record-paced
/// rendezvous hold it for about 1200.
const FRAME_BOUND: usize = 4096;

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

/// A live battle: three party seats, four enemy seats, and the summon seat
/// above them. The bodies read all three groups, so all of them have to be
/// seated or an arm early-outs before the write the ladder is here to see.
fn battle_world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 3,
            ..Default::default()
        },
        ..World::default()
    };
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    for slot in 0..8 {
        world.actors[slot].active = true;
        world.actors[slot].battle.max_hp = 400;
        world.actors[slot].battle.hp = 400;
        world.actors[slot].battle.mp = 99;
        world.actors[slot].battle.liveness = 1;
        world.actors[slot].battle.anim_rate = AnimRate(8);
    }
    world.actors[0].battle.active_target = 3;
    world.battle_ctx.active_actor = 0;
    world.casting.summon_actor_slot = Some(SUMMON_SLOT as u8);
    world
}

/// One id's walk. Returns `(phases observed, frames a ported body ran,
/// reached Done, the values ctx+0x278 took in order)`.
fn walk(world: &mut World, id: u8) -> (Vec<u8>, usize, bool, Vec<u8>) {
    world.casting.module_phase = 0;
    world.casting.module_ctx_278 = 0;
    let mut phases = vec![0u8];
    let mut ctx_278 = vec![0u8];
    let mut ported = 0usize;
    let mut done = false;
    for _ in 0..FRAME_BOUND {
        let phase = world.casting.module_phase;
        // PROT 0909's stager is reached through its parts' op-`0x20` calls,
        // not off this argument; the other four modules' stagers only hand
        // spawn records to the pool.
        let arm = 2;
        let Some(run) = world.run_cast_module_code(id, arm) else {
            break;
        };
        if run.tick_ported {
            ported += 1;
        }
        if world.casting.module_phase != phase {
            phases.push(world.casting.module_phase);
        }
        if ctx_278.last() != Some(&world.casting.module_ctx_278) {
            ctx_278.push(world.casting.module_ctx_278);
        }
        if run.tick_ported && !run.busy {
            done = true;
            break;
        }
    }
    (phases, ported, done, ctx_278)
}

#[test]
fn every_player_seru_tick_body_walks_its_phase_chain() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");

    // The resident pool, so the ladder runs the same shape the scene host
    // does rather than a band with no module staged.
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut pool = legaia_overlay_images::cast_effect_pool::CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let Some(entry) = archive.entries.get(idx as usize).cloned() else {
            continue;
        };
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_err() {
            continue;
        }
        pool.insert(idx, &bytes);
    }
    let pool = std::sync::Arc::new(pool);

    let mut entered = 0usize;
    for (i, id) in (0x87u8..=0x8B).enumerate() {
        let entry = 909 + i as u32;
        let mut world = battle_world();
        world.install_menu_text(&scus);
        world.casting.effect_pool = Some(pool.clone());

        assert_eq!(
            world.cast_module_for(id),
            Some(entry),
            "id {id:#04x} must seat PROT {entry} against the disc's own spell table"
        );

        let (phases, ported, done, ctx_278) = walk(&mut world, id);
        assert!(
            ported > 0,
            "id {id:#04x} (PROT {entry}) never entered a ported tick body"
        );
        assert!(
            phases.len() > 1,
            "id {id:#04x} (PROT {entry}) never advanced its module phase: {phases:?}"
        );
        assert!(
            phases.contains(&seru::SERU_B_DONE_PHASE) && done,
            "id {id:#04x} (PROT {entry}) never reached the terminal phase: {phases:?}"
        );
        entered += ported;

        // Each body's own signature write, so the walk is not vacuous.
        match entry {
            909 => {
                assert!(
                    phases.contains(&seru::VIGURO_DAMAGE_PHASE),
                    "PROT 0909 skipped its damage arm: {phases:?}"
                );
                assert_eq!(
                    world.actors[SUMMON_SLOT].battle.render_flag,
                    seru::VIGURO_DONE_RENDER_FLAG,
                    "PROT 0909's terminal arm leaves the summon seat on 2"
                );
            }
            910 => {
                assert_ne!(
                    world.actors[SUMMON_SLOT].battle.queued_anim, 0,
                    "PROT 0910 stages clips on the summon seat"
                );
                assert_eq!(
                    world.casting.module_swordie.hits,
                    u32::from(seru::SWORDIE_SLASHES),
                    "PROT 0910's arm timers land all four slashes"
                );
            }
            911 => {
                assert!(
                    !phases.contains(&6) && !phases.contains(&7) && !phases.contains(&8),
                    "PROT 0911's arm 5 jumps the phase to 9, so 6..8 are unreachable: {phases:?}"
                );
                assert!(
                    phases.contains(&seru::ORB_ARM5_TARGET_PHASE),
                    "PROT 0911 never reached its heal arm: {phases:?}"
                );
            }
            912 => assert!(
                phases.contains(&seru::FREED_DAMAGE_PHASE),
                "PROT 0912 skipped its damage arm: {phases:?}"
            ),
            _ => {
                assert!(
                    phases.contains(&seru::NOVA_DAMAGE_PHASE),
                    "PROT 0913 skipped its damage arm: {phases:?}"
                );
                // Arm 9 writes the clip register into ctx+0x278 and arm 0x10
                // clears it on its way out (`sb zero,0x278` at `0x801F8154`).
                assert_eq!(
                    ctx_278,
                    [0, seru::NOVA_ARM9_CLIP, 0],
                    "PROT 0913's arm 9 writes ctx+0x278 out of the clip register, and arm \
                     0x10 clears it"
                );
            }
        }
    }

    eprintln!(
        "[ok] player-Seru tick ladder: 5 ids (PROT 0909..0913) walked their chains, \
         {entered} ported tick frames"
    );
}

/// PROT 0909 parks on phases `3` and `8` and only its **stager** moves it on.
/// This is the property the ladder's walk rests on, asserted directly
/// so a regression shows up here rather than as a mysterious bound hit.
///
/// Three ways. A rendezvous phase entered with no record behind it holds for
/// good. Through the band seam with the module's parts running, each
/// rendezvous lasts at least as long as its record's script waits before its
/// op-`0x20` call. And with the records seated but their parts stopped, the
/// phase holds for good again: a seated record is the only caller, so the
/// band keeps no clock of its own in its place.
#[test]
fn the_viguro_rendezvous_needs_the_stager() {
    use legaia_engine_cast::cast_module_camera::VIGURO_STAGER_CALLERS;
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");

    // 1. No record spawned: nothing will call the stager.
    for phase in seru::VIGURO_RENDEZVOUS_PHASES {
        let mut world = battle_world();
        world.install_menu_text(&scus);
        world.casting.module_phase = phase;
        for _ in 0..FRAME_BOUND {
            let run = world
                .run_cast_module_code(0x87, 2)
                .expect("PROT 0909 seats");
            assert!(run.tick_ported, "the tick body ran");
            assert!(run.busy, "and reports busy while it holds");
        }
        assert_eq!(
            world.casting.module_phase, phase,
            "phase {phase} holds without the stager"
        );
    }

    // 2. The band seam with the parts running: the records release both.
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut pool = legaia_overlay_images::cast_effect_pool::CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let Some(entry) = archive.entries.get(idx as usize).cloned() else {
            continue;
        };
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_ok() {
            pool.insert(idx, &bytes);
        }
    }
    let pool = std::sync::Arc::new(pool);
    let band_world = || {
        let mut world = battle_world();
        world.install_menu_text(&scus);
        world.casting.effect_pool = Some(pool.clone());
        world.arm_summon_stager(0, 0x87);
        world.casting.summon_actor_slot = Some(SUMMON_SLOT as u8);
        world.battle_ctx.action_state = 0x35;
        world
    };
    let mut world = band_world();
    let mut held = std::collections::BTreeMap::<u8, usize>::new();
    for _ in 0..FRAME_BOUND {
        let phase = world.casting.module_phase;
        let busy = world.summon_stager_tick();
        world.tick_summon(legaia_engine_core::world::EFFECT_SCENE_GRAPH_STEP);
        world.tick_battle_animations();
        *held.entry(phase).or_default() += 1;
        if !busy {
            break;
        }
    }
    for (phase, (_, _, wait)) in seru::VIGURO_RENDEZVOUS_PHASES
        .iter()
        .zip(VIGURO_STAGER_CALLERS)
    {
        let frames = held.get(phase).copied().unwrap_or(0);
        assert!(
            frames > usize::from(wait),
            "phase {phase} lasts the record's {wait} frames before its call: held {frames} \
             ({held:?})"
        );
    }
    assert!(
        world.casting.summon_stager.is_none(),
        "the band ran out past both rendezvous: {held:?}"
    );

    // 3. The records seated, their parts stopped: no call, so no release.
    let mut world = band_world();
    while world.casting.module_phase != 3 {
        world.summon_stager_tick();
    }
    for _ in 0..FRAME_BOUND {
        world.summon_stager_tick();
    }
    assert_eq!(
        world.casting.module_phase, 3,
        "a seated record is the only caller: phase 3 holds while its part is stopped"
    );
    for _ in 0..FRAME_BOUND {
        world.summon_stager_tick();
        world.tick_summon(legaia_engine_core::world::EFFECT_SCENE_GRAPH_STEP);
        if world.casting.module_phase != 3 {
            break;
        }
    }
    assert_eq!(
        world.casting.module_phase, 4,
        "and the part's call releases it once the part runs"
    );
    eprintln!(
        "[ok] PROT 0909's two rendezvous hold without their records and move for the stager \
         ({held:?})"
    );
}
