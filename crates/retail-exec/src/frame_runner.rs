//! A field frame run off a PCSX-Redux save state, priced the way the
//! emulator prices it.
//!
//! The state's CPU is resumed where it was saved and runs the game on: the
//! master loop, the mode-`3` per-frame handler `0x80025EEC` (the field tick
//! `FUN_8001698C`, the master frame driver `FUN_80016444(1)`, the pacer
//! `FUN_80016B6C`), the kernel's syscalls, the `VSync` waits. Each listed
//! routine's inclusive cost is collected the way
//! `scripts/pcsx-redux/autorun_field_frame_profile.lua` collects it on the
//! emulator (entry to the return address it was called with), so a pass is
//! priced with the arguments, camera, lists and scratchpad retail hands it on
//! that frame, not with a seat a test builds.
//!
//! No interrupt is taken. On PCSX-Redux the vblank handler (the kernel's
//! dispatch, libetc's callbacks and the pad exchange over SIO) spends about
//! `11.6` hblanks a vblank, and a DMA or CD-ROM interrupt about one, inside
//! whatever routine was running; the profile reports that time per call
//! (`irq_cycles`) and the runner is held against the profile less it. The
//! vblank still counts ([`Cpu::vblank_counter`]), so the `VSync` waits end
//! where they end on the emulator.
//!
//! Behind the I/O window stand only the interrupt controller, root counter 1
//! and a GPU that is always ready (see [`crate::Memory`]): DMA registers read
//! as the state saved them (idle) and writes to them go nowhere, so an
//! ordering table handed to `DrawOTag` is never drawn - PCSX-Redux's scale,
//! where the GPU costs nothing. VRAM is kept as data only: the state's VRAM,
//! written by `LoadImage` and `MoveImage` and read back by `StoreImage` as
//! each is entered (each then runs on, priced), so a routine that captures
//! VRAM into RAM (the CLUT-cell cycler's capture) reads what retail reads.
//! Drawing never reaches it.

use crate::pcsx_state::PcsxState;
use crate::{A0, A1, A2, ClosedCall, Cpu, Memory, PCSX_CYCLES_PER_VBLANK, Spent, Stop};
use anyhow::{Result, bail};
use std::cell::RefCell;
use std::rc::Rc;

/// The mode table's per-frame handler for the field (`0x8007078C + 3 * 0x18
/// + 0x10`): one call is one game frame.
pub const FRAME_HANDLER: u32 = 0x8002_5EEC;

/// libgpu's `LoadImage(RECT *, u_long *)`, `StoreImage(RECT *, u_long *)`
/// and `MoveImage(RECT *, x, y)`.
const LOAD_IMAGE: u32 = 0x8005_83C8;
const STORE_IMAGE: u32 = 0x8005_842C;
const MOVE_IMAGE: u32 = 0x8005_8490;
const VRAM_W: usize = 1024;
const VRAM_H: usize = 512;

/// The pacer's ring of frame times (`u16` x 16) and its cursor.
const RING: u32 = 0x8008_4098;
const RING_CURSOR: u32 = 0x8007_B758;
/// libetc's vblank counter (`VSync(-1)`, the word `FUN_8005FCCC` waits on).
const VSYNC_COUNTER: u32 = 0x8007_A894;
/// The pad / card interrupt handler the kernel chains on the vblank.
const SIO_INTERRUPT: u32 = 0x8006_CFC8;
/// The frame step byte.
const STEP: u32 = 0x1F80_0393;
/// The frame's ordering table: its base (`*(0x1F8003F4)`) and its length in
/// entries (`0x1F8003A6`), the pair the pacer hands `DrawOTag` the far end
/// of (`0x80017264..0x8001728C`).
const OT_BASE: u32 = 0x1F80_03F4;
const OT_LEN: u32 = 0x1F80_03A6;

/// Primitives linked into the frame's ordering table so far: its chain
/// walked from the head `DrawOTag` takes (a `ClearOTagR` table runs from the
/// last entry down to the terminator), counting every link whose tag
/// carries a length (the table's own entries carry none).
pub fn linked_primitives(m: &mut Memory) -> u32 {
    let (base, len) = (m.rd32(OT_BASE), u32::from(m.rd16(OT_LEN)));
    if base & 0x1FFF_FFFF >= 0x20_0000 || len == 0 || len > 0x4000 {
        return 0;
    }
    let mut p = base.wrapping_add(len * 4 - 4) & 0x00FF_FFFF;
    let mut n = 0u32;
    // A chain never holds more links than RAM holds tags.
    for _ in 0..0x8_0000 {
        let tag = m.rd32(0x8000_0000 | p);
        n += u32::from(tag >> 24 != 0);
        let next = tag & 0x00FF_FFFF;
        if next == 0x00FF_FFFF || next >= 0x20_0000 {
            break;
        }
        p = next;
    }
    n
}

