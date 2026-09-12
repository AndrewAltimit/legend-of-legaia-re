//! A tiny R3000 subset - exactly the instructions the injected routines use,
//! delay slots and the `hi`/`lo` multiply-divide pair included - so assembled words can be run against a model in
//! unit tests instead of being read. Memory is a sparse byte map; anything
//! unmapped reads as zero. Test-only.

use std::collections::HashMap;

pub(crate) struct Cpu {
    pub r: [u32; 32],
    pub pc: u32,
    /// The multiply / divide result pair.
    pub hi: u32,
    pub lo: u32,
    pub mem: HashMap<u32, u8>,
    pub steps: usize,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            r: [0; 32],
            pc: 0,
            hi: 0,
            lo: 0,
            mem: HashMap::new(),
            steps: 0,
        }
    }
    pub fn load(&mut self, va: u32, bytes: &[u8]) {
        for (i, b) in bytes.iter().enumerate() {
            self.mem.insert(va + i as u32, *b);
        }
    }
    pub fn load_words(&mut self, va: u32, w: &[u32]) {
        self.load(
            va,
            &w.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>(),
        );
    }
    pub fn rd8(&self, a: u32) -> u8 {
        *self.mem.get(&a).unwrap_or(&0)
    }
    pub fn rd16(&self, a: u32) -> u16 {
        u16::from_le_bytes([self.rd8(a), self.rd8(a + 1)])
    }
    pub fn rd32(&self, a: u32) -> u32 {
        u32::from_le_bytes([
            self.rd8(a),
            self.rd8(a + 1),
            self.rd8(a + 2),
            self.rd8(a + 3),
        ])
    }
    pub fn wr8(&mut self, a: u32, v: u8) {
        self.mem.insert(a, v);
    }
    pub fn wr16(&mut self, a: u32, v: u16) {
        self.load(a, &v.to_le_bytes());
    }
    pub fn wr32(&mut self, a: u32, v: u32) {
        self.load(a, &v.to_le_bytes());
    }
    /// Execute one instruction (with its delay slot for control flow).
    pub fn exec(&mut self, w: u32) {
        let op = w >> 26;
        let rs = ((w >> 21) & 31) as usize;
        let rt = ((w >> 16) & 31) as usize;
        let rd = ((w >> 11) & 31) as usize;
        let sa = (w >> 6) & 31;
        let imm = (w & 0xffff) as u16;
        let simm = imm as i16 as i32 as u32;
        let mut next = self.pc + 4;
        let mut branch: Option<u32> = None;
        match op {
            0 => match w & 0x3f {
                0x00 => self.r[rd] = self.r[rt] << sa,
                0x02 => self.r[rd] = self.r[rt] >> sa,
                0x03 => self.r[rd] = ((self.r[rt] as i32) >> sa) as u32,
                0x04 => self.r[rd] = self.r[rt] << (self.r[rs] & 31),
                0x06 => self.r[rd] = self.r[rt] >> (self.r[rs] & 31),
                0x08 => branch = Some(self.r[rs]),
                0x10 => self.r[rd] = self.hi,
                0x12 => self.r[rd] = self.lo,
                0x18 => {
                    let p = (self.r[rs] as i32 as i64) * (self.r[rt] as i32 as i64);
                    self.lo = p as u32;
                    self.hi = (p >> 32) as u32;
                }
                0x19 => {
                    let p = u64::from(self.r[rs]) * u64::from(self.r[rt]);
                    self.lo = p as u32;
                    self.hi = (p >> 32) as u32;
                }
                0x1b => {
                    // Division by zero is undefined on the R3000; the model
                    // keeps it loud so a routine never leans on it.
                    assert_ne!(self.r[rt], 0, "divu by zero at {:#x}", self.pc);
                    self.lo = self.r[rs] / self.r[rt];
                    self.hi = self.r[rs] % self.r[rt];
                }
                0x09 => {
                    // `jalr rd, rs` - the link register defaults to $ra.
                    let target = self.r[rs];
                    self.r[if rd == 0 { 31 } else { rd }] = self.pc + 8;
                    branch = Some(target);
                }
                0x21 => self.r[rd] = self.r[rs].wrapping_add(self.r[rt]),
                0x23 => self.r[rd] = self.r[rs].wrapping_sub(self.r[rt]),
                0x24 => self.r[rd] = self.r[rs] & self.r[rt],
                0x25 => self.r[rd] = self.r[rs] | self.r[rt],
                0x2a => self.r[rd] = u32::from((self.r[rs] as i32) < (self.r[rt] as i32)),
                0x2b => self.r[rd] = u32::from(self.r[rs] < self.r[rt]),
                f => panic!("unsupported SPECIAL funct {f:#x} at {:#x}", self.pc),
            },
            0x01 => {
                // REGIMM: rt = 0 bltz, 1 bgez.
                let neg = (self.r[rs] as i32) < 0;
                let take = match rt {
                    0 => neg,
                    1 => !neg,
                    o => panic!("unsupported REGIMM {o:#x} at {:#x}", self.pc),
                };
                if take {
                    branch = Some(self.pc.wrapping_add(4).wrapping_add(simm << 2));
                }
            }
            0x02 => branch = Some((self.pc & 0xF000_0000) | ((w & 0x03ff_ffff) << 2)),
            0x03 => {
                self.r[31] = self.pc + 8;
                branch = Some((self.pc & 0xF000_0000) | ((w & 0x03ff_ffff) << 2));
            }
            0x04 => {
                if self.r[rs] == self.r[rt] {
                    branch = Some(self.pc.wrapping_add(4).wrapping_add(simm << 2));
                }
            }
            0x05 => {
                if self.r[rs] != self.r[rt] {
                    branch = Some(self.pc.wrapping_add(4).wrapping_add(simm << 2));
                }
            }
            0x06 => {
                if (self.r[rs] as i32) <= 0 {
                    branch = Some(self.pc.wrapping_add(4).wrapping_add(simm << 2));
                }
            }
            0x07 => {
                if (self.r[rs] as i32) > 0 {
                    branch = Some(self.pc.wrapping_add(4).wrapping_add(simm << 2));
                }
            }
            0x09 => self.r[rt] = self.r[rs].wrapping_add(simm),
            0x0a => self.r[rt] = u32::from((self.r[rs] as i32) < (simm as i32)),
            0x0b => self.r[rt] = u32::from(self.r[rs] < simm),
            0x0c => self.r[rt] = self.r[rs] & u32::from(imm),
            0x0d => self.r[rt] = self.r[rs] | u32::from(imm),
            0x0f => self.r[rt] = u32::from(imm) << 16,
            0x20 => self.r[rt] = self.rd8(self.r[rs].wrapping_add(simm)) as i8 as i32 as u32,
            0x21 => self.r[rt] = self.rd16(self.r[rs].wrapping_add(simm)) as i16 as i32 as u32,
            0x23 => self.r[rt] = self.rd32(self.r[rs].wrapping_add(simm)),
            0x24 => self.r[rt] = u32::from(self.rd8(self.r[rs].wrapping_add(simm))),
            0x25 => self.r[rt] = u32::from(self.rd16(self.r[rs].wrapping_add(simm))),
            0x28 => self.wr8(self.r[rs].wrapping_add(simm), self.r[rt] as u8),
            0x29 => self.wr16(self.r[rs].wrapping_add(simm), self.r[rt] as u16),
            0x2b => self.wr32(self.r[rs].wrapping_add(simm), self.r[rt]),
            o => panic!("unsupported opcode {o:#x} at {:#x}", self.pc),
        }
        self.r[0] = 0;
        if let Some(target) = branch {
            // Delay slot, then the jump. Load delays are not modelled - the
            // routines are asserted delay-safe by the static tests above.
            let slot = self.rd32(self.pc + 4);
            self.pc += 4;
            self.exec_plain(slot);
            next = target;
        }
        self.pc = next;
        self.steps += 1;
    }
    /// A delay-slot instruction (never itself a branch in these routines).
    pub fn exec_plain(&mut self, w: u32) {
        let saved = self.pc;
        self.exec(w);
        assert_eq!(self.pc, saved + 4, "branch in a delay slot");
    }
    /// Run until the PC reaches one of `stops`, or give up.
    pub fn run_until(&mut self, stops: &[u32]) -> u32 {
        while !stops.contains(&self.pc) {
            assert!(self.steps < 10_000, "runaway at {:#x}", self.pc);
            let w = self.rd32(self.pc);
            self.exec(w);
        }
        self.pc
    }
}
