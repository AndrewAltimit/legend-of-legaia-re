//! Reach conversion: the two **capture-class damage wrappers** as the live
//! battle path reaches them - `FUN_801DD6B4` (resist-bypass) and
//! `FUN_801DD4B0` (guard-respecting), wired at
//! `world/battle/casting.rs`'s `capture_bypass_predamage` /
//! `capture_respect_predamage`.
//!
//! The reach-triage GATED row names the gate as "a capture-class boss cast",
//! and says the gate already has a seeded oracle
//! (`world/battle/tests/battle_capture_class_disc.rs`) that no union member
//! can ever be: it is a `#[cfg(test)]` module inside the crate, and
//! `CANONICAL_LADDERS` takes `--test <name>` integration binaries. This is the
//! integration-binary twin of that gate.
//!
//! It is also a different *route*. The in-crate oracle calls
//! `World::cast_spell_on_slots` directly; every seam between a host and the
//! fold - `arm_monster_cast`, `fold_pending_cast`,
//! `cast_spell_on_slots_prepaid`, `enemy_move_predamage` - is
//! `pub(in crate::world)`, so the only public way in is `World::tick`. This
//! ladder therefore drives the ordinary live battle loop and lets the monster
//! AI pick the cast, which is what makes it a reach measurement rather than a
//! kernel test.
//!
//! ## The gate, and what is seeded
//!
//! The routing key is disc data: the spell table's `+0x00` class byte `'c'`
//! (`legaia_asset::spell_names::SpellEntry::is_capture_class`). A monster
//! reaches that path only when its own record's magic list holds such an id,
//! so the seeded state is one monster record's move list - the L3 shape, one
//! write, with the content (the ids, their class bytes, their baked powers)
//! taken from the disc rather than invented.
//!
//! ## Non-vacuity
//!
//! "The party lost HP" is not enough - a shared-kernel cast does that too. The
//! contrast is the routing itself: the same fight, same seed, same move, run
//! once with the disc spell table installed and once without it. Without the
//! table `is_capture_class_move` answers `false` for every id and the hit
//! takes the shared kernel `FUN_801DD0AC`, so a difference in the damage is
//! the wrapper branch and nothing else.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_core::monster_catalog::{
    MonsterCatalog, MonsterDef, catalog_from_monster_archive, vanilla_formation_table,
};
use legaia_engine_core::move_power::MovePowerCatalog;
use legaia_engine_core::spells::{SpellCatalog, SpellDef, SpellEffect, SpellElement, SpellTarget};
use legaia_engine_core::world::{Actor, SceneMode, World};
use std::path::PathBuf;

/// PROT entry holding the monster archive - the records whose magic lists are
/// the gate.
const MONSTER_ARCHIVE_PROT: usize = 867;

const PARTY: u8 = 3;
const MONSTERS: u8 = 1;
/// The monster seat: the first row above the party.
const CASTER: u8 = PARTY;

/// Frames to drive before giving up on the monster taking its turn.
const MAX_FRAMES: usize = 1200;

/// Fixed RNG seed, so both halves of the contrast draw the same stream.
const SEED: u32 = 0x00C0_FFEE;

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

fn prot_entry(dir: &std::path::Path, index: usize) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(index)
        .cloned()
        .unwrap_or_else(|| panic!("PROT {index} entry"));
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .unwrap_or_else(|_| panic!("read PROT {index}"));
    bytes
}

/// Every spell id whose disc record carries the capture class byte.
fn capture_class_ids(scus: &[u8]) -> Vec<u8> {
    let table = legaia_asset::spell_names::SpellNameTable::from_scus(scus)
        .expect("the SCUS spell table parses");
    (0u8..=0xFF)
        .filter(|id| table.entry(*id).is_some_and(|e| e.is_capture_class()))
        .collect()
}

/// A `SpellDef` the monster AI can pick: affordable, single-target, damaging.
///
/// The magnitude here is the *placeholder* the fold replaces - a capture-class
/// hit takes its power from the module's baked constant
/// (`World::baked_module_power`) or, failing that, from the move-power table,
/// never from this record.
fn castable(id: u8) -> SpellDef {
    SpellDef {
        id,
        name: format!("capture {id:#04X}"),
        mp_cost: 0,
        target: SpellTarget::OneEnemy,
        element: SpellElement::Neutral,
        effect: SpellEffect::Damage {
            base_power: 50,
            element: SpellElement::Neutral,
        },
        ..Default::default()
    }
}

