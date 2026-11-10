//! The SCUS colour-space pair the move VM's HSV ramp (`0x2F` sub-ops
//! `0x1F` / `0x20`) and the CLUT-cell cycler go through: RGB to HSV
//! `FUN_8001A78C` against `rgb_to_hsv`, HSV to RGB `FUN_8001A8DC` against
//! `hsv_to_rgb`, and its capping wrapper `FUN_8001A6C8` against the same with
//! the `0xF8` cap, over the byte cube and hue, saturation and value past
//! their ranges.

use crate::common::{Rng, scus_cpu};
use legaia_engine_vm::move_vm::{hsv_to_rgb, rgb_to_hsv};
use psx_retail_exec::{Cpu, Stop};

const RGB_TO_HSV: u32 = 0x8001_A78C;
const HSV_TO_RGB: u32 = 0x8001_A8DC;
const HSV_TO_RGB_CAPPED: u32 = 0x8001_A6C8;
const OUT: u32 = 0x8014_0000;
/// `Cpu::call`'s stack pointer at entry.
const STACK_TOP: u32 = 0x801F_FF00;

/// Room for the stack arguments past `a3`.
fn stack_args(cpu: &mut Cpu, words: &[u32]) {
    let sp = STACK_TOP;
    for (i, w) in words.iter().enumerate() {
        cpu.mem.wr32(sp + 0x10 + i as u32 * 4, *w);
    }
}

fn out3(cpu: &mut Cpu) -> (i32, i32, i32) {
    (
        cpu.mem.rd32(OUT) as i32,
        cpu.mem.rd32(OUT + 4) as i32,
        cpu.mem.rd32(OUT + 8) as i32,
    )
}

#[test]
fn rgb_to_hsv_matches_retail() {
    differential!(scus(RGB_TO_HSV));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4853_5631);
    let mut grey = 0;
    for _ in 0..100_000 {
        let pick = |rng: &mut Rng| match rng.below(4) {
            0 => [0, 1, 0xF8, 0xFF][rng.below(4) as usize],
            _ => rng.below(256) as i32,
        };
        let (r, g, b) = (pick(&mut rng), pick(&mut rng), pick(&mut rng));
        let (r, g, b) = if rng.below(10) == 0 {
            (r, r, r)
        } else {
            (r, g, b)
        };
        // `*h = a3`, `*s = sp[0x10]`, `*v = sp[0x14]`.
        stack_args(&mut cpu, &[OUT + 4, OUT + 8]);
        for i in 0..3 {
            cpu.mem.wr32(OUT + i * 4, 0xDEAD_BEEF);
        }
        let stop = cpu.call(RGB_TO_HSV, &[r as u32, g as u32, b as u32, OUT]);
        assert_eq!(stop, Stop::Returned);
        assert_eq!(out3(&mut cpu), rgb_to_hsv(r, g, b), "rgb ({r}, {g}, {b})");
        grey += u32::from(r == g && g == b);
    }
    assert!(grey > 5000);
}

#[test]
fn hsv_to_rgb_matches_retail() {
    differential!(scus(HSV_TO_RGB), scus(HSV_TO_RGB_CAPPED));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4853_5632);
    let mut capped = 0;
    for _ in 0..100_000 {
        let h = match rng.below(4) {
            0 => rng.below(0x168) as i32,
            1 => rng.below(0x168 * 4) as i32 - 0x168 * 2,
            2 => [0, 59, 60, 0x167, 0x168, -1, -0x168][rng.below(7) as usize],
            _ => rng.below(0x168) as i32,
        };
        let s = match rng.below(3) {
            0 => rng.below(0x140) as i32 - 0x20,
            _ => rng.below(0x100) as i32,
        };
        let v = match rng.below(3) {
            0 => rng.below(0x140) as i32 - 0x20,
            _ => rng.below(0x100) as i32,
        };
        let port = hsv_to_rgb(h, s, v);
        for i in 0..3 {
            cpu.mem.wr32(OUT + i * 4, 0xDEAD_BEEF);
        }
        let stop = cpu.call(HSV_TO_RGB, &[OUT, h as u32, s as u32, v as u32]);
        assert_eq!(stop, Stop::Returned);
        assert_eq!(out3(&mut cpu), port, "hsv ({h}, {s}, {v})");

        // `(h, s, v, *r, *g = sp[0x10], *b = sp[0x14])`, each capped at `0xF8`.
        stack_args(&mut cpu, &[OUT + 4, OUT + 8]);
        let stop = cpu.call(HSV_TO_RGB_CAPPED, &[h as u32, s as u32, v as u32, OUT]);
        assert_eq!(stop, Stop::Returned);
        let cap = (port.0.min(0xF8), port.1.min(0xF8), port.2.min(0xF8));
        assert_eq!(out3(&mut cpu), cap, "capped hsv ({h}, {s}, {v})");
        capped += u32::from(cap != port);
    }
    assert!(capped > 5000, "{capped}");
}