/// A routine whose cost a frame reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Routine {
    pub va: u32,
    pub name: &'static str,
}

const fn r(va: u32, name: &'static str) -> Routine {
    Routine { va, name }
}

/// The routines the frame profile names, plus the frame's own parts.
pub const FIELD_ROUTINES: &[Routine] = &[
    r(FRAME_HANDLER, "field frame handler"),
    r(0x8001_698C, "field tick"),
    r(0x8001_6444, "master frame driver"),
    r(0x8002_519C, "actor tick pass"),
    r(0x8001_D140, "actor render pass"),
    r(0x8001_ADA4, "actor draw"),
    r(0x8002_6CE4, "field render"),
    r(0x801F_7088, "decoration pass"),
    r(0x801F_6D48, "ground emitter"),
    r(0x801F_69EC, "ground emitter (alt)"),
    r(0x801D_E840, "field VM"),
    r(0x801D_6058, "ambient emitter"),
    r(0x8003_F3FC, "fog walk"),
    r(0x8001_9D50, "CLUT-cell cycler"),
    r(0x8001_6B6C, "frame pacer"),
];

/// One routine over one frame.
#[derive(Debug, Clone, Copy)]
pub struct PassRow {
    pub routine: Routine,
    pub spent: Spent,
    /// Primitives its calls linked into the frame's ordering table, its
    /// callees' included ([`linked_primitives`] at each call's return less
    /// at its entry).
    pub linked: i64,
}

/// One frame's costs.
#[derive(Debug, Clone)]
pub struct FrameCost {
    /// Every listed routine, in list order.
    pub rows: Vec<PassRow>,
    /// Every call of a listed routine, in the order they returned.
    pub calls: Vec<ClosedCall>,
    /// Instructions and cycles of the whole frame, `VSync` waits included.
    pub steps: u64,
    pub cycles: u64,
    /// The pacer's newest ring entry and the step after the frame.
    pub ring: u16,
    pub step: u8,
}

impl FrameCost {
    /// What `va` spent on this frame.
    pub fn of(&self, va: u32) -> Spent {
        self.row(va).map_or_else(Spent::default, |r| r.spent)
    }
    pub fn row(&self, va: u32) -> Option<&PassRow> {
        self.rows.iter().find(|r| r.routine.va == va)
    }
}

/// A field state resumed, frame by frame.
pub struct FrameRunner {
    pub cpu: Cpu,
    routines: Vec<Routine>,
    pc: u32,
    /// Instruction budget a frame.
    pub budget: u64,
}

impl FrameRunner {
    pub fn new(state: &PcsxState) -> Self {
        Self::with_routines(state, FIELD_ROUTINES)
    }

    /// A runner reporting `routines` (inclusive, as the profile reports).
    pub fn with_routines(state: &PcsxState, routines: &[Routine]) -> Self {
        let mut cpu = state.cpu();
        cpu.syscall_exceptions = true;
        cpu.vblank_counter = Some(VSYNC_COUNTER);
        // The state is saved on the vblank event.
        cpu.next_vblank = PCSX_CYCLES_PER_VBLANK;
        cpu.hardware_zero_div = true;
        cpu.profile_mark = Some(linked_primitives);
        for r in routines {
            cpu.profile(r.va);
        }
        seat_vram(&mut cpu, &state.vram);
        Self {
            cpu,
            routines: routines.to_vec(),
            pc: state.pc,
            budget: 200_000_000,
        }
    }

    /// Take the vblank interrupt from here on (`Cpu::vblank_irq`): the
    /// kernel's dispatch and libetc's vblank callbacks run, so what they
    /// advance (the sound sequencer, the game's own vsync counters) moves as
    /// it does on the emulator, and their time lands in whatever routine was
    /// running, as the emulator's profile reads it before its interrupt time
    /// is taken out. The pad and card exchange over SIO
    /// (`FUN_8006CFC8`, reached from the kernel's interrupt chain) would wait
    /// on a serial port nothing answers; it returns at once instead. The rest
    /// of the vblank interrupt costs about `24100` cycles here against
    /// PCSX-Redux's `24400..26200`, so the exchange is a small part of it.
    pub fn take_interrupts(&mut self) {
        self.cpu.vblank_counter = None;
        self.cpu.vblank_irq = true;
        self.cpu.stub(SIO_INTERRUPT, 0);
    }

