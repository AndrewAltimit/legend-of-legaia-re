use super::*;

impl Gte {
    // ---------------------------------------------------------------------
    // Register-transfer + memory ops.
    //
    // The PSX cop2 (GTE) sits behind four MIPS instructions for moving data
    // between the CPU register file and the cop2 register file:
    //
    //   - MFC2 rt, rd        -- CPU rt ← data register rd
    //   - MTC2 rt, rd        -- data register rd ← CPU rt
    //   - CFC2 rt, rd        -- CPU rt ← control register rd
    //   - CTC2 rt, rd        -- control register rd ← CPU rt
    //
    // …plus two memory ops:
    //
    //   - LWC2 rd, off(base) -- data register rd ← *(base + off)
    //   - SWC2 rd, off(base) -- *(base + off) ← data register rd
    //
    // The retail TMD renderer + lighting pipeline use these heavily - every
    // vertex load is `LWC2 cop2cr0..cop2cr5` (V0/V1/V2 packed pairs), every
    // captured RGB writeback is `SWC2 cop2cr20..22`. Engines that want to
    // replay a captured GTE trace exactly need this transport layer.
    //
    // The data/control register indices match the public cop2 layout
    // (Nocash PSX hardware reference).
    // ---------------------------------------------------------------------

    /// Read one of the 32 cop2 data registers (cop2cr0..cop2cr31).
    /// Returns the raw 32-bit value - the same layout an MFC2 instruction
    /// would observe in the receiving CPU register.
    pub fn read_data(&self, idx: u8) -> u32 {
        match idx & 0x1F {
            0 => pack_i16_lo_hi(self.v[0].x as i16, self.v[0].y as i16),
            1 => sign_extend_i16(self.v[0].z as i16),
            2 => pack_i16_lo_hi(self.v[1].x as i16, self.v[1].y as i16),
            3 => sign_extend_i16(self.v[1].z as i16),
            4 => pack_i16_lo_hi(self.v[2].x as i16, self.v[2].y as i16),
            5 => sign_extend_i16(self.v[2].z as i16),
            6 => u32::from_le_bytes(self.rgbc),
            7 => self.otz as u32,
            8 => sign_extend_i16(self.ir0 as i16),
            9 => sign_extend_i16(self.ir1 as i16),
            10 => sign_extend_i16(self.ir2 as i16),
            11 => sign_extend_i16(self.ir3 as i16),
            12 => pack_i16_lo_hi(self.sxy_fifo[0].x as i16, self.sxy_fifo[0].y as i16),
            13 => pack_i16_lo_hi(self.sxy_fifo[1].x as i16, self.sxy_fifo[1].y as i16),
            14 | 15 => pack_i16_lo_hi(self.sxy_fifo[2].x as i16, self.sxy_fifo[2].y as i16),
            16 => self.sz_fifo[0] as u32,
            17 => self.sz_fifo[1] as u32,
            18 => self.sz_fifo[2] as u32,
            19 => self.sz_fifo[3] as u32,
            20 => u32::from_le_bytes(self.rgb_fifo[0]),
            21 => u32::from_le_bytes(self.rgb_fifo[1]),
            22 => u32::from_le_bytes(self.rgb_fifo[2]),
            23 => self.res1,
            24 => self.mac0 as u32,
            25 => clamp_i32_from_i64(self.mac1) as u32,
            26 => clamp_i32_from_i64(self.mac2) as u32,
            27 => clamp_i32_from_i64(self.mac3) as u32,
            // IRGB / ORGB read the IR1/IR2/IR3 saturation as a 15-bit BGR555
            // packed colour (Nocash PSX cop2cr28/cr29 read shape).
            28 | 29 => packed_irgb(self.ir1, self.ir2, self.ir3),
            // LZCS / LZCR - `LZCS` is the source the next read of LZCR will
            // count leading zeros / ones on. We expose the raw cached value
            // and the count.
            30 => self.lzcs as u32,
            31 => count_leading_same(self.lzcs),
            _ => unreachable!(),
        }
    }

