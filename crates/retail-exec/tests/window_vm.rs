//! The menu overlay's window-widget VM `FUN_801D6628` (PROT 0899) and the
//! SCUS window-list helpers it calls, run live: lookup `FUN_80035334`, create
//! `FUN_800326AC`, slide `FUN_800357FC`, snap `FUN_800358C0`, close
//! `FUN_80035978`, close-all `FUN_80035A4C` and destroy `FUN_800319A8`,
//! against `MenuWidgetState` driven by `legaia_engine_vm::run`.
//!
//! The heap is a bump allocator behind `FUN_80017888` with `FUN_80017B94` a
//! no-op, and the content builder `FUN_80030628` and pen set `FUN_8003C110`
//! return at once; everything else is the bytes. After every program the
//! whole list is compared node by node in list order: id, live position,
//! style byte, motion word, its progress, and the motion sub-object's
//! duration, source and target. Between programs a stand-in for the window
//! walker moves positions and progress on both sides alike, so a close
//! reverses a slide part-way and a slide starts from somewhere other than
//! its park. Programs are every one the disc's `jal`-site scan recovers,
//! chained at random, then generated ones over every opcode; the screen box
//! the park edges read is the menu's and random ones. The walker's motion
//! pass `FUN_80031AE4` runs against `MenuWidgetState::step_motion` the same
//! way, the latch `_DAT_8007BB80` it leaves compared after every pass.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::menu_widget::{
    MenuWidgetScripts, MenuWidgetState, ScreenBox, WIDGET_MOTION_FRAMES, WidgetMotion,
};
use legaia_engine_vm::{self as vm, Host};
use psx_retail_exec::{A1, Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const VM: u32 = 0x801D_6628;
const LOOKUP: u32 = 0x8003_5334;
const CREATE: u32 = 0x8003_26AC;
const SLIDE: u32 = 0x8003_57FC;
const SNAP: u32 = 0x8003_58C0;
const CLOSE: u32 = 0x8003_5978;
const CLOSE_ALL: u32 = 0x8003_5A4C;
const DESTROY: u32 = 0x8003_19A8;
const MOTION: u32 = 0x8003_1AE4;
/// `_DAT_8007BB80` (`gp + 0x868`), the walker's slide latch.
const LATCH: u32 = 0x8007_BB80;
/// `DAT_1F800393`, the frame delta the walker adds.
const FRAME_DELTA: u32 = 0x1F80_0393;
const ALLOC: u32 = 0x8001_7888;
const FREE: u32 = 0x8001_7B94;
const CONTENT: u32 = 0x8003_0628;
const PEN: u32 = 0x8003_C110;
const TABLE: u32 = 0x801E_4738;
/// `gp + 0x148`.
const HEAD: u32 = 0x8007_B460;
const SCREEN: u32 = 0x1F80_0388;
const PROGRAM: u32 = 0x8014_0000;
const HEAP: u32 = 0x8015_0000;

/// One node as both sides hold it: id, pos, style, motion, progress, and the
/// sub-object's duration, source and target.
type Node = (u16, (i16, i16), u8, i16, i16, i16, (i16, i16), (i16, i16));

struct Rig {
    cpu: Cpu,
    port: MenuWidgetState,
    overlay: Vec<u8>,
}

fn rig() -> Option<Rig> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    let overlay = d.read_entry_footprint(899).expect("PROT 0899");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let next = Rc::new(Cell::new(HEAP));
    let n = next.clone();
    cpu.hook(ALLOC, move |r, m| {
        let size = (r.r[A1] + 3) & !3;
        let at = n.get();
        for i in (0..size).step_by(4) {
            m.wr32(at + i, 0xCDCD_CDCD);
        }
        n.set(at + size);
        r.r[V0] = at;
    });
    cpu.stub(FREE, 0);
    cpu.stub(CONTENT, 0);
    cpu.stub(PEN, 0);
    let mut port = MenuWidgetState::default();
    port.set_defaults_from_table(
        &legaia_game_tables::menu_windows::parse(&overlay).expect("window table"),
    );
    Some(Rig { cpu, port, overlay })
}

