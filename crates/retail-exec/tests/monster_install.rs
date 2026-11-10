//! The monster record -> actor install `FUN_80054CB0` against the two kernels
//! the port builds a seat from: `boost_profile` (through
//! `MonsterDef::installed_stats`, both profiles, picked by the scripted-fight
//! flag `ctx[+0x287]`) and `clip_ladder::monster_reaction_map` over the
//! record's entry tags.
//!
//! Every monster seat and the summon body's seat `4`, every stat halfword,
//! entry lists from none to long with the reaction tags repeated, missing and
//! on entry `0`. Compared: the HP / MP triples, the six stats' working and
//! base halves, the two bytes the install clears, the `+0x230` word and the
//! reaction map `+0x1EF..+0x1F3`. The name half of the routine (the actor's
//! `+0x1BC` copy and the context's Seru-name line) runs, with the text
//! measurer stubbed, but is not compared.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_battle_loop::clip_ladder::monster_reaction_map;
use psx_retail_exec::Stop;

const INSTALL: u32 = 0x8005_4CB0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const RECORD: u32 = 0x8014_0000;
const NAME: u32 = 0x8014_0800;
const ENTRIES: u32 = 0x8014_1000;

#[test]
fn a_monster_record_installs_as_retail_installs_it() {
    differential!(scus(INSTALL));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    cpu.mem.load(NAME, b"\x01Gobu Gobu\0");
    // The name line's width; the line is not compared.
    cpu.stub(0x8003_5F04, 0x20);
    let mut rng = Rng(0x494E_5354);
    let (mut scripted, mut fallback, mut repeated) = (0, 0, 0);
    for n in 0..20_000 {
        let seat = rng.below(5);
        let flag = rng.below(2) == 0;
        cpu.mem
            .wr8(CTX + 0x287, if flag { 1 + rng.below(3) as u8 } else { 0 });
        let mut raw: Vec<u8> = (0..0x4C).map(|_| rng.next() as u8).collect();
        raw[0..4].copy_from_slice(&NAME.to_le_bytes());
        let stat = |rng: &mut Rng| -> u16 {
            match rng.below(4) {
                0 => rng.next() as u16,
                _ => rng.below(400) as u16,
            }
        };
        let hp = stat(&mut rng);
        let mp = stat(&mut rng);
        let stats: [u16; 6] = std::array::from_fn(|_| stat(&mut rng));
        let put = |raw: &mut Vec<u8>, off: usize, v: u16| {
            raw[off..off + 2].copy_from_slice(&v.to_le_bytes())
        };
        put(&mut raw, 0x0C, hp);
        put(&mut raw, 0x10, mp);
        for (off, v) in [0x0E, 0x12, 0x14, 0x16, 0x18, 0x1A].iter().zip(stats) {
            put(&mut raw, *off, v);
        }
        let count = rng.below(16) as u8;
        raw[0x4A] = count;
        let tags: Vec<u8> = (0..count)
            .map(|_| match rng.below(3) {
                0 => [2u8, 3, 4, 5, 0x0B][rng.below(5) as usize],
                _ => rng.below(0x20) as u8,
            })
            .collect();
        let mut seen = [0u8; 0x20];
        for t in &tags {
            seen[usize::from(*t)] += 1;
        }
        if [2usize, 3, 4, 5, 0xB].iter().any(|&t| seen[t] > 1) {
            repeated += 1;
        }
        cpu.mem.load(RECORD, &raw);
        for (i, t) in tags.iter().enumerate() {
            let at = ENTRIES + i as u32 * 0x100;
            cpu.mem.wr32(RECORD + 0x4C + i as u32 * 4, at);
            cpu.mem.wr8(at, *t);
        }
        let actor = ACTOR + (seat + 3) * 0x400;
        // A fresh seat: the reaction slots clear, everything else stale.
        for o in 0..0x400 {
            cpu.mem.wr8(actor + o, rng.next() as u8);
        }
        cpu.mem.load(actor + 0x1EF, &[0; 5]);
        assert_eq!(cpu.call(INSTALL, &[RECORD, seat]), Stop::Returned, "n {n}");

        let mut def = MonsterDef::new(1, "x", hp, 0);
        def.raw_stats = stats;
        let bs = def.installed_stats(flag);
        let block = cpu.mem.read(actor, 0x400);
        let h = |o: u32| u16::from_le_bytes([block[o as usize], block[o as usize + 1]]);
        assert_eq!(
            [h(0x14C), h(0x14E), h(0x172), h(0x150), h(0x152), h(0x174)],
            [hp, hp, hp, mp, mp, mp],
            "n {n}"
        );
        let retail_stats: Vec<(u16, u16)> = [0x154u32, 0x158, 0x15C, 0x160, 0x168, 0x164]
            .iter()
            .map(|&o| (h(o), h(o + 2)))
            .collect();
        let port_stats: Vec<(u16, u16)> = bs.iter().map(|&v| (v, v)).collect();
        assert_eq!(retail_stats, port_stats, "n {n} flag {flag} {stats:?}");
        assert_eq!(
            (cpu.mem.rd8(actor + 0x1D9), cpu.mem.rd8(actor + 0x1DA)),
            (0, 0),
            "n {n}"
        );
        assert_eq!(
            cpu.mem.rd32(actor + 0x230),
            u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]])
        );
        let map = monster_reaction_map(&tags);
        assert_eq!(
            cpu.mem.read(actor + 0x1EF, 5),
            map.to_vec(),
            "n {n} tags {tags:?}"
        );
        if flag {
            scripted += 1;
        }
        if seen[4] == 0 && seen[2] > 0 {
            fallback += 1;
        }
    }
    assert!(
        scripted > 5000 && fallback > 1000 && repeated > 1000,
        "{scripted} {fallback} {repeated}"
    );
}