    /// The run's last distinct calls, newest first, for a stop's message.
    fn call_tail(&self) -> String {
        let mut tail: Vec<String> = Vec::new();
        for c in self.cpu.calls.iter().rev() {
            let t = format!("{c:08X}");
            if tail.last() != Some(&t) {
                tail.push(t);
            }
            if tail.len() >= 24 {
                break;
            }
        }
        format!("last calls {}", tail.join(" "))
    }

    /// Run the state's frame in progress out, to the next frame's start.
    pub fn settle(&mut self) -> Result<()> {
        let stop = self.cpu.resume(self.pc, FRAME_HANDLER, 1, self.budget);
        if stop != Stop::Returned {
            bail!("settling stopped: {stop:x?}; {}", self.call_tail());
        }
        self.pc = FRAME_HANDLER;
        Ok(())
    }

    /// Run one whole field frame.
    pub fn frame(&mut self) -> Result<FrameCost> {
        self.cpu.reset_profile();
        let clock = self.cpu.mem.clock;
        let stop = self.cpu.resume(self.pc, FRAME_HANDLER, 1, self.budget);
        if stop != Stop::Returned {
            bail!("frame stopped: {stop:x?}; {}", self.call_tail());
        }
        let calls = self.cpu.profile_log.clone();
        let rows = self
            .routines
            .iter()
            .map(|r| PassRow {
                routine: *r,
                spent: self.cpu.spent(r.va),
                linked: calls
                    .iter()
                    .filter(|c| c.entry == r.va)
                    .map(|c| i64::from(c.mark.1) - i64::from(c.mark.0))
                    .sum(),
            })
            .collect();
        let cursor = self.cpu.mem.rd32(RING_CURSOR);
        let ring = self.cpu.mem.rd16(RING + (cursor.wrapping_sub(1) % 16) * 2);
        Ok(FrameCost {
            rows,
            calls,
            steps: self.cpu.steps,
            cycles: self.cpu.mem.clock - clock,
            ring,
            step: self.cpu.mem.rd8(STEP),
        })
    }
}

/// Keep the state's VRAM beside the run as data: `LoadImage` / `MoveImage`
/// write it and `StoreImage` reads it back into RAM, each as it is entered.
fn seat_vram(cpu: &mut Cpu, bytes: &[u8]) {
    let mut px = vec![0u16; VRAM_W * VRAM_H];
    for (i, c) in bytes.as_chunks::<2>().0.iter().take(px.len()).enumerate() {
        px[i] = u16::from_le_bytes([c[0], c[1]]);
    }
    let vram = Rc::new(RefCell::new(px));
    fn rect(m: &mut Memory, a: u32) -> (i32, i32, i32, i32) {
        let mut h = |o: u32| i32::from(m.rd16(a + o) as i16);
        (h(0), h(2), h(4).max(0), h(6).max(0))
    }
    fn at(x: i32, y: i32) -> usize {
        (y.rem_euclid(VRAM_H as i32) as usize) * VRAM_W + x.rem_euclid(VRAM_W as i32) as usize
    }
    let v = vram.clone();
    cpu.probe(LOAD_IMAGE, move |r, m, _| {
        let (x, y, w, h) = rect(m, r.r[A0]);
        let mut src = r.r[A1];
        let mut v = v.borrow_mut();
        for j in 0..h {
            for i in 0..w {
                v[at(x + i, y + j)] = m.rd16(src);
                src = src.wrapping_add(2);
            }
        }
    });
    let v = vram.clone();
    cpu.probe(STORE_IMAGE, move |r, m, _| {
        let (x, y, w, h) = rect(m, r.r[A0]);
        let mut dst = r.r[A1];
        let v = v.borrow();
        for j in 0..h {
            for i in 0..w {
                m.wr16(dst, v[at(x + i, y + j)]);
                dst = dst.wrapping_add(2);
            }
        }
    });
    cpu.probe(MOVE_IMAGE, move |r, m, _| {
        let (x, y, w, h) = rect(m, r.r[A0]);
        let (dx, dy) = (r.r[A1] as i32, r.r[A2] as i32);
        let mut v = vram.borrow_mut();
        let block: Vec<u16> = (0..h)
            .flat_map(|j| (0..w).map(move |i| (i, j)))
            .map(|(i, j)| v[at(x + i, y + j)])
            .collect();
        for (k, (i, j)) in (0..h).flat_map(|j| (0..w).map(move |i| (i, j))).enumerate() {
            v[at(dx + i, dy + j)] = block[k];
        }
    });
}
