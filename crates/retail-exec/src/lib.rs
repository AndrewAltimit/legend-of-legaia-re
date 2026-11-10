//! A user-mode R3000 interpreter for running retail routines off the disc.
//!
//! The differential oracle under `tests/` loads `SCUS_942.54` and an overlay
//! from the user's disc, seeds RAM, calls a routine and compares what it wrote
//! with the Rust kernel whose `// PORT:` tag names that routine. It answers the
//! question the catalog's gates cannot: does the port compute what the bytes
//! compute? See `docs/tooling/retail-kernel-differential.md`.
//!
//! No COP0 and no GTE: a routine that reaches one stops with
//! [`Stop::Unsupported`]. Calls can be replaced by a [`Hook`], which is how
//! `rand()` is scripted. Load-delay slots are not modelled; the retail compiler
//! never reads a register in the slot after loading it.

use std::collections::HashMap;

pub mod ledger;

/// Main RAM, mirrored at `0x0000_0000`, `0x8000_0000` and `0xA000_0000`.
pub const RAM_SIZE: usize = 0x20_0000;
/// The return address [`Cpu::call`] plants; reaching it ends the call.
pub const RETURN_SENTINEL: u32 = 0x8FFF_FFF0;
const SCRATCH_BASE: u32 = 0x1F80_0000;
const SCRATCH_SIZE: usize = 0x400;
/// Where the stack starts, in a part of RAM no routine under test reaches.
const STACK_TOP: u32 = 0x801F_FF00;

/// Why a call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The routine returned to the sentinel.
    Returned,
    /// An instruction this interpreter does not carry (GTE, COP0, ...).
    Unsupported { pc: u32, word: u32 },
    /// The step budget ran out.
    StepLimit { pc: u32 },
    /// A data access outside RAM and the scratchpad.
    BadAccess { pc: u32, addr: u32 },
    /// A `divu` / `div` by zero: undefined on the R3000 in every way a port
    /// could rely on, so a routine that reaches it is reported.
    DivideByZero { pc: u32 },
}

/// A routine replaced by Rust: runs in place of the call, then returns to `ra`.
pub type Hook = Box<dyn FnMut(&mut Regs, &mut Memory)>;

/// The integer register file.
#[derive(Debug, Clone, Default)]
pub struct Regs {
    pub r: [u32; 32],
    pub hi: u32,
    pub lo: u32,
}

pub const V0: usize = 2;
pub const V1: usize = 3;
pub const A0: usize = 4;
pub const A1: usize = 5;
pub const A2: usize = 6;
pub const A3: usize = 7;
pub const GP: usize = 28;
pub const SP: usize = 29;
pub const RA: usize = 31;

/// Flat RAM plus the scratchpad.
#[derive(Clone)]
pub struct Memory {
    ram: Vec<u8>,
    scratch: Vec<u8>,
    /// First address an access failed on, read by the run loop.
    bad: Option<u32>,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            ram: vec![0; RAM_SIZE],
            scratch: vec![0; SCRATCH_SIZE],
            bad: None,
        }
    }
}

impl Memory {
    fn slot(&mut self, addr: u32) -> Option<&mut u8> {
        let phys = addr & 0x1FFF_FFFF;
        if (phys as usize) < RAM_SIZE {
            return self.ram.get_mut(phys as usize);
        }
        if (SCRATCH_BASE..SCRATCH_BASE + SCRATCH_SIZE as u32).contains(&phys) {
            return self.scratch.get_mut((phys - SCRATCH_BASE) as usize);
        }
        self.bad.get_or_insert(addr);
        None
    }
    pub fn rd8(&mut self, a: u32) -> u8 {
        self.slot(a).map_or(0, |b| *b)
    }
    pub fn rd16(&mut self, a: u32) -> u16 {
        u16::from_le_bytes([self.rd8(a), self.rd8(a.wrapping_add(1))])
    }
    pub fn rd32(&mut self, a: u32) -> u32 {
        u32::from_le_bytes([
            self.rd8(a),
            self.rd8(a.wrapping_add(1)),
            self.rd8(a.wrapping_add(2)),
            self.rd8(a.wrapping_add(3)),
        ])
    }
    pub fn wr8(&mut self, a: u32, v: u8) {
        if let Some(b) = self.slot(a) {
            *b = v;
        }
    }
    pub fn wr16(&mut self, a: u32, v: u16) {
        for (i, b) in v.to_le_bytes().into_iter().enumerate() {
            self.wr8(a.wrapping_add(i as u32), b);
        }
    }
    pub fn wr32(&mut self, a: u32, v: u32) {
        for (i, b) in v.to_le_bytes().into_iter().enumerate() {
            self.wr8(a.wrapping_add(i as u32), b);
        }
    }
    /// Copy bytes in at `va`.
    pub fn load(&mut self, va: u32, bytes: &[u8]) {
        for (i, b) in bytes.iter().enumerate() {
            self.wr8(va.wrapping_add(i as u32), *b);
        }
    }
    /// Copy `len` bytes out from `va`.
    pub fn read(&mut self, va: u32, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| self.rd8(va.wrapping_add(i as u32)))
            .collect()
    }
}

