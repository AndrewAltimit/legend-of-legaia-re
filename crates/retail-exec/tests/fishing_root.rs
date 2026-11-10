//! The SCUS square root the fishing lure's record rests on, `FUN_8005AF0C`,
//! and its hyperbolic rotation `FUN_8005ADB8`, against
//! `fishing_actors::{scus_sqrt, hyperbolic_root}` over the whole range: zero,
//! powers of two and their neighbours, the squares a lure's `dx * dx + dz *
//! dz` reaches, a sum that wrapped negative, and anything. The root's normaliser reads the
//! leading-bit count off the GTE (`LZCS` / `LZCR`, `FUN_8005BA1C`), which the
//! interpreter runs.

use crate::common::{Rng, scus_cpu};
use legaia_engine_fishing::fishing_actors::{hyperbolic_root, scus_sqrt};
use psx_retail_exec::{Stop, V0};

const ROOT: u32 = 0x8005_AF0C;
const ROTATE: u32 = 0x8005_ADB8;

#[test]
fn the_root_matches_retail() {
    differential!(scus(ROOT), scus(ROTATE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5351_5254);
    let mut inputs: Vec<i32> = vec![0, 1, 2, 3, 4, 0x7FFF_FFFF];
    for b in 0..31 {
        let p = 1i32 << b;
        inputs.extend([p - 1, p, p + 1]);
    }
    for _ in 0..200_000 {
        let n = match rng.below(4) {
            0 => {
                let (dx, dz) = (
                    rng.below(0x2000) as i32 - 0x1000,
                    rng.below(0x2000) as i32 - 0x1000,
                );
                dx * dx + dz * dz
            }
            1 => rng.below(0x1_0000) as i32,
            // A sum of squares that wrapped: the normaliser counts a negative
            // input's leading ones.
            2 => rng.next() as i32 | i32::MIN,
            _ => (rng.next() >> 1) as i32,
        };
        inputs.push(n);
    }
    let mut fails = Vec::new();
    for &n in &inputs {
        assert_eq!(cpu.call(ROOT, &[n as u32]), Stop::Returned);
        let r = cpu.regs.r[V0] as i32;
        let p = scus_sqrt(n);
        if r != p && fails.len() < 8 {
            fails.push(format!("root {n:#x}: retail {r} port {p}"));
        }
    }
    // The rotation on its own, over the normalised range the root hands it
    // (`0x40_0000..0x100_0000`) and past it.
    for _ in 0..200_000 {
        let arg = match rng.below(3) {
            0 => 0x40_0000 + rng.below(0xC0_0000) as i32,
            1 => rng.below(0x40_0000) as i32,
            _ => rng.next() as i32,
        };
        assert_eq!(cpu.call(ROTATE, &[arg as u32]), Stop::Returned);
        let r = cpu.regs.r[V0] as i32;
        let p = hyperbolic_root(arg);
        if r != p && fails.len() < 8 {
            fails.push(format!("rotate {arg:#x}: retail {r} port {p}"));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}
