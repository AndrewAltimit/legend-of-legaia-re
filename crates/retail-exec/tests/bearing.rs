//! The 12-bit bearing `FUN_80019B28` against `bearing_12bit_approx`, the form
//! every battle consumer calls: points across the whole `i16` plane, on the
//! axes and diagonals, and at the short distances a battle stage reaches.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_action::{
    ARCTAN_LUT_LEN, approx_arctan_lut, bearing_12bit_approx,
};
use legaia_retail_exec::{Stop, V0};

const BEARING: u32 = 0x8001_9B28;
const ATAN_LUT: u32 = 0x8006_F4C8;

#[test]
fn the_port_arctan_table_is_the_retail_one() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let port = approx_arctan_lut();
    let retail: Vec<i16> = (0..ARCTAN_LUT_LEN as u32)
        .map(|i| cpu.mem.rd16(ATAN_LUT + 2 * i) as i16)
        .collect();
    let off: Vec<String> = (0..ARCTAN_LUT_LEN)
        .filter(|&i| retail[i] != port[i])
        .map(|i| format!("[{i:#x}] retail {} port {}", retail[i], port[i]))
        .collect();
    assert!(
        off.is_empty(),
        "{} entries: {:?}",
        off.len(),
        &off[..off.len().min(8)]
    );
}

#[test]
fn the_bearing_is_the_retail_bearing() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4245_4152);
    let mut cases: Vec<[i16; 4]> = Vec::new();
    for _ in 0..20_000 {
        let span = [0x7FFFu32, 0x1000, 0x200, 0x10][rng.below(4) as usize];
        let mut c = || (rng.below(2 * span + 1) as i32 - span as i32) as i16;
        let p = [c(), c(), c(), c()];
        cases.push(p);
    }
    for d in [-3i16, -1, 0, 1, 3, 100] {
        for e in [-3i16, -1, 0, 1, 3, 100] {
            cases.push([0, 0, d, e]);
        }
    }
    let mut bad = Vec::new();
    for p in &cases {
        // a0..a3 = p1z, p1x, p2z, p2x, each sign-extended as a halfword arg.
        let args: Vec<u32> = p.iter().map(|&v| v as i32 as u32).collect();
        assert_eq!(cpu.call(BEARING, &args), Stop::Returned, "{p:?}");
        let retail = cpu.regs.r[V0] & 0xFFF;
        let port = u32::from(bearing_12bit_approx(p[0], p[1], p[2], p[3]));
        if retail != port {
            bad.push(format!("{p:?}: retail {retail:#x} port {port:#x}"));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {}: {:?}",
        bad.len(),
        cases.len(),
        &bad[..bad.len().min(8)]
    );
}