/// The battle the contrast runs in. `install_spell_table` is the one axis the
/// two halves differ on.
fn battle_world(
    scus: &[u8],
    overlay: &[u8],
    catalog: &MonsterCatalog,
    monster_id: u16,
    ids: &[u8],
    install_spell_table: bool,
) -> World {
    let mut w = World::new();
    while w.actors.len() < (PARTY + MONSTERS) as usize {
        w.actors.push(Actor::default());
    }
    w.party.party_count = PARTY;
    w.load_party(legaia_save::Party::zeroed(PARTY as usize));
    w.set_formation_table(vanilla_formation_table(), catalog.clone());
    w.enter_battle(PARTY, MONSTERS);
    if install_spell_table {
        w.install_menu_text(scus);
    }
    w.tables.move_power =
        Some(MovePowerCatalog::from_overlay_0898(overlay).expect("the move-power table parses"));
    w.tables.monster_catalog = catalog.clone();
    let mut spells = SpellCatalog::new();
    for id in ids {
        spells.insert(castable(*id));
    }
    w.tables.spell_catalog = spells;
    for i in 0..(PARTY + MONSTERS) as usize {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 4000;
        w.actors[i].battle.max_hp = 4000;
        w.actors[i].battle.mp = 200;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.actors[CASTER as usize].battle_monster_id = Some(monster_id);
    w.mode = SceneMode::Battle;
    w.toggles.live_gameplay_loop = true;
    // The caster must not leave before it casts: the picker's once-per-pass
    // flee checkpoint (`FUN_801EC0DC`) is a live roll on the battle stream,
    // and the scripted no-escape flag (`ctx+0x287`) is the gate retail tests
    // first.
    w.battle.no_escape = true;
    w.rng_state = SEED;
    w
}

/// Drive the fight until the monster's cast folds. Returns
/// `(frame, move id, damage the party seat took)`.
fn first_monster_cast_hit(w: &mut World) -> Option<(usize, u8, u16)> {
    let mut armed: Option<u8> = None;
    let mut hp: Vec<u16> = (0..PARTY as usize).map(|i| w.actors[i].battle.hp).collect();
    for frame in 0..MAX_FRAMES {
        w.tick();
        if w.mode != SceneMode::Battle {
            return None;
        }
        let a = &w.actors[CASTER as usize];
        // Category 2 is the Magic band the cast arm installs.
        if a.battle.action_category == 2 {
            armed = Some(a.battle.params[0]);
        }
        for (i, was) in hp.iter_mut().enumerate() {
            let now = w.actors[i].battle.hp;
            if now < *was {
                let taken = *was - now;
                *was = now;
                if let Some(id) = armed {
                    return Some((frame, id, taken));
                }
            }
        }
    }
    None
}

/// The seven move ids whose capture-class cast takes the **resist-bypass**
/// wrapper `FUN_801DD6B4`. Retail's census, mirrored from
/// `world/battle/casting.rs`'s own `CAPTURE_BYPASS_MOVE_IDS` (private to that
/// module); every other capture-class id takes the guard-respecting
/// `FUN_801DD4B0`.
const BYPASS_MOVE_IDS: [u8; 7] = [0x37, 0x5C, 0x5D, 0x5E, 0x79, 0x7A, 0x7B];

/// What a rung needs off the disc, resolved once.
struct Corpus {
    scus: Vec<u8>,
    overlay: Vec<u8>,
    catalog: MonsterCatalog,
    /// Every capture-class id the move-power table also carries a record for.
    usable: Vec<u8>,
}

fn corpus(dir: &std::path::Path) -> Corpus {
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let overlay = prot_entry(
        dir,
        legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX,
    );
    let archive867 = prot_entry(dir, MONSTER_ARCHIVE_PROT);
    let capture_ids = capture_class_ids(&scus);
    assert!(
        !capture_ids.is_empty(),
        "the disc spell table names no capture-class id - the routing key did not parse"
    );
    let power = MovePowerCatalog::from_overlay_0898(&overlay).expect("move-power table");
    // Only an id the move-power table carries a record for reaches a wrapper
    // at all: the `power` argument is that record's, and without one the fold
    // keeps the MP-scaled placeholder and no wrapper runs.
    let usable: Vec<u8> = capture_ids
        .iter()
        .copied()
        .filter(|id| power.record_for_move_id(*id).is_some())
        .collect();
    assert!(
        !usable.is_empty(),
        "no capture-class id has a move-power record - the fold would keep the placeholder"
    );
    let ids: Vec<u16> = (1u16..=0x1FF).collect();
    Corpus {
        scus,
        overlay,
        catalog: catalog_from_monster_archive(&archive867, &ids),
        usable,
    }
}

/// Seat `ids` as the magic list of a real disc monster record and drive the
/// fight twice - once with the disc spell table installed (so the class byte
/// routes the hit to a capture wrapper) and once without it (so the same hit
/// takes the shared kernel `FUN_801DD0AC`). Returns
/// `(monster id, move id, routed damage, shared damage, frame delta)`.
fn drive_contrast(c: &Corpus, ids: &[u8]) -> (u16, u8, u16, u16, usize) {
    let monster_id = c
        .catalog
        .get(1)
        .map(|d| d.id)
        .expect("the disc catalog holds monster 1");
    let mut catalog = c.catalog.clone();
    let mut def: MonsterDef = catalog
        .get(monster_id)
        .cloned()
        .expect("the seeded monster exists");
    // The seeded gate, and the only thing seeded: one record's move list. The
    // ids themselves, their class bytes and their baked powers are the disc's.
    def.magic_attacks = ids.to_vec();
    catalog.insert(def);

    let mut routed = battle_world(&c.scus, &c.overlay, &catalog, monster_id, ids, true);
    let with_table = first_monster_cast_hit(&mut routed);
    let mut shared = battle_world(&c.scus, &c.overlay, &catalog, monster_id, ids, false);
    let without_table = first_monster_cast_hit(&mut shared);

    let (frame_a, id_a, dmg_a) = with_table.expect(
        "the monster never landed a cast in the routed run - the seeded magic list did not reach \
         the AI",
    );
    let (frame_b, id_b, dmg_b) =
        without_table.expect("the monster never landed a cast in the shared-kernel run");
    assert_eq!(
        id_a, id_b,
        "the two halves cast different moves, so a damage difference would not be the routing"
    );
    assert!(
        ids.contains(&id_a),
        "the cast that landed was move {id_a:#04X}, which is not one of the seeded ids"
    );
    assert!(
        routed.cast_module_for(id_a).is_some(),
        "move {id_a:#04X} is capture-class but names no PROT 0935..0966 module, so the baked \
         power the wrapper is handed would not exist"
    );
    assert!(
        shared.cast_module_for(id_a).is_none(),
        "the shared-kernel half still resolved a band module for move {id_a:#04X}, so the two \
         halves are not contrasting the routing"
    );
    assert!(dmg_a > 0 && dmg_b > 0, "neither run landed a hit");
    // The frames are deliberately NOT required to match, and the reason is a
    // property of the byte rather than of the harness: the record's `+0x00`
    // class is read twice - by `is_capture_class_move` (the routing this
    // ladder measures) and by the action-seed band pick
    // (`legaia_engine_vm::battle_action`'s `action_seed`, which compares the
    // same byte against `0x14`). Installing the table moves the whole Magic
    // band as well as the kernel.
    (monster_id, id_a, dmg_a, dmg_b, frame_a.abs_diff(frame_b))
}

/// Rung 1 - the **resist-bypass** wrapper `FUN_801DD6B4`
/// (`capture_bypass_predamage`). Its roll is a different shape from the shared
/// kernel's (one attacker draw against two, a flat spell-power term instead of
/// the AGL pair, and defence terms weighted `>> 1` instead of `>> 4`), so the
/// contrast separates them by the number they produce.
#[test]
fn a_bypass_class_boss_cast_rolls_the_resist_bypass_wrapper() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let c = corpus(&dir);
    let ids: Vec<u8> = c
        .usable
        .iter()
        .copied()
        .filter(|id| BYPASS_MOVE_IDS.contains(id))
        .collect();
    assert!(
        !ids.is_empty(),
        "none of the seven bypass ids is capture-class with a move-power record - the census and \
         the disc disagree"
    );
    let (monster, move_id, routed, shared, delta) = drive_contrast(&c, &ids);
    assert_ne!(
        routed, shared,
        "move {move_id:#04X} rolled the same number through the bypass wrapper as through the \
         shared kernel - the wrapper branch did not fire"
    );
    eprintln!(
        "[ok] bypass-wrapper rung: monster {monster} cast move {move_id:#04X}; wrapper {routed} \
         HP against shared kernel {shared} HP ({delta} frames apart) over {} bypass ids",
        ids.len()
    );
}

