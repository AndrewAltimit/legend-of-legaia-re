//! The Use list's applicability probe `FUN_8003043C`, run with the action
//! validator `FUN_8003FB10` live beneath it, against the dim bit the port's
//! Use list (`WorldBagRows::bag_use_rows`) puts on the same item, over every
//! field-usable item id and party states that sit on each arm's edges.

use crate::common::{Rng, scus, scus_cpu};
use legaia_engine_battle_vm::status_effects::StatusKind;
use legaia_engine_core::world::World;
use legaia_engine_menu_runtime::bag_rows::WorldBagRows;
use legaia_engine_menu_runtime::field_menu_dispatch::build_pause_items_session;
use legaia_engine_menu_runtime::menu_validator::spell_affects_anyone;
use legaia_engine_menus::items::ItemCatalog;
use legaia_game_tables::item_effect::ItemEffectTable;
use legaia_retail_exec::{Cpu, Stop, V0};
use legaia_save::{CharacterRecord, Party};

const PROBE: u32 = 0x8003_043C;
const MODE: u32 = 0x8007_B83C;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const PARTY_COUNT: u32 = 0x8008_4594;
const PARTY_SLOTS: u32 = 0x8008_4598;
const ENGINE_FLAGS: u32 = 0x1F80_0394;
const SYSTEM_FLAGS: u32 = 0x8008_5758;
const GATE_WORD: u32 = 0x8007_B600;
/// `Cpu::call`'s stack top.
const STACK_TOP: u32 = 0x801F_FF00;
/// A zero halfword the stale stack words point at.
const ZERO_WORD: u32 = 0x8014_0000;

/// The ailments a record carries out of battle.
const KINDS: [StatusKind; 5] = [
    StatusKind::Venom,
    StatusKind::Toxic,
    StatusKind::Stone,
    StatusKind::Curse,
    StatusKind::Rot,
];

fn edge(rng: &mut Rng, max: u16) -> u16 {
    match rng.below(5) {
        0 => 0,
        1 => max,
        2 => max.saturating_sub(1),
        _ => rng.below(u32::from(max) + 1) as u16,
    }
}

fn stat(rng: &mut Rng, cap: u16) -> u16 {
    match rng.below(4) {
        0 => cap,
        1 => cap - 1,
        _ => rng.below(u32::from(cap) + 1) as u16,
    }
}

