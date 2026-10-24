//! The slot-B modules' rumble stores reach the motor words through the
//! module runner, on the arm and edge the table names
//! (`legaia_engine_cast::cast_module_rumble`).

use super::*;
use legaia_engine_cast::cast_module_rumble::{MODULE_RUMBLE, ModuleRumble, RumbleStore};

/// One tick a store changed the rumble state on.
#[derive(Debug, Clone, Copy)]
struct Store {
    entry: u32,
    arm: u8,
    pulse: i32,
    level: i32,
    pulse_rate: i32,
    level_rate: i32,
}

fn snapshot(w: &World) -> (i32, i32, i32, i32) {
    (
        w.rumble.motors.pulse,
        w.rumble.motors.level,
        w.rumble.battle_decay.pulse_rate,
        w.rumble.battle_decay.level_rate,
    )
}

/// Run one module to its end, from a seeded rumble state that no table row
/// stores (so a store shows as a change), and log every store.
fn drive(world: &mut World, entry: u32, mut tick: impl FnMut(&mut World) -> bool) -> Vec<Store> {
    world.rumble.motors.pulse = 0x33;
    world.rumble.motors.level = 0x33;
    world.rumble.battle_decay.pulse_rate = 0x77;
    world.rumble.battle_decay.level_rate = 0x77;
    let mut out = Vec::new();
    for _ in 0..20_000 {
        let before = snapshot(world);
        let arm = world.casting.module_phase;
        let busy = tick(world);
        let after = snapshot(world);
        if after != before {
            out.push(Store {
                entry,
                arm,
                pulse: after.0,
                level: after.1,
                pulse_rate: after.2,
                level_rate: after.3,
            });
            // Re-seed, so the next store of the same values shows too.
            world.rumble.motors.pulse = 0x33;
            world.rumble.motors.level = 0x33;
            world.rumble.battle_decay.pulse_rate = 0x77;
            world.rumble.battle_decay.level_rate = 0x77;
        }
        if !busy {
            break;
        }
    }
    out
}

fn summon_world() -> World {
    let mut world = module_code_world();
    world.party.party_count = 3;
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.active_target = 3;
    for i in 0..8 {
        world.actors[i].battle.hp = 9000;
        world.actors[i].battle.max_hp = 9000;
    }
    world
}

fn capture_world(entry: u32, id: u8) -> Option<World> {
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    let mut world = module_code_world();
    world.party.party_count = 3;
    world.battle_ctx.active_actor = 3;
    world.actors[3].battle_monster_id = Some(180);
    world.actors[3].battle.active_target = 0;
    for i in 0..8 {
        world.actors[i].battle.hp = 9000;
        world.actors[i].battle.max_hp = 9000;
    }
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[id as usize].class = CAPTURE_CLASS;
    entries[id as usize].sub_class = (entry - 935) as u8;
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    (world.cast_module_for(id) == Some(entry)).then_some(world)
}

/// Every store each driven module made, with the Battles row on or off.
fn all_stores(battles_off: bool) -> Vec<Store> {
    use legaia_engine_cast::cast_module_ticks::capture_trampoline_for;
    let mut stores = Vec::new();
    for id in 0x81u8..=0xA0 {
        let mut world = summon_world();
        world.toggles.retail_options.vibration_battles = u8::from(battles_off);
        let Some(entry) = world.cast_module_for(id) else {
            continue;
        };
        world.arm_summon_stager(0, id);
        stores.extend(drive(&mut world, entry, |w| w.summon_stager_tick()));
    }
    for entry in 935u32..=966 {
        let ids: Vec<u8> = match capture_trampoline_for(entry) {
            Some(t) => t.arms.iter().map(|(id, _)| *id).collect(),
            None => vec![0xF0],
        };
        for id in ids {
            let Some(mut world) = capture_world(entry, id) else {
                continue;
            };
            world.toggles.retail_options.vibration_battles = u8::from(battles_off);
            world.arm_capture_cast_module(id);
            stores.extend(drive(&mut world, entry, |w| {
                let phase = w.casting.module_phase;
                w.run_cast_module_code(id, phase).is_some_and(|r| r.busy)
            }));
        }
    }
    stores
}

fn matches(row: &ModuleRumble, s: &Store) -> bool {
    if (row.entry, row.arm) != (s.entry, s.arm) {
        return false;
    }
    match row.store {
        RumbleStore::Write(w) => {
            (w.pulse, w.level, w.pulse_rate, w.level_rate)
                == (s.pulse, s.level, s.pulse_rate, s.level_rate)
        }
        // The motor words keep the seed; only the rates move.
        RumbleStore::Rates {
            pulse_rate,
            level_rate,
        } => (s.pulse, s.level, s.pulse_rate, s.level_rate) == (0x33, 0x33, pulse_rate, level_rate),
        RumbleStore::OzmaSpray => {
            (0xA1..=0xE0).contains(&s.pulse)
                && (0x81..=0xC0).contains(&s.level)
                && (s.pulse_rate, s.level_rate) == (8, 8)
        }
        RumbleStore::PueraAlternate { odd, even } => {
            (s.pulse == odd || s.pulse == even)
                && s.level == s.pulse
                && (s.pulse_rate, s.level_rate) == (4, 4)
        }
    }
}

/// The shared finisher's own stores (`FUN_801DDB30`), which a module's
/// damage-wrapper hit reaches.
fn finisher_store(s: &Store) -> bool {
    use legaia_engine_system::rumble::battle_writes as bw;
    [bw::FINISHER_PARTY_STRUCK, bw::FINISHER_MONSTER_STRUCK]
        .iter()
        .any(|w| {
            (w.pulse, w.level, w.pulse_rate, w.level_rate)
                == (s.pulse, s.level, s.pulse_rate, s.level_rate)
        })
}

/// Every table row the engine drives stores on its own arm, and nothing
/// else in the band stores but the finisher's hits.
#[test]
fn every_module_rumble_store_lands_on_its_arm() {
    let stores = all_stores(false);
    let unreached: Vec<(u32, u8)> = MODULE_RUMBLE
        .iter()
        .filter(|row| !stores.iter().any(|s| matches(row, s)))
        .map(|row| (row.entry, row.arm))
        .collect();
    assert!(
        unreached.is_empty(),
        "rows no driven tick stored: {unreached:?}"
    );
    let stray: Vec<&Store> = stores
        .iter()
        .filter(|s| !finisher_store(s) && !MODULE_RUMBLE.iter().any(|row| matches(row, s)))
        .collect();
    assert!(stray.is_empty(), "stores no row names: {stray:x?}");
}

/// A pass-edge store fires once per cast, on the arm's pass: Kemaro's arm 3.
#[test]
fn a_pass_store_fires_once() {
    let stores = all_stores(false);
    let kemaro = stores.iter().filter(|s| s.entry == 918).count();
    assert_eq!(kemaro, 1, "{stores:x?}");
}

/// The Battles row off: no module touches the rumble state.
#[test]
fn battles_off_silences_every_module() {
    let stores = all_stores(true);
    assert!(stores.is_empty(), "{stores:x?}");
}
