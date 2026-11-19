//! A user-mode R3000 interpreter for running retail routines off the disc.
//!
//! The differential oracle under `tests/` loads `SCUS_942.54` and an overlay
//! from the user's disc, seeds RAM, calls a routine and compares what it wrote
//! with the Rust kernel whose `// PORT:` tag names that routine. It answers the
//! question the catalog's gates cannot: does the port compute what the bytes
//! compute? See `docs/tooling/retail-kernel-differential.md`.
//!
//! The GTE is the [`gte`] module, written from the hardware documentation and
//! independent of the engine's own GTE. COP0 carries `mfc0` / `mtc0` as plain
//! register moves and `rfe` as the status-stack pop; anything else on it stops
//! with [`Stop::Unsupported`]. Calls can be replaced by a [`Hook`], which is
//! how `rand()` is scripted. Load-delay slots are not modelled; the retail
//! compiler never reads a register in the slot after loading it (or after an
//! `mfc2` / `cfc2`), and GTE command latency is not modelled either.

pub mod gte;

use std::collections::HashMap;

pub mod frame_runner;
pub mod ledger;
pub mod pcsx_state;

/// Main RAM, mirrored at `0x0000_0000`, `0x8000_0000` and `0xA000_0000`.
pub const RAM_SIZE: usize = 0x20_0000;
/// The return address [`Cpu::call`] plants; reaching it ends the call.
pub const RETURN_SENTINEL: u32 = 0x8FFF_FFF0;
const SCRATCH_BASE: u32 = 0x1F80_0000;
const SCRATCH_SIZE: usize = 0x400;
/// The hardware window a PCSX-Redux state carries: the scratchpad's
/// kilobyte, then the memory-mapped I/O registers to `0x1F81_0000`.
const IO_BASE: u32 = 0x1F80_1000;
const IO_END: u32 = 0x1F81_0000;
/// The BIOS ROM, 512 KiB at `0x1FC0_0000`.
const BIOS_BASE: u32 = 0x1FC0_0000;
const BIOS_SIZE: usize = 0x8_0000;
/// CPU cycles in one scanline on PCSX-Redux's NTSC timing
/// (`33868800 / 60 / 263`), the unit the frame pacer's ring counts in.
pub const PCSX_CYCLES_PER_HBLANK: f64 = 33_868_800.0 / 60.0 / 263.0;
/// Where the stack starts, in a part of RAM no routine under test reaches.
const STACK_TOP: u32 = 0x801F_FF00;

/// Why a call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The routine returned to the sentinel.
    Returned,
    /// An instruction this interpreter does not carry (`syscall`, an undefined
    /// GTE command, a COP0 operation other than a move or `rfe`, ...).
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

/// A [`Hook`] that sees the whole CPU, its counters included (a `VSync`
/// stand-in that answers from the instructions run so far).
pub type CpuHook = Box<dyn FnMut(&mut Cpu)>;

/// A probe: called when the run reaches its address, before the instruction
/// there executes, with read access to the CPU and GTE state; it changes
/// nothing and the run continues.
pub type Probe = Box<dyn FnMut(&Regs, &mut Memory, &gte::Gte)>;

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

/// Which part of the memory map a data access lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Ram,
    Scratch,
    Io,
    Bios,
    /// Anything else (the cache-control word at `0xFFFE_0130`, an unmapped
    /// expansion port).
    Other,
}

impl Region {
    /// The region `addr` decodes to, through any of the KUSEG / KSEG0 /
    /// KSEG1 mirrors.
    pub fn of(addr: u32) -> Self {
        if addr >= 0xC000_0000 {
            return Self::Other;
        }
        let phys = addr & 0x1FFF_FFFF;
        if phys < 0x80_0000 {
            Self::Ram
        } else if (SCRATCH_BASE..SCRATCH_BASE + SCRATCH_SIZE as u32).contains(&phys) {
            Self::Scratch
        } else if (SCRATCH_BASE..IO_END).contains(&phys) {
            Self::Io
        } else if (BIOS_BASE..BIOS_BASE + BIOS_SIZE as u32).contains(&phys) {
            Self::Bios
        } else {
            Self::Other
        }
    }
}