impl Rig {
    fn seat_screen(&mut self, s: ScreenBox) {
        for (i, v) in [s.left, s.top, s.right, s.bottom].into_iter().enumerate() {
            self.cpu.mem.wr16(SCREEN + 2 * i as u32, v);
        }
        self.port.screen = s;
    }

    fn clear(&mut self) {
        self.cpu.mem.wr32(HEAD, 0);
        self.port.reset();
    }

    fn retail_nodes(&mut self) -> Vec<(u32, Node)> {
        let m = &mut self.cpu.mem;
        let head = m.rd32(HEAD);
        let mut out = Vec::new();
        if head == 0 {
            return out;
        }
        let mut n = m.rd32(head);
        while n != head {
            assert!(out.len() < 64, "list loops");
            let sub = m.rd32(n + 0x24);
            let pair =
                |m: &mut psx_retail_exec::Memory, a: u32| (m.rd16(a) as i16, m.rd16(a + 2) as i16);
            out.push((
                n,
                (
                    m.rd16(n + 8),
                    pair(m, n + 0xA),
                    m.rd8(n + 0x1D),
                    m.rd16(n + 0x20) as i16,
                    m.rd16(n + 0x1E) as i16,
                    m.rd16(sub) as i16,
                    pair(m, sub + 6),
                    pair(m, sub + 0xA),
                ),
            ));
            n = m.rd32(n);
        }
        out
    }

    fn port_nodes(&self) -> Vec<Node> {
        self.port
            .windows()
            .map(|(id, w)| {
                let motion = match w.motion {
                    WidgetMotion::Still => 0,
                    WidgetMotion::Sliding => 1,
                    WidgetMotion::Closing => -1,
                };
                (
                    u16::from(id),
                    (w.pos.x, w.pos.y),
                    w.style,
                    motion,
                    w.progress,
                    WIDGET_MOTION_FRAMES,
                    (w.from.x, w.from.y),
                    (w.target.x, w.target.y),
                )
            })
            .collect()
    }

    fn compare(&mut self, what: &str) {
        let retail: Vec<Node> = self.retail_nodes().into_iter().map(|(_, n)| n).collect();
        assert_eq!(retail, self.port_nodes(), "{what}");
    }

    fn run(&mut self, program: &[u8]) {
        self.cpu.mem.load(PROGRAM, program);
        assert_eq!(self.cpu.call(VM, &[PROGRAM]), Stop::Returned);
        let end = vm::run(&mut self.port, program).expect("terminated");
        let off = self.cpu.regs.r[V0].wrapping_sub(PROGRAM) as usize;
        assert_eq!(off, end, "the VM returns its end pointer");
    }

    /// The window walker's part, done on both sides alike: each node's live
    /// position, progress and motion word moved somewhere it can stand.
    fn walk(&mut self, rng: &mut Rng) {
        for (addr, node) in self.retail_nodes() {
            if rng.below(2) == 0 {
                continue;
            }
            let id = node.0 as u8;
            let pos = vm::Position::new(
                rng.below(0x200) as i16 - 0x80,
                rng.below(0x180) as i16 - 0x60,
            );
            let progress = rng.below(13) as i16;
            let motion = [
                WidgetMotion::Still,
                WidgetMotion::Sliding,
                WidgetMotion::Closing,
            ][rng.below(3) as usize];
            let m = &mut self.cpu.mem;
            m.wr16(addr + 0xA, pos.x as u16);
            m.wr16(addr + 0xC, pos.y as u16);
            m.wr16(addr + 0x1E, progress as u16);
            m.wr16(
                addr + 0x20,
                match motion {
                    WidgetMotion::Still => 0,
                    WidgetMotion::Sliding => 1,
                    WidgetMotion::Closing => 0xFFFF,
                },
            );
            let w = self.port.window_mut(id).expect("same list");
            w.pos = pos;
            w.progress = progress;
            w.motion = motion;
        }
    }
}

