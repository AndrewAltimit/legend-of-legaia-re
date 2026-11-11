//! The dance overlay's entry `FUN_801CEF54` (PROT 0980), whole, against the
//! record `dance_scene_entry` carries: every call it makes runs stubbed
//! (`Cpu::stub_outside` over the overlay's band), and the calls the record
//! describes are hooked and logged - the display width and ordering-table
//! depth, the two buffer sizes, the venue block and audio bank it streams, the
//! actor it spawns and the five face stamps with the mode global in force at
//! each. The stores it makes itself are read back: the scene block base, the
//! dancer's spawn position, the camera pair and angles, GTE `H`, the visible
//! tile window and the per-dancer slots it clears.

use crate::common::scus_cpu;
use legaia_engine_minigames::dance::dance_scene_entry;
use legaia_minigame_formats::minigame_slot_scene::SIN_TABLE_VA;
use psx_retail_exec::{A0, A1, A2, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const ENTRY: u32 = 0x801C_EF54;
const ACTOR: u32 = 0x8014_0000;
const WORK: u32 = 0x8015_0000;
const SIN_PTR: u32 = 0x8007_B81C;
const MODE: u32 = 0x801D_514C;
/// The four parallel per-dancer arrays the entry clears.
const DANCER_ARRAYS: [u32; 4] = [0x801D_544C, 0x801D_53CC, 0x801D_578C, 0x801D_57CC];

#[derive(Debug, Default)]
struct Log {
    width: Vec<u32>,
    ot: Vec<u32>,
    alloc: Vec<u32>,
    prims: Vec<u32>,
    venue: Vec<u32>,
    bank: Vec<u32>,
    spawn: Vec<u32>,
    stamps: Vec<(u8, u8)>,
    stamp_mode: Vec<u32>,
}

#[test]
fn the_dance_entry_stages_the_ports_record() {
    differential!(prot(980, ENTRY));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(980).expect("PROT 0980");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    cpu.mem.wr32(SIN_PTR, SIN_TABLE_VA);
    // Junk where the entry stores, so a store the record does not carry shows.
    for a in DANCER_ARRAYS {
        for i in 0..4 {
            cpu.mem.wr32(a + 4 * i, 0xA5A5_A5A5);
        }
    }
    cpu.mem.load(ACTOR, &[0xA5; 0x100]);
    cpu.stub_outside = Some((ENTRY, ENTRY + 0x51C));
    let log = Rc::new(RefCell::new(Log::default()));
    let hook = |cpu: &mut Cpu, va: u32, f: fn(&mut Log, &psx_retail_exec::Regs, u32), ret: u32| {
        let l = log.clone();
        cpu.hook(va, move |r, m| {
            let mode = m.rd32(MODE);
            f(&mut l.borrow_mut(), r, mode);
            r.r[V0] = ret;
        });
    };
    hook(&mut cpu, 0x8001_DAF8, |l, r, _| l.width.push(r.r[A0]), 0);
    hook(&mut cpu, 0x8001_DCF8, |l, r, _| l.ot.push(r.r[A0]), 0);
    hook(&mut cpu, 0x8001_7888, |l, r, _| l.alloc.push(r.r[A1]), WORK);
    hook(&mut cpu, 0x8001_E3B8, |l, r, _| l.prims.push(r.r[A0]), 0);
    hook(&mut cpu, 0x8001_F7C0, |l, r, _| l.venue.push(r.r[A2]), 0);
    hook(&mut cpu, 0x8001_FC00, |l, r, _| l.bank.push(r.r[A0]), 0);
    hook(
        &mut cpu,
        0x8002_0DE0,
        |l, r, _| l.spawn.push(r.r[A0]),
        ACTOR,
    );
    hook(
        &mut cpu,
        0x801D_03C4,
        |l, r, mode| {
            l.stamps.push((r.r[A0] as u8, r.r[A1] as u8));
            l.stamp_mode.push(mode);
        },
        0,
    );
    // The streaming flag up: the retail CD arm, no dev path build.
    cpu.mem.wr16(0x8007_B8C2, 1);
    assert_eq!(cpu.call(ENTRY, &[]), Stop::Returned);

    let want = dance_scene_entry();
    let log = log.borrow();
    assert_eq!(log.width, [u32::from(want.screen_width)], "display width");
    assert_eq!(log.ot, [u32::from(want.ot_depth)], "ordering-table depth");
    assert_eq!(log.alloc, [want.work_buffer_bytes], "work buffer");
    assert_eq!(log.prims, [want.prim_buffer_bytes], "primitive buffer");
    assert_eq!(
        (log.venue.as_slice(), log.bank.as_slice()),
        (
            [want.stream_ids.0].as_slice(),
            [want.stream_ids.1].as_slice()
        ),
        "stream ids"
    );
    assert_eq!(log.spawn, [0x801D_42E4], "the beat clock's template");
    assert_eq!(log.stamps, want.face_stamps, "face stamps");
    assert_eq!(log.stamp_mode, want.face_stamp_mode, "mode at each stamp");

    assert_eq!(
        cpu.mem.rd32(0x8008_4540),
        u32::from(want.scene_block_base),
        "block base"
    );
    let spawn = (
        cpu.mem.rd16(ACTOR + 0x14) as i16,
        cpu.mem.rd16(ACTOR + 0x16) as i16,
        cpu.mem.rd16(ACTOR + 0x18) as i16,
    );
    assert_eq!(spawn, want.dancer_spawn, "dancer spawn");
    assert_eq!(cpu.mem.rd32(0x8008_40B8), 0, "camera block +0");
    assert_eq!(
        (cpu.mem.rd32(0x8008_40BC), cpu.mem.rd32(0x8008_40C0)),
        want.camera_pair,
        "camera pair"
    );
    assert_eq!(
        (
            cpu.mem.rd16(0x8007_B790),
            cpu.mem.rd16(0x8007_B792),
            cpu.mem.rd16(0x8007_B794)
        ),
        want.camera_angles,
        "camera angles"
    );
    assert_eq!(cpu.mem.rd16(0x8007_B6F4), want.gte_h, "GTE H");
    let window = (
        cpu.mem.rd8(0x1F80_03E8) as i8,
        cpu.mem.rd8(0x1F80_03E9) as i8,
        cpu.mem.rd8(0x1F80_03EA) as i8,
        cpu.mem.rd8(0x1F80_03EB) as i8,
    );
    assert_eq!(window, want.view_window, "visible tile window");
    for a in DANCER_ARRAYS {
        let cleared = (0..4).take_while(|i| cpu.mem.rd32(a + 4 * i) == 0).count();
        assert_eq!(cleared, want.cleared_dancer_slots, "dancer slots at {a:#x}");
        assert_eq!(
            cpu.mem.rd32(a + 12),
            0xA5A5_A5A5,
            "slot past the floor at {a:#x}"
        );
    }
}
