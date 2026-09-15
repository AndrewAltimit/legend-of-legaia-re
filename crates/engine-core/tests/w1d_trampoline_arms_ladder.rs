//! Disc-gated **trampoline-arm ladder**: seat every `(PROT entry, action id)`
//! pair the seven modules `legaia_engine_vm::cast_arm_ticks` carries, walk its
//! module phase to the end, and assert the body actually entered.
//!
//! Why it exists. The fourteen bodies are reached only through their module's
//! **trampoline**, which switches on the caster's queued action id - so an id
//! the trampoline does not name ticks nothing, and a ladder keyed on the PROT
//! entry alone would run one body for a cell that holds two. Three of these
//! bodies also share the VA `0x801F6A04` across three images, which is why the
//! dispatch seam keys on the pair and why this ladder asserts the pair.
//!
//! Sibling of `w4d_cast_band_ladder.rs`, which walks the whole band one frame
//! deep; this one walks seven modules to their terminal arm.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::cast_arm_ticks as arms;
use std::path::PathBuf;

/// `(PROT entry, action id, body VA)` - the fourteen bodies, fifteen arms
/// (PROT 0940's split is reached by two ids).
const ROWS: [(u32, u8, u32); 15] = [
    (940, 0xAC, arms::GLARE_DIVIDE_BLIND_TICK),
    (940, 0x50, arms::GLARE_DIVIDE_SPLIT_TICK),
    (940, 0xAE, arms::GLARE_DIVIDE_SPLIT_TICK),
    (941, 0x51, arms::STEAL_TICK),
    (941, 0xB9, arms::STEAL_SWEEP_TICK),
    (943, 0x40, arms::CURSE_SINGLE_TICK),
    (943, 0xB5, arms::CURSE_MP_DRAIN_TICK),
    (944, 0x37, arms::GUILTY_CROSS_TICK),
    (944, 0x53, arms::GUILTY_CROSS_CURSE_TICK),
    (950, 0x5A, arms::ROLLING_FLARE_TICK),
    (950, 0xAB, arms::ROLLING_FLARE_SWEEP_TICK),
    (956, 0x71, arms::WATER_HAZARD_TICK),
    (962, 0xA2, arms::BLADE_BREATH_A_TICK),
    (962, 0xA3, arms::BLADE_BREATH_B_TICK),
    (962, 0xA4, arms::BLADE_BREATH_C_TICK),
];

/// Frames to drive one body before giving up. The longest arm map here is
/// fourteen and two bodies hold on a clip gate, so this is comfortably past
/// every terminal arm.
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

/// A live battle with a full party row and three monster seats: the scoped
/// bodies branch on the caster's `+0x1DD`, and the sweeps bound on `ctx[+0]` /
/// `ctx[+1]`, so an under-populated table would make the interesting legs
/// unreachable.
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
    for slot in 0usize..6 {
        world.actors[slot].active = true;
        world.actors[slot].battle.max_hp = 4000;
        world.actors[slot].battle.hp = 4000;
        world.actors[slot].battle.mp = 77;
        world.actors[slot].battle.liveness = 1;
        world.actors[slot].battle.anim_rate = legaia_engine_vm::battle_anim_rate::AnimRate(8);
    }
    // The caster is a monster seat aiming at the party row, which is what a
    // capture-class cast is in retail.
    world.battle_ctx.active_actor = 3;
    world.actors[3].battle.active_target = 0;
    world.party.inventory.insert(0x20, 3);
    world.party.inventory.insert(0x21, 1);
    world
}

fn seeded(base: &World, id: u8) -> World {
    let mut w = battle_world();
    w.menu.text = base.menu.text.clone();
    w.casting.effect_pool = base.casting.effect_pool.clone();
    let caster = w.battle_ctx.active_actor as usize;
    // The byte the trampoline reads: `caster[+0x1DF]`.
    w.actors[caster].battle.params[0] = id;
    w.casting.module_phase = 0;
    w.casting.module_ctx_278 = 0;
    w
}

