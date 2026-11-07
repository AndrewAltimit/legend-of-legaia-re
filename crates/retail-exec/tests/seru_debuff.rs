//! The Seru side-effect stager `FUN_801F3D3C` against
//! `World::stage_seru_side_effect`, in random encounters and scripted fights,
//! over single, group and party targets, then the finisher's stat switch.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_core::world::World;
use legaia_game_tables::element_affinity;
use legaia_game_tables::seru_side_effect::SeruSideEffectTable;
use legaia_save::Party;
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const STAGER: u32 = 0x801F_3D3C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const RECORDS: u32 = 0x801C_9348;
const SUMMON_RECORD: u32 = 0x801C_9358;
const SEATS: u32 = 0x8007_BD10;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const ACTOR: u32 = 0x8013_0000;
const MONSTER: u32 = 0x8015_0000;
const STAGED: u32 = 0x801F_6960;
const SENTINEL: u32 = 0xAAAA;

/// (actor offset, record offset) of the compared halfword, in `raw_stats` order.
const COMPARED: [(u32, u32); 6] = [
    (0x156, 0x0E),
    (0x15A, 0x12),
    (0x15E, 0x14),
    (0x162, 0x16),
    (0x16A, 0x18),
    (0x166, 0x1A),
];

#[test]
fn a_stager_call_stages_what_retail_stages() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(898).expect("PROT 0898");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(0x8005_6798, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    cpu.stub(0x801D_8DE8, 0);
    let mut w = World::new();
    w.tables.element_affinity = element_affinity::parse(&overlay);
    w.tables.seru_side_effects = SeruSideEffectTable::parse(&overlay);
    assert!(w.tables.seru_side_effects.is_some());

    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8u32 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    for m in 0..3u32 {
        cpu.mem.wr8(SEATS + m, m as u8 + 1);
    }
    for m in 0..5u32 {
        cpu.mem.wr32(RECORDS + m * 4, MONSTER + m * 0x80);
    }

    let mut rng = Rng(0x5345_5255);
    let (mut staged, mut suppressed, mut moved) = (0, 0, 0);
    for round in 0..40000 {
        let caster = rng.below(3) as u8;
        let spell = 0x81 + rng.below(0x0B) as u8;
        let level = 1 + rng.below(9) as u8;
        let summon_el = rng.below(7) as u8;
        let scripted = rng.below(2) == 0;
        let monsters = 1 + rng.below(4) as u8;
        let els: [u8; 4] = std::array::from_fn(|_| rng.below(8) as u8);
        let alive: [bool; 4] = std::array::from_fn(|i| i as u8 >= monsters || rng.below(3) != 0);
        let class = match rng.below(4) {
            0 => rng.below(3) as u8,
            1 => 8,
            2 => 9,
            _ => 3 + rng.below(u32::from(monsters)) as u8,
        };
        let seed = rng.next();
        // Each compared halfword: as the record has it, or moved off.
        let raw: [[u16; 6]; 4] =
            std::array::from_fn(|_| std::array::from_fn(|_| 1 + rng.below(4000) as u16));
        let base: [[u16; 6]; 4] = std::array::from_fn(|m| {
            std::array::from_fn(|k| {
                if rng.below(4) == 0 {
                    raw[m][k] / 2
                } else {
                    raw[m][k]
                }
            })
        });

        // Retail.
        cpu.mem.wr8(CTX + 0x13, caster);
        cpu.mem.wr8(CTX + 0x287, u8::from(scripted));
        let me = ACTOR + u32::from(caster) * 0x400;
        cpu.mem.wr8(me + 0x1DF, spell);
        cpu.mem.wr8(me + 0x1DD, class);
        let rec = CHAR_RECORDS + u32::from(caster) * CHAR_STRIDE;
        for i in 0..0x20 {
            cpu.mem.wr8(rec + 0x13D + i, 0);
            cpu.mem.wr8(rec + 0x161 + i, 0);
        }
        cpu.mem.wr8(rec + 0x13D, spell);
        cpu.mem.wr8(rec + 0x161, level);
        cpu.mem.wr32(SUMMON_RECORD, MONSTER + 4 * 0x80);
        cpu.mem.wr8(MONSTER + 4 * 0x80 + 0x1D, summon_el);
        for m in 0..4u32 {
            let mi = m as usize;
            let a = ACTOR + (3 + m) * 0x400;
            let r = MONSTER + m * 0x80;
            cpu.mem.wr8(r + 0x1D, els[mi]);
            cpu.mem
                .wr16(a + 0x14C, u16::from(alive[mi] && (m as u8) < monsters));
            for (k, (ao, ro)) in COMPARED.iter().enumerate() {
                cpu.mem.wr16(a + ao, base[mi][k]);
                cpu.mem.wr16(r + ro, raw[mi][k]);
            }
            // No battle write moves the MP base, so it stands on its record.
            cpu.mem.wr16(a + 0x152, 50);
            cpu.mem.wr16(r + 0x10, 50);
        }
        cpu.mem.wr32(STAGED, SENTINEL);
        state.set(seed);
        assert_eq!(cpu.call(STAGER, &[]), Stop::Returned, "round {round}");
        let retail_staged = cpu.mem.rd32(STAGED);
        let retail = (
            (retail_staged != SENTINEL).then_some(retail_staged as u8),
            state.get(),
        );

        // Port.
        let mut party = Party::zeroed(3);
        let mut list = party.members[usize::from(caster)].spell_list();
        list.ids[0] = spell;
        list.levels[0] = level;
        party.members[usize::from(caster)].set_spell_list(list);
        w.load_party(party);
        w.enter_battle(3, monsters);
        w.party.party_count = 3;
        w.battle.scripted_fight = scripted;
        w.tables.summon_elements.insert(spell, summon_el);
        for m in 0..usize::from(monsters) {
            let seat = 3 + m;
            let id = 0x40 + m as u16;
            let mut def = MonsterDef::new(id, "seat", 100, 10);
            def.raw_stats = raw[m];
            w.tables.monster_catalog.insert(def);
            let a = &mut w.actors[seat];
            a.battle_monster_id = Some(id);
            a.battle_element = Some(els[m]);
            a.battle.hp = u16::from(alive[m]);
            a.battle.max_hp = 1;
            a.battle.liveness = u16::from(alive[m]);
            a.battle.agl_base = base[m][0];
            w.battle.attack_base[seat] = base[m][1];
            w.battle.defense_base[seat] = Some((base[m][2], 0));
            w.battle.accuracy_base[seat] = base[m][4];
            w.battle.speed_base[seat] = base[m][5];
        }
        w.actors[usize::from(caster)].battle.active_target = class;
        w.rng_state = seed;
        let targets: Vec<u8> = match class {
            0..=2 => vec![class],
            8 => vec![0, 1, 2],
            9 => (3..3 + monsters).collect(),
            t => vec![t],
        };
        let port = (
            w.stage_seru_side_effect(caster, spell, &targets)
                .map(|(_, amount)| amount),
            w.rng_state,
        );
        assert_eq!(
            retail, port,
            "round {round} caster {caster} spell {spell:#x} level {level} element {summon_el} scripted {scripted} class {class} monsters {monsters} alive {alive:?} first-record element {}",
            els[0]
        );
        staged += u32::from(retail.0.is_some());
        suppressed += u32::from(scripted && retail.0.is_none() && level >= 3);
        moved += u32::from(!scripted && retail.0.is_none() && level >= 3);
    }
    assert!(staged > 5000, "{staged}");
    assert!(suppressed > 1000, "{suppressed}");
    assert!(moved > 100, "{moved}");
}
