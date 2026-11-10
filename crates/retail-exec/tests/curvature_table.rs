//! The overworld curvature table builder `FUN_800271A8`, run off the disc
//! through its GTE loop (`RTPS` of `(0, ramp, 2000)` under `H = 0x3C0` and the
//! base matrix `_DAT_8007BF10`), against `overworld_curvature::curvature_table`
//! and its closed form: the scene-load build (game arguments, `OFY = 120`),
//! the map display's rebuild inside a frame (`OFY = 114`, every entry
//! [`REBUILD_BIAS`] lower) and its flat build `(0, 0)`.

use crate::common::scus_cpu;
use legaia_render_kernels::overworld_curvature::{
    CURVATURE_ENTRIES, CURVATURE_REACHABLE, REBUILD_BIAS, curvature_closed_form, curvature_table,
};
use psx_retail_exec::Stop;

const BUILD: u32 = 0x8002_71A8;
/// The gate byte the builder tests first.
const GATE: u32 = 0x8007_B6A8;
const RAMP_PTR: u32 = 0x8007_BB08;
const TABLE_PTR: u32 = 0x8007_BB04;
/// The base matrix block `FUN_8003D1A4` loads: `S * I`, zero translation.
const BASE_MATRIX: u32 = 0x8007_BF10;
const RAMP_BUF: u32 = 0x8012_0000;
const TABLE_BUF: u32 = 0x8013_0000;

fn build(step: u32, base: u32, ofy: i32) -> Option<Vec<i16>> {
    let mut cpu = scus_cpu()?;
    cpu.mem.wr8(GATE, 1);
    cpu.mem.wr32(RAMP_PTR, RAMP_BUF);
    cpu.mem.wr32(TABLE_PTR, TABLE_BUF);
    for (k, v) in [0x6000u16, 0, 0, 0, 0x6000, 0, 0, 0, 0x6000]
        .iter()
        .enumerate()
    {
        cpu.mem.wr16(BASE_MATRIX + 2 * k as u32, *v);
    }
    for k in 0..3 {
        cpu.mem.wr32(BASE_MATRIX + 0x14 + 4 * k, 0);
    }
    cpu.gte.write_ctrl(24, 160 << 16);
    cpu.gte.write_ctrl(25, (ofy << 16) as u32);
    assert_eq!(cpu.call(BUILD, &[step, base]), Stop::Returned);
    assert!(cpu.gte_ops >= 0x2000, "the RTPS loop ran");
    Some(
        (0..CURVATURE_ENTRIES as u32)
            .map(|i| cpu.mem.rd16(TABLE_BUF + 2 * i) as i16)
            .collect(),
    )
}

#[test]
fn curvature_table_matches_the_builder() {
    differential!(scus(BUILD));
    let Some(retail) = build(0x28, 0x2A_B980, 120) else {
        return;
    };
    let port = curvature_table();
    let bad: Vec<String> = (0..CURVATURE_ENTRIES)
        .filter(|&i| retail[i] != port[i])
        .take(8)
        .map(|i| format!("[{i:#x}] retail {} port {}", retail[i], port[i]))
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    for (i, r) in retail.iter().enumerate().take(CURVATURE_REACHABLE + 1) {
        assert_eq!(
            i32::from(*r),
            curvature_closed_form(i),
            "closed form [{i:#x}]"
        );
    }

    // Rebuilt inside a frame: OFY 114, every entry short of the saturated
    // tail the bias lower.
    let lifted = build(0x28, 0x2A_B980, 114).unwrap();
    for i in 0..=CURVATURE_REACHABLE {
        assert_eq!(lifted[i], port[i] + REBUILD_BIAS, "rebuilt [{i:#x}]");
    }
    // The map display's flat build: no drop, the bias alone.
    let flat = build(0, 0, 114).unwrap();
    assert!(flat.iter().all(|v| *v == REBUILD_BIAS), "flat table");
}