/// The interpreter.
#[derive(Default)]
pub struct Cpu {
    pub regs: Regs,
    pub mem: Memory,
    hooks: HashMap<u32, Hook>,
    /// Instructions executed by the last [`Cpu::call`].
    pub steps: u64,
    /// The addresses reached by a `jal` / `jalr` with no hook, in order.
    pub calls: Vec<u32>,
    /// Run a `div` / `divu` by zero as the R3000 does (`hi` = the dividend, `lo`
    /// = all ones, or `1` for a negative signed dividend) instead of stopping.
    /// For a routine with no `break` guard, where that result is what ships.
    pub hardware_zero_div: bool,
    /// The instruction budget of one call, `0` for the default. A test whose
    /// routine can spin for ever lowers it to reach that verdict sooner.
    pub step_budget: u64,
    /// When set, a call that lands outside this VA range returns at once with
    /// `v0 = 0`, for a large routine whose calls are all side effects. A hook
    /// still wins.
    pub stub_outside: Option<(u32, u32)>,
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a PS-X EXE: its text segment at its load address, `gp` from the
    /// header.
    pub fn load_psx_exe(&mut self, exe: &[u8]) {
        let word = |o: usize| u32::from_le_bytes([exe[o], exe[o + 1], exe[o + 2], exe[o + 3]]);
        let load = word(0x18);
        let size = word(0x1C) as usize;
        self.regs.r[GP] = word(0x14);
        self.mem
            .load(load, &exe[0x800..(0x800 + size).min(exe.len())]);
    }

    /// Replace the routine at `va`.
    pub fn hook(&mut self, va: u32, f: impl FnMut(&mut Regs, &mut Memory) + 'static) {
        self.hooks.insert(va, Box::new(f));
    }

    /// Make the routine at `va` return at once with `v0 = ret`.
    pub fn stub(&mut self, va: u32, ret: u32) {
        self.hook(va, move |r, _| r.r[V0] = ret);
    }

    /// Call `entry` with up to four arguments and run it to its return.
    pub fn call(&mut self, entry: u32, args: &[u32]) -> Stop {
        for (i, a) in args.iter().take(4).enumerate() {
            self.regs.r[A0 + i] = *a;
        }
        self.regs.r[SP] = STACK_TOP;
        self.regs.r[RA] = RETURN_SENTINEL;
        self.steps = 0;
        self.calls.clear();
        let budget = if self.step_budget == 0 {
            2_000_000
        } else {
            self.step_budget
        };
        let body = self.mem.read(entry, ledger::BODY_LEN);
        // A hook entry may sit outside memory; the read must not stop the run.
        self.mem.bad = None;
        let stop = self.run(entry, budget);
        ledger::record(entry, body, &stop);
        stop
    }

    fn run(&mut self, entry: u32, budget: u64) -> Stop {
        let mut pc = entry;
        // Where the instruction after the current one lives: a taken branch
        // lands here one instruction late, which is the delay slot.
        let mut next = pc.wrapping_add(4);
        loop {
            if pc == RETURN_SENTINEL {
                return Stop::Returned;
            }
            if let Some(mut h) = self.hooks.remove(&pc) {
                h(&mut self.regs, &mut self.mem);
                self.hooks.insert(pc, h);
                pc = self.regs.r[RA];
                next = pc.wrapping_add(4);
                continue;
            }
            if let Some((lo, hi)) = self.stub_outside
                && !(lo..hi).contains(&pc)
            {
                self.steps += 1;
                if self.steps > budget {
                    return Stop::StepLimit { pc };
                }
                self.regs.r[V0] = 0;
                pc = self.regs.r[RA];
                next = pc.wrapping_add(4);
                continue;
            }
            self.steps += 1;
            if self.steps > budget {
                return Stop::StepLimit { pc };
            }
            let w = self.mem.rd32(pc);
            let after = next.wrapping_add(4);
            match self.step(pc, w, next) {
                Ok(Some(target)) => {
                    pc = next;
                    next = target;
                }
                Ok(None) => {
                    pc = next;
                    next = after;
                }
                Err(stop) => return stop,
            }
            self.regs.r[0] = 0;
            if let Some(addr) = self.mem.bad.take() {
                return Stop::BadAccess { pc, addr };
            }
        }
    }

