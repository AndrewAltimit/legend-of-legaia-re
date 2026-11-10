//! The VDF mesh-morph applier `FUN_8001C604` and its GTE blend loop
//! `FUN_8005B038` against `vdf_morph::{stage_group_morph,
//! apply_weighted_deltas}`.
//!
//! The applier copies a TMD group's rest-pose vertices to the scratch window
//! under the top of the asset buffer (`*0x8007B85C + 0x62C00 - count * 8`),
//! retargets the group's vertex pointer there, and adds every record of every
//! morph slot that names the group, scaled by the slot's weight through the
//! GTE's `GPF` (`sf = 1`, `lm = 0`). Compared: the staged vertices, the
//! retargeted pointer, the returned rest-pose pointer and the rest pose left
//! as it was.
//!
//! Every generated record carries at least one delta: the blend loop is a
//! `do`-`while` on its end pointer, so a record of zero deltas would run it
//! until the pointer wraps. The applier keeps each record inside its group.

use crate::common::{Rng, scus_cpu};
use legaia_scene_formats::vdf_morph::{VdfMorphRecord, apply_weighted_deltas, stage_group_morph};
use psx_retail_exec::{Stop, V0};

const STAGE: u32 = 0x8001_C604;
const BLEND: u32 = 0x8005_B038;
const ASSET_BUF_PTR: u32 = 0x8007_B85C;
const ASSET_BUF: u32 = 0x8010_0000;
const VDF_TABLE: u32 = 0x8008_3E58;
const ACTOR: u32 = 0x8018_0000;
const GROUPS: u32 = 0x8018_1000;
const PAIRS: u32 = 0x8018_2000;
const REST: u32 = 0x8018_4000;
const ENTRIES: u32 = 0x8019_0000;
const DST: u32 = 0x8019_8000;
const DELTAS: u32 = 0x8019_C000;

fn weight(rng: &mut Rng) -> u16 {
    [
        0x1000,
        0x800,
        0,
        1,
        0x7FFF,
        0x8000,
        0xF000,
        rng.next() as u16,
    ][rng.below(8) as usize]
}

fn delta(rng: &mut Rng) -> i16 {
    [
        0,
        1,
        -1,
        0x7FFF,
        -0x8000,
        rng.below(0x200) as i16 - 0x100,
        rng.next() as i16,
    ][rng.below(7) as usize]
}

/// A VDF sub-entry: records for `group` and for others, each inside an
/// `n_verts` group.
fn gen_entry(rng: &mut Rng, group: u32, n_verts: u32) -> Vec<u8> {
    let records = rng.below(5);
    let mut b = records.to_le_bytes().to_vec();
    for _ in 0..records {
        let g = if rng.below(3) == 0 {
            rng.below(8)
        } else {
            group
        };
        let count = 1 + rng.below(n_verts);
        let dst = rng.below(n_verts - count + 1);
        b.extend_from_slice(&g.to_le_bytes());
        b.extend_from_slice(&dst.to_le_bytes());
        b.extend_from_slice(&count.to_le_bytes());
        for _ in 0..count {
            for _ in 0..3 {
                b.extend_from_slice(&delta(rng).to_le_bytes());
            }
            b.extend_from_slice(&(rng.next() as u16).to_le_bytes());
        }
    }
    b
}

#[test]
fn the_blend_loop_matches_retail() {
    differential!(scus(BLEND));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x424C_4E44);
    for run in 0..20_000 {
        let count = 1 + rng.below(40) as usize;
        let dst: Vec<u8> = (0..count * 8).map(|_| rng.next() as u8).collect();
        let mut deltas = Vec::new();
        for _ in 0..count {
            for _ in 0..3 {
                deltas.extend_from_slice(&delta(&mut rng).to_le_bytes());
            }
            deltas.extend_from_slice(&(rng.next() as u16).to_le_bytes());
        }
        let w = weight(&mut rng);
        cpu.mem.load(DST, &dst);
        cpu.mem.load(DELTAS, &deltas);
        let args = [DST, DELTAS, count as u32, u32::from(w)];
        assert_eq!(cpu.call(BLEND, &args), Stop::Returned);
        let retail = cpu.mem.read(DST, count * 8);
        let mut port = dst.clone();
        let rec = VdfMorphRecord {
            group_id: 0,
            dst_index: 0,
            deltas: &deltas,
        };
        apply_weighted_deltas(&mut port, 0, &rec, w as i16);
        assert_eq!(retail, port, "run {run} weight {w:#x}");
        assert_eq!(cpu.gte_ops, count as u64);
    }
}