    /// Write one of the 32 cop2 data registers (MTC2 / LWC2 destination).
    /// Most writes mirror straight back into the typed register file; the
    /// SXY FIFO slots advance / push as the hardware does.
    pub fn write_data(&mut self, idx: u8, val: u32) {
        match idx & 0x1F {
            0 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.v[0].x = lo as i32;
                self.v[0].y = hi as i32;
            }
            1 => self.v[0].z = (val as i32 as i16) as i32,
            2 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.v[1].x = lo as i32;
                self.v[1].y = hi as i32;
            }
            3 => self.v[1].z = (val as i32 as i16) as i32,
            4 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.v[2].x = lo as i32;
                self.v[2].y = hi as i32;
            }
            5 => self.v[2].z = (val as i32 as i16) as i32,
            6 => self.rgbc = val.to_le_bytes(),
            7 => self.otz = (val & 0xFFFF) as u16,
            8 => self.ir0 = (val as i32 as i16) as i32,
            9 => self.ir1 = (val as i32 as i16) as i32,
            10 => self.ir2 = (val as i32 as i16) as i32,
            11 => self.ir3 = (val as i32 as i16) as i32,
            12 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.sxy_fifo[0] = ScreenXY::new(lo as i32, hi as i32);
            }
            13 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.sxy_fifo[1] = ScreenXY::new(lo as i32, hi as i32);
            }
            14 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.sxy_fifo[2] = ScreenXY::new(lo as i32, hi as i32);
            }
            // SXYP - write-only "push": SXY0 ← SXY1 ← SXY2 ← new.
            15 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.sxy_fifo[0] = self.sxy_fifo[1];
                self.sxy_fifo[1] = self.sxy_fifo[2];
                self.sxy_fifo[2] = ScreenXY::new(lo as i32, hi as i32);
            }
            16 => self.sz_fifo[0] = (val & 0xFFFF) as u16,
            17 => self.sz_fifo[1] = (val & 0xFFFF) as u16,
            18 => self.sz_fifo[2] = (val & 0xFFFF) as u16,
            19 => self.sz_fifo[3] = (val & 0xFFFF) as u16,
            20 => self.rgb_fifo[0] = val.to_le_bytes(),
            21 => self.rgb_fifo[1] = val.to_le_bytes(),
            22 => self.rgb_fifo[2] = val.to_le_bytes(),
            23 => self.res1 = val,
            24 => self.mac0 = val as i32,
            25 => self.mac1 = val as i32 as i64,
            26 => self.mac2 = val as i32 as i64,
            27 => self.mac3 = val as i32 as i64,
            28 => {
                // IRGB write: unpack 15-bit BGR555 and broadcast to IR1/2/3.
                let r = (val & 0x1F) as i32 * 0x80;
                let g = ((val >> 5) & 0x1F) as i32 * 0x80;
                let b = ((val >> 10) & 0x1F) as i32 * 0x80;
                self.ir1 = r;
                self.ir2 = g;
                self.ir3 = b;
            }
            // ORGB and LZCR are read-only on hardware; ignore writes.
            29 | 31 => {}
            // LZCS write caches the source for the next LZCR read.
            30 => self.lzcs = val as i32,
            _ => unreachable!(),
        }
    }

    /// Read one of the 32 cop2 control registers (cop2cr32..cop2cr63 in
    /// hardware terms, indexed 0..31 here).
    pub fn read_ctrl(&self, idx: u8) -> u32 {
        match idx & 0x1F {
            0 => pack_i16_lo_hi(self.rot.m[0][0], self.rot.m[0][1]),
            1 => pack_i16_lo_hi(self.rot.m[0][2], self.rot.m[1][0]),
            2 => pack_i16_lo_hi(self.rot.m[1][1], self.rot.m[1][2]),
            3 => pack_i16_lo_hi(self.rot.m[2][0], self.rot.m[2][1]),
            4 => sign_extend_i16(self.rot.m[2][2]),
            5 => self.trans.x as u32,
            6 => self.trans.y as u32,
            7 => self.trans.z as u32,
            8 => pack_i16_lo_hi(self.light.m[0][0], self.light.m[0][1]),
            9 => pack_i16_lo_hi(self.light.m[0][2], self.light.m[1][0]),
            10 => pack_i16_lo_hi(self.light.m[1][1], self.light.m[1][2]),
            11 => pack_i16_lo_hi(self.light.m[2][0], self.light.m[2][1]),
            12 => sign_extend_i16(self.light.m[2][2]),
            13 => self.back_color.x as u32,
            14 => self.back_color.y as u32,
            15 => self.back_color.z as u32,
            16 => pack_i16_lo_hi(self.light_color.m[0][0], self.light_color.m[0][1]),
            17 => pack_i16_lo_hi(self.light_color.m[0][2], self.light_color.m[1][0]),
            18 => pack_i16_lo_hi(self.light_color.m[1][1], self.light_color.m[1][2]),
            19 => pack_i16_lo_hi(self.light_color.m[2][0], self.light_color.m[2][1]),
            20 => sign_extend_i16(self.light_color.m[2][2]),
            21 => self.far_color.x as u32,
            22 => self.far_color.y as u32,
            23 => self.far_color.z as u32,
            24 => self.ofx as u32,
            25 => self.ofy as u32,
            26 => (self.h as u32) & 0xFFFF,
            27 => self.dqa as u32,
            28 => self.dqb as u32,
            29 => (self.zsf3 as u32) & 0xFFFF,
            30 => (self.zsf4 as u32) & 0xFFFF,
            31 => self.flag,
            _ => unreachable!(),
        }
    }

    /// Write one of the 32 cop2 control registers (CTC2 / LWC2 destination).
    pub fn write_ctrl(&mut self, idx: u8, val: u32) {
        match idx & 0x1F {
            0 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.rot.m[0][0] = lo;
                self.rot.m[0][1] = hi;
            }
            1 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.rot.m[0][2] = lo;
                self.rot.m[1][0] = hi;
            }
            2 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.rot.m[1][1] = lo;
                self.rot.m[1][2] = hi;
            }
            3 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.rot.m[2][0] = lo;
                self.rot.m[2][1] = hi;
            }
            4 => self.rot.m[2][2] = val as i32 as i16,
            5 => self.trans.x = val as i32,
            6 => self.trans.y = val as i32,
            7 => self.trans.z = val as i32,
            8 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light.m[0][0] = lo;
                self.light.m[0][1] = hi;
            }
            9 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light.m[0][2] = lo;
                self.light.m[1][0] = hi;
            }
            10 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light.m[1][1] = lo;
                self.light.m[1][2] = hi;
            }
            11 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light.m[2][0] = lo;
                self.light.m[2][1] = hi;
            }
            12 => self.light.m[2][2] = val as i32 as i16,
            13 => self.back_color.x = val as i32,
            14 => self.back_color.y = val as i32,
            15 => self.back_color.z = val as i32,
            16 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light_color.m[0][0] = lo;
                self.light_color.m[0][1] = hi;
            }
            17 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light_color.m[0][2] = lo;
                self.light_color.m[1][0] = hi;
            }
            18 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light_color.m[1][1] = lo;
                self.light_color.m[1][2] = hi;
            }
            19 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light_color.m[2][0] = lo;
                self.light_color.m[2][1] = hi;
            }
            20 => self.light_color.m[2][2] = val as i32 as i16,
            21 => self.far_color.x = val as i32,
            22 => self.far_color.y = val as i32,
            23 => self.far_color.z = val as i32,
            24 => self.ofx = val as i32,
            25 => self.ofy = val as i32,
            26 => self.h = (val & 0xFFFF) as i32,
            27 => self.dqa = val as i32,
            28 => self.dqb = val as i32,
            29 => self.zsf3 = (val & 0xFFFF) as i16 as i32,
            30 => self.zsf4 = (val & 0xFFFF) as i16 as i32,
            31 => self.flag = val,
            _ => unreachable!(),
        }
    }

    /// `MFC2` - move from cop2 data register `rd` to a returned `u32`. CPU
    /// callers stash the result in their integer register file.
    pub fn mfc2(&mut self, rd: u8) -> u32 {
        // MFC2 has a 1-cycle stall (no GTE op charge); we model it as a
        // single cycle to keep the pacing accumulator monotonic.
        self.cycles = self.cycles.saturating_add(1);
        self.read_data(rd)
    }

    /// `MTC2` - move CPU `val` into cop2 data register `rd`.
    pub fn mtc2(&mut self, rd: u8, val: u32) {
        self.cycles = self.cycles.saturating_add(1);
        self.write_data(rd, val);
    }

    /// `CFC2` - move from cop2 control register `rd`.
    pub fn cfc2(&mut self, rd: u8) -> u32 {
        self.cycles = self.cycles.saturating_add(1);
        self.read_ctrl(rd)
    }

    /// `CTC2` - move CPU `val` into cop2 control register `rd`.
    pub fn ctc2(&mut self, rd: u8, val: u32) {
        self.cycles = self.cycles.saturating_add(1);
        self.write_ctrl(rd, val);
    }

    /// `LWC2 rd, off(base)` - load 32 bits from memory and write into cop2
    /// data register `rd`. The caller supplies a [`Cop2Mem`] for the actual
    /// load - the GTE doesn't model main memory itself.
    ///
    /// The effective address is `base + off` (the `off` is sign-extended to
    /// 32 bits by the MIPS pipeline before the call). The host's memory
    /// implementation is responsible for the alignment guarantee - most
    /// retail traces hit aligned addresses.
    pub fn lwc2<M: Cop2Mem + ?Sized>(&mut self, mem: &mut M, rd: u8, addr: u32) {
        self.cycles = self.cycles.saturating_add(1);
        let val = mem.cop2_load(addr);
        self.write_data(rd, val);
    }

    /// `SWC2 rd, off(base)` - store cop2 data register `rd` into memory.
    pub fn swc2<M: Cop2Mem + ?Sized>(&mut self, mem: &mut M, rd: u8, addr: u32) {
        self.cycles = self.cycles.saturating_add(1);
        let val = self.read_data(rd);
        mem.cop2_store(addr, val);
    }

    /// Bulk load V0/V1/V2 from three consecutive packed vertices at `addr`.
    /// Each vertex is 8 bytes (xy as a packed u32 at +0, z sign-extended in
    /// the next u32 at +4); the helper consumes 24 bytes total. Mirrors the
    /// canonical retail emit:
    ///
    /// ```text
    /// LWC2 0, 0(t0)    # V0.xy
    /// LWC2 1, 4(t0)    # V0.z
    /// LWC2 2, 8(t0)    # V1.xy
    /// LWC2 3, 12(t0)   # V1.z
    /// LWC2 4, 16(t0)   # V2.xy
    /// LWC2 5, 20(t0)   # V2.z
    /// ```
    pub fn load_vertices<M: Cop2Mem + ?Sized>(&mut self, mem: &mut M, addr: u32) {
        for i in 0..3u32 {
            let xy_off = addr + i * 8;
            let z_off = xy_off + 4;
            self.lwc2(mem, (i as u8) * 2, xy_off);
            self.lwc2(mem, (i as u8) * 2 + 1, z_off);
        }
    }
}
