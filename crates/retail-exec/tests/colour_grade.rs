//! The field overlay's geometry colour grade (PROT 0897), the field VM's
//! `4C E6`: the per-object walk `FUN_801D5E20` against
//! `primitive_colour_grade::shift_primitive_colours`, and the resident-table
//! walk `FUN_801D8280` against `shift_resident_tmds`, with the SCUS colour
//! pair `FUN_8001A78C` / `FUN_8001A6C8` live beneath both.
//!
//! The second test runs the prologue's own two ops over TMDs off the disc,
//! each made resident by the registrar `FUN_800268DC` first.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_container_formats::tmd_scan::scan_buffer;
use legaia_engine_field::primitive_colour_grade::{
    COLOUR_COUNT_TABLE_ADDR, COLOUR_COUNTS, HsvShift, shift_primitive_colours, shift_resident_tmds,
};
use psx_retail_exec::{Cpu, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;
const GRADE: u32 = 0x801D_5E20;
const WALK: u32 = 0x801D_8280;
const REGISTER: u32 = 0x8002_68DC;
const TABLE: u32 = 0x8007_C018;
const LAST: u32 = 0x8007_BB38;
const OBJECT: u32 = 0x8010_0000;
const BLOCK: u32 = 0x8010_1000;
const TMDS: u32 = 0x8010_0000;

fn cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    cpu.step_budget = 400_000_000;
    Some(cpu)
}

fn shift(rng: &mut Rng) -> HsvShift {
    let pick = |rng: &mut Rng, span: u32| match rng.below(4) {
        0 => 0,
        _ => rng.below(span * 2 + 1) as i32 - span as i32,
    };
    HsvShift {
        hue: pick(rng, 0x167),
        saturation: pick(rng, 0x120),
        value: pick(rng, 0x120),
    }
}

fn args(s: &HsvShift) -> [u32; 3] {
    [s.hue as u32, s.saturation as u32, s.value as u32]
}

#[test]
fn the_port_s_colour_count_table_is_the_overlay_s() {
    not_a_differential!(
        "compares the port's colour-count table with the PROT 0897 bytes at 0x801F26F0; runs no routine"
    );
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert_eq!(
        cpu.mem.read(COLOUR_COUNT_TABLE_ADDR, COLOUR_COUNTS.len()),
        COLOUR_COUNTS
    );
}

#[test]
fn the_colour_grade_walks_a_block_as_retail_does() {
    differential!(prot(897, GRADE));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4752_4144);
    let (mut graded, mut empty, mut unlit, mut capped) = (0, 0, 0, 0);
    for round in 0..3000 {
        let mut block = Vec::new();
        for _ in 0..rng.below(5) {
            let count = match rng.below(5) {
                0 => 0,
                _ => 1 + rng.below(6) as u16,
            };
            // Every selector the table covers, both parities of the flags
            // word's low bit.
            let flags = (rng.below(COLOUR_COUNTS.len() as u32) as u16) << 1 | rng.below(2) as u16;
            let ilen = 4 + rng.below(6) as u8;
            block.extend_from_slice(&count.to_le_bytes());
            block.extend_from_slice(&flags.to_le_bytes());
            block.extend_from_slice(&[rng.next() as u8, ilen, rng.next() as u8, rng.next() as u8]);
            for _ in 0..(usize::from(count) + 1) * usize::from(ilen) * 4 {
                block.push(rng.next() as u8);
            }
            // A header's first word is never zero: an empty group of flags 0
            // would end the walk on both sides, which the terminator covers.
            if count == 0 && flags == 0 {
                let at = block.len() - (usize::from(ilen) * 4 + 8) + 2;
                block[at] = 0x18;
            }
            empty += u32::from(count == 0);
            unlit += u32::from(count != 0 && COLOUR_COUNTS[usize::from(flags >> 1)] == 0);
        }
        block.extend_from_slice(&[0; 4]);
        block.extend((0..16).map(|_| rng.next() as u8));
        let s = shift(&mut rng);
        cpu.mem.load(BLOCK, &block);
        cpu.mem.wr32(OBJECT + 0x10, BLOCK);
        let [h, sat, v] = args(&s);
        assert_eq!(
            cpu.call(GRADE, &[OBJECT, h, sat, v]),
            Stop::Returned,
            "round {round}"
        );
        let mut port = block.clone();
        let n = shift_primitive_colours(&mut port, &s);
        let retail = cpu.mem.read(BLOCK, block.len());
        assert!(
            retail == port,
            "round {round} {s:?}: {:?}",
            diff_offsets(&retail, &port)
        );
        graded += n;
        capped += port
            .iter()
            .zip(&block)
            .filter(|(a, b)| **a == 0xF8 && **b > 0xF8)
            .count();
    }
    assert!(
        graded > 10_000 && empty > 500 && unlit > 1000 && capped > 50,
        "{graded} {empty} {unlit} {capped}"
    );
}

