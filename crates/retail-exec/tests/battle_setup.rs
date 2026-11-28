//! Battle setup `FUN_800513F0`, run whole with its loaders stubbed, against
//! `World::enter_battle_from_formation`: the scripted-fight byte
//! `ctx[+0x287]`, where each party and monster body is spawned (the seat rows
//! at `0x800775C8` / `0x80077608`, the family addends included), the `rand()`
//! each seat draws for its opening clip phase, the special-battle word
//! `_DAT_8007BAC0` and the loader's fade-in.
//!
//! The spawn `FUN_80024C88` is hooked: it hands back a node holding the seat
//! it was given, as the real one stores it, which the setup then copies into
//! the battle actor's `+0x3C`. `FUN_80052FA0` (the party assembly) is hooked
//! to its one store the setup reads, the member count `ctx[+0]`.
//!
//! The backdrop pair the setup allocates (`ctx[+0x106C]` / `ctx[+0x1070]`)
//! is compared too: the object-list edit that drops object 1 unless
//! `DAT_8007B64B` is set (`battle_backdrop::drawn_object_indices_gated`), the
//! second copy's transform off the `DAT_80078B50` list
//! (`MirrorXTable`), and the outdoor byte `0x8007BDA8` off the
//! `DAT_80078C1C` list (`OutdoorCueTable`).

use crate::common::{Rng, scus, scus_cpu};
use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
use legaia_engine_battle_vm::battle_ground_grid::OutdoorCueTable;
use legaia_engine_core::world::World;
use legaia_engine_system::fade::FadeTemplate;
use legaia_save::Party;
use legaia_scene_formats::battle_backdrop::{
    MirrorXTable, drawn_object_indices_gated, drawn_object_indices_rebound,
};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

const SETUP: u32 = 0x8005_13F0;
const SETUP_END: u32 = 0x8005_1D84;
const PARTY_ASSEMBLY: u32 = 0x8005_2FA0;
const SPAWN: u32 = 0x8002_4C88;
const ALLOC_NODE: u32 = 0x8002_0DE0;
const ALLOC: u32 = 0x8001_7888;
const RAND: u32 = 0x8005_6798;
const SPAWN_FADE: u32 = 0x8002_4E80;

const CTX_PTR: u32 = 0x8007_BD24;
const FLAGS: u32 = 0x8007_BD60;
const PARTY_IDS: u32 = 0x8007_BD10;
const CELLS: u32 = 0x8007_BD0C;
const SPECIAL: u32 = 0x8007_BAC0;
const MAP_ID: u32 = 0x8008_4540;
const WHITE_INTRO: u32 = 0x8007_B8FC;
const SHELL_SHARED: u32 = 0x8007_B64B;
const PARTY_SOURCES: u32 = 0x801C_9360;
const MONSTER_SOURCES: u32 = 0x801C_9348;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_ROWS: u32 = 0x8007_6764;

const CTX: u32 = 0x8010_0000;
const ACTOR: u32 = 0x8011_0000;
const NODE: u32 = 0x8012_0000;
const SOURCE: u32 = 0x8013_0000;
const RECORD: u32 = 0x8014_0000;
const HEAP: u32 = 0x8016_0000;