/// Data accesses by region. PCSX-Redux's `Memory::read8/16/32` (data reads)
/// and `write8/16/32` add one cycle each whatever the region, so
/// [`Accesses::total`] is the term [`Cpu::cost_pcsx`] adds; the split is
/// there for a timing model that prices regions differently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Accesses {
    pub ram: u64,
    pub scratch: u64,
    pub io: u64,
    pub bios: u64,
    pub other: u64,
}

impl Accesses {
    pub fn total(&self) -> u64 {
        self.ram + self.scratch + self.io + self.bios + self.other
    }
    fn note(&mut self, addr: u32) {
        match Region::of(addr) {
            Region::Ram => self.ram += 1,
            Region::Scratch => self.scratch += 1,
            Region::Io => self.io += 1,
            Region::Bios => self.bios += 1,
            Region::Other => self.other += 1,
        }
    }
    /// `self - earlier`, field by field.
    pub fn since(&self, earlier: &Self) -> Self {
        Self {
            ram: self.ram - earlier.ram,
            scratch: self.scratch - earlier.scratch,
            io: self.io - earlier.io,
            bios: self.bios - earlier.bios,
            other: self.other - earlier.other,
        }
    }
    fn add(&mut self, o: &Self) {
        self.ram += o.ram;
        self.scratch += o.scratch;
        self.io += o.io;
        self.bios += o.bios;
        self.other += o.other;
    }
}

/// Flat RAM plus the scratchpad, and optionally the I/O window and the BIOS
/// ROM.
#[derive(Clone)]
pub struct Memory {
    ram: Vec<u8>,
    scratch: Vec<u8>,
    /// The I/O registers `0x1F80_1000..0x1F81_0000` when mapped: reads see
    /// these bytes, writes are dropped (no device behind them), so a status
    /// register reads as the state left it.
    io: Option<Vec<u8>>,
    /// The BIOS ROM when mapped, read-only.
    bios: Option<Vec<u8>>,
    /// First address an access failed on, read by the run loop.
    bad: Option<u32>,
    /// CPU cycles on PCSX-Redux's scale since the memory was built: two an
    /// instruction and one a data access ([`Cpu::cost_pcsx`]), plus what a
    /// hook adds for time it stands in for (a `VSync` wait).
    pub clock: u64,
    /// With the I/O window mapped, `GPUSTAT` reads ready (bits 26..28), the
    /// DMA registers hold what is written and each transfer completes at
    /// once (only channel 6, the ordering-table clear, moves data), and the
    /// interrupt controller is live:
    /// `I_STAT` (`0x1F80_1070`, a write acknowledges the bits it clears)
    /// and `I_MASK` (`0x1F80_1074`).
    pub i_stat: u32,
    pub i_mask: u32,
    /// Root counter 1's count when the clock read zero; with the window
    /// mapped the counter (`0x1F80_1110`) counts scanlines off the clock.
    pub rcnt1_seed: u32,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            ram: vec![0; RAM_SIZE],
            scratch: vec![0; SCRATCH_SIZE],
            io: None,
            bios: None,
            bad: None,
            clock: 0,
            i_stat: 0,
            i_mask: 0,
            rcnt1_seed: 0,
        }
    }
}

/// PCSX-Redux's integer cycles a scanline (`psxcounters.cc`).
const CYCLES_PER_SCANLINE: u64 = 33_868_800 / (60 * 263);
/// A vblank's period on the same clock.
pub const PCSX_CYCLES_PER_VBLANK: u64 = CYCLES_PER_SCANLINE * 263;