/// A cheap fingerprint of everything the fourteen bodies write, so "the body
/// entered" is an observed state change and not just a returned flag.
fn fingerprint(w: &World) -> Vec<u64> {
    let mut out = vec![
        w.casting.module_phase as u64,
        w.casting.module_ctx_278 as u64,
        w.battle_ctx.turn_cursor as u64,
    ];
    for a in &w.actors {
        out.push(
            (a.active as u64)
                | (a.battle.hp as u64) << 1
                | (a.battle.mp as u64) << 17
                | (a.battle.field_flags as u64) << 33
                | (a.battle.render_flag as u64) << 49
                | (a.battle.queued_anim as u64) << 57,
        );
        out.push(a.battle.init_key as u64 | (a.battle.anim_rate.0 as u64) << 16);
    }
    out
}

#[test]
fn every_trampoline_arm_enters_its_body_and_reaches_a_terminal_step() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");

    let mut base = battle_world();
    base.install_menu_text(&scus);
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
    base.casting.effect_pool = Some(std::sync::Arc::new(pool));

    let mut entered = 0usize;
    let mut finished = 0usize;
    let mut unseated: Vec<(u32, u8)> = Vec::new();
    let mut changed = 0usize;

    for (entry, id, body) in ROWS {
        // The engine's own resolver has to agree that this id pages this
        // module; if it does not, the disc's spell table disagrees with the
        // trampoline map and that is a finding, not a silent skip.
        match base.cast_module_for(id) {
            Some(e) if e == entry => {}
            _ => {
                unseated.push((entry, id));
                continue;
            }
        }
        assert_eq!(
            legaia_engine_vm::cast_module_ticks::capture_tick_body(entry, id),
            Some(body),
            "PROT {entry} id {id:#04X}: the trampoline map names a different body"
        );

        let mut w = seeded(&base, id);
        let before = fingerprint(&w);
        let mut saw_port = false;
        let mut saw_done = false;
        for _ in 0..MAX_FRAMES {
            let Some(run) = w.run_cast_module_code(id, 0) else {
                break;
            };
            assert_eq!(run.prot_entry, entry, "id {id:#04X} paged another module");
            if run.tick_ported {
                saw_port = true;
                if !run.busy {
                    saw_done = true;
                    break;
                }
            } else {
                break;
            }
        }
        assert!(
            saw_port,
            "PROT {entry} id {id:#04X} (body {body:#010X}) never entered a ported body"
        );
        entered += 1;
        if saw_done {
            finished += 1;
        }
        if fingerprint(&w) != before {
            changed += 1;
        }
    }

    assert!(
        unseated.is_empty(),
        "the disc spell table seated no module for {unseated:?} - the trampoline map and \
         `cast_module_for` disagree"
    );
    assert_eq!(
        entered,
        ROWS.len(),
        "every one of the {} arms must enter its body",
        ROWS.len()
    );
    assert_eq!(
        finished,
        ROWS.len(),
        "every body must reach its terminal arm within {MAX_FRAMES} frames"
    );
    assert_eq!(
        changed,
        ROWS.len(),
        "every body must leave an observable state change (non-vacuity)"
    );
    eprintln!(
        "[ok] trampoline-arm ladder: {entered}/{} arms entered their body, \
         {finished} reached a terminal step, {changed} changed observable state",
        ROWS.len()
    );
}

/// The band's other ladder must stay green with the fourteen new arms wired:
/// a body that now reports `Done` where the dispatch used to return `None`
/// changes what `run_cast_module_code` says for those ids.
#[test]
fn the_new_arms_do_not_hold_the_band_forever() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let mut base = battle_world();
    base.install_menu_text(&scus);
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
    base.casting.effect_pool = Some(std::sync::Arc::new(pool));

    for (entry, id, _) in ROWS {
        if base.cast_module_for(id) != Some(entry) {
            continue;
        }
        let mut w = seeded(&base, id);
        // Seed a phase past every arm map in the set; retail reports Busy
        // there, the port reports Done so a stray byte cannot hold battle
        // phase `0x70`.
        w.casting.module_phase = 0xFE;
        let run = w.run_cast_module_code(id, 0).expect("module resident");
        assert!(
            !run.tick_ported || !run.busy,
            "PROT {entry} id {id:#04X} holds on an out-of-map phase"
        );
    }
    eprintln!("[ok] no arm holds the band on an out-of-map phase");
}
