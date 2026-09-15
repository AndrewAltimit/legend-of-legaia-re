//! Disc-gated **player-Seru tick ladder**: seat spell ids `0x81..=0x86`
//! (PROT 0903..0908) and walk each module's phase chain from `0` to its
//! terminal through `World::run_cast_module_code`.
//!
//! Why it exists, separately from `w4d_cast_band_ladder`. That ladder steps
//! two *arms* per band entry and asks whether a ported body was reached at
//! all. These six bodies are `beq` **chains** fifteen arms deep whose
//! simulation writes live in the late arms - PROT 0903's hit is arm 11, PROT
//! 0906's sweep is arm 13, PROT 0907's kill / confuse fork is arm 13 - so a
//! two-arm probe enters the prologue and nothing else. This one walks every
//! phase until the tick reports done, and asserts each body actually wrote
//! battle state on the way through.
//!
//! What it does **not** assert: damage. The seam feeds no roll (the engine
//! folds a cast's HP outcome once, at `cast_spell_on_slots_prepaid`), so the
//! observable change is the choreography - staged clips, render flags,
//! retargets and animation rates - which is exactly the half these bodies
//! own.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::cast_module_ticks::SUMMON_SEAT;
use std::path::PathBuf;

/// The six ids this lane ports, and the PROT entry each resolves to.
const IDS: [(u8, u32); 6] = [
    (0x81, 903),
    (0x82, 904),
    (0x83, 905),
    (0x84, 906),
    (0x85, 907),
    (0x86, 908),
];

/// Hard stop on the walk. The deepest chain in the set names sixteen arms
/// plus the `0xFF` terminal, and PROT 0906's stager arm can advance a second
/// time, so anything past this is a body that never reported done.
const MAX_FRAMES: usize = 64;

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

/// A minimal live battle with a party seat, four monster seats and the summon
/// seat above them. All six bodies read the caster, the victim and
/// `actor_table[7]`, and three of them sweep seats `3..=6`, so the row has to
/// be wide enough or the sweep arm writes nothing and the ladder goes vacuous.
fn battle_world() -> World {
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
    for slot in [0usize, 3, 4, 5, 6, SUMMON_SEAT as usize] {
        world.actors[slot].active = true;
        world.actors[slot].battle.max_hp = 400;
        world.actors[slot].battle.hp = 400;
        world.actors[slot].battle.mp = 99;
        world.actors[slot].battle.liveness = 1;
    }
    world.actors[0].battle.active_target = 3;
    world.battle_ctx.active_actor = 0;
    world
}

/// The fields these bodies write that survive `write_cast_actor_state`, for
/// the seats they write them on.
fn choreography_probe(w: &World) -> Vec<(u8, u8, u8, u8)> {
    (0..8u8)
        .map(|s| {
            let a = &w.actors[s as usize];
            (
                a.battle.queued_anim,
                a.battle.render_flag,
                a.battle.active_target,
                a.battle.anim_rate.get(),
            )
        })
        .collect()
}

#[test]
fn every_player_seru_tick_walks_its_phase_chain_and_writes_state() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");

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

    let mut walked = 0usize;
    let mut total_frames = 0usize;
    let mut bodies_that_wrote = 0usize;

    for (id, expect_entry) in IDS {
        let mut w = battle_world();
        w.install_menu_text(&scus);
        w.casting.effect_pool = Some(pool.clone());

        assert_eq!(
            w.cast_module_for(id),
            Some(expect_entry),
            "id {id:#04x} did not resolve to its PROT entry"
        );

        let before = choreography_probe(&w);
        let mut frames = 0usize;
        let mut phases: Vec<u8> = Vec::new();
        let mut entered = false;
        let mut finished = false;

        while frames < MAX_FRAMES {
            let Some(run) = w.run_cast_module_code(id, 0) else {
                panic!("id {id:#04x}: no band entry resident - the pool did not install");
            };
            assert_eq!(
                run.prot_entry, expect_entry,
                "id {id:#04x} ran a different entry than the resolver named"
            );
            entered |= run.tick_ported;
            phases.push(run.phase);
            frames += 1;
            if !run.busy {
                finished = true;
                break;
            }
        }

        assert!(entered, "id {id:#04x}: no ported tick body was entered");
        assert!(
            finished,
            "id {id:#04x}: the chain never reported done in {MAX_FRAMES} frames \
             (phases {phases:?})"
        );
        // Non-vacuity 1: the phase walked past the prologue, so the late arms
        // that hold the simulation writes were actually reached.
        let deepest = phases
            .iter()
            .copied()
            .filter(|&p| p != 0xFF)
            .max()
            .unwrap_or(0);
        assert!(
            deepest >= 8,
            "id {id:#04x}: the walk only reached phase {deepest} (phases {phases:?})"
        );
        // Non-vacuity 2: something the body writes is visible on the world.
        let after = choreography_probe(&w);
        if after != before {
            bodies_that_wrote += 1;
        }

        walked += 1;
        total_frames += frames;
    }

    assert_eq!(walked, IDS.len(), "not every id was walked");
    assert_eq!(
        bodies_that_wrote,
        IDS.len(),
        "{bodies_that_wrote} of {} bodies left the battle state untouched",
        IDS.len()
    );
    eprintln!(
        "[ok] player-Seru tick ladder: {walked} of {} ids walked to their terminal \
         in {total_frames} frames, all {bodies_that_wrote} wrote battle state",
        IDS.len()
    );
}
