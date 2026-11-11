//! The cursor sprite primitive `FUN_8002B994(kind, mode, x, y)`, run live,
//! against `engine-dialog`'s `CursorSprites`: the frame it draws and the idle
//! bob it adds, read off the one `POLY_FT4` it links.
//!
//! Mode `1` is the animated call the port models. Mode `0` and a kind past
//! the table are held to what the disassembly says of them: frame zero,
//! unmoved, no state touched; and nothing at all.

use crate::common::{Rng, scus_cpu};
use legaia_engine_dialog::cursor_sprite::{CURSOR_RECORDS, CursorPose, CursorSprites};
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const CURSOR_SPRITE: u32 = 0x8002_B994;
const ADD_PRIM: u32 = 0x8003_D2C4;
const PACKET_CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8010_0000;
const OT: u32 = 0x8018_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
const FRAMES: u32 = 0x801C_6000;
const TIMERS: u32 = 0x801C_6010;
const RECORDS: u32 = 0x8007_3D18;

#[test]
fn the_cursor_sprite_animates_as_retail_animates_it() {
    differential!(scus(CURSOR_SPRITE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let linked: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let l = linked.clone();
    cpu.hook(ADD_PRIM, move |r, _| {
        l.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    cpu.mem.wr32(OT_BASE, OT);
    // The port's table is the executable's.
    for (k, rec) in CURSOR_RECORDS.iter().enumerate() {
        let at = RECORDS + k as u32 * 0x18;
        assert_eq!(cpu.mem.rd8(at), rec.frames, "kind {k}: frames");
        assert_eq!(cpu.mem.rd16(at + 2) as i16, rec.period, "kind {k}: period");
        for f in 0..usize::from(rec.frames) {
            let uv = (
                cpu.mem.rd16(at + 8 + f as u32 * 4) as u8,
                cpu.mem.rd16(at + 10 + f as u32 * 4) as u8,
            );
            assert_eq!(uv, rec.uv[f], "kind {k} frame {f}");
        }
    }
    let mut port = CursorSprites::default();
    let mut rng = Rng(0x4355_5253);
    // Each kind sits at a spot for a while, then moves.
    let mut at = [(0xD2i16, 0x4Ai16); 4];
    let (mut bobbed, mut flipped, mut moved, mut still) = (0, 0, 0, 0);
    for call in 0..40_000 {
        let kind = rng.below(4) as usize;
        if rng.below(60) == 0 {
            at[kind] = (rng.below(300) as i16, rng.below(220) as i16);
            moved += 1;
        }
        let (x, y) = at[kind];
        let step = 1 + rng.below(3) as u8;
        cpu.mem.wr8(FRAME_STEP, step);
        cpu.mem.wr32(PACKET_CURSOR, PACKETS);
        linked.borrow_mut().clear();
        // One call in eight is the still draw, which leaves the state alone.
        let mode = if rng.below(8) == 0 { 0 } else { 1 };
        let before = (
            cpu.mem.rd32(FRAMES + kind as u32 * 4),
            cpu.mem.rd32(TIMERS + kind as u32 * 4),
        );
        assert_eq!(
            cpu.call(CURSOR_SPRITE, &[kind as u32, mode, x as u32, y as u32]),
            Stop::Returned
        );
        let pose = if mode == 1 {
            port.call(kind, x, y, step)
        } else {
            still += 1;
            let after = (
                cpu.mem.rd32(FRAMES + kind as u32 * 4),
                cpu.mem.rd32(TIMERS + kind as u32 * 4),
            );
            assert_eq!(before, after, "call {call}: mode 0 touches no state");
            CursorPose::default()
        };
        let what = format!("call {call}: kind {kind} mode {mode} at ({x}, {y}) step {step}");
        assert_eq!(*linked.borrow(), vec![(OT + 4, PACKETS)], "{what}");
        assert_eq!(cpu.mem.rd32(PACKET_CURSOR), PACKETS + 0x28, "{what}");
        let p = PACKETS;
        assert_eq!(cpu.mem.rd32(p + 4), 0x2E80_8080, "{what}: a POLY_FT4");
        assert_eq!(cpu.mem.rd16(p + 0x16), 0x1E, "{what}: the system-UI page");
        let xy = |o: u32, cpu: &mut psx_retail_exec::Cpu| {
            (cpu.mem.rd16(p + o) as i16, cpu.mem.rd16(p + o + 2) as i16)
        };
        let (x0, y0) = (x + pose.dx as i16, y + pose.dy as i16);
        assert_eq!(
            [
                xy(8, &mut cpu),
                xy(0x10, &mut cpu),
                xy(0x18, &mut cpu),
                xy(0x20, &mut cpu)
            ],
            [(x0, y0), (x0 + 16, y0), (x0, y0 + 16), (x0 + 16, y0 + 16)],
            "{what}: {pose:?}"
        );
        let uv = CURSOR_RECORDS[kind].uv[usize::from(pose.frame)];
        assert_eq!(
            (cpu.mem.rd8(p + 0xC), cpu.mem.rd8(p + 0xD)),
            uv,
            "{what}: frame {}",
            pose.frame
        );
        // The strip's far edge is fifteen texels across, sixteen down.
        assert_eq!(
            (cpu.mem.rd8(p + 0x24), cpu.mem.rd8(p + 0x25)),
            (uv.0.wrapping_add(15), uv.1.wrapping_add(16)),
            "{what}"
        );
        bobbed += u32::from(pose.dx != 0);
        flipped += u32::from(pose.frame != 0);
    }
    assert!(
        bobbed > 5000 && flipped > 1000 && moved > 300 && still > 3000,
        "{bobbed} {flipped} {moved} {still}"
    );
    // A kind past the table draws nothing.
    linked.borrow_mut().clear();
    assert_eq!(cpu.call(CURSOR_SPRITE, &[4, 1, 10, 10]), Stop::Returned);
    assert!(linked.borrow().is_empty());
}
