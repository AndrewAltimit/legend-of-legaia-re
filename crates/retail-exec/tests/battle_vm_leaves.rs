//! Three battle-band leaves of `SCUS_942.54` whose ports sit in
//! `engine-battle-vm`:
//!
//! - `FUN_80046898`, the out-of-battle gate arm `0x82` tails into, against
//!   `validator::item_count_gate` over every sign and the `0xE0` edge.
//! - `FUN_80046978`, the armed screen wash's drain, against
//!   `rgb_wash_scale::scale_rgb24`: the submit `FUN_80024EE4` must receive
//!   the scaled colour, ABR `2` and the farthest bucket, the trigger word must
//!   be cleared, and an unarmed drain must submit nothing.
//! - `FUN_80055854`, the two-level record copier, against
//!   `nested_record_copy::copy_nested_records`: the whole destination and the
//!   returned cursor, over empty, negative and filled counts.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_battle_vm::battle_action::item_count_gate;
use legaia_engine_battle_vm::nested_record_copy::copy_nested_records;
use legaia_engine_battle_vm::rgb_wash_scale::scale_rgb24;
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const GATE: u32 = 0x8004_6898;
const DRAIN: u32 = 0x8004_6978;
const COPY: u32 = 0x8005_5854;
const SUBMIT: u32 = 0x8002_4EE4;
/// `gp[0x2E8]`, `gp[0x9D0]` and `gp[0x9D4]` with `gp = 0x8007B318`.
const COOLDOWN: u32 = 0x8007_B600;
const WASH_RGB: u32 = 0x8007_BCE8;
const WASH_ARMED: u32 = 0x8007_BCEC;
const SCALE: u32 = 0x1F80_0393;
const OT_LEN: u32 = 0x1F80_03A6;

#[test]
fn the_cooldown_gate_is_retails() {
    differential!(scus(GATE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4741_5445);
    let mut seen = [0u32; 2];
    for n in 0..4_000 {
        let v = match n % 4 {
            0 => 0xDE + rng.below(4) as i32,
            1 => -(rng.below(0x1000) as i32),
            2 => rng.next() as i32,
            _ => rng.below(0x200) as i32,
        };
        cpu.mem.wr32(COOLDOWN, v as u32);
        assert_eq!(cpu.call(GATE, &[]), Stop::Returned);
        let retail = cpu.regs.r[V0] != 0;
        assert_eq!(retail, item_count_gate(v), "cooldown {v:#x}");
        seen[usize::from(retail)] += 1;
    }
    assert!(seen[0] > 100 && seen[1] > 100, "{seen:?}");
}

#[test]
fn the_wash_drain_submits_what_retail_submits() {
    differential!(scus(DRAIN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let got: Rc<RefCell<Vec<(u32, u32, u32)>>> = Rc::default();
    {
        let got = got.clone();
        cpu.hook(SUBMIT, move |r, _| {
            got.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]));
            r.r[V0] = 0;
        });
    }
    let mut rng = Rng(0x5741_5348);
    let (mut armed, mut clamped) = (0, 0);
    for n in 0..20_000 {
        let rgb = rng.next();
        let scale = match n % 3 {
            0 => 1,
            1 => rng.below(4) as u8,
            _ => rng.next() as u8,
        };
        let ot = 1 + rng.below(0x1000) as u16;
        let arm = if rng.below(4) == 0 { 0 } else { rng.next() | 1 };
        cpu.mem.wr32(WASH_RGB, rgb);
        cpu.mem.wr32(WASH_ARMED, arm);
        cpu.mem.wr8(SCALE, scale);
        cpu.mem.wr16(OT_LEN, ot);
        got.borrow_mut().clear();
        assert_eq!(cpu.call(DRAIN, &[]), Stop::Returned);
        let calls = got.borrow().clone();
        if arm == 0 {
            assert!(calls.is_empty(), "unarmed drain submitted: {calls:x?}");
            assert_eq!(cpu.mem.rd32(WASH_ARMED), 0);
            continue;
        }
        armed += 1;
        let want = scale_rgb24(rgb, scale);
        if want != (rgb & 0xFF_FFFF) {
            clamped += 1;
        }
        assert_eq!(
            calls,
            vec![(u32::from(ot) - 1, 2, want)],
            "rgb {rgb:#x} scale {scale}"
        );
        assert_eq!(cpu.mem.rd32(WASH_ARMED), 0, "trigger not cleared");
        assert_eq!(cpu.mem.rd32(WASH_RGB), rgb, "colour word moved");
    }
    assert!(armed > 10_000 && clamped > 5_000, "{armed} {clamped}");
}

#[test]
fn the_nested_copy_copies_what_retail_copies() {
    differential!(scus(COPY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const SRC: u32 = 0x8011_0000;
    const DST: u32 = 0x8012_0000;
    const LEN: usize = 0x800;
    let mut rng = Rng(0x4E45_5354);
    let (mut empty, mut filled) = (0, 0);
    for round in 0..3_000 {
        let mut src: Vec<u32> = vec![0; 3];
        src[0] = rng.next();
        src[1] = rng.next();
        let outer: i32 = match rng.below(5) {
            0 => 0,
            1 => -(1 + rng.below(4) as i32),
            _ => 1 + rng.below(6) as i32,
        };
        src[2] = outer as u32;
        for _ in 0..outer.max(0) {
            let inner: i32 = match rng.below(5) {
                0 => 0,
                1 => -(1 + rng.below(3) as i32),
                _ => 1 + rng.below(12) as i32,
            };
            src.extend([rng.next(), rng.next(), inner as u32]);
            src.extend((0..inner.max(0) * 2).map(|_| rng.next()));
        }
        if outer <= 0 {
            empty += 1;
        } else {
            filled += 1;
        }
        // Words past the structure, which neither side may read.
        let tail: Vec<u32> = (0..16).map(|_| rng.next()).collect();
        let mut src_bytes: Vec<u8> = src.iter().flat_map(|w| w.to_le_bytes()).collect();
        src_bytes.extend(tail.iter().flat_map(|w| w.to_le_bytes()));
        cpu.mem.load(SRC, &src_bytes);
        let junk: Vec<u8> = (0..LEN * 4).map(|_| rng.next() as u8).collect();
        cpu.mem.load(DST, &junk);
        assert_eq!(cpu.call(COPY, &[DST, SRC]), Stop::Returned);
        let retail_end = cpu.regs.r[V0];
        let retail = cpu.mem.read(DST, LEN * 4);

        let mut dst: Vec<u32> = junk
            .chunks(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let mut port_src = src.clone();
        port_src.extend(&tail);
        let n = copy_nested_records(&port_src, &mut dst);
        let port: Vec<u8> = dst.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(
            retail,
            port,
            "round {round}: {:?}",
            diff_offsets(&retail, &port)
        );
        assert_eq!(retail_end, DST + 4 * n as u32, "round {round}: cursor");
        assert_eq!(
            n,
            src.len(),
            "round {round}: the port copied past the structure"
        );
    }
    assert!(empty > 300 && filled > 1_000, "{empty} {filled}");
}