#[test]
fn battle_setup_seats_and_draws_as_retail_does() {
    differential!(scus(SETUP));
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mirror = MirrorXTable::from_scus(&scus).expect("mirror table");
    let outdoor = OutdoorCueTable::from_scus(&scus).expect("outdoor table");
    cpu.stub_outside = Some((SETUP, SETUP_END));
    cpu.hook(PARTY_ASSEMBLY, |_, m| {
        let n = (0..3).filter(|&i| m.rd8(PARTY_IDS + i) != 0).count();
        m.wr8(CTX, n as u8);
    });
    // Each spawn's node carries the seat it was handed (`+0x14..+0x18`).
    let spawned = Rc::new(RefCell::new(0u32));
    let s = spawned.clone();
    cpu.hook(SPAWN, move |r, m| {
        let mut n = s.borrow_mut();
        let node = NODE + *n * 0x400;
        *n += 1;
        for i in 0..3 {
            let v = m.rd16(r.r[A0] + 2 * i);
            m.wr16(node + 0x14 + 2 * i, v);
        }
        // The object table the registration snapshot indexes.
        m.wr32(node + 0x44, node + 0x100);
        r.r[V0] = node;
    });
    let s = spawned.clone();
    // The backdrop's object table: a count and that many pointers, the same
    // on both copies (both are allocated off one source).
    let objects = Rc::new(Cell::new(2u32));
    let o = objects.clone();
    cpu.hook(ALLOC_NODE, move |r, m| {
        let mut n = s.borrow_mut();
        let node = NODE + *n * 0x400;
        *n += 1;
        for b in (0..0x200).step_by(4) {
            m.wr32(node + b, 0);
        }
        m.wr32(node + 0x44, node + 0x100);
        m.wr32(node + 0x100, o.get());
        for k in 0..o.get() {
            m.wr32(node + 0x104 + 4 * k, 0xA000_0000 + k);
        }
        r.r[V0] = node;
    });
    cpu.stub(ALLOC, HEAP);
    let draws = Rc::new(RefCell::new((Vec::<u32>::new(), 0usize)));
    let d = draws.clone();
    cpu.hook(RAND, move |r, _| {
        let mut d = d.borrow_mut();
        let i = d.1;
        r.r[V0] = d.0[i];
        d.1 += 1;
    });
    let fades = Rc::new(RefCell::new(Vec::new()));
    let f = fades.clone();
    cpu.hook(SPAWN_FADE, move |r, m| {
        let w: Vec<i16> = (0..13).map(|i| m.rd16(r.r[A0] + 2 * i) as i16).collect();
        f.borrow_mut().push(FadeTemplate {
            kind: w[0],
            duration: w[1],
            start_rgb: [w[3], w[4], w[5]],
            end_rgb: [w[7], w[8], w[9]],
            mode: [w[10], w[11], r.r[A1] as i16],
        });
        r.r[V0] = 0x801A_0000;
    });

    cpu.mem.wr32(CTX_PTR, CTX);
    for k in 0..8u32 {
        cpu.mem.wr32(ACTORS + 4 * k, ACTOR + k * 0x400);
    }
    for i in 0..3u32 {
        let src = SOURCE + i * 0x100;
        cpu.mem.wr32(PARTY_SOURCES + 4 * i, src);
        cpu.mem.wr32(src, RECORD + i * 0x200);
        cpu.mem.wr32(src + 0x50, src + 0x80);
        // The blob's object-index pair sits at its own `+0x20` offset.
        cpu.mem.wr8(src + 0x80 + 0x20, 0x24);
    }
    for s in 0..4u32 {
        let src = SOURCE + 0x800 + s * 0x100;
        cpu.mem.wr32(MONSTER_SOURCES + 4 * s, src);
        cpu.mem.wr32(src + 0x4C, RECORD + (3 + s) * 0x200);
    }
    for k in 0..7u32 {
        let rec = RECORD + k * 0x200;
        cpu.mem.wr32(rec + 0x88, rec + 0x8C);
    }
    const OUTDOOR_BYTE: u32 = 0x8007_BDA8;

    let mut rng = Rng(0x5345_5455);
    let (mut families, mut seated, mut raseru) = ([0usize; 3], 0usize, 0);
    let (mut stages, mut kept) = ([0usize; 4], 0usize);
    for case in 0..3000 {
        let pc = 1 + rng.below(3);
        let mc = 1 + rng.below(4);
        let scripted = rng.below(3) == 0;
        let map_id = match rng.below(6) {
            4 => u32::from(mirror.ids()[rng.below(mirror.ids().len() as u32) as usize]),
            5 => u32::from(outdoor.ids()[rng.below(outdoor.ids().len() as u32) as usize]),
            k => [0x0Cu32, 0x15, 3, 0x40][k as usize],
        };
        let shared = rng.below(3) == 0;
        let shell_objects = 2 + rng.below(4);
        let lead: u8 = match rng.below(6) {
            0 => 0x3D + rng.below(3) as u8,
            1 => 0xAF,
            _ => 1 + rng.below(0x3C) as u8,
        };
        let special = [0u32, 0x200, 0x300, 0x100][rng.below(4) as usize];
        let white = rng.below(2);
        // The roster ids, Terra's `4` among them, and the monsters' size
        // class: the node `+0x58` separation word each seat gets.
        let mut ids = [1u8, 2, 3, 4];
        for i in (1..4).rev() {
            ids.swap(i, rng.below(i as u32 + 1) as usize);
        }
        let size = rng.below(8) as u8;
        let rolls: Vec<u32> = (0..8).map(|_| rng.next() & 0x7FFF).collect();
        let frames: Vec<u8> = (0..7).map(|_| 1 + rng.below(60) as u8).collect();

        for b in 0..0x1100 {
            cpu.mem.wr8(CTX + b, 0);
        }
        cpu.mem.wr8(CTX + 1, mc as u8);
        cpu.mem.wr8(FLAGS, if scripted { 0x80 } else { 0 });
        for i in 0..3 {
            cpu.mem
                .wr8(PARTY_IDS + i, if i < pc { ids[i as usize] } else { 0 });
        }
        for s in 0..4 {
            cpu.mem
                .wr16(MONSTER_ROWS + s * 0x18 + 0x14, if s < mc { 2 } else { 0 });
        }
        for (k, f) in frames.iter().enumerate() {
            cpu.mem.wr8(RECORD + k as u32 * 0x200 + 0x8D, *f);
        }
        cpu.mem.wr8(CELLS, lead);
        for s in 0..4u32 {
            cpu.mem.wr8(SOURCE + 0x800 + s * 0x100 + 0x1F, size);
        }
        cpu.mem.wr32(SPECIAL, special);
        cpu.mem.wr32(MAP_ID, map_id);
        cpu.mem.wr32(WHITE_INTRO, white);
        cpu.mem.wr8(SHELL_SHARED, u8::from(shared));
        cpu.mem.wr8(OUTDOOR_BYTE, 0xFF);
        objects.set(shell_objects);
        *spawned.borrow_mut() = 0;
        *draws.borrow_mut() = (rolls.clone(), 0);
        fades.borrow_mut().clear();
        assert_eq!(cpu.call(SETUP, &[]), Stop::Returned, "case {case}");

        let mut w = World::new();
        w.load_party(Party::zeroed(4));
        w.set_active_party(ids[..pc as usize].iter().map(|&id| id - 1).collect());
        w.party.party_count = pc as u8;
        let mut def =
            legaia_engine_battle::monster_catalog::MonsterDef::new(u16::from(lead), "m", 10, 1);
        def.size_class = size;
        w.tables.monster_catalog.insert(def);
        w.battle.map_id = map_id;
        // Seat the port's stream on the same draws: the setup's are its first.
        let mut slots = vec![FormationSlot::new(u16::from(lead))];
        slots.resize(mc as usize, FormationSlot::new(u16::from(lead)));
        let mut formation = FormationDef::new(1, slots);
        formation.header_flags = u8::from(scripted);
        let mut twin = World::new();
        twin.rng_state = w.rng_state;
        let port_draws: Vec<u32> = (0..pc + mc).map(|_| twin.next_rand()).collect();
        w.enter_battle_from_formation(&formation);

        let ctx = format!(
            "case {case}: pc {pc} mc {mc} scripted {scripted} map {map_id:#x} lead {lead:#x}"
        );
        assert_eq!(
            cpu.mem.rd8(CTX + 0x287),
            w.battle_ctx.scripted_fight,
            "{ctx}: ctx[+0x287]"
        );
        // One draw a seated body, party first.
        assert_eq!(draws.borrow().1 as u32, pc + mc, "{ctx}: draws");
        for slot in 0..(pc + mc) as usize {
            let retail_seat = if slot < pc as usize {
                slot
            } else {
                3 + slot - pc as usize
            } as u32;
            let actor = ACTOR + retail_seat * 0x400;
            let node = cpu.mem.rd32(actor + 0x22C);
            let pos: [i16; 3] =
                std::array::from_fn(|i| cpu.mem.rd16(actor + 0x3C + 2 * i as u32) as i16);
            let m = &w.actors[slot].move_state;
            assert_eq!(
                pos,
                [m.world_x, m.world_y, m.world_z],
                "{ctx}: slot {slot} seat"
            );
            // The phase: the port holds the draw until the seat's clip is in.
            let f = u32::from(frames[retail_seat as usize]);
            let (roll, draw) = (rolls[slot], port_draws[slot]);
            assert_eq!(cpu.mem.rd16(node + 0x68), ((roll % f) << 4) as u16);
            assert_eq!(w.actors[slot].battle_idle_phase_draw, Some(draw), "{ctx}");
            assert_eq!(
                cpu.mem.rd16(node + 0x58) as i16,
                w.drive_battle_separation_radius(slot),
                "{ctx}: slot {slot} separation word, ids {ids:?} size {size}"
            );
        }
        // The formation roll `FUN_80051D84`, stubbed here, may raise the
        // Ra-Seru bit again on the port's side; the setup's own pass is the
        // kernel's.
        assert_eq!(
            cpu.mem.rd32(SPECIAL),
            legaia_engine_battle_vm::battle_formulas::battle_init_special_word(special, lead),
            "{ctx}"
        );
        assert_eq!(cpu.mem.rd8(FLAGS) & 0x80, 0, "the scripted bit is consumed");
        // The loader's fade-in: white or black to clear over `0x14` frames.
        assert_eq!(
            *fades.borrow(),
            vec![FadeTemplate {
                kind: if white != 0 { 1 } else { 2 },
                duration: 0x14,
                start_rgb: [0xFF; 3],
                end_rgb: [0; 3],
                mode: [0, 1, 1],
            }]
        );
        assert_eq!(cpu.mem.rd32(WHITE_INTRO), 0);
        // The backdrop pair.
        let stage = map_id as u16;
        let keep: Vec<u32> = drawn_object_indices_gated(shell_objects as usize, shared)
            .into_iter()
            .map(|k| 0xA000_0000 + k as u32)
            .collect();
        for (copy, at) in [("first", 0x106C), ("second", 0x1070)] {
            let node = cpu.mem.rd32(CTX + at);
            let tbl = cpu.mem.rd32(node + 0x44);
            let n = cpu.mem.rd32(tbl);
            let list: Vec<u32> = (0..n).map(|k| cpu.mem.rd32(tbl + 4 + 4 * k)).collect();
            assert_eq!(list, keep, "{ctx}: {copy} copy's objects, shared {shared}");
            // The array past the count stays as the edit left it, and PROT
            // 0968's hand-back reads slot 1 over slot 0 there: a two-object
            // shell's slot 1 still names object 1.
            let rebound: Vec<u32> = (0..n)
                .map(|k| cpu.mem.rd32(tbl + 4 + 4 * k.max(1)))
                .collect();
            let want: Vec<u32> = drawn_object_indices_rebound(shell_objects as usize, shared, true)
                .into_iter()
                .map(|k| 0xA000_0000 + k as u32)
                .collect();
            assert_eq!(rebound, want, "{ctx}: {copy} copy rebound, shared {shared}");
        }
        let second = cpu.mem.rd32(CTX + 0x1070);
        let mirrored = mirror.contains_runtime_id(stage);
        assert_eq!(
            (cpu.mem.rd16(second + 0x5A), cpu.mem.rd16(second + 0x26)),
            if mirrored { (2, 0) } else { (0, 0x800) },
            "{ctx}: second copy"
        );
        assert_eq!(
            cpu.mem.rd8(OUTDOOR_BYTE),
            u8::from(outdoor.contains_runtime_id(stage)),
            "{ctx}: outdoor byte"
        );
        stages[usize::from(mirrored) + 2 * usize::from(outdoor.contains_runtime_id(stage))] += 1;
        kept += usize::from(shared);
        let map_arm = matches!(map_id, 0x0C | 0x15) && (0x3D..=0x3F).contains(&lead);
        families[usize::from(scripted) + usize::from(map_arm)] += 1;
        seated += (pc + mc) as usize;
        raseru += usize::from(cpu.mem.rd32(SPECIAL) & 0x200 != 0);
    }
    assert!(
        families.iter().all(|&n| n > 20) && seated > 9000 && raseru > 300,
        "{families:?} {seated} {raseru}"
    );
    assert!(
        stages[0] > 100 && stages[1] > 100 && stages[2] + stages[3] > 100 && kept > 300,
        "{stages:?} {kept}"
    );
}
