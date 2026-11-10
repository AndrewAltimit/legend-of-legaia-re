//! Small SCUS leaves that run on the GTE: the leading-zero square root
//! `FUN_8005B0B8` (`LZCS` / `LZCR` over the table at `0x80078E84`) against
//! `camera_zone::sqrt0`, and the morph blend `FUN_8005B038` (`GPF sf=1` per
//! delta, wrapping add) against `vdf_morph::apply_weighted_deltas`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_field::camera_zone::sqrt0;
use legaia_scene_formats::vdf_morph::{VdfMorphRecord, apply_weighted_deltas};
use psx_retail_exec::{Stop, V0};

const SQRT: u32 = 0x8005_B0B8;
const BLEND: u32 = 0x8005_B038;
const DST: u32 = 0x8010_0000;
const DELTAS: u32 = 0x8011_0000;

#[test]
fn sqrt0_matches_the_lzc_square_root() {
    differential!(scus(SQRT));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5351_5254);
    let mut inputs: Vec<i32> = (0..=0x1000).collect();
    inputs.extend((0..31).flat_map(|b| [1i32 << b, (1i32 << b) - 1, (1i32 << b) + 1]));
    inputs.push(i32::MAX);
    inputs.extend((0..40000).map(|_| (rng.next() >> rng.below(31)) as i32 & i32::MAX));
    for a in inputs.into_iter().filter(|a| *a >= 0) {
        assert_eq!(cpu.call(SQRT, &[a as u32]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0] as i32, sqrt0(a), "sqrt0({a:#x})");
    }
}

#[test]
fn the_morph_blend_matches_the_gpf_loop() {
    differential!(scus(BLEND));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x4D4F_5250);
    let mut saturated = 0;
    for round in 0..2000 {
        let count = 1 + rng.below(24) as usize;
        let start = rng.below(4) as usize;
        let big = round % 4 == 0;
        let mut deltas = vec![0u8; count * 8];
        for b in deltas.chunks_mut(2) {
            let v = if big {
                rng.next() as u16
            } else {
                (rng.below(0x400) as i32 - 0x200) as u16
            };
            b.copy_from_slice(&v.to_le_bytes());
        }
        let mut dst = vec![0u8; (start + count + 2) * 8];
        for b in dst.iter_mut() {
            *b = rng.next() as u8;
        }
        let weight = if big {
            rng.next() as i16
        } else {
            rng.below(0x2000) as i16 - 0x1000
        };
        cpu.mem.load(DST, &dst);
        cpu.mem.load(DELTAS, &deltas);
        let args = [
            DST + 8 * start as u32,
            DELTAS,
            count as u32,
            weight as u16 as u32,
        ];
        assert_eq!(cpu.call(BLEND, &args), Stop::Returned);
        let rec = VdfMorphRecord {
            group_id: 0,
            dst_index: start as u32,
            deltas: &deltas,
        };
        let mut port = dst.clone();
        apply_weighted_deltas(&mut port, start, &rec, weight);
        let got = cpu.mem.read(DST, dst.len());
        assert_eq!(
            got,
            port,
            "round {round} weight {weight} count {count}: {:?}",
            crate::common::diff_offsets(&got, &port)
        );
        saturated += usize::from(deltas.chunks(2).any(|d| {
            (i32::from(i16::from_le_bytes([d[0], d[1]])) * i32::from(weight)) >> 12 > 0x7FFF
        }));
    }
    assert!(saturated > 0, "no saturating delta");
}