impl Memory {
    /// Map the I/O window, seeded with `bytes` from `0x1F80_1000` (a
    /// PCSX-Redux state's hardware blob from its `0x1000` offset).
    pub fn map_io(&mut self, bytes: &[u8]) {
        let mut io = vec![0; (IO_END - IO_BASE) as usize];
        let n = bytes.len().min(io.len());
        io[..n].copy_from_slice(&bytes[..n]);
        let word = |o: usize| u32::from_le_bytes([io[o], io[o + 1], io[o + 2], io[o + 3]]);
        self.i_stat = word(0x70);
        self.i_mask = word(0x74);
        self.rcnt1_seed = word(0x110) & 0xFFFF;
        self.io = Some(io);
    }

    /// DMA channel 6 (OTC), started by its CHCR's top byte: the reverse
    /// ordering table `ClearOTagR` asks for - from `MADR` down, `BCR`
    /// entries, each linking to the one below it, the last the terminator
    /// `0x00FF_FFFF` - written as the DMA would, then the busy bit dropped.
    fn clear_ot(&mut self) {
        let word = |m: &Self, a: u32| {
            let io = m.io.as_ref().expect("mapped");
            let o = (a - IO_BASE) as usize;
            u32::from_le_bytes([io[o], io[o + 1], io[o + 2], io[o + 3]])
        };
        let madr = word(self, 0x1F80_10E0) & 0x00FF_FFFC;
        let n = match word(self, 0x1F80_10E4) & 0xFFFF {
            0 => 0x1_0000,
            n => n,
        };
        for i in 0..n {
            let a = madr.wrapping_sub(4 * i) & 0x00FF_FFFF;
            let v = if i + 1 == n {
                0x00FF_FFFF
            } else {
                a.wrapping_sub(4) & 0x00FF_FFFF
            };
            if (a as usize) < RAM_SIZE {
                self.ram[a as usize..a as usize + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }

    /// A byte of a live I/O register, for the registers the window models.
    fn live_io(&self, phys: u32) -> Option<u8> {
        let sh = (phys & 3) * 8;
        let word = match phys & !3 {
            0x1F80_1070 => self.i_stat,
            0x1F80_1074 => self.i_mask,
            // GPUSTAT: the GPU takes a command, a VRAM read and a DMA at
            // once, as PCSX-Redux's free GPU always does.
            0x1F80_1814 => {
                let io = self.io.as_ref()?;
                u32::from_le_bytes([io[0x814], io[0x815], io[0x816], io[0x817]]) | 0x1C00_0000
            }
            0x1F80_1110 => (self.rcnt1_seed + (self.clock / CYCLES_PER_SCANLINE) as u32) & 0xFFFF,
            _ => return None,
        };
        Some((word >> sh) as u8)
    }
    /// Map a BIOS ROM image at `0x1FC0_0000`.
    pub fn map_bios(&mut self, bytes: &[u8]) {
        let mut rom = vec![0; BIOS_SIZE];
        let n = bytes.len().min(BIOS_SIZE);
        rom[..n].copy_from_slice(&bytes[..n]);
        self.bios = Some(rom);
    }
    /// Main RAM, 2 MiB from physical address 0.
    pub fn ram(&self) -> &[u8] {
        &self.ram
    }
    fn slot(&mut self, addr: u32) -> Option<&mut u8> {
        let phys = addr & 0x1FFF_FFFF;
        if (phys as usize) < RAM_SIZE {
            return self.ram.get_mut(phys as usize);
        }
        if (SCRATCH_BASE..SCRATCH_BASE + SCRATCH_SIZE as u32).contains(&phys) {
            return self.scratch.get_mut((phys - SCRATCH_BASE) as usize);
        }
        if let Some(io) = self.io.as_mut()
            && (IO_BASE..IO_END).contains(&phys)
        {
            return io.get_mut((phys - IO_BASE) as usize);
        }
        if let Some(rom) = self.bios.as_mut()
            && (BIOS_BASE..BIOS_BASE + BIOS_SIZE as u32).contains(&phys)
        {
            return rom.get_mut((phys - BIOS_BASE) as usize);
        }
        self.bad.get_or_insert(addr);
        None
    }

    pub fn rd8(&mut self, a: u32) -> u8 {
        if self.io.is_some()
            && let Some(b) = self.live_io(a & 0x1FFF_FFFF)
        {
            return b;
        }
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
        // A mapped I/O window or ROM swallows the write; an unmapped one
        // still reports the address through `slot`.
        match Region::of(a) {
            Region::Io if self.io.is_some() => {
                let phys = a & 0x1FFF_FFFF;
                let sh = (phys & 3) * 8;
                match phys & !3 {
                    0x1F80_1070 => self.i_stat &= !(0xFF << sh) | (u32::from(v) << sh),
                    0x1F80_1074 => {
                        self.i_mask = (self.i_mask & !(0xFF << sh)) | (u32::from(v) << sh)
                    }
                    // The DMA channels' registers hold what is written; a
                    // transfer completes at once (CHCR's busy bit reads clear).
                    0x1F80_1080..=0x1F80_10EF => {
                        // CHCR's top byte: start (bit 28) and busy (bit 24)
                        // drop as the transfer is done.
                        let chcr_top = phys & 0xF == 0xB;
                        if let Some(io) = self.io.as_mut() {
                            io[(phys - IO_BASE) as usize] = if chcr_top { v & !0x11 } else { v };
                        }
                        if phys == 0x1F80_10EB && v & 0x01 != 0 {
                            self.clear_ot();
                        }
                    }
                    _ => {}
                }
                return;
            }
            Region::Bios if self.bios.is_some() => return,
            _ => {}
        }
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
    /// The data accesses of the last [`Cpu::call`], by region, counted the
    /// way PCSX-Redux's interpreter charges them (`psxinterpreter.cc` /
    /// `psxmem.cc` / `gte-transfer.cc`): one for each load, store, `lwl`,
    /// `lwr`, `lwc2` and `swc2`, two for `swl` / `swr` (a read then a write),
    /// none for an instruction fetch. See [`Cpu::cost_pcsx`].
    pub accesses: Accesses,
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
    /// Coprocessor 2, seeded and read by a test like RAM.
    pub gte: gte::Gte,
    /// The COP0 register file, as `mtc0` left it (`SR` is register 12).
    pub cop0: [u32; 32],
    /// GTE commands executed by the last [`Cpu::call`], so a test can prove
    /// it reached its geometry arm.
    pub gte_ops: u64,
    /// Entries reached by a jump rather than a call - a dispatcher's
    /// table-driven leaves - that the ledger notes each time the run lands
    /// on them, as [`Cpu::call`] notes its own entry.
    pub watched: std::collections::HashSet<u32>,
    probes: HashMap<u32, Probe>,
    /// Routines whose inclusive cost [`Cpu::profile`] collects.
    profiled: HashMap<u32, Spent>,
    /// Profiled calls not yet returned: `(entry, return address, steps and
    /// accesses at entry)`.
    open_calls: Vec<(u32, u32, u64, Accesses, u32)>,
    /// Every profiled call as it closed, since [`Cpu::reset_profile`].
    pub profile_log: Vec<ClosedCall>,
    /// A reading taken as each profiled call opens and closes (the
    /// primitives linked into the ordering table so far, say):
    /// [`ClosedCall::mark`].
    pub profile_mark: Option<fn(&mut Memory) -> u32>,
    cpu_hooks: HashMap<u32, CpuHook>,
    /// Take `syscall` as the R3000 does - `EPC`, `Cause`, the status-stack
    /// push and a jump to the exception vector `0x8000_0080` - so a mapped
    /// kernel's handler runs it, instead of stopping on it.
    pub syscall_exceptions: bool,
    /// [`Cpu::resume`]'s halt: the address, and how many more arrivals there
    /// run on before the run stops.
    halt: Option<(u32, u64)>,
    /// The clock reading at which the next vblank arrives.
    pub next_vblank: u64,
    /// The word the vblank interrupt handler counts vblanks in (libetc's
    /// `VSync` counter). No interrupt is taken: each time [`Memory::clock`]
    /// passes [`Cpu::next_vblank`] one is added there at no cost, so a
    /// `VSync` wait spins until the clock reaches its vblank and no handler
    /// time lands in the routine that was running.
    pub vblank_counter: Option<u32>,
}

/// One profiled call, as it closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosedCall {
    pub entry: u32,
    pub spent: Spent,
    /// [`Cpu::profile_mark`]'s reading at the call's entry and at its return.
    pub mark: (u32, u32),
}

/// What a profiled routine spent: calls, and the instructions and data
/// accesses inside them, callees included.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spent {
    pub calls: u64,
    pub steps: u64,
    pub accesses: Accesses,
}

impl Spent {
    /// Cycles on PCSX-Redux's scale: two an instruction, one a data access.
    pub fn cycles_pcsx(&self) -> u64 {
        2 * self.steps + self.accesses.total()
    }
    /// [`Self::cycles_pcsx`] in scanlines, the frame pacer's unit.
    pub fn hblanks_pcsx(&self) -> f64 {
        self.cycles_pcsx() as f64 / PCSX_CYCLES_PER_HBLANK
    }
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

    /// Run the routine at `va` again, dropping a hook set on it.
    pub fn unhook(&mut self, va: u32) {
        self.hooks.remove(&va);
    }

    /// Observe the run each time it reaches `va` (any instruction, not only
    /// an entry): `f` sees the registers, memory and GTE as they stand before
    /// that instruction executes.
    pub fn probe(&mut self, va: u32, f: impl FnMut(&Regs, &mut Memory, &gte::Gte) + 'static) {
        self.probes.insert(va, Box::new(f));
    }

    /// Collect the inclusive cost of the routine at `va` over the calls that
    /// follow, the way `scripts/pcsx-redux/autorun_field_frame_profile.lua`
    /// does on the emulator: reaching `va` opens a call with the `ra` it
    /// holds there, reaching that return address closes the newest open call
    /// of `va` made from it. Callees count in; a hooked callee costs nothing.
    pub fn profile(&mut self, va: u32) {
        self.profiled.entry(va).or_default();
    }

    /// What the routine at `va` has spent since [`Cpu::profile`] armed it.
    pub fn spent(&self, va: u32) -> Spent {
        self.profiled.get(&va).copied().unwrap_or_default()
    }

    /// Zero every profiled routine's totals, keeping them armed.
    pub fn reset_profile(&mut self) {
        for v in self.profiled.values_mut() {
            *v = Spent::default();
        }
        self.open_calls.clear();
        self.profile_log.clear();
    }

    /// The last call's cost on PCSX-Redux's scale: its interpreter adds
    /// `Emulator::BIAS` (2) cycles an instruction (`psxinterpreter.cc`) and
    /// `Memory` adds one a data access whatever the region (`psxmem.cc`);
    /// the GTE and the GPU cost nothing there.
    pub fn cost_pcsx(&self) -> u64 {
        2 * self.steps + self.accesses.total()
    }

    /// [`Cpu::call`] with its own instruction budget, for a routine (a whole
    /// render pass) longer than the default.
    pub fn call_with_budget(&mut self, entry: u32, args: &[u32], budget: u64) -> Stop {
        let kept = std::mem::replace(&mut self.step_budget, budget);
        let stop = self.call(entry, args);
        self.step_budget = kept;
        stop
    }

    fn note_profile(&mut self, pc: u32) {
        if !self.open_calls.is_empty() && self.open_calls.iter().any(|c| c.1 == pc) {
            let mut closed: Vec<u32> = Vec::new();
            let mut i = self.open_calls.len();
            while i > 0 {
                i -= 1;
                let (entry, ra, steps, acc, mark) = self.open_calls[i];
                if ra == pc && !closed.contains(&entry) {
                    closed.push(entry);
                    self.open_calls.remove(i);
                    let one = Spent {
                        calls: 1,
                        steps: self.steps - steps,
                        accesses: self.accesses.since(&acc),
                    };
                    let s = self.profiled.entry(entry).or_default();
                    s.calls += 1;
                    s.steps += one.steps;
                    s.accesses.add(&one.accesses);
                    let now = self.profile_mark.map_or(0, |f| f(&mut self.mem));
                    self.profile_log.push(ClosedCall {
                        entry,
                        spent: one,
                        mark: (mark, now),
                    });
                }
            }
        }
        if self.profiled.contains_key(&pc) {
            let mark = self.profile_mark.map_or(0, |f| f(&mut self.mem));
            self.open_calls
                .push((pc, self.regs.r[RA], self.steps, self.accesses, mark));
        }
    }

    /// Replace the routine at `va` with one that sees the whole CPU.
    pub fn hook_cpu(&mut self, va: u32, f: impl FnMut(&mut Cpu) + 'static) {
        self.cpu_hooks.insert(va, Box::new(f));
    }

    /// Run on from `pc` with the registers as they stand - a save state's
    /// CPU - until the run reaches `halt` for the `count`-th time (`Stop::
    /// Returned`), or `budget` instructions. Counters, the call log and open
    /// profiled calls start afresh, as for [`Cpu::call`].
    pub fn resume(&mut self, pc: u32, halt: u32, count: u64, budget: u64) -> Stop {
        self.steps = 0;
        self.accesses = Accesses::default();
        self.open_calls.clear();
        self.calls.clear();
        self.gte_ops = 0;
        // Standing on the halt address already is not an arrival.
        let count = count.max(1) + u64::from(pc == halt);
        self.halt = Some((halt, count));
        let stop = self.run(pc, budget);
        self.halt = None;
        stop
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
        self.accesses = Accesses::default();
        self.open_calls.clear();
        self.calls.clear();
        self.gte_ops = 0;
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
            if let Some(a) = self.vblank_counter {
                while self.mem.clock >= self.next_vblank {
                    let n = self.mem.rd32(a);
                    self.mem.wr32(a, n.wrapping_add(1));
                    self.next_vblank += PCSX_CYCLES_PER_VBLANK;
                }
            }
            if let Some((at, left)) = self.halt
                && pc == at
            {
                if left <= 1 {
                    return Stop::Returned;
                }
                self.halt = Some((at, left - 1));
            }
            if !self.profiled.is_empty() {
                self.note_profile(pc);
            }
            if let Some(mut p) = self.probes.remove(&pc) {
                p(&self.regs, &mut self.mem, &self.gte);
                self.probes.insert(pc, p);
            }
            if self.watched.contains(&pc) {
                let body = self.mem.read(pc, ledger::BODY_LEN);
                ledger::record(pc, body, &Stop::Returned);
            }
            if let Some(mut h) = self.cpu_hooks.remove(&pc) {
                h(self);
                self.cpu_hooks.insert(pc, h);
                pc = self.regs.r[RA];
                next = pc.wrapping_add(4);
                continue;
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
                self.mem.clock += 2;
                pc = self.regs.r[RA];
                next = pc.wrapping_add(4);
                continue;
            }
            self.steps += 1;
            self.mem.clock += 2;
            if self.steps > budget {
                return Stop::StepLimit { pc };
            }
            let w = self.mem.rd32(pc);
            if w & 0xFC00_003F == 0x0000_000C && self.syscall_exceptions {
                // In a delay slot the exception returns to the branch.
                let delay = next != pc.wrapping_add(4);
                self.cop0[14] = if delay { pc.wrapping_sub(4) } else { pc };
                self.cop0[13] =
                    (self.cop0[13] & !0x8000_007C) | (8 << 2) | (u32::from(delay) << 31);
                let sr = self.cop0[12];
                self.cop0[12] = (sr & !0x3F) | ((sr << 2) & 0x3F);
                pc = 0x8000_0080;
                next = pc.wrapping_add(4);
                continue;
            }
            let after = next.wrapping_add(4);
            let touched = self.accesses.total();
            let stepped = self.step(pc, w, next);
            self.mem.clock += self.accesses.total() - touched;
            match stepped {
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
        match op {
            0x20..=0x26 | 0x28 | 0x29 | 0x2B | 0x32 | 0x3A => self.accesses.note(ea),
            // `swl` / `swr` read the word and write it back.
            0x2A | 0x2E => {
                self.accesses.note(ea);
                self.accesses.note(ea);
            }
            _ => {}
        }
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
            // COP0: register moves and `rfe`.
            0x10 => match rs {
                0x00 => r.r[rt] = self.cop0[rd],
                0x04 => self.cop0[rd] = vt,
                0x10 if w & 0x3F == 0x10 => {
                    let sr = self.cop0[12];
                    self.cop0[12] = (sr & !0xF) | ((sr >> 2) & 0xF);
                }
                _ => return Err(unsupported),
            },
            // COP2: the GTE.
            0x12 => {
                if w & (1 << 25) != 0 {
                    if !self.gte.execute(w & 0x01FF_FFFF) {
                        return Err(unsupported);
                    }
                    self.gte_ops += 1;
                } else {
                    match rs {
                        0x00 => r.r[rt] = self.gte.read_data(rd),
                        0x02 => r.r[rt] = self.gte.read_ctrl(rd),
                        0x04 => self.gte.write_data(rd, vt),
                        0x06 => self.gte.write_ctrl(rd, vt),
                        _ => return Err(unsupported),
                    }
                }
            }
            0x32 => {
                let v = self.mem.rd32(ea);
                self.gte.write_data(rt, v);
            }
            0x3A => {
                let v = self.gte.read_data(rt);
                self.mem.wr32(ea, v);
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

    /// PCSX-Redux's charge: two cycles an instruction, one a data access
    /// (two for `swl`), counted by region, none for the fetch.
    #[test]
    fn accesses_are_counted_as_pcsx_redux_counts_them() {
        let mut cpu = Cpu::new();
        // lw t0,0(a0) ; sw t0,0(a1) ; swl t0,1(a0) ; lwc2 0,0(a1) ; jr ra ; nop
        let code: [u32; 6] = [
            0x8C88_0000,
            0xACA8_0000,
            0xA888_0001,
            0xC8A0_0000,
            0x03E0_0008,
            0,
        ];
        for (i, w) in code.iter().enumerate() {
            cpu.mem.wr32(0x8001_0000 + i as u32 * 4, *w);
        }
        assert_eq!(
            cpu.call(0x8001_0000, &[0x8002_0000, 0x1F80_0010]),
            Stop::Returned
        );
        assert_eq!(cpu.steps, 6);
        assert_eq!(
            cpu.accesses,
            Accesses {
                ram: 3,
                scratch: 2,
                ..Accesses::default()
            }
        );
        assert_eq!(cpu.cost_pcsx(), 17);
        assert_eq!(Region::of(0x1F80_1814), Region::Io);
        assert_eq!(Region::of(0xBFC0_0000), Region::Bios);
    }

    #[test]
    fn hook_replaces_a_call_and_unsupported_stops() {
        let mut cpu = Cpu::new();
        // jal 0x80020000 ; nop ; ...
        cpu.mem.wr32(0x8001_0000, 0x0C00_8000);
        cpu.mem.wr32(0x8001_0004, 0);
        cpu.mem.wr32(0x8001_0008, 0x0000_000C); // syscall
        cpu.stub(0x8002_0000, 7);
        assert_eq!(
            cpu.call(0x8001_0000, &[]),
            Stop::Unsupported {
                pc: 0x8001_0008,
                word: 0x0000_000C
            }
        );
        assert_eq!(cpu.regs.r[V0], 7);
    }
}