    fn step(&mut self, pc: u32, w: u32, next: u32) -> Result<Option<u32>, Stop> {
        let op = w >> 26;
        let rs = ((w >> 21) & 31) as usize;
        let rt = ((w >> 16) & 31) as usize;
        let rd = ((w >> 11) & 31) as usize;
        let sa = (w >> 6) & 31;
        let imm = w & 0xFFFF;
        let simm = (w & 0xFFFF) as u16 as i16 as i32 as u32;
        let r = &mut self.regs;
        let (vs, vt) = (r.r[rs], r.r[rt]);
        let ea = vs.wrapping_add(simm);
        let btarget = next.wrapping_add(simm << 2);
        let jtarget = (next & 0xF000_0000) | ((w & 0x03FF_FFFF) << 2);
        let taken = |c: bool| if c { Some(btarget) } else { None };
        let unsupported = Stop::Unsupported { pc, word: w };
        let mut out = None;
        match op {
            0 => match w & 0x3F {
                0x00 => r.r[rd] = vt << sa,
                0x02 => r.r[rd] = vt >> sa,
                0x03 => r.r[rd] = ((vt as i32) >> sa) as u32,
                0x04 => r.r[rd] = vt << (vs & 31),
                0x06 => r.r[rd] = vt >> (vs & 31),
                0x07 => r.r[rd] = ((vt as i32) >> (vs & 31)) as u32,
                0x08 => out = Some(vs),
                0x09 => {
                    r.r[rd] = next.wrapping_add(4);
                    self.calls.push(vs);
                    out = Some(vs);
                }
                0x10 => r.r[rd] = r.hi,
                0x11 => r.hi = vs,
                0x12 => r.r[rd] = r.lo,
                0x13 => r.lo = vs,
                0x18 => {
                    let p = i64::from(vs as i32) * i64::from(vt as i32);
                    r.lo = p as u32;
                    r.hi = (p >> 32) as u32;
                }
                0x19 => {
                    let p = u64::from(vs) * u64::from(vt);
                    r.lo = p as u32;
                    r.hi = (p >> 32) as u32;
                }
                0x1A => {
                    if vt == 0 {
                        if !self.hardware_zero_div {
                            return Err(Stop::DivideByZero { pc });
                        }
                        r.hi = vs;
                        r.lo = if (vs as i32) < 0 { 1 } else { u32::MAX };
                        return Ok(None);
                    }
                    r.lo = (vs as i32).wrapping_div(vt as i32) as u32;
                    r.hi = (vs as i32).wrapping_rem(vt as i32) as u32;
                }
                0x1B => {
                    if vt == 0 {
                        if !self.hardware_zero_div {
                            return Err(Stop::DivideByZero { pc });
                        }
                        r.hi = vs;
                        r.lo = u32::MAX;
                        return Ok(None);
                    }
                    r.lo = vs / vt;
                    r.hi = vs % vt;
                }
                0x20 | 0x21 => r.r[rd] = vs.wrapping_add(vt),
                0x22 | 0x23 => r.r[rd] = vs.wrapping_sub(vt),
                0x24 => r.r[rd] = vs & vt,
                0x25 => r.r[rd] = vs | vt,
                0x26 => r.r[rd] = vs ^ vt,
                0x27 => r.r[rd] = !(vs | vt),
                0x2A => r.r[rd] = u32::from((vs as i32) < (vt as i32)),
                0x2B => r.r[rd] = u32::from(vs < vt),
                _ => return Err(unsupported),
            },
            1 => {
                let neg = (vs as i32) < 0;
                match rt {
                    0 => out = taken(neg),
                    1 => out = taken(!neg),
                    0x10 => {
                        r.r[31] = next.wrapping_add(4);
                        out = taken(neg);
                    }
                    0x11 => {
                        r.r[31] = next.wrapping_add(4);
                        out = taken(!neg);
                    }
                    _ => return Err(unsupported),
                }
            }
            2 => out = Some(jtarget),
            3 => {
                r.r[31] = next.wrapping_add(4);
                self.calls.push(jtarget);
                out = Some(jtarget);
            }
            4 => out = taken(vs == vt),
            5 => out = taken(vs != vt),
            6 => out = taken((vs as i32) <= 0),
            7 => out = taken((vs as i32) > 0),
            8 | 9 => r.r[rt] = vs.wrapping_add(simm),
            0x0A => r.r[rt] = u32::from((vs as i32) < (simm as i32)),
            0x0B => r.r[rt] = u32::from(vs < simm),
            0x0C => r.r[rt] = vs & imm,
            0x0D => r.r[rt] = vs | imm,
            0x0E => r.r[rt] = vs ^ imm,
            0x0F => r.r[rt] = imm << 16,
            0x20 => r.r[rt] = self.mem.rd8(ea) as i8 as i32 as u32,
            0x21 => r.r[rt] = self.mem.rd16(ea) as i16 as i32 as u32,
            0x23 => r.r[rt] = self.mem.rd32(ea),
            0x24 => r.r[rt] = u32::from(self.mem.rd8(ea)),
            0x25 => r.r[rt] = u32::from(self.mem.rd16(ea)),
            0x22 => {
                let shift = (ea & 3) * 8;
                let word = self.mem.rd32(ea & !3);
                let keep = if shift == 24 {
                    0
                } else {
                    vt & (0x00FF_FFFF >> shift)
                };
                r.r[rt] = keep | (word << (24 - shift));
            }
            0x26 => {
                let shift = (ea & 3) * 8;
                let word = self.mem.rd32(ea & !3);
                let keep = if shift == 0 {
                    0
                } else {
                    vt & (0xFFFF_FF00u32 << (24 - shift))
                };
                r.r[rt] = keep | (word >> shift);
            }
            0x28 => self.mem.wr8(ea, vt as u8),
            0x29 => self.mem.wr16(ea, vt as u16),
            0x2B => self.mem.wr32(ea, vt),
            0x2A => {
                let shift = (ea & 3) * 8;
                let word = self.mem.rd32(ea & !3);
                let keep = if shift == 24 {
                    0
                } else {
                    word & (0xFFFF_FF00u32 << shift)
                };
                self.mem.wr32(ea & !3, keep | (vt >> (24 - shift)));
            }
            0x2E => {
                let shift = (ea & 3) * 8;
                let word = self.mem.rd32(ea & !3);
                let keep = if shift == 0 {
                    0
                } else {
                    word & (0x00FF_FFFF >> (24 - shift))
                };
                self.mem.wr32(ea & !3, keep | (vt << shift));
            }
            _ => return Err(unsupported),
        }
        Ok(out)
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    /// A routine with a delay-slot store, a loop and an unaligned word read.
    #[test]
    fn delay_slots_and_unaligned_loads() {
        let mut cpu = Cpu::new();
        // loop: addiu v0,v0,1 ; bne v0,a0,loop ; addiu v1,v1,2 ; jr ra ; nop
        let code: [u32; 5] = [
            0x2442_0001,
            0x1444_FFFE,
            0x2463_0002,
            0x03E0_0008,
            0x0000_0000,
        ];
        for (i, w) in code.iter().enumerate() {
            cpu.mem.wr32(0x8001_0000 + i as u32 * 4, *w);
        }
        assert_eq!(cpu.call(0x8001_0000, &[5]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], 5);
        assert_eq!(cpu.regs.r[V1], 10);

        // lwl t0,3(a0) ; lwr t0,0(a0) reads an unaligned word.
        cpu.mem.load(0x8002_0001, &[0x11, 0x22, 0x33, 0x44]);
        let code: [u32; 4] = [0x8888_0003, 0x9888_0000, 0x03E0_0008, 0x0100_1021];
        for (i, w) in code.iter().enumerate() {
            cpu.mem.wr32(0x8001_0100 + i as u32 * 4, *w);
        }
        assert_eq!(cpu.call(0x8001_0100, &[0x8002_0001]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], 0x4433_2211);
    }

    #[test]
    fn hook_replaces_a_call_and_unsupported_stops() {
        let mut cpu = Cpu::new();
        // jal 0x80020000 ; nop ; ...
        cpu.mem.wr32(0x8001_0000, 0x0C00_8000);
        cpu.mem.wr32(0x8001_0004, 0);
        cpu.mem.wr32(0x8001_0008, 0x4A00_0000); // a GTE word
        cpu.stub(0x8002_0000, 7);
        assert_eq!(
            cpu.call(0x8001_0000, &[]),
            Stop::Unsupported {
                pc: 0x8001_0008,
                word: 0x4A00_0000
            }
        );
        assert_eq!(cpu.regs.r[V0], 7);
    }
}