/// Seed the four records, the present list, the ailments, the Doors' gates
/// and flags and the Incense window on both sides; returns the list.
fn seed_party(cpu: &mut Cpu, w: &mut World, rng: &mut Rng) -> Vec<u8> {
    let mut party = Party::zeroed(4);
    cpu.mem.wr32(MODE, 0);
    for (s, rec) in party.members.iter_mut().enumerate() {
        let r = RECORDS + s as u32 * STRIDE;
        let mut raw = vec![0u8; STRIDE as usize];
        let hp_max = 1 + rng.below(9999) as u16;
        let mp_max = rng.below(1000) as u16;
        let put = |raw: &mut Vec<u8>, off: usize, v: u16| {
            raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        put(&mut raw, 0x104, hp_max);
        put(&mut raw, 0x106, edge(rng, hp_max));
        put(&mut raw, 0x108, mp_max);
        put(&mut raw, 0x10A, edge(rng, mp_max));
        for (off, cap) in [
            (0x110, 280),
            (0x112, 999),
            (0x114, 999),
            (0x116, 999),
            (0x118, 999),
            (0x11A, 999),
        ] {
            put(&mut raw, off, stat(rng, cap));
        }
        *rec = CharacterRecord::parse(&raw).expect("record");
        for (b, v) in raw.iter().enumerate() {
            cpu.mem.wr8(r + b as u32, *v);
        }
    }
    // The present list: up to three of the four records, any order.
    let mut order = [0u8, 1, 2, 3];
    for i in (1..4).rev() {
        order.swap(i, rng.below(i as u32 + 1) as usize);
    }
    let n = 1 + rng.below(3) as usize;
    let list = order[..n].to_vec();
    w.load_party(party);
    w.install_present_party_list(list.clone());
    w.battle.status_effects = Default::default();
    for s in 0..4u8 {
        if rng.below(3) == 0 {
            w.battle
                .status_effects
                .apply(s, KINDS[rng.below(KINDS.len() as u32) as usize]);
        }
        let word = w.battle.status_effects.display_flags(s);
        cpu.mem.wr16(RECORDS + u32::from(s) * STRIDE + 0x12E, word);
    }
    cpu.mem.wr8(PARTY_COUNT, n as u8);
    for (i, s) in list.iter().enumerate() {
        cpu.mem.wr8(PARTY_SLOTS + i as u32, *s);
    }
    // The Doors' scene gates and flags, and the Incense window.
    let (mut light, mut wind) = (rng.below(4) == 0, rng.below(4) == 0);
    if rng.below(2) == 0 {
        w.encounters.region_setup =
            Some(legaia_engine_battle::region_encounter::RegionBattleSetup {
                stage_variant: 0,
                keep_backdrop_object_1: None,
                door_of_light_blocked: light,
                door_of_wind_blocked: wind,
                world_map_return: None,
            });
    } else {
        (light, wind) = (false, false);
    }
    let engine = (u32::from(light) << 20) | (u32::from(wind) << 21);
    let (f5, f6) = (rng.below(4) == 0, rng.below(4) == 0);
    if f5 {
        w.system_flag_set(5);
    }
    if f6 {
        w.system_flag_set(6);
    }
    let window = [0, 0x40, 0xDF, 0xE0, 0x100][rng.below(5) as usize];
    w.locomotion.walk_regen_window = window;
    cpu.mem.wr32(ENGINE_FLAGS, engine);
    cpu.mem
        .wr8(SYSTEM_FLAGS, (u8::from(f5) << 2) | (u8::from(f6) << 1));
    cpu.mem.wr32(GATE_WORD, window as u32);
    // Out of battle the validator fills its four pointer arrays for slots
    // `0..3` only (`sltiu v0,v0,0x3` at `0x8003FC38`); arm `0x01`'s walk
    // reads a present id of `3` through the array's fourth word, which is
    // whatever the stack held. Hold that to a pointer at zero, the dead
    // quad the port answers for a slot past the three.
    cpu.mem.wr32(ZERO_WORD, 0);
    for a in (STACK_TOP - 0x400..STACK_TOP).step_by(4) {
        cpu.mem.wr32(a, ZERO_WORD);
    }
    list
}

#[test]
fn the_use_list_probe_answers_what_retail_answers() {
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = ItemEffectTable::from_scus(&scus).expect("item effects");
    let ids: Vec<u8> = (1..=255u8)
        // The ids the field Use list asks the probe about: not equipment, not
        // the tail group, field-usable (`FUN_80030628`, `0x80030828..`).
        .filter(|&id| {
            let kind = table.kind(id);
            table.effect(id).is_some_and(|e| {
                e.field_usable() && kind != 1 && !(kind == 2 && e.flags & 0x08 != 0)
            })
        })
        .collect();
    assert!(ids.len() > 20, "{} field-usable ids", ids.len());
    let mut rng = Rng(0x5052_4F42);
    let mut failures = Vec::new();
    let (mut lit, mut dim, mut screened) = (0, 0, 0);
    // Both answers seen per validator arm, so each arm is held, not skipped.
    let mut arms = std::collections::BTreeMap::<u8, [bool; 2]>::new();
    for round in 0..40000u32 {
        let id = ids[rng.below(ids.len() as u32) as usize];
        let mut w = World::new();
        w.set_item_effects(table.clone());
        w.set_item_catalog(ItemCatalog::vanilla());
        let list = seed_party(&mut cpu, &mut w, &mut rng);
        cpu.regs.r[V0] = 0;
        let stop = cpu.call(PROBE, &[u32::from(id)]);
        assert_eq!(stop, Stop::Returned, "item {id:#x}");
        let retail = cpu.regs.r[V0] != 0;

        w.party.inventory.clear();
        w.party.inventory.add(id, 1);
        let rows = w.bag_use_rows(false).expect("rows");
        let row = rows.iter().find(|r| r.id == id).expect("the item's row");
        let mut port = !row.dim;
        // The Items screen's confirm gate: a row the probe greys cannot be
        // used, and one it lights can, wherever the catalog offers the item.
        let screen = build_pause_items_session(&w);
        if w.tables
            .item_catalog
            .get(id)
            .is_some_and(|e| e.usable_in_field)
        {
            screened += 1;
            let usable = !screen.inner.filtered_items.is_empty();
            if usable != retail {
                port = usable;
            }
        }
        let class = table.effect(id).map_or(0, |e| e.class);
        arms.entry(class).or_default()[usize::from(retail)] = true;
        if retail {
            lit += 1;
        } else {
            dim += 1;
        }
        if retail != port && failures.len() < 12 {
            let recs: Vec<String> = (0..4)
                .map(|s| {
                    let m = &w.party.roster.members[s];
                    let h = m.hp_mp_sp();
                    format!(
                        "[{} hp {}/{} mp {}/{} st {:#x}]",
                        s,
                        h.hp_cur,
                        h.hp_max,
                        h.mp_cur,
                        h.mp_max,
                        w.battle.status_effects.display_flags(s as u8)
                    )
                })
                .collect();
            failures.push(format!(
                "round {round} item {id:#04x} effect {:?}: retail {retail} port {port} list {list:?} {}",
                table.effect(id),
                recs.join(" ")
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(lit > 4000 && dim > 4000, "lit {lit} dim {dim}");
    assert!(screened > 4000, "{screened} through the Items screen");
    // Every class a field-usable item carries: heal, party heal, MP, stat
    // seeds, the three one-member classes, both Doors, Incense.
    for class in [0x00, 0x01, 0x02, 0x06, 0x0B, 0x0C, 0x0D, 0x80, 0x81, 0x82] {
        assert_eq!(
            arms.get(&class),
            Some(&[true, true]),
            "arm {class:#x}: {arms:?}"
        );
    }
}

/// The probe's twin over the spell table, `FUN_8003053C`, which greys a Magic
/// row and refuses its cast, against `menu_validator::spell_affects_anyone`.
#[test]
fn the_magic_list_probe_answers_what_retail_answers() {
    const SPELL_PROBE: u32 = 0x8003_053C;
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5350_454C);
    let mut probe = World::new();
    probe.install_menu_text(&scus);
    let ids: Vec<u8> = (1..=255u8)
        .filter(|&id| spell_affects_anyone(&probe, id).is_some())
        .collect();
    assert!(ids.len() > 20, "{} spells", ids.len());
    let (mut failures, mut lit, mut dim) = (Vec::new(), 0, 0);
    for round in 0..20000u32 {
        let id = ids[rng.below(ids.len() as u32) as usize];
        let mut w = World::new();
        w.install_menu_text(&scus);
        let list = seed_party(&mut cpu, &mut w, &mut rng);
        cpu.regs.r[V0] = 0;
        assert_eq!(cpu.call(SPELL_PROBE, &[u32::from(id)]), Stop::Returned);
        let retail = cpu.regs.r[V0] != 0;
        let port = spell_affects_anyone(&w, id).expect("spell entry");
        if retail {
            lit += 1;
        } else {
            dim += 1;
        }
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} spell {id:#04x}: retail {retail} port {port} list {list:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(lit > 1000 && dim > 1000, "lit {lit} dim {dim}");
}
