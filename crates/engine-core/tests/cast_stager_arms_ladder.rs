//! Disc-gated **cast stager-arms ladder**: cast every slot-B module whose
//! stager arms run off its parts' op-`0x20` calls, in a live battle, and let
//! the parts' move programs run far enough to make those calls.
//!
//! Why it exists. A module's stager (`0x801F6734`'s row, reached through
//! `gp[+0x714]`) is not run off the module phase: a seated part's own move
//! program calls it with move-VM op `0x20` (`MoveVmHostImpl::ext_20`), and
//! the band drains the queued calls into `World::run_part_stager_calls` on
//! the module's next tick. The cast-band ladders (`cast_band_ladder`,
//! `cast_module_bodies_ladder`, ...) step module code with no part running,
//! so no queued call ever reaches an arm and every stager kernel in
//! `legaia_engine_cast::cast_module_ticks` read *live but never entered*
//! ([`reach-triage.md`](../../../docs/tooling/reach-triage.md)).
//!
//! The drive is the ordinary engine path with one seat chosen: the battle is
//! a real encounter, the cast is handed to the action SM by the
//! [`InflightCastSeed`] seam the retail comparison uses, and every frame is
//! a headless driver's frame - the scene tick and then the world's frame
//! tail, whose effect scene-graph step runs the parts. Nothing below calls a
//! stager kernel.
//!
//! The capture cells (PROT 0938..0966) are monster spells in retail, and are
//! cast here from party seat 0 like the rest. The band keys on the spell
//! record's class byte, not on the caster's side, so the module, its body
//! and its parts are the ones a monster's cast pages; the caster's own
//! approach and pose are not what this ladder measures.
//!
//! Non-vacuity: `CastFxState::stager_arm_calls_run` counts the calls handed
//! to a stager arm. Each module named in [`EXPECTED`] has to move it during
//! its own cast.
//!
//! Export WITHOUT `--release` (an optimised build inlines the kernels). Skips
//! (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_battle::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_battle::spells::{
    SpellCatalog, SpellDef, SpellEffect, SpellElement, SpellTarget,
};
use legaia_engine_battle_loop::battle_state::INFLIGHT_GROUND_SLOTS;
use legaia_engine_battle_loop::battle_state::InflightCastSeed;
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::{InputState, PadButton};
use legaia_overlay_images::cast_effect_pool::{
    CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The band entries whose stager arms run off their parts' op-`0x20` calls
/// (`World::runs_part_stager_calls`, private to the crate; transcribed so a
/// module dropped from that list shows here as an unexpected zero).
const STAGER_ENTRIES: [u32; 28] = [
    905, 906, 907, 908, 910, 911, 912, 913, 914, 915, 917, 918, 921, 924, 925, 928, 929, 930, 931,
    932, 933, 934, 938, 940, 944, 946, 961, 962,
];

/// The entries whose cast must hand at least one call to a stager arm: every
/// entry of [`STAGER_ENTRIES`] but two.
///
/// - PROT 0940's stager `FUN_801F82CC` is a bare `jr ra`, so there is no arm
///   to reach, and none of its four ids is the per-arm body
///   (`capture_spawns_per_arm`) the drain runs for.
/// - PROT 0944's Guilty Cross (id `0x37`, body `0x801F6A04`) is the per-arm
///   body, but its cast seats two parts and never the arm-3 record
///   `0x801F8210` whose program calls stager arms 1 and 2, so no call is
///   made. Open: the arm-3 seat, not this ladder.
const EXPECTED: &[u32] = &[
    905, 906, 907, 908, 910, 911, 912, 913, 914, 915, 917, 918, 921, 924, 925, 928, 929, 930, 931,
    932, 933, 934, 938, 946, 961, 962,
];

/// Frames one cast is given, past the frame the cast was armed. PROT 0906
/// runs past a thousand; a creature summon's whole chain runs longer.
const CAST_FRAMES: usize = 0x1800;
/// Frames kept running after the cast folds: PROT 0908's slam and PROT
/// 0913's late arms make their stager calls after the outcome has folded.
const TAIL_FRAMES: usize = 0xC0;

/// Retail's enemy-group target code on the action record's `+0x1DD`.
const TARGET_ENEMY_GROUP: u8 = 9;

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

fn prot_entry(dir: &Path, idx: usize) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries[idx].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .unwrap_or_else(|e| panic!("read PROT {idx}: {e:#}"));
    bytes
}