#[test]
fn the_morph_staging_matches_retail() {
    differential!(scus(STAGE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(ASSET_BUF_PTR, ASSET_BUF);
    let mut rng = Rng(0x5644_464D);
    let (mut fails, mut blended) = (Vec::new(), 0u64);
    for run in 0..5000 {
        let group = rng.below(8);
        let n_verts = 1 + rng.below(60);
        let rest: Vec<u8> = (0..n_verts * 8).map(|_| rng.next() as u8).collect();
        cpu.mem.load(REST, &rest);
        cpu.mem.wr32(ACTOR + 0x44, GROUPS);
        cpu.mem.wr32(GROUPS + 4 + group * 4, PAIRS);
        cpu.mem.wr32(PAIRS, REST);
        cpu.mem.wr32(PAIRS + 4, n_verts);
        let slots = rng.below(5);
        cpu.mem.wr8(ACTOR + 0x6C, slots as u8);
        let mut entries: Vec<(Vec<u8>, u16)> = Vec::new();
        let mut at = ENTRIES;
        for s in 0..slots {
            let e = gen_entry(&mut rng, group, n_verts);
            let w = weight(&mut rng);
            let idx = rng.below(32);
            cpu.mem.wr32(VDF_TABLE + idx * 4, at);
            cpu.mem.load(at, &e);
            cpu.mem.wr8(ACTOR + 0xB0 + s, idx as u8);
            cpu.mem.wr16(ACTOR + 0xA0 + s * 2, w);
            at += e.len() as u32 + 0x10;
            entries.push((e, w));
        }
        // A later slot naming the same table index overwrites the pointer:
        // both sides read the entry each slot's index resolves to now.
        let resolved: Vec<(Vec<u8>, i16)> = (0..slots)
            .map(|s| {
                let idx = u32::from(cpu.mem.rd8(ACTOR + 0xB0 + s));
                let ptr = cpu.mem.rd32(VDF_TABLE + idx * 4);
                let len = entries.iter().map(|(e, _)| e.len()).max().unwrap_or(0);
                let bytes = cpu.mem.read(ptr, len.max(4));
                let w = cpu.mem.rd16(ACTOR + 0xA0 + s * 2) as i16;
                (bytes, w)
            })
            .collect();
        assert_eq!(cpu.call(STAGE, &[ACTOR, group]), Stop::Returned);
        let scratch = ASSET_BUF + 0x62C00 - n_verts * 8;
        let retail = (
            cpu.regs.r[V0],
            cpu.mem.rd32(PAIRS),
            cpu.mem.read(scratch, (n_verts * 8) as usize),
            cpu.mem.read(REST, rest.len()) == rest,
        );
        let slot_refs: Vec<(&[u8], i16)> = resolved.iter().map(|(b, w)| (&b[..], *w)).collect();
        let port = (
            REST,
            scratch,
            stage_group_morph(&rest, group, &slot_refs),
            true,
        );
        blended += cpu.gte_ops;
        if retail != port && fails.len() < 6 {
            let at = retail.2.iter().zip(&port.2).position(|(a, b)| a != b);
            fails.push(format!(
                "run {run} group {group} verts {n_verts} slots {slots}: ret {:#x}/{:#x} ptr \
                 {:#x}/{:#x} rest kept {} first vertex diff {at:?}",
                retail.0, port.0, retail.1, port.1, retail.3
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(blended > 50_000, "{blended}");
}
