//! The SCUS procedural mesh builder `FUN_80028158` off the disc - the render
//! dispatcher's default arm, the battle ground shadow and the transition's
//! spin-up ring all build through it - against
//! `effect_default_arm::build`, over every shape `0..7`, every texture mode,
//! column counts and phases on both sides of the arc rules, random parameter
//! blocks and the screen page, with `rand()` (`FUN_80056798`) scripted alike
//! on both sides. Compared: every byte the port reports writing in the
//! object from `out` to the end of its zero tail, and every byte it reports
//! not writing must hold the seeded pattern.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_effects::effect_default_arm::build;
use legaia_engine_effects::effect_ribbon::RetailTrig;
use psx_retail_exec::{Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BUILD: u32 = 0x8002_8158;
const RAND: u32 = 0x8005_6798;
const SCREEN_PAGE: u32 = 0x8007_B74C;
const OUT: u32 = 0x8012_0000;
const SRC: u32 = 0x8011_0000;
const REGION: usize = 0x4_0000;

#[test]
fn the_mesh_builder_matches_build() {
    differential!(scus(BUILD));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    // A zero column count divides by zero with no `break` guard
    // (`divu 0x1000, t8`); the R3000's quotient ships.
    cpu.hardware_zero_div = true;
    let stream: Rc<RefCell<Vec<u32>>> = Rc::default();
    let feed = stream.clone();
    cpu.hook(RAND, move |r, _| {
        r.r[V0] = feed.borrow_mut().pop().unwrap_or(0);
    });
    let mut rng = Rng(0x4D45_5348);
    let mut shapes = [0usize; 8];
    for round in 0..3000 {
        let shape = rng.below(8);
        let sub = match rng.below(6) {
            5 => 0,
            n => n,
        };
        let mode = shape << 3 | sub << 8 | rng.below(8);
        let count = match rng.below(4) {
            0 => rng.below(4),
            _ => 3 + rng.below(0x30),
        };
        let phase = match rng.below(3) {
            0 => 0,
            1 => rng.below(count.max(1) + 2),
            _ => rng.below(0x400),
        };
        let packed = count | phase << 8;
        let mut src = [0u8; 0x24];
        for b in src.iter_mut() {
            *b = rng.next() as u8;
        }
        // Radii, heights and the rectangle in the ranges the game uses.
        let put = |src: &mut [u8; 0x24], off: usize, v: i16| {
            src[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        put(&mut src, 0x18, rng.below(0x800) as i16);
        put(&mut src, 0x1A, rng.below(0x1000) as i16);
        put(&mut src, 0x1C, rng.below(0x400) as i16 - 0x200);
        put(&mut src, 0x1E, rng.below(0x400) as i16 - 0x200);
        let page = if rng.below(2) == 0 { 0 } else { 1 };
        cpu.mem.wr16(SCREEN_PAGE, page as u16);
        let draws: Vec<u32> = (0..0x400).map(|_| rng.below(0x8000)).collect();
        *stream.borrow_mut() = draws.iter().rev().copied().collect();
        // The vertex pads (`+6` of each 8-byte vertex from `out + 0x28`) are
        // zero: shapes 1 and 3 extrapolate vertex 6 lane by lane from the
        // plane's first component pointer, so on the planes that start at
        // `z` the middle lane reads two pads nothing wrote and stores into
        // vertex 6's pad. The port's buffer starts zeroed; RAM holds the last
        // frame's object. No reader takes a vertex pad.
        let pattern: Vec<u8> = (0..REGION)
            .map(|i| {
                if i >= 0x28 && (i - 0x28) % 8 >= 6 {
                    0
                } else {
                    (i as u8).wrapping_mul(0x5B) ^ 0xA5
                }
            })
            .collect();
        cpu.mem.load(OUT, &pattern);
        cpu.mem.load(SRC, &src);
        let stop = cpu.call(BUILD, &[OUT, mode, packed, SRC]);

        let mut port_draws = draws.iter().copied();
        let port = build(
            OUT,
            mode,
            packed,
            &src,
            &RetailTrig,
            || port_draws.next().unwrap_or(0),
            page,
        );
        let ctx = format!("round {round} mode {mode:#x} packed {packed:#x} src {src:02x?}");
        let Some(port) = port else {
            assert!(shape >= 8, "no build, {ctx}");
            continue;
        };
        assert_eq!(stop, Stop::Returned, "{ctx}");
        let got = cpu.mem.read(OUT, port.bytes.len());
        let want: Vec<u8> = port
            .bytes
            .iter()
            .zip(&port.written)
            .enumerate()
            .map(|(i, (b, w))| if *w { *b } else { pattern[i] })
            .collect();
        assert!(
            got == want,
            "object, {ctx}: {:?}",
            diff_offsets(&got, &want)
        );
        // Nothing past the zero tail.
        let tail = cpu.mem.read(OUT + port.bytes.len() as u32, 0x200);
        assert_eq!(
            tail,
            pattern[port.bytes.len()..port.bytes.len() + 0x200],
            "past the tail, {ctx}"
        );
        let used = 0x400 - stream.borrow().len();
        assert_eq!(used, draws.len() - port_draws.len(), "rand() draws, {ctx}");
        shapes[shape as usize] += 1;
    }
    assert!(shapes.iter().all(|n| *n > 200), "shapes {shapes:?}");
}