/// The pool the scene host builds, straight off `PROT.DAT`.
fn build_pool(dir: &Path) -> CastEffectPool {
    let mut pool = CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let bytes = prot_entry(dir, idx as usize);
        assert!(pool.insert(idx, &bytes), "PROT {idx} is in the band");
    }
    pool
}

/// A spell the catalog can dispatch for an id the pinned Seru catalog does
/// not carry. The module, not this record, decides what the cast does.
fn placeholder(id: u8) -> SpellDef {
    SpellDef {
        id,
        name: format!("band {id:#04X}"),
        mp_cost: 0,
        target: SpellTarget::AllEnemies,
        element: SpellElement::Neutral,
        effect: SpellEffect::Damage {
            base_power: 1,
            element: SpellElement::Neutral,
        },
        ..Default::default()
    }
}

struct Disc {
    scus: Vec<u8>,
    overlay_898: Arc<[u8]>,
    pool: Arc<CastEffectPool>,
}

fn build_world(disc: &Disc, ids: &[u8]) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.set_battle_attack(i as u8, 60);
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.install_menu_text(&disc.scus);
    let mut spells: SpellCatalog = legaia_engine_battle::retail_magic::retail_seru_magic_catalog();
    for &id in ids {
        if spells.get(id).is_none() {
            spells.insert(placeholder(id));
        }
    }
    w.tables.spell_catalog = spells;
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.locomotion.camera_azimuth = 0;

    use legaia_engine_battle::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("cast_stager_arms_ladder");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w.battle.no_escape = true;
    w
}

/// One frame the way a headless driver runs it (`BootSession::tick`): the
/// scene tick, then the world's frame tail, whose effect scene-graph step
/// runs the cast module's parts and so their op-`0x20` calls.
fn frame(w: &mut World) {
    let _ = w.take_pending_summon_spawn();
    let _ = w.tick();
    let _ = w.step_world_frame_tail(None, None, |_| None);
}

fn enter_battle(w: &mut World, disc: &Disc) -> bool {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        frame(w);
        if w.mode == SceneMode::Battle {
            w.set_pad(0);
            w.install_cast_effect_pool(disc.pool.clone());
            w.tables.move_power_overlay = Some(disc.overlay_898.clone());
            return true;
        }
    }
    false
}

/// Keep both sides standing for the whole cast: a module that kills its
/// row, or a capture spell that wipes the party, ends the fight before the
/// late arms make their calls.
///
/// Every HP write goes through `set_hp_synced` (a bare `hp` write leaves the
/// action SM's `0x51` bar-drain gate parked).
fn refill(w: &mut World) {
    let pc = w.party.party_count as usize;
    for i in 0..pc {
        w.actors[i].battle.max_hp = 9000;
        w.actors[i].battle.set_hp_synced(9000);
        w.actors[i].battle.liveness = 1;
        w.actors[i].battle.mp = 250;
    }
    w.actors[pc].battle.max_hp = 9000;
    w.actors[pc].battle.set_hp_synced(9000);
}

/// How one cast went.
#[derive(Debug, Default, Clone, Copy)]
struct CastRun {
    armed: bool,
    folded: bool,
    calls: u32,
    ended_battle: bool,
    frames: usize,
    /// The most parts the cast's scene held at once.
    parts: usize,
}