fn screens(rng: &mut Rng) -> ScreenBox {
    if rng.below(2) == 0 {
        ScreenBox::default()
    } else {
        ScreenBox {
            left: rng.below(0x40) as u16,
            top: rng.below(0x40) as u16,
            right: 0x100 + rng.below(0x80) as u16,
            bottom: 0xC0 + rng.below(0x60) as u16,
        }
    }
}

#[test]
fn the_disc_programs_drive_the_list_retail_drives() {
    differential!(prot(899, VM));
    let Some(mut r) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let programs = MenuWidgetScripts::resolve_from_overlay(&r.overlay)
        .expect("programs")
        .programs;
    assert!(programs.len() >= 30, "{}", programs.len());
    let mut rng = Rng(0x5749_4E44);
    let (mut closes, mut parks, mut diagonal) = (0, 0, 0);
    for chain in 0..400 {
        let screen = screens(&mut rng);
        r.seat_screen(screen);
        r.clear();
        for step in 0..12 {
            let (va, bytes) = &programs[rng.below(programs.len() as u32) as usize];
            r.run(bytes);
            r.compare(&format!("chain {chain} step {step} program {va:#x}"));
            let nodes = r.port_nodes();
            closes += nodes.iter().filter(|n| n.3 == -1).count();
            parks += nodes.iter().filter(|n| n.1 != n.7).count();
            diagonal += nodes.iter().filter(|n| n.4 != 0).count();
            r.walk(&mut rng);
        }
    }
    assert!(
        closes > 500 && parks > 500 && diagonal > 200,
        "{closes} {parks} {diagonal}"
    );
}

#[test]
fn generated_programs_drive_the_list_retail_drives() {
    differential!(prot(899, VM));
    let Some(mut r) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4F50_5321);
    for chain in 0..300 {
        r.seat_screen(screens(&mut rng));
        r.clear();
        for step in 0..10 {
            let mut prog = Vec::new();
            for _ in 0..rng.below(8) {
                let op = 1 + rng.below(0xE) as u8;
                let id = [rng.below(52), rng.below(6)][rng.below(2) as usize] as u8;
                let w = if rng.below(4) == 0 {
                    0
                } else {
                    rng.next() as u16
                };
                prog.extend_from_slice(&[op, id, w as u8, (w >> 8) as u8]);
            }
            prog.extend_from_slice(&[0; 4]);
            r.run(&prog);
            r.compare(&format!("chain {chain} step {step} {prog:02x?}"));
            r.walk(&mut rng);
        }
    }
}

/// Each helper called on its own over lists the VM built.
#[test]
fn the_window_helpers_match_their_ports() {
    differential!(
        scus(LOOKUP),
        scus(CREATE),
        scus(SLIDE),
        scus(SNAP),
        scus(CLOSE),
        scus(CLOSE_ALL),
        scus(DESTROY)
    );
    let Some(mut r) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4845_4C50);
    let mut hits = [0u32; 7];
    for chain in 0..300 {
        r.seat_screen(screens(&mut rng));
        r.clear();
        for step in 0..40 {
            let id = rng.below(52) as u8;
            let (x, y) = (
                rng.below(0x200) as i16 - 0x80,
                rng.below(0x180) as i16 - 0x60,
            );
            let pos = vm::Position::new(x, y);
            let k = rng.below(7) as usize;
            let what = format!("chain {chain} step {step} helper {k} id {id}");
            let args = [u32::from(id), x as i32 as u32, y as i32 as u32];
            match k {
                0 => {
                    assert_eq!(r.cpu.call(LOOKUP, &[u32::from(id)]), Stop::Returned);
                    assert_eq!(r.cpu.regs.r[V0] != 0, r.port.actor_exists(id), "{what}");
                }
                1 => {
                    let desc = TABLE + u32::from(id) * 16;
                    assert_eq!(r.cpu.call(CREATE, &[u32::from(id), desc]), Stop::Returned);
                    r.port.spawn(id, r.port.default_position(id));
                }
                2 => {
                    assert_eq!(r.cpu.call(SLIDE, &args), Stop::Returned);
                    r.port.slide_to(id, pos);
                }
                3 => {
                    assert_eq!(r.cpu.call(SNAP, &args), Stop::Returned);
                    r.port.snap_to(id, pos);
                }
                4 => {
                    assert_eq!(r.cpu.call(CLOSE, &args[..1]), Stop::Returned);
                    r.port.begin_close(id);
                }
                5 => {
                    if rng.below(4) != 0 {
                        continue;
                    }
                    assert_eq!(r.cpu.call(CLOSE_ALL, &[]), Stop::Returned);
                    r.port.close_all();
                }
                _ => {
                    assert_eq!(r.cpu.call(DESTROY, &args[..1]), Stop::Returned);
                    r.port.destroy(id);
                }
            }
            hits[k] += 1;
            r.compare(&what);
            if rng.below(3) == 0 {
                r.walk(&mut rng);
            }
        }
    }
    assert!(hits.iter().all(|&h| h > 300), "{hits:?}");
}