#[test]
fn the_resident_walk_grades_the_disc_s_tmds_as_retail_does() {
    differential!(prot(897, WALK));
    let (Some(mut cpu), Some(disc)) = (cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The prologue's two ops, then one generated shift a pair.
    let sepia = [
        HsvShift {
            hue: 0,
            saturation: -0x100,
            value: 0,
        },
        HsvShift {
            hue: 0x38,
            saturation: 0x90,
            value: -0x1E,
        },
    ];
    let mut rng = Rng(0x5345_5049);
    let mut pending: Vec<Vec<u8>> = Vec::new();
    let (mut tmds, mut words, mut pairs) = (0usize, 0usize, 0usize);
    let mut run = |cpu: &mut Cpu, batch: &mut Vec<Vec<u8>>, rng: &mut Rng| {
        // Lay the batch out, make each resident, and list it in the table.
        let mut at = TMDS;
        let mut placed = Vec::new();
        for (i, bytes) in batch.iter().enumerate() {
            cpu.mem.load(at, bytes);
            assert_eq!(cpu.call(REGISTER, &[at]), Stop::Returned);
            cpu.mem.wr32(TABLE + 4 * i as u32, at);
            placed.push(at);
            at += (bytes.len() as u32 + 0x103) & !3;
        }
        cpu.mem.wr32(LAST, batch.len() as u32 - 1);
        let mut ops = sepia.to_vec();
        ops.push(shift(rng));
        for s in &ops {
            let [h, sat, v] = args(s);
            assert_eq!(cpu.call(WALK, &[h, sat, v]), Stop::Returned);
            let n = shift_resident_tmds(batch.iter_mut().map(Vec::as_mut_slice), s);
            for (bytes, &va) in batch.iter().zip(&placed) {
                // The object table holds addresses on retail's side.
                let objects = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
                let body = 12 + objects * 0x1C;
                let retail = cpu.mem.read(va + body as u32, bytes.len() - body);
                assert!(
                    retail == bytes[body..],
                    "{s:?}: {:?}",
                    diff_offsets(&retail, &bytes[body..])
                );
            }
            words += n;
        }
        tmds += batch.len();
        pairs += usize::from(batch.len() > 1);
        batch.clear();
    };
    let mut want = 1;
    for entry in (0..1233).step_by(5) {
        let Ok(buf) = disc.read_entry(entry) else {
            continue;
        };
        for hit in scan_buffer(&buf).into_iter().take(2) {
            let bytes = &buf[hit.offset..hit.offset + hit.byte_len];
            if bytes.len() > 0x2_0000 || legaia_tmd::parse(bytes).is_err() {
                continue;
            }
            pending.push(bytes.to_vec());
            if pending.len() == want {
                run(&mut cpu, &mut pending, &mut rng);
                want = want % 3 + 1;
            }
        }
    }
    if !pending.is_empty() {
        run(&mut cpu, &mut pending, &mut rng);
    }
    // A last index of `-1` walks nothing.
    cpu.mem.wr32(LAST, u32::MAX);
    cpu.mem.wr32(TABLE, 0);
    assert_eq!(cpu.call(WALK, &[1, 2, 3]), Stop::Returned);
    assert!(
        tmds > 40 && words > 50_000 && pairs > 10,
        "tmds {tmds} words {words} pairs {pairs}"
    );
}
