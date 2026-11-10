//! Case 4 of the battle player-file streamer `FUN_80052770` (`SCUS_942.54`,
//! `0x80052980..0x80052A74`): the equipment-section selector, against
//! `legaia_actor_formats::battle_char_assembly::select_sections`.
//!
//! The streamer is a state machine on `0x8007B648`; with the state at `4` the
//! one arm runs, walks the member's 12-byte `[id, offset, size]` descriptors
//! against the live character record's five equip bytes, writes the chosen
//! `(offset, size)` pair per section to `0x801C93A0` and advances the state.

use crate::common::{Rng, scus_cpu};
use legaia_actor_formats::battle_char_assembly::select_sections;
use legaia_actor_formats::battle_data_pack::{BattleDataPack, Record};
use psx_retail_exec::Stop;

const STREAMER: u32 = 0x8005_2770;
const STATE: u32 = 0x8007_B648;
const MEMBER_IX: u32 = 0x8007_B318 + 0xA7C;
const CURSOR: u32 = 0x8007_B318 + 0xA80;
const PARTY: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4140;
const SELECTED: u32 = 0x801C_93A0;
const TABLE: u32 = 0x8012_0000;

#[test]
fn the_equipment_selector_matches_retail() {
    differential!(scus(STREAMER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4551_5550);
    let (mut matched, mut defaulted, mut doubled) = (0, 0, 0);
    for round in 0..20000 {
        // Five sections, each a run of variant entries closed by its id-0
        // default, with now and then a repeated id inside one section.
        let mut records = Vec::new();
        let mut offset = 0u32;
        let mut push = |records: &mut Vec<Record>, id: u32, rng: &mut Rng| {
            let size = 0x800 * (1 + rng.below(4));
            records.push(Record {
                index: records.len(),
                id,
                data_offset: offset,
                size,
            });
            offset += size;
        };
        let mut ids: Vec<Vec<u32>> = Vec::new();
        for _ in 0..5 {
            let n = rng.below(5);
            let mut sec: Vec<u32> = (0..n).map(|_| 1 + rng.below(0x30)).collect();
            if n > 0 && rng.below(4) == 0 {
                let d = sec[rng.below(n) as usize];
                sec.push(d);
            }
            for &id in &sec {
                push(&mut records, id, &mut rng);
            }
            push(&mut records, 0, &mut rng);
            ids.push(sec);
        }
        let equipped: [u8; 5] = std::array::from_fn(|s| {
            match rng.below(4) {
                0 => 0,
                1 => 1 + rng.below(0x30) as u8,
                _ if !ids[s].is_empty() => ids[s][rng.below(ids[s].len() as u32) as usize] as u8,
                _ => 0,
            }
        });
        let member = rng.below(3);
        let char_id = 1 + rng.below(4);
        for (i, r) in records.iter().enumerate() {
            let at = TABLE + 12 * i as u32;
            cpu.mem.wr32(at, r.id);
            cpu.mem.wr32(at + 4, r.data_offset);
            cpu.mem.wr32(at + 8, r.size);
        }
        cpu.mem.wr8(STATE, 4);
        cpu.mem.wr8(MEMBER_IX, member as u8);
        cpu.mem.wr8(PARTY + member, char_id as u8);
        let rec = RECORDS + 0x414 * (char_id - 1) + 0x75E;
        for (s, e) in equipped.iter().enumerate() {
            cpu.mem.wr8(rec + s as u32, *e);
        }
        cpu.mem.wr32(CURSOR, TABLE);
        // Case 3 clears the five size words; the offsets stand as they were.
        for s in 0..5u32 {
            cpu.mem.wr32(SELECTED + 8 * s, 0xDEAD_0000 + s);
            cpu.mem.wr32(SELECTED + 8 * s + 4, 0);
        }
        assert_eq!(cpu.call(STREAMER, &[]), Stop::Returned, "round {round}");

        let pack = BattleDataPack {
            table_offset: 0,
            records: records.clone(),
            data_base: 0,
        };
        let want = select_sections(&pack, &equipped).expect("five sections");
        let ctx = format!("round {round} equipped {equipped:?} ids {ids:?}");
        assert_eq!(cpu.mem.rd8(STATE), 5, "the state advances: {ctx}");
        for (s, w) in want.iter().enumerate() {
            let at = SELECTED + 8 * s as u32;
            assert_eq!(
                (cpu.mem.rd32(at), cpu.mem.rd32(at + 4)),
                (w.data_offset, w.size),
                "section {s}: {ctx}"
            );
            if w.id == 0 {
                defaulted += 1;
            } else {
                matched += 1;
            }
        }
        doubled += u32::from(ids.iter().enumerate().any(|(s, sec)| {
            sec.iter().filter(|&&i| i == u32::from(equipped[s])).count() > 1
        }));
    }
    assert!(
        matched > 20000 && defaulted > 20000 && doubled > 500,
        "{matched} {defaulted} {doubled}"
    );
}