/// The motion walker `FUN_80031AE4` frame by frame over lists the disc's
/// programs built, programs landing between passes so closes catch slides
/// part-way; the latch it leaves (`_DAT_8007BB80`) and every node after each
/// pass. The draw walker destroys a node whose close completed (`-2`) in
/// the same frame, so the retail side does that too before comparing.
#[test]
fn the_motion_walker_matches_its_port() {
    differential!(scus(MOTION));
    let Some(mut r) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let programs = MenuWidgetScripts::resolve_from_overlay(&r.overlay)
        .expect("programs")
        .programs;
    let mut rng = Rng(0x4D4F_5645);
    let (mut moved, mut idle, mut landed) = (0u32, 0u32, 0u32);
    for chain in 0..300 {
        r.seat_screen(screens(&mut rng));
        r.clear();
        for step in 0..60 {
            if rng.below(8) == 0 {
                let (va, bytes) = &programs[rng.below(programs.len() as u32) as usize];
                r.run(bytes);
                r.compare(&format!("chain {chain} step {step} program {va:#x}"));
            }
            let delta = [1u8, 1, 1, 2, 3][rng.below(5) as usize];
            r.cpu.mem.wr8(FRAME_DELTA, delta);
            assert_eq!(r.cpu.call(MOTION, &[]), Stop::Returned);
            for (addr, node) in r.retail_nodes() {
                if node.3 == -2 {
                    let id = u32::from(r.cpu.mem.rd16(addr + 8));
                    assert_eq!(r.cpu.call(DESTROY, &[id]), Stop::Returned);
                    landed += 1;
                }
            }
            let latch = r.cpu.mem.rd32(LATCH);
            let what = format!("chain {chain} step {step} delta {delta}");
            assert_eq!(latch, r.port.step_motion(delta), "{what} latch");
            assert_eq!(latch, r.port.slide_latch(), "{what}");
            r.compare(&what);
            if latch == 0 {
                idle += 1;
            } else {
                moved += 1;
            }
        }
    }
    assert!(
        moved > 2000 && idle > 2000 && landed > 500,
        "{moved} {idle} {landed}"
    );
}

/// A sub-screen's first frame runs its program; every window it moves
/// slides for the twelve-frame duration, so the latch the next eleven
/// frames read stays up and the twelfth reads zero - the frame the screen
/// takes the pad.
#[test]
fn a_fresh_slide_holds_the_latch_eleven_passes() {
    differential!(scus(MOTION));
    let Some(mut r) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    r.seat_screen(ScreenBox::default());
    r.clear();
    // The root picker's program `0x801E4BC0`.
    let root = legaia_game_tables::widget_script::script_bytes_at(&r.overlay, 0x801E_4BC0)
        .expect("root program");
    r.run(&root);
    r.cpu.mem.wr8(FRAME_DELTA, 1);
    let mut latches = Vec::new();
    for _ in 0..13 {
        assert_eq!(r.cpu.call(MOTION, &[]), Stop::Returned);
        latches.push(r.cpu.mem.rd32(LATCH));
        r.port.step_motion(1);
    }
    assert!(latches[..11].iter().all(|&l| l == 3), "{latches:?}");
    assert_eq!(&latches[11..], &[0, 0]);
}