/// Rung 2 - the **guard-respecting** wrapper `FUN_801DD4B0`
/// (`capture_respect_predamage`), the majority arm.
///
/// This rung deliberately does **not** assert that the two numbers differ, and
/// the reason is a measurement rather than a caveat: on the same stat bridge
/// and the same draw stream the respect wrapper and the shared kernel produce
/// the *same* damage. Both roll `rand % ((power>>2)+1) + rand % ((agl>>1)+1) +
/// (hp>>8) + power + agl*2` against `rand % ((agl>>1)+1) + (hp>>8) +
/// (stat_a>>4) + (stat_b>>4) + agl*2`, which is what
/// `battle_damage_wrappers`' own doc means by "identical arithmetic to the
/// shared kernel's defender roll". So a damage contrast cannot see this arm at
/// all, and what the rung asserts instead is the routing itself - the band
/// module resolves on the routed half and does not on the other - plus the hit
/// landing.
#[test]
fn a_respect_class_boss_cast_reaches_the_fold_through_its_own_wrapper() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let c = corpus(&dir);
    let ids: Vec<u8> = c
        .usable
        .iter()
        .copied()
        .filter(|id| !BYPASS_MOVE_IDS.contains(id))
        .collect();
    assert!(
        !ids.is_empty(),
        "every capture-class id with a move-power record is a bypass id - the respect arm has no \
         carrier"
    );
    let (monster, move_id, routed, shared, delta) = drive_contrast(&c, &ids);
    eprintln!(
        "[ok] respect-wrapper rung: monster {monster} cast move {move_id:#04X}; wrapper {routed} \
         HP against shared kernel {shared} HP ({delta} frames apart) over {} respect ids",
        ids.len()
    );
}