/// Cast `id` once from party seat 0 and run its band out.
fn run_cast(w: &mut World, disc: &Disc, id: u8) -> CastRun {
    let mut run = CastRun::default();
    if w.mode != SceneMode::Battle && !enter_battle(w, disc) {
        return run;
    }
    // Wait for the first command prompt (and the entry sweep under it).
    for _ in 0..0x400 {
        if w.battle.command.is_some() {
            break;
        }
        w.set_pad(0);
        frame(w);
    }
    refill(w);
    w.battle.inflight_seed = Some(InflightCastSeed {
        caster: 0,
        spell_id: id,
        target: TARGET_ENEMY_GROUP,
        ground: [None; INFLIGHT_GROUND_SLOTS],
        idle: [None; INFLIGHT_GROUND_SLOTS],
    });
    let before = w.casting.stager_arm_calls_run;
    let mut tail = TAIL_FRAMES;
    for _ in 0..CAST_FRAMES {
        if w.mode != SceneMode::Battle {
            run.ended_battle = true;
            break;
        }
        if w.casting.pending_cast.is_some() {
            run.armed = true;
        } else if run.armed {
            run.folded = true;
            if tail == 0 {
                break;
            }
            tail -= 1;
        }
        w.set_pad(0);
        refill(w);
        frame(w);
        run.frames += 1;
        if let Some(sc) = w.casting.active_summon.as_ref() {
            run.parts = run.parts.max(sc.parts.len());
        }
    }
    w.set_pad(0);
    w.battle.inflight_seed = None;
    // The next cast's scene is its own.
    w.casting.active_summon = None;
    run.calls = w.casting.stager_arm_calls_run.wrapping_sub(before);
    run
}

#[test]
fn every_part_stager_module_runs_its_arms_in_a_live_cast() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] cast stager arms: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let overlay_898: Arc<[u8]> = Arc::from(
        prot_entry(
            &dir,
            legaia_game_tables::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX,
        )
        .as_slice(),
    );
    let disc = Disc {
        scus,
        overlay_898,
        pool: Arc::new(build_pool(&dir)),
    };

    // Every id the engine's own resolver pages each stager module for. A
    // capture cell can hold several bodies, and only some of them run the
    // stager, so every id of a capture cell is cast.
    let probe = build_world(&disc, &[]);
    let mut ids_for: std::collections::BTreeMap<u32, Vec<u8>> = Default::default();
    for id in 0u8..=0xFF {
        if let Some(e) = probe.cast_module_for(id)
            && STAGER_ENTRIES.contains(&e)
        {
            ids_for.entry(e).or_default().push(id);
        }
    }
    let all_ids: Vec<u8> = ids_for.values().flatten().copied().collect();
    let mut w = build_world(&disc, &all_ids);
    assert!(
        enter_battle(&mut w, &disc),
        "no encounter in 6000 field ticks"
    );

    let mut report: Vec<(u32, u8, CastRun)> = Vec::new();
    for &entry in &STAGER_ENTRIES {
        let Some(ids) = ids_for.get(&entry) else {
            report.push((entry, 0, CastRun::default()));
            continue;
        };
        // An action-id module has one id; every body of a capture cell is
        // cast, since only some of them run the stager.
        let ids: Vec<u8> = if entry >= 935 {
            ids.clone()
        } else {
            vec![ids[0]]
        };
        for id in ids {
            let r = run_cast(&mut w, &disc, id);
            eprintln!(
                "[cast] PROT {entry} id {id:#04x}: armed={} folded={} ended={} frames={} \
                 parts={} stager calls={}",
                r.armed, r.folded, r.ended_battle, r.frames, r.parts, r.calls
            );
            report.push((entry, id, r));
        }
    }

    let reached: std::collections::BTreeSet<u32> = report
        .iter()
        .filter(|(_, _, r)| r.calls > 0)
        .map(|(e, _, _)| *e)
        .collect();
    let missing: Vec<u32> = EXPECTED
        .iter()
        .copied()
        .filter(|e| !reached.contains(e))
        .collect();
    eprintln!(
        "cast_stager_arms_ladder: {} of {} stager modules handed a call to an arm: {reached:?}",
        reached.len(),
        STAGER_ENTRIES.len()
    );
    assert!(
        missing.is_empty(),
        "these modules' casts handed no op-0x20 call to a stager arm: {missing:?}"
    );
    assert!(
        !reached.is_empty(),
        "no cast handed a single op-0x20 call to a stager arm - the parts never ran"
    );
}
