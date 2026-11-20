//! A PCSX-Redux save state as a seat for the interpreter: main RAM, the
//! scratchpad and I/O window, the BIOS ROM and the CPU / GTE register files,
//! read from the state's protobuf (`src/core/sstate.h` in PCSX-Redux: memory
//! is top-level field 3 - `ram` 1, `rom` 2, `hardware` 4 - and the registers
//! field 4 - `gpr` 1 with `lo` / `hi` in slots 32 / 33, `cp0` 2, `cp2d` 3,
//! `cp2c` 4, `pc` 5, `in_isr` 18 - and the GPU field 5, `vram` 3).

use crate::{Cpu, RAM_SIZE};
use anyhow::{Context, Result, bail};
use std::path::Path;

/// What a state holds that a run needs.
pub struct PcsxState {
    /// Main RAM, 2 MiB.
    pub ram: Vec<u8>,
    /// The 64 KiB hardware blob: the scratchpad, then the I/O registers.
    pub hardware: Vec<u8>,
    /// The BIOS ROM the state was made with.
    pub bios: Vec<u8>,
    /// VRAM, 1024 x 512 halfwords (GPU message field 5, `vram` 3); empty
    /// when the state carries none.
    pub vram: Vec<u8>,
    /// `r0..r31`, then `lo`, `hi`.
    pub gpr: [u32; 34],
    pub cp0: [u32; 32],
    pub cp2d: [u32; 32],
    pub cp2c: [u32; 32],
    /// Where the CPU stood when the state was saved.
    pub pc: u32,
    /// Whether it stood inside the exception handler.
    pub in_isr: bool,
}

fn varint(b: &[u8], mut o: usize) -> Option<(u64, usize)> {
    let mut v = 0u64;
    let mut s = 0;
    loop {
        let c = *b.get(o)?;
        o += 1;
        v |= u64::from(c & 0x7F) << s;
        if c < 0x80 {
            return Some((v, o));
        }
        s += 7;
        if s > 63 {
            return None;
        }
    }
}

/// One field of a message: number, wire type, and either its varint value or
/// its payload span.
enum Field {
    Varint(u64),
    Bytes(usize, usize),
}

fn fields(b: &[u8], start: usize, end: usize) -> Option<Vec<(u64, Field)>> {
    let mut out = Vec::new();
    let mut o = start;
    while o < end {
        let (tag, o2) = varint(b, o)?;
        o = match tag & 7 {
            0 => {
                let (v, o3) = varint(b, o2)?;
                out.push((tag >> 3, Field::Varint(v)));
                o3
            }
            1 => o2 + 8,
            5 => o2 + 4,
            2 => {
                let (len, o3) = varint(b, o2)?;
                let len = len as usize;
                out.push((tag >> 3, Field::Bytes(o3, len)));
                o3 + len
            }
            _ => return None,
        };
    }
    (o == end).then_some(out)
}

fn bytes_of(f: &[(u64, Field)], n: u64) -> Option<(usize, usize)> {
    f.iter().find_map(|(k, v)| match v {
        Field::Bytes(s, l) if *k == n => Some((*s, *l)),
        _ => None,
    })
}

fn varint_of(f: &[(u64, Field)], n: u64) -> Option<u64> {
    f.iter().find_map(|(k, v)| match v {
        Field::Varint(x) if *k == n => Some(*x),
        _ => None,
    })
}

fn packed<const N: usize>(b: &[u8], (start, len): (usize, usize)) -> Option<[u32; N]> {
    let mut out = [0u32; N];
    let mut o = start;
    let mut n = 0;
    while o < start + len {
        let (v, o2) = varint(b, o)?;
        *out.get_mut(n)? = v as u32;
        n += 1;
        o = o2;
    }
    (n == N).then_some(out)
}

impl PcsxState {
    pub fn from_path(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_bytes(&raw)
    }

    pub fn from_bytes(raw: &[u8]) -> Result<Self> {
        let p = psx_pcsx_redux::sstate_payload(raw)?;
        let top = fields(&p, 0, p.len()).context("state is not a protobuf message")?;
        let (ms, ml) = bytes_of(&top, 3).context("no memory message")?;
        let mem = fields(&p, ms, ms + ml).context("bad memory message")?;
        let (rs, rl) = bytes_of(&mem, 1).context("no RAM")?;
        if rl < RAM_SIZE {
            bail!("RAM blob is {rl:#x} bytes");
        }
        let (bs, bl) = bytes_of(&mem, 2).context("no BIOS ROM")?;
        let (hs, hl) = bytes_of(&mem, 4).context("no hardware blob")?;
        let (gs, gl) = bytes_of(&top, 4).context("no registers message")?;
        let regs = fields(&p, gs, gs + gl).context("bad registers message")?;
        let vram = bytes_of(&top, 5)
            .and_then(|(s, l)| fields(&p, s, s + l))
            .and_then(|g| bytes_of(&g, 3))
            .map_or_else(Vec::new, |(s, l)| p[s..s + l].to_vec());
        Ok(Self {
            ram: p[rs..rs + RAM_SIZE].to_vec(),
            hardware: p[hs..hs + hl].to_vec(),
            bios: p[bs..bs + bl].to_vec(),
            vram,
            gpr: packed(&p, bytes_of(&regs, 1).context("no gpr")?).context("gpr")?,
            cp0: packed(&p, bytes_of(&regs, 2).context("no cp0")?).context("cp0")?,
            cp2d: packed(&p, bytes_of(&regs, 3).context("no cp2d")?).context("cp2d")?,
            cp2c: packed(&p, bytes_of(&regs, 4).context("no cp2c")?).context("cp2c")?,
            pc: varint_of(&regs, 5).context("no pc")? as u32,
            in_isr: varint_of(&regs, 18).unwrap_or(0) != 0,
        })
    }

    /// The RAM word at `va` (KSEG0 / KUSEG).
    pub fn u32_at(&self, va: u32) -> u32 {
        let o = (va & 0x1F_FFFF) as usize;
        u32::from_le_bytes([
            self.ram[o],
            self.ram[o + 1],
            self.ram[o + 2],
            self.ram[o + 3],
        ])
    }

    /// A CPU standing on this state: RAM, scratchpad, the I/O window (reads
    /// see the registers as saved, writes are dropped), the BIOS ROM, and the
    /// integer, COP0 and GTE register files.
    pub fn cpu(&self) -> Cpu {
        let mut cpu = Cpu::new();
        cpu.mem.load(0x8000_0000, &self.ram);
        cpu.mem.load(
            0x1F80_0000,
            &self.hardware[..0x400.min(self.hardware.len())],
        );
        if self.hardware.len() > 0x1000 {
            cpu.mem.map_io(&self.hardware[0x1000..]);
        }
        cpu.mem.map_bios(&self.bios);
        cpu.regs.r.copy_from_slice(&self.gpr[..32]);
        cpu.regs.lo = self.gpr[32];
        cpu.regs.hi = self.gpr[33];
        cpu.cop0 = self.cp0;
        cpu.gte.data = self.cp2d;
        cpu.gte.ctrl = self.cp2c;
        cpu
    }
}
