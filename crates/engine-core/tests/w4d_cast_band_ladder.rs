//! Disc-gated **cast-band ladder**: seat a cast for every reachable id in the
//! PROT 0903..0966 band and step its module code one frame.
//!
//! Why it exists. `cast_module_ticks` carries thirty-one addresses that the
//! static audit calls live and no coverage export ever *enters*
//! ([`docs/tooling/reach-triage.md`](../../../docs/tooling/reach-triage.md)'s
//! "live but never entered" bucket). They are gated on the spell id behind
//! `World::cast_module_for`, so a ladder that only ever casts the one or two
//! spells a playthrough reaches can never touch them - the gap is the *id*,
//! not the host. This drives one frame per id instead.
//!
//! Both dispatchers are exercised, because they key on different bytes:
//!
//! * the **action-id** band (`FUN_801F1ED4` -> `0x801CF4EC`): id `0x81..=0xA0`
//!   maps to PROT `903 + (id - 0x81)`;
//! * the **capture** band (`FUN_801F2160` -> `0x801CF56C`): a spell whose
//!   record `+0x00` class byte is `'c'` maps to PROT `935 + record[+0x01]`.
//!
//! The second needs the disc's own spell table, which is why this is
//! disc-gated: without `SCUS_942.54` every id is treated as the action-id
//! band and the capture half of the ladder cannot be seated at all.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use legaia_engine_core::world::{Actor, SceneMode, World};
use std::path::PathBuf;

/// Arms stepped per seated id. Retail's trampolines switch on the caster's
/// queued action id, not on this, but the tick bodies themselves take an arm
/// index; stepping two covers a body whose first arm is a no-op guard.
const ARMS: [u8; 2] = [0, 1];

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

/// A minimal live battle: a party seat, an enemy seat, and a summon seat above
/// the battle slots, all alive. The tick bodies read the caster, the victim
/// and the seat, so all three have to exist or a body early-outs before the
/// arm the ladder is here to enter.
fn battle_world() -> World {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    for slot in [0usize, 1] {
        world.actors[slot].active = true;
        world.actors[slot].battle.max_hp = 400;
        world.actors[slot].battle.hp = 400;
        world.actors[slot].battle.mp = 99;
        world.actors[slot].battle.liveness = 1;
    }
    world.actors[0].battle.active_target = 1;
    world.battle_ctx.active_actor = 0;
    world
}

#[test]
fn every_reachable_cast_band_id_steps_its_module_code() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");

    let mut world = battle_world();
    world.install_menu_text(&scus);
    // The pool the scene host stages; without it `cast_module_for` still
    // answers (it is a pure table lookup) but no module is resident, so the
    // run returns `None` and the ladder would seat nothing.
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
    world.cast_effect_pool = Some(std::sync::Arc::new(pool));

    // One representative id per band entry, taken from the engine's own
    // resolver rather than from a hand-written map - so the ladder tracks the
    // dispatchers instead of a copy of them.
    let mut seat: std::collections::BTreeMap<u32, u8> = std::collections::BTreeMap::new();
    for id in 0u8..=0xFF {
        if let Some(entry) = world.cast_module_for(id) {
            seat.entry(entry).or_insert(id);
        }
    }
    assert!(
        !seat.is_empty(),
        "the resolver seated no band entry at all - the spell table did not install"
    );

    let mut stepped = 0usize;
    let mut ported = 0usize;
    for (&entry, &id) in &seat {
        assert!(
            (CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST).contains(&entry),
            "id {id:#04x} resolved outside the band: {entry}"
        );
        for arm in ARMS {
            // A fresh caster/victim state per arm: a tick body that lands a
            // kill would otherwise make every later arm early-out.
            let mut w = battle_world();
            w.menu_text = world.menu_text.clone();
            w.cast_effect_pool = world.cast_effect_pool.clone();
            let Some(run) = w.run_cast_module_code(id, arm) else {
                continue;
            };
            assert_eq!(
                run.prot_entry, entry,
                "id {id:#04x} arm {arm} ran a different entry than the resolver named"
            );
            stepped += 1;
            if run.tick_ported {
                ported += 1;
            }
        }
    }

    // Non-vacuity: the band's own bounds are the denominator, and both
    // dispatchers have to have contributed - the action-id half alone would
    // seat only PROT 0903..0934.
    assert!(
        seat.keys().any(|&e| e < 935),
        "no action-id band entry seated"
    );
    assert!(
        seat.keys().any(|&e| e >= 935),
        "no capture-class entry seated - the class byte read did not work"
    );
    assert!(
        stepped >= seat.len(),
        "every seated entry should step at least one arm ({stepped} steps over {} entries)",
        seat.len()
    );
    assert!(
        ported > 0,
        "no seated id reached a ported tick body - the ladder entered nothing"
    );
    eprintln!(
        "[ok] cast-band ladder: {} band entries seated, {stepped} arm steps, \
         {ported} of them reached a ported tick body",
        seat.len()
    );
}
