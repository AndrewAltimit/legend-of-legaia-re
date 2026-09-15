//! Disc-gated ladder for the **player Seru-magic tick bodies** of action ids
//! `0x87..=0x8B` (PROT 0909..0913), ported in
//! `legaia_engine_vm::cast_seru_ticks_b`.
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
//! **rendezvous** phases (`3` and `8`) are walked the way retail walks them -
//! by calling the module's move-VM stager arm `0`, which is the only thing
//! that bumps `ctx[+0x279]` there.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_anim_rate::AnimRate;
use legaia_engine_vm::cast_seru_ticks_b as seru;
use std::path::PathBuf;

/// The summon seat every body in the band poses (`actor_table[7]`).
const SUMMON_SLOT: usize = 7;
/// Frames stepped per id before the ladder gives up. Every chain here is at
/// most 21 arms plus a settle arm plus the terminal, so this is generous.
const FRAME_BOUND: usize = 96;

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
        party: legaia_engine_core::world::PartyState {
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
/// reached Done)`.
fn walk(world: &mut World, id: u8) -> (Vec<u8>, usize, bool) {
    world.casting.module_phase = 0;
    world.casting.module_ctx_278 = 0;
    let mut phases = vec![0u8];
    let mut ported = 0usize;
    let mut done = false;
    for _ in 0..FRAME_BOUND {
        let phase = world.casting.module_phase;
        // PROT 0909 parks on phases 3 and 8 until its move-VM stager bumps
        // the phase; every other arm of that stager - and every arm of the
        // other four modules' stagers - only hands spawn records to the pool.
        let arm = if seru::VIGURO_RENDEZVOUS_PHASES.contains(&phase) {
            0
        } else {
            2
        };
        let Some(run) = world.run_cast_module_code(id, arm) else {
            break;
        };
        if run.tick_ported {
            ported += 1;
        }
        if world.casting.module_phase != phase {
            phases.push(world.casting.module_phase);
        }
        if run.tick_ported && !run.busy {
            done = true;
            break;
        }
    }
    (phases, ported, done)
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
    let mut pool = legaia_asset::cast_effect_pool::CastEffectPool::new();
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

        let (phases, ported, done) = walk(&mut world, id);
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
            910 => assert_ne!(
                world.actors[SUMMON_SLOT].battle.queued_anim, 0,
                "PROT 0910 stages clips on the summon seat and nothing else"
            ),
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
                assert_eq!(
                    world.casting.module_ctx_278,
                    seru::NOVA_ARM9_CLIP,
                    "PROT 0913's arm 9 writes ctx+0x278 out of the clip register"
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
/// This is the property the ladder's arm choice rests on, asserted directly
/// so a regression shows up here rather than as a mysterious bound hit.
#[test]
fn the_viguro_rendezvous_needs_the_stager() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let mut world = battle_world();
    world.install_menu_text(&scus);

    for phase in seru::VIGURO_RENDEZVOUS_PHASES {
        world.casting.module_phase = phase;
        // Stager arm 2 is a pure spawn arm: it writes no state and no phase.
        let run = world
            .run_cast_module_code(0x87, 2)
            .expect("PROT 0909 seats");
        assert!(run.tick_ported, "the tick body ran");
        assert_eq!(
            world.casting.module_phase, phase,
            "phase {phase} holds without the stager"
        );
        assert!(run.busy, "and reports busy while it holds");

        // Stager arm 0 carries `lbu 0x279 / addiu 1 / sb 0x279`.
        world
            .run_cast_module_code(0x87, 0)
            .expect("PROT 0909 seats");
        assert_eq!(
            world.casting.module_phase,
            phase + 1,
            "the stager's arm 0 releases phase {phase}"
        );
    }
    eprintln!("[ok] PROT 0909's two rendezvous phases hold for the tick and move for the stager");
}
