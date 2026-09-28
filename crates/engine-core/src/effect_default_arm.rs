//! The render dispatcher's **default draw-kind-4 emitter** - the procedural
//! ring / disc / fan builder every render-mode-4 node without a `0x2000` or
//! `0x4000` bit in `+0x9E` draws through - as a per-frame mesh.
//!
//! `FUN_8001ADA4` case 4 builds its node's geometry into the scratch block
//! `*(0x8007B85C) + 0x5DC00`, points every slot of the node's model list
//! `+0x44` at `out + 0xC` (`0x8001B08C..0x8001B0B4`) and, when
//! `+0x9E & 0x6000 == 0`, calls `FUN_80028158(out, +0x9E, (s16)+0x9C +
//! (((s16)+0xC8 >> 3) << 8), actor + 0x9C)` (`0x8001B128..0x8001B15C`). The
//! case then falls into the ordinary model draw at `0x8001B160`, so the
//! result is drawn exactly like a mesh part - the same path the `0x4000`
//! sprite arm ([`crate::effect_sprite_arm`]) takes: scaled by
//! `+0x72 / 0x1000`, turned by the rotation banks, the colour word `+0x74`
//! and level `+0x78` ORing ABE / ABR into the packets and depth-cueing the
//! packet colours ([`crate::baka_impact_fx::ColourWord`]). The builder is SCUS
//! code, so it draws in every mode.
//!
//! The same builder has three direct callers besides the dispatcher: the
//! battle per-actor draw's **ground shadow** (`FUN_80048A08`,
//! `jal` at `0x8004927C`, mode `1`, 24 columns - [`ground_shadow_mesh`]), the
//! field battle-intro ring (`overlay_field_battle_intro_801d1cfc`, mode `0`,
//! 96 columns) and Baka Fighter's floor disc (`overlay_baka_fighter_801d6bb8`,
//! mode `1`, 24 columns).
//!
//! ## What the builder writes
//!
//! [`build`] is a transliteration of the disassembly
//! (`ghidra/scripts/funcs/80028158.txt`, `0x80028158..0x80029720`) onto a byte
//! buffer: it writes exactly the words retail writes, at the offsets retail
//! writes them, and records which bytes it touched - retail never clears the
//! scratch block, so a byte the builder does not write keeps whatever the
//! previous draw left there. The output is a Legaia TMD object at `out + 0xC`
//! (vertex top `out + 0x28`, no normals, one primitive group of `GT4`
//! packets - `flags 0x26`, `ilen 9`, `mode 0x3C`, the ribbon's row) followed by
//! twenty zero words. [`decode`] reads that object back into quads.
//!
//! The arguments: `mode` = `+0x9E`, `packed` = the count word above
//! (`count = packed & 0xFF`, `phase = packed >> 8`, a logical shift), and
//! `src` = the node's `+0x9C..+0xC0` block - `+0x04` / `+0x08` the two colour
//! words (`+0xA0` / `+0xA4`), `+0x0C..+0x12` a UV rectangle, `+0x14` / `+0x16`
//! tpage / CLUT, `+0x18` / `+0x1A` the inner / outer radius (`+0xB4` /
//! `+0xB6`, negative clamped to zero), `+0x1C` / `+0x1E` the inner / outer
//! height (`+0xB8` / `+0xBA`), `+0x20` / `+0x22` the two in-plane scales
//! (`+0xBC` / `+0xBE`, `0x1000` = 1).
//!
//! The mode word splits three ways:
//!
//! * `mode & 3` - the **plane**: which vertex component each of the builder's
//!   three axes lands in. `0`: `(a, b, c)` -> `(+0, +2, +4)`; `1`: `(+0, +4,
//!   +2)`; `2`: `(+4, +2, +0)`; `3`: `(+4, +0, +2)` (`0x800283D8..0x800284A0`).
//! * `(mode >> 3) & 0xF` - the **shape**, through the eight-entry jump table
//!   at `0x80010BC0` (shapes `0/4/5/6/7` share arm `0x80028284`, `1/3` arm
//!   `0x80028310`, `2` arm `0x8002833C`). A shape `>= 8` skips the table and
//!   reads stack words no path initialised, so [`build`] returns `None`.
//! * `mode >> 8` - the **texture mode**, but only for shape `0`: any other
//!   shape masks `mode` to its low byte first (`0x800281F0`). Zero samples the
//!   fixed 2x2 patch `(0..2, 0xF0..0xF2)` of page `0x001F` / CLUT `0x7F84`;
//!   non-zero subdivides every quad four ways radially and maps `src`'s UV
//!   rectangle onto it ([`TextureMode`]).
//!
//! | shape | geometry |
//! |---|---|
//! | `0` | ring of `count` columns (`count >= 3`), an inner and an outer vertex each; `radius 0` inner makes a disc. `phase == 0` turns the ring half a column; `0 < phase < count` draws an open arc of `phase` quads over `count + 1` columns. |
//! | `1` | per column three inner/outer pairs at angle `+0`, `+phase`, `+2 phase`, plus an extrapolated vertex; three quads a column (a crown / fin). |
//! | `2` | per column three pairs; vertex 2 is moved to the origin and every column fans one quad to it (a star). |
//! | `3` | as `1` with two degenerate quads - a triangle crown. |
//! | `4` | ring whose radius jitters per column by `rand()` (`FUN_80056798`, three calls a column). |
//! | `5` | as `4`, offset so the ring's first inner vertex is the origin, with the height ramping `c0 -> c1` across the columns. |
//! | `6` | ring whose outer vertex is squashed onto the inner one's `b` coordinate. |
//! | `7` | ring whose inner vertices are shifted so the column at angle `-phase - 0x800` pivots at the origin. |
//!
//! A census of the render-mode-4 nodes in the 98 catalogued mednafen states
//! finds modes `0x00`, `0x01`, `0x08`, `0x10`, `0x18` and `0x100` - shapes `0..3`
//! and texture mode `1` - and none of shapes `4..7`.
//!
//! PORT: FUN_80028158
//!
//! REF: FUN_8001ADA4 (case 4's default arm, `0x8001B128..0x8001B160`),
//! FUN_80048A08 (the ground-shadow call), FUN_80056798 (`rand`)

use legaia_engine_vm::move_vm::ActorState;

use crate::baka_impact_fx::ColourWord;
use crate::effect_ribbon::TrigTable;

/// Texture page word the untextured modes' packets carry (`li a3,0x1f` at
/// `0x80028C74`) - the same 2x2 patch the ribbon samples.
pub const DEFAULT_ARM_TPAGE: u16 = 0x001F;
/// CLUT word the untextured modes' packets carry (`li t0,0x7f84` at
/// `0x80028C78`).
pub const DEFAULT_ARM_CLUT: u16 = 0x7F84;
/// Bytes one `GT4` packet occupies (`ilen 9`).
pub const PACKET_BYTES: u32 = 0x24;
/// Where retail builds the node's geometry: `*(0x8007B85C) + 0x5DC00`.
pub const SCRATCH_OFFSET: u32 = 0x5_DC00;
/// Where the battle ground shadow is built: `*(0x8007B85C) + 0x62400`.
pub const SHADOW_SCRATCH_OFFSET: u32 = 0x6_2400;
/// The shadow disc's column count (`li a2,0x18` at `0x80049240`).
pub const SHADOW_SEGMENTS: u32 = 0x18;
/// The shadow's draw flag word (`lui a1,0x8a00` at `0x800492A8`):
/// semi-transparent, blend rule `2` (`B - F`).
pub const SHADOW_FLAG_WORD: u32 = 0x8A00_0000;

/// `mode >> 8` for shape `0`: how the packets are textured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureMode {
    /// `0` - one quad a column, the fixed 2x2 patch.
    Patch,
    /// `1` - polar map: the inner edge at the rectangle's centre, the outer
    /// at `0xFF / 0x100` of its half-extent (`0x80028CF8`).
    Polar,
    /// `2` - the rectangle on every column, `u` across the ring.
    Rect,
    /// `3` - the rectangle turned a quarter.
    RectTurned,
    /// `4` - polar with the inner edge at `r0 / r1` of the half-extent.
    PolarAnnulus,
    /// Any other value: retail leaves the first packet's UV words stale.
    Stale(u32),
}

impl TextureMode {
    /// Classify `mode >> 8`.
    pub fn from_sub(sub: u32) -> Self {
        match sub {
            0 => Self::Patch,
            1 => Self::Polar,
            2 => Self::Rect,
            3 => Self::RectTurned,
            4 => Self::PolarAnnulus,
            n => Self::Stale(n),
        }
    }
}

/// The builder's output: the bytes from `out` up to the end of the zero tail,
/// and which of them it wrote.
#[derive(Debug, Clone)]
pub struct Build {
    /// Virtual address of `out` (the pointers in the object header are
    /// absolute, as retail's are).
    pub out_va: u32,
    /// `out[0..]`.
    pub bytes: Vec<u8>,
    /// `true` where the builder stored a byte.
    pub written: Vec<bool>,
    /// The shape (`(mode >> 3) & 0xF`).
    pub shape: u32,
}

/// Byte buffer addressed by virtual address, with a pad below `out` for the
/// few stores retail makes at negative packet offsets.
struct Mem {
    lo: u32,
    bytes: Vec<u8>,
    written: Vec<bool>,
}

impl Mem {
    const PAD: u32 = 0x200;

    fn new(out_va: u32, len: u32) -> Self {
        let n = (Self::PAD + len) as usize;
        Self {
            lo: out_va.wrapping_sub(Self::PAD),
            bytes: vec![0; n],
            written: vec![false; n],
        }
    }
    fn idx(&self, va: u32) -> Option<usize> {
        let i = va.wrapping_sub(self.lo) as usize;
        (i < self.bytes.len()).then_some(i)
    }
    fn sb(&mut self, va: u32, v: u32) {
        if let Some(i) = self.idx(va) {
            self.bytes[i] = v as u8;
            self.written[i] = true;
        }
    }
    fn sh(&mut self, va: u32, v: u32) {
        self.sb(va, v);
        self.sb(va.wrapping_add(1), v >> 8);
    }
    fn sw(&mut self, va: u32, v: u32) {
        self.sh(va, v);
        self.sh(va.wrapping_add(2), v >> 16);
    }
    fn lbu(&self, va: u32) -> u32 {
        self.idx(va).map_or(0, |i| u32::from(self.bytes[i]))
    }
    fn lhu(&self, va: u32) -> u32 {
        self.lbu(va) | (self.lbu(va.wrapping_add(1)) << 8)
    }
    fn lh(&self, va: u32) -> i32 {
        i32::from(self.lhu(va) as u16 as i16)
    }
    fn lw(&self, va: u32) -> u32 {
        self.lhu(va) | (self.lhu(va.wrapping_add(2)) << 16)
    }
}

/// `src` halfword / word reads (the node's `+0x9C` block).
fn src_hu(src: &[u8], off: usize) -> u32 {
    u32::from(u16::from_le_bytes([src[off], src[off + 1]]))
}
fn src_h(src: &[u8], off: usize) -> i32 {
    i32::from(i16::from_le_bytes([src[off], src[off + 1]]))
}
fn src_w(src: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([src[off], src[off + 1], src[off + 2], src[off + 3]])
}

/// `mult` + `mflo` + `sra 0xc`: the low word of the product, arithmetic
/// shift.
fn mul12(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b) >> 12
}

/// MIPS `div` remainder (`mfhi`), with the R3000's divide-by-zero result
/// (`HI = dividend`).
fn mips_rem(a: i32, b: i32) -> i32 {
    if b == 0 { a } else { a.wrapping_rem(b) }
}

/// MIPS `div` quotient (`mflo`), with the R3000's divide-by-zero result
/// (`-1` for a non-negative dividend, `1` for a negative one).
fn mips_div(a: i32, b: i32) -> i32 {
    if b == 0 {
        if a >= 0 { -1 } else { 1 }
    } else {
        a.wrapping_div(b)
    }
}

/// Run the builder.
///
/// PORT: FUN_80028158 (`0x80028158..0x80029720`, the whole routine)
///
/// `src` is the `0x24`-byte block at the call's fourth argument; `rand` is
/// `FUN_80056798` (only shapes `4` / `5` call it, three times a column);
/// `screen_page` is the halfword at `0x8007B74C`, which picks between the two
/// 15-bit pages `0x100` / `0x110` when a textured node's tpage word carries
/// bit `0x4000` (`0x80028C3C..0x80028C64`). `None` for a shape `>= 8`, whose
/// path reads uninitialised stack words.
pub fn build<T: TrigTable, R: FnMut() -> u32>(
    out_va: u32,
    mode: u32,
    packed: u32,
    src: &[u8; 0x24],
    trig: &T,
    mut rand: R,
    screen_page: i16,
) -> Option<Build> {
    let shape = (mode >> 3) & 0xF;
    if shape >= 8 {
        return None;
    }
    // Registers / stack slots, named for what they carry.
    let mut count = packed & 0xFF; // t8
    let mut phase = packed >> 8; // sp28 (srl: never negative)
    let mut sub_mode = if shape != 0 { mode & 0xFF } else { mode }; // spC4
    let mut r0 = src_hu(src, 0x18) as u16; // sp30
    let mut r1 = src_hu(src, 0x1A) as u16; // sp38
    let c0 = src_h(src, 0x1C); // sp40
    let c1 = src_h(src, 0x1E); // sp44
    if (r0 as i16) < 0 {
        r0 = 0;
    }
    if (r1 as i16) < 0 {
        r1 = 0;
    }
    // `divu 0x1000, t8` on the call's own count, before any arm fixes a zero.
    // (`divu` by zero leaves `LO = 0xFFFFFFFF` on the R3000.)
    let step = 0x1000u32.checked_div(count).unwrap_or(u32::MAX); // sp20
    let orig_count = count as i32; // s6
    let mut arc = 0u32; // sp50
    let pairs; // sp90
    let (nvert, mut nprim);
    match shape {
        0 | 4..=7 => {
            // 0x80028284
            if count < 3 {
                count = 3;
            }
            if phase == 0 {
                phase = step >> 1;
                nprim = count;
            } else if phase < count {
                arc = phase;
                count += 1;
                nprim = arc;
                phase = 0;
            } else {
                phase = 0;
                nprim = count;
            }
            nvert = count * 2;
            pairs = 1;
        }
        1 | 3 => {
            // 0x80028310
            if count == 0 {
                count = 1;
            }
            pairs = 3;
            nvert = count << 3;
            nprim = count * 3;
        }
        _ => {
            // 2: 0x8002833C
            if count == 0 {
                count = 1;
            }
            phase = (0x1000 / count) >> 1;
            pairs = 3;
            nprim = count;
            nvert = count * 6;
        }
    }
    let mut nvert = nvert;
    if sub_mode >> 8 != 0 {
        nprim <<= 2;
        nvert = (nvert >> 1) * 5;
    }
    let obj = out_va.wrapping_add(0xC);
    let vtop = out_va.wrapping_add(0x28);
    let prim_top = vtop.wrapping_add(nvert << 3);
    let len = 0x28 + (nvert << 3) + 8 + nprim.max(4) * PACKET_BYTES + 0x50 + 0x40;
    let mut m = Mem::new(out_va, len);
    m.sw(out_va, 0);
    m.sw(out_va.wrapping_add(4), 0);
    m.sw(out_va.wrapping_add(8), 0);
    m.sw(obj.wrapping_add(0x14), nprim);
    m.sw(obj.wrapping_add(0x4), nvert);
    m.sw(obj, vtop);
    m.sw(obj.wrapping_add(0xC), 0);
    m.sw(obj.wrapping_add(0x10), prim_top);
    // Plane: component pointers A / B / C (sp14 / sp18 / sp1C).
    let (a_off, b_off, c_off) = match mode & 3 {
        0 => (0, 2, 4),
        1 => (0, 4, 2),
        2 => (4, 2, 0),
        _ => (4, 0, 2),
    };
    let pa = vtop.wrapping_add(a_off);
    let pb = vtop.wrapping_add(b_off);
    let pc = vtop.wrapping_add(c_off);
    sub_mode >>= 8;
    let sx = src_hu(src, 0x20) as i32; // sp48
    let sy = src_hu(src, 0x22) as i32; // sp4C
    let trig_at = |angle: u32| -> (i32, i32) {
        let a = (angle & 0xFFF) as i32;
        (mul12(trig.sin(a), sx), mul12(trig.cos(a), sy))
    };
    let r0s = i32::from(r0 as i16);
    let r1s = i32::from(r1 as i16);
    let mut t4: u32 = 0; // vertex cursor, halfwords
    if shape == 4 || shape == 5 {
        // 0x800288AC
        let (mut s4, mut s3) = (0i32, 0i32);
        if shape == 5 {
            let (s0, s1) = trig_at(0u32.wrapping_sub(phase).wrapping_sub(0x400));
            s4 = mul12(s0, r0s);
            s3 = mul12(s1, r0s);
        }
        let half = r1s >> 1; // sp5C
        let mut ang = 0u32; // sp7C
        let mut qa = pa.wrapping_add(t4 * 2); // s8
        let mut qb = pb.wrapping_add(t4 * 2); // sp80
        let mut qc = pc.wrapping_add(t4 * 2); // sp90
        for col in 0..count {
            let (s0, s1) = trig_at(ang.wrapping_sub(phase).wrapping_sub(0x400));
            let r = mips_rem(rand() as i32, r1s);
            let rr = r0s.wrapping_add(r - half); // sp94
            m.sh(qa, (mul12(s0, rr) - s4) as u32);
            m.sh(qb, (mul12(s1, rr) - s3) as u32);
            let jitter = |v: u32| rr.wrapping_add((((v & 0xF) + 1) << 2) as i32);
            if shape == 4 {
                let ja = jitter(rand());
                m.sh(qa.wrapping_add(8), (mul12(s0, ja) - s4) as u32);
                let jb = jitter(rand());
                m.sh(qb.wrapping_add(8), (mul12(s1, jb) - s3) as u32);
                m.sh(qc, c0 as u32);
                m.sh(qc.wrapping_add(8), c1 as u32);
            } else {
                let ja = jitter(rand());
                m.sh(qa.wrapping_add(8), (mul12(s0, ja) - s4) as u32);
                let ramp = mips_div(c1.wrapping_sub(c0), orig_count);
                let jb = jitter(rand());
                m.sh(qb.wrapping_add(8), (mul12(s1, jb) - s3) as u32);
                let h = c0.wrapping_add(ramp.wrapping_mul(col as i32));
                m.sh(qc, h as u32);
                m.sh(qc.wrapping_add(8), h as u32);
            }
            qa = qa.wrapping_add(0x10);
            qb = qb.wrapping_add(0x10);
            qc = qc.wrapping_add(0x10);
            ang = ang.wrapping_add(step);
        }
    } else {
        // 0x800284D8
        let (mut s4, mut s3) = (0i32, 0i32);
        if shape == 7 {
            let (s0, s1) = trig_at(0u32.wrapping_sub(phase).wrapping_sub(0x800));
            s4 = mul12(s0, r1s) - mul12(s0, r0s);
            s3 = mul12(s1, r1s) - mul12(s1, r0s);
        }
        let mut col_ang = 0u32; // sp94
        for _ in 0..count {
            let mut t0 = pa.wrapping_add(t4 * 2);
            let mut a3 = pb.wrapping_add(t4 * 2);
            let mut t3 = t4 * 2;
            let mut t6 = 0u32;
            for _ in 0..pairs {
                let (s0, s1) = trig_at(
                    col_ang
                        .wrapping_add(t6)
                        .wrapping_sub(phase)
                        .wrapping_sub(0x400),
                );
                if shape == 6 {
                    m.sh(t0, mul12(s0, r0s) as u32);
                    let b = mul12(s1, r0s);
                    m.sh(a3, b as u32);
                    m.sh(t0.wrapping_add(8), mul12(mul12(s0, r1s), r0s) as u32);
                    m.sh(a3.wrapping_add(8), b as u32);
                } else {
                    m.sh(t0, (mul12(s0, r0s) - s4) as u32);
                    m.sh(a3, (mul12(s1, r0s) - s3) as u32);
                    m.sh(t0.wrapping_add(8), mul12(s0, r1s) as u32);
                    m.sh(a3.wrapping_add(8), mul12(s1, r1s) as u32);
                }
                let t1 = pc.wrapping_add(t3);
                m.sh(t1, c0 as u32);
                m.sh(t1.wrapping_add(8), c1 as u32);
                if sub_mode != 0 {
                    t3 = t3.wrapping_add(0x18);
                    t4 = t4.wrapping_add(0xC);
                    let da = m.lh(t0.wrapping_add(8)) - m.lh(t0);
                    let db = m.lh(a3.wrapping_add(8)) - m.lh(a3);
                    let dc = c1 - m.lh(t1);
                    for (p, d) in [(t0, da), (a3, db), (t1, dc)] {
                        let base = m.lhu(p);
                        m.sh(p.wrapping_add(0x10), base.wrapping_add((d >> 2) as u32));
                        m.sh(
                            p.wrapping_add(0x18),
                            base.wrapping_add(((d << 1) >> 2) as u32),
                        );
                        m.sh(
                            p.wrapping_add(0x20),
                            base.wrapping_add((((d << 1) + d) >> 2) as u32),
                        );
                    }
                    t0 = t0.wrapping_add(0x18);
                    a3 = a3.wrapping_add(0x18);
                }
                a3 = a3.wrapping_add(0x10);
                t0 = t0.wrapping_add(0x10);
                t3 = t3.wrapping_add(0x10);
                t4 = t4.wrapping_add(8);
                t6 = t6.wrapping_add(phase);
            }
            if shape == 1 || shape == 3 {
                t4 = t4.wrapping_add(8);
            }
            col_ang = col_ang.wrapping_add(step);
        }
    }

    // 0x80028BE4: the group header.
    let mut t5 = m.lw(obj.wrapping_add(0x10));
    m.sh(t5, m.lhu(obj.wrapping_add(0x14)));
    m.sh(t5.wrapping_add(2), 0x26);
    m.sb(t5.wrapping_add(5), 9);
    m.sb(t5.wrapping_add(7), 0x3C);
    t5 = t5.wrapping_add(8);
    let (tpage, clut) = if sub_mode != 0 {
        let mut a3 = src_hu(src, 0x14);
        if a3 & 0x4000 != 0 {
            let base = if screen_page != 0 { 0x110 } else { 0x100 };
            a3 = (a3 & 0x60) + (a3 & 0xF) + base;
        }
        (a3, src_hu(src, 0x16))
    } else {
        (u32::from(DEFAULT_ARM_TPAGE), u32::from(DEFAULT_ARM_CLUT))
    };
    let u0 = src_hu(src, 0xC);
    let v0 = src_hu(src, 0xE);
    let u1 = src_hu(src, 0x10);
    let v1 = src_hu(src, 0x12);
    let hu = (u1.wrapping_sub(u0) as i32) >> 1;
    let hv = (v1.wrapping_sub(v0) as i32) >> 1;
    let cu = (u0 as i32).wrapping_add(hu);
    let cv = (v0 as i32).wrapping_add(hv);
    let t7 = (src_w(src, 4) & 0xFF_FFFF).wrapping_add(0x3C00_0000);
    let t6 = (src_w(src, 8) & 0xFF_FFFF).wrapping_add(0x3C00_0000);
    match sub_mode {
        1 => {
            r0 = 0;
            r1 = 0xFF;
        }
        4 => {
            let q = mips_div(i32::from(r0 as i16) << 8, i32::from(r1 as i16));
            r1 = 0xFF;
            r0 = q as u16;
        }
        _ => {}
    }

    if sub_mode != 0 {
        // 0x80028D44: four subdivided packets a column.
        let s8 = clut << 16;
        let tp = tpage << 16; // sp94
        let inner = i32::from(r0 as i16); // sp60
        let outer = i32::from(r1 as i16); // sp90
        let mut vbase = 0u32; // sp64
        let (mut w68, mut w6c, mut w70, mut w74) = (
            0x0030_0028u32,
            0x0008_0010u32,
            0x0028_0028u32,
            0x0010_0000u32,
        );
        let mut ang78 = step;
        let mut t3 = t5.wrapping_add(0x8C);
        let polar = |a: u32, r: i32| -> u32 {
            let a = (a & 0xFFF) as i32;
            let (s0, s1) = (trig.sin(a), trig.cos(a));
            let u = cu.wrapping_add(s0.wrapping_mul(r).wrapping_mul(hu) >> 20);
            let v = cv.wrapping_add(s1.wrapping_mul(r).wrapping_mul(hv) >> 20);
            ((v as u32) << 8).wrapping_add(u as u32)
        };
        let mut col = 0u32;
        while col < m.lw(obj.wrapping_add(0x14)) >> 2 {
            match TextureMode::from_sub(sub_mode) {
                TextureMode::Polar | TextureMode::PolarAnnulus => {
                    if col == 0 {
                        let a = 0u32.wrapping_sub(phase).wrapping_sub(0x400);
                        m.sw(t3.wrapping_sub(0x7C), s8.wrapping_add(polar(a, inner)));
                        m.sw(t3.wrapping_sub(0x78), tp.wrapping_add(polar(a, outer)));
                    } else {
                        m.sw(
                            t3.wrapping_sub(0x7C),
                            s8.wrapping_add(m.lhu(t3.wrapping_sub(0x104))),
                        );
                        m.sw(
                            t3.wrapping_sub(0x78),
                            tp.wrapping_add(m.lhu(t3.wrapping_sub(0x96))),
                        );
                    }
                    let a = ang78.wrapping_sub(phase).wrapping_sub(0x400);
                    m.sw(t3.wrapping_sub(0x74), polar(a, inner));
                    m.sh(t3.wrapping_sub(0x72), polar(a, outer));
                }
                TextureMode::Rect => {
                    m.sw(t3.wrapping_sub(0x7C), s8 + (v0 << 8) + u0);
                    m.sw(t3.wrapping_sub(0x78), tp + (v0 << 8) + u1);
                    m.sw(t3.wrapping_sub(0x74), (v1 << 8) + u0);
                    m.sh(t3.wrapping_sub(0x72), (v1 << 8) + u1);
                }
                TextureMode::RectTurned => {
                    m.sw(t3.wrapping_sub(0x7C), s8 + (v0 << 8) + u1);
                    m.sw(t3.wrapping_sub(0x78), tp + (v1 << 8) + u1);
                    m.sw(t3.wrapping_sub(0x74), (v0 << 8) + u0);
                    m.sh(t3.wrapping_sub(0x72), (v1 << 8) + u0);
                }
                TextureMode::Patch | TextureMode::Stale(_) => {}
            }
            // 0x800290E8: copy the first packet's UV words onward.
            let w10 = m.lw(t3.wrapping_sub(0x7C));
            let w14 = m.lw(t3.wrapping_sub(0x78));
            let w18 = m.lw(t3.wrapping_sub(0x74));
            let h1a = m.lhu(t3.wrapping_sub(0x72));
            m.sw(t3.wrapping_sub(0x58), w10);
            m.sw(t3.wrapping_sub(0x54), w14);
            m.sw(t3.wrapping_sub(0x34), w10);
            m.sw(t3.wrapping_sub(0x30), w14);
            m.sw(t3.wrapping_sub(0x10), w10);
            m.sw(t3.wrapping_sub(0xC), w14);
            m.sw(t3.wrapping_sub(0x8), w18);
            m.sh(t3.wrapping_sub(0x6), h1a);
            // Interpolate u, v across the four packets.
            for s6 in 0..2u32 {
                let t4 = t5.wrapping_add(s6);
                let t2 = t5.wrapping_add(0x18 + s6);
                interpolate_bytes(
                    &mut m,
                    t4.wrapping_add(0x1A),
                    t2.wrapping_sub(8),
                    t2.wrapping_sub(4),
                    t2,
                    t2.wrapping_add(2),
                    0x1A,
                    0x22,
                    6,
                    0,
                );
            }
            // Colours and index words.
            m.sw(t5, t7);
            m.sw(t3.wrapping_sub(0x88), t6);
            m.sw(t3.wrapping_sub(0x84), t7);
            m.sw(t3.wrapping_sub(0x80), t6);
            m.sw(t3.wrapping_sub(0x68), t7);
            m.sw(t3.wrapping_sub(0x44), t7);
            m.sw(t3.wrapping_sub(0x20), t7);
            m.sw(t3.wrapping_sub(0x70), w74);
            m.sw(t3.wrapping_sub(0x1C), t6);
            m.sw(t3.wrapping_sub(0x14), t6);
            m.sw(t3.wrapping_sub(0x6C), w70);
            m.sw(t3.wrapping_sub(0x4), w6c);
            m.sw(t3, w68);
            let mut a0 = t5.wrapping_add(0x44);
            for t2 in 0..3u32 {
                let lo = (vbase + t2 + 2) << 3;
                let hi = (vbase + t2 + 7) << 3;
                m.sh(a0.wrapping_sub(0x26), lo);
                m.sh(a0.wrapping_sub(0x22), hi);
                m.sh(a0.wrapping_sub(4), lo);
                m.sh(a0, hi);
                a0 = a0.wrapping_add(PACKET_BYTES);
            }
            // Interpolate r, g, b across the four packets.
            for s6 in 0..3u32 {
                let t4 = t5.wrapping_add(s6);
                let a3 = t5.wrapping_add(8 + s6);
                interpolate_bytes(
                    &mut m,
                    t4.wrapping_add(0xC),
                    t4,
                    a3.wrapping_sub(4),
                    a3,
                    a3.wrapping_add(4),
                    0x18,
                    0x20,
                    8,
                    0,
                );
            }
            vbase += 5;
            w68 = w68.wrapping_add(0x0028_0028);
            w6c = w6c.wrapping_add(0x0028_0028);
            w70 = w70.wrapping_add(0x0028_0028);
            w74 = w74.wrapping_add(0x0028_0028);
            ang78 = ang78.wrapping_add(step);
            t3 = t3.wrapping_add(0x90);
            col += 1;
            t5 = t5.wrapping_add(0x90);
        }
        // 0x8002933C: close a full ring onto column 0.
        if arc == 0 {
            m.sw(t5.wrapping_sub(0x70), 0x0010_0000);
            m.sw(t5.wrapping_sub(0x4C), 0x0018_0010);
            m.sw(t5.wrapping_sub(0x28), 0x0020_0018);
            m.sw(t5.wrapping_sub(0x4), 0x0008_0020);
        }
    } else {
        // 0x80029378: one patch-textured packet a column.
        let uv0 = (clut << 16) | 0xF000;
        let uv1 = (tpage << 16) | 0xF002;
        let (mut i01, mut i23) = (0x0018_0010u32, 0x0008_0000u32);
        let mut k = 0u32;
        while k < m.lw(obj.wrapping_add(0x14)) {
            m.sw(t5.wrapping_add(0x18), 0xF200);
            m.sw(t5.wrapping_add(0x10), uv0);
            m.sw(t5.wrapping_add(0x14), uv1);
            m.sh(t5.wrapping_add(0x1A), 0xF202);
            m.sw(t5, t7);
            m.sw(t5.wrapping_add(4), t6);
            m.sw(t5.wrapping_add(8), t7);
            m.sw(t5.wrapping_add(0xC), t6);
            m.sw(t5.wrapping_add(0x1C), i01);
            m.sw(t5.wrapping_add(0x20), i23);
            i23 = i23.wrapping_add(0x0010_0010);
            i01 = i01.wrapping_add(0x0010_0010);
            k += 1;
            t5 = t5.wrapping_add(PACKET_BYTES);
        }
        if matches!(shape, 0 | 4..=7) && arc == 0 {
            m.sw(t5.wrapping_sub(8), 0x0008_0000);
        }
    }

    if shape == 1 || shape == 3 {
        // 0x80029468: extrapolate vertex 6, then rewrite the packets.
        t5 = m.lw(obj.wrapping_add(0x10)).wrapping_add(8);
        for col in 0..count {
            for t2 in 0..3u32 {
                let a0 = pa.wrapping_add(((col << 5) + t2) * 2);
                let d = m.lh(a0.wrapping_add(0x18)) - m.lh(a0.wrapping_add(0x10));
                m.sh(
                    a0.wrapping_add(0x30),
                    m.lhu(a0.wrapping_add(0x18)).wrapping_add((d >> 1) as u32),
                );
            }
            let b = col * 8;
            let pair = |lo: u32, hi: u32| ((b + lo) << 3).wrapping_add((b + hi) << 19);
            let packets: [([u32; 4], u32, u32); 3] = if shape == 1 {
                [
                    ([t6, t6, t7, t7], pair(0, 1), pair(2, 3)),
                    ([t7, t7, t6, t6], pair(3, 2), pair(5, 4)),
                    ([t6, t7, t6, t6], pair(1, 3), pair(6, 5)),
                ]
            } else {
                [
                    ([t6, t6, t6, t7], pair(1, 1), pair(2, 3)),
                    ([t6, t6, t7, t6], pair(2, 2), pair(3, 5)),
                    ([t6, t7, t6, t6], pair(1, 3), pair(6, 5)),
                ]
            };
            for (cols, i01, i23) in packets {
                for (n, c) in cols.into_iter().enumerate() {
                    m.sw(t5.wrapping_add(n as u32 * 4), c);
                }
                m.sw(t5.wrapping_add(0x1C), i01);
                m.sw(t5.wrapping_add(0x20), i23);
                t5 = t5.wrapping_add(PACKET_BYTES);
            }
        }
    }
    if shape == 2 {
        // 0x80029654: vertex 2 to the origin, one fan quad a column.
        m.sh(pa.wrapping_add(0x10), 0);
        m.sh(pb.wrapping_add(0x10), 0);
        m.sh(pc.wrapping_add(0x10), 0);
        t5 = m.lw(obj.wrapping_add(0x10)).wrapping_add(8);
        let (mut i01, mut i23) = (0x0010_0000u32, 0x0020_0018u32);
        for _ in 0..count {
            m.sw(t5, t6);
            m.sw(t5.wrapping_add(4), t7);
            m.sw(t5.wrapping_add(8), t6);
            m.sw(t5.wrapping_add(0xC), t6);
            m.sw(t5.wrapping_add(0x1C), i01);
            m.sw(t5.wrapping_add(0x20), i23);
            t5 = t5.wrapping_add(PACKET_BYTES);
            i23 = i23.wrapping_add(0x0030_0030);
            i01 = i01.wrapping_add(0x30);
        }
    }
    // 0x800296E0: twenty zero words after the last packet.
    for w in 0..20u32 {
        m.sw(t5.wrapping_add(w * 4), 0);
    }
    let end = t5.wrapping_add(0x50).wrapping_sub(out_va) as usize;
    let start = Mem::PAD as usize;
    let end = (start + end).min(m.bytes.len());
    Some(Build {
        out_va,
        bytes: m.bytes[start..end].to_vec(),
        written: m.written[start..end].to_vec(),
        shape,
    })
}

/// The byte-lane interpolation both four-way subdivision loops run
/// (`0x8002912C..0x800291A0` over the UV bytes, `0x80029248..0x800292BC` over
/// the colour bytes): with `p0 / p1` the first packet's two near-edge bytes
/// and `q0 / q1` its two far-edge bytes, packet `k` (`k = 1..3`) takes
/// `p0 + ((p1 - p0) * k >> 2)` and `q0 + ((q1 - q0) * k >> 2)`, each stored
/// twice - as packet `k`'s first byte and packet `k - 1`'s last. `a2` is the
/// loop's running pointer (`+0x24` a packet); `near_hi` / `far_hi` its two
/// forward offsets and `near_lo` / `far_lo` its two backward ones.
#[allow(clippy::too_many_arguments)]
fn interpolate_bytes(
    m: &mut Mem,
    mut a2: u32,
    p0: u32,
    p1: u32,
    q0: u32,
    q1: u32,
    near_hi: u32,
    far_hi: u32,
    near_lo: u32,
    far_lo: u32,
) {
    let dp = m.lbu(p1) as i32 - m.lbu(p0) as i32;
    let dq = m.lbu(q1) as i32 - m.lbu(q0) as i32;
    let (mut ap, mut aq) = (dp, dq);
    for _ in 1..4 {
        let fq = aq >> 2;
        aq += dq;
        let fp = ap >> 2;
        let v = (m.lbu(p0) as i32 + fp) as u32;
        ap += dp;
        m.sb(a2.wrapping_add(near_hi), v);
        m.sb(a2.wrapping_sub(near_lo), v);
        let w = (m.lbu(q0) as i32 + fq) as u32;
        m.sb(a2.wrapping_add(far_hi), w);
        m.sb(a2.wrapping_sub(far_lo), w);
        a2 = a2.wrapping_add(PACKET_BYTES);
    }
}

/// One `GT4` packet of the built object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    /// Corner vertices as indices into [`Decoded::verts`] (the index
    /// halfwords `/ 8`), packet order: `+0x1C` low / high, `+0x20` low / high.
    pub verts: [usize; 4],
    /// Corner colours (low 24 bits of words `+0x00..+0x0C`).
    pub rgb: [[u8; 3]; 4],
    /// Corner texels (`+0x10`, `+0x14`, `+0x18`, `+0x1A`).
    pub uvs: [[u8; 2]; 4],
    /// CLUT word (`+0x12`).
    pub clut: u16,
    /// Tpage word (`+0x16`).
    pub tpage: u16,
}

/// The object a [`Build`] holds, read back the way the TMD renderer reads it.
#[derive(Debug, Clone, Default)]
pub struct Decoded {
    /// Vertex block, `(x, y, z)` at 8-byte stride.
    pub verts: Vec<[i16; 3]>,
    /// The group's packets.
    pub packets: Vec<Packet>,
}

/// Read a [`Build`]'s object header, vertex block and packet group. A packet
/// whose index halfword runs past the vertex block is dropped.
pub fn decode(b: &Build) -> Decoded {
    let rd = |off: u32| -> Option<u32> {
        let i = off as usize;
        (i + 4 <= b.bytes.len()).then(|| {
            u32::from_le_bytes([b.bytes[i], b.bytes[i + 1], b.bytes[i + 2], b.bytes[i + 3]])
        })
    };
    let rel = |va: u32| va.wrapping_sub(b.out_va);
    let (Some(vtop), Some(nvert), Some(ptop)) = (rd(0xC), rd(0x10), rd(0x1C)) else {
        return Decoded::default();
    };
    let (vtop, ptop) = (rel(vtop), rel(ptop));
    let mut out = Decoded::default();
    for k in 0..nvert {
        let (Some(xy), Some(z)) = (rd(vtop + k * 8), rd(vtop + k * 8 + 4)) else {
            break;
        };
        out.verts.push([xy as i16, (xy >> 16) as i16, z as i16]);
    }
    let Some(hdr) = rd(ptop) else {
        return out;
    };
    let count = hdr & 0xFFFF;
    for k in 0..count {
        let p = ptop + 8 + k * PACKET_BYTES;
        let w: Option<Vec<u32>> = (0..9).map(|n| rd(p + n * 4)).collect();
        let Some(w) = w else { break };
        let idx = [w[7] & 0xFFFF, w[7] >> 16, w[8] & 0xFFFF, w[8] >> 16].map(|h| (h >> 3) as usize);
        if idx.iter().any(|&i| i >= out.verts.len()) {
            continue;
        }
        let rgb = |c: u32| [c as u8, (c >> 8) as u8, (c >> 16) as u8];
        let uv = |h: u32| [h as u8, (h >> 8) as u8];
        out.packets.push(Packet {
            verts: idx,
            rgb: [rgb(w[0]), rgb(w[1]), rgb(w[2]), rgb(w[3])],
            uvs: [uv(w[4]), uv(w[5]), uv(w[6]), uv(w[6] >> 16)],
            clut: (w[4] >> 16) as u16,
            tpage: (w[5] >> 16) as u16,
        });
    }
    out
}

/// A decoded object as a local-space VRAM mesh: each packet four vertices
/// (`[0, 1, 2]` + `[2, 1, 3]`), scaled by `scale / 0x1000`, the corner
/// colours run through the colour word's depth cue and its ABE / ABR ORed
/// into the TSB - the model draw at `0x8001B160` that case 4 falls into.
pub fn vram_mesh(d: &Decoded, scale: u16, colour: &ColourWord) -> legaia_tmd::mesh::VramMesh {
    let k = f32::from(scale) / 4096.0;
    let mut mesh = legaia_tmd::mesh::VramMesh {
        positions: Vec::with_capacity(d.packets.len() * 4),
        uvs: Vec::with_capacity(d.packets.len() * 4),
        cba_tsb: Vec::with_capacity(d.packets.len() * 4),
        indices: Vec::with_capacity(d.packets.len() * 6),
        normals: Vec::with_capacity(d.packets.len() * 4),
        colors: Vec::with_capacity(d.packets.len() * 4),
    };
    for p in &d.packets {
        let base = mesh.positions.len() as u32;
        let tsb = legaia_tmd::mesh::pack_tsb_semi(
            (p.tpage & !0x60) | (u16::from(colour.abr) << 5),
            colour.semi,
        );
        for c in 0..4 {
            mesh.positions
                .push(d.verts[p.verts[c]].map(|v| f32::from(v) * k));
            mesh.uvs.push(p.uvs[c]);
            mesh.cba_tsb.push([p.clut, tsb]);
            mesh.normals.push([0.0; 3]);
            mesh.colors.push(colour.cue(p.rgb[c]));
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    mesh
}

/// The draw-kind-4 render mode (`actor[+0x56]`).
pub const DRAW_KIND_MULTI: i16 = 4;

/// Whether a part's state is a live draw-kind-4 node on the default arm
/// (`+0x56 == 4`, `+0x9E & 0x6000 == 0`, `0x8001B128..0x8001B134`).
pub fn is_default_arm(s: &ActorState) -> bool {
    s.move_substate == DRAW_KIND_MULTI && s.field_9e & 0x6000 == 0
}

/// The call arguments the dispatcher builds off a node
/// (`0x8001B13C..0x8001B15C`): `(mode, packed, src)` - `+0x9E`,
/// `(s16)+0x9C + (((s16)+0xC8 >> 3) << 8)`, and the `+0x9C..+0xC0` block.
pub fn call_args(s: &ActorState) -> Option<(u32, u32, [u8; 0x24])> {
    if !is_default_arm(s) {
        return None;
    }
    let count = i32::from(s.actor_u16(0x9C) as i16);
    let total = i32::from(s.actor_u16(0xC8) as i16) >> 3;
    let mut src = [0u8; 0x24];
    for off in (0..0x24).step_by(2) {
        src[off..off + 2].copy_from_slice(&s.actor_u16(0x9C + off).to_le_bytes());
    }
    Some((
        u32::from(s.field_9e),
        count.wrapping_add(total << 8) as u32,
        src,
    ))
}

/// The BIOS `rand()` stream (`FUN_80056798`, A0 `0x2F`) from a fixed seed -
/// the port rebuilds the jittered shapes from one seed a draw rather than
/// sharing the game's RNG state.
fn bios_rand(seed: u32) -> impl FnMut() -> u32 {
    let mut s = seed;
    move || {
        s = s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        (s >> 16) & 0x7FFF
    }
}

/// A node's default-arm geometry as a local-space VRAM mesh, or `None` for a
/// node that is not on the default arm (or whose shape is `>= 8`).
pub fn default_arm_vram_mesh(s: &ActorState) -> Option<legaia_tmd::mesh::VramMesh> {
    let (mode, packed, src) = call_args(s)?;
    let b = build(
        0x8000_0000 + SCRATCH_OFFSET,
        mode,
        packed,
        &src,
        &crate::effect_ribbon::RetailTrig,
        bios_rand(0),
        0,
    )?;
    let mesh = vram_mesh(&decode(&b), s.field_72, &ColourWord::of(s));
    (!mesh.indices.is_empty()).then_some(mesh)
}

/// The default-arm draw of a part, in the same `(world_pos, rot)` form a
/// ribbon or a sprite-arm quad takes.
pub fn default_arm_draw(s: &ActorState) -> Option<crate::effect_ribbon::RibbonDraw> {
    const A: f32 = std::f32::consts::TAU / 4096.0;
    let mesh = default_arm_vram_mesh(s)?;
    Some(crate::effect_ribbon::RibbonDraw {
        mesh,
        world_pos: [s.world_x as f32, s.world_y as f32, s.world_z as f32],
        rot: [
            (s.render_24 as f32) * A,
            (s.y_rot.wrapping_add(s.render_26) as f32) * A,
            (s.render_28 as f32) * A,
        ],
        flags_52: s.field_52,
    })
}

/// The `src` block the battle per-actor draw hands the builder for the
/// ground shadow (`0x80049204..0x80049280`): the centre / rim colours at
/// `+0x04` / `+0x08`, radius `0` / `radius` at `+0x18` / `+0x1A`, heights
/// `0`, scales `0x1000`. The UV / tpage words are not written (texture mode
/// `0` never reads them).
pub fn ground_shadow_src(inner: u32, outer: u32, radius: i16) -> [u8; 0x24] {
    let mut src = [0u8; 0x24];
    src[4..8].copy_from_slice(&inner.to_le_bytes());
    src[8..12].copy_from_slice(&outer.to_le_bytes());
    src[0x1A..0x1C].copy_from_slice(&radius.to_le_bytes());
    src[0x20..0x22].copy_from_slice(&0x1000u16.to_le_bytes());
    src[0x22..0x24].copy_from_slice(&0x1000u16.to_le_bytes());
    src
}

/// The battle ground shadow's mesh (`FUN_80048A08`, `0x800491FC..0x800492BC`):
/// the builder in mode `1` (the XZ plane, shape `0`) over
/// [`SHADOW_SEGMENTS`] columns, drawn through `FUN_80043390` with
/// [`SHADOW_FLAG_WORD`] - semi-transparent, subtractive. `inner` / `outer` /
/// `radius` come from `legaia_engine_vm::battle_actor_draw::shadow_plan`.
///
/// PORT: FUN_80048A08 (the ground-shadow build and draw, `0x800491FC..0x800492BC`)
pub fn ground_shadow_mesh(inner: u32, outer: u32, radius: i16) -> legaia_tmd::mesh::VramMesh {
    let src = ground_shadow_src(inner, outer, radius);
    let b = build(
        0x8000_0000 + SHADOW_SCRATCH_OFFSET,
        1,
        SHADOW_SEGMENTS,
        &src,
        &crate::effect_ribbon::RetailTrig,
        bios_rand(0),
        0,
    )
    .expect("shape 0 always builds");
    let colour = ColourWord {
        semi: true,
        abr: ((SHADOW_FLAG_WORD >> 24) & 3) as u8,
        far: [0; 3],
        ir0: 1,
    };
    vram_mesh(&decode(&b), 0x1000, &colour)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect_ribbon::AnalyticTrig;

    fn src(r0: i16, r1: i16, c0: i16, c1: i16) -> [u8; 0x24] {
        let mut s = [0u8; 0x24];
        s[4..8].copy_from_slice(&0x0011_2233u32.to_le_bytes());
        s[8..12].copy_from_slice(&0x0044_5566u32.to_le_bytes());
        s[0x18..0x1A].copy_from_slice(&r0.to_le_bytes());
        s[0x1A..0x1C].copy_from_slice(&r1.to_le_bytes());
        s[0x1C..0x1E].copy_from_slice(&c0.to_le_bytes());
        s[0x1E..0x20].copy_from_slice(&c1.to_le_bytes());
        s[0x20..0x22].copy_from_slice(&0x1000u16.to_le_bytes());
        s[0x22..0x24].copy_from_slice(&0x1000u16.to_le_bytes());
        s
    }

    fn run(mode: u32, packed: u32, s: &[u8; 0x24]) -> (Build, Decoded) {
        let b = build(
            0x8010_0000,
            mode,
            packed,
            s,
            &AnalyticTrig::new(),
            bios_rand(1),
            0,
        )
        .expect("builds");
        let d = decode(&b);
        (b, d)
    }

    fn radius(v: [i16; 3], plane_zero: usize) -> f64 {
        let mut s = 0.0;
        for (k, c) in v.iter().enumerate() {
            if k != plane_zero {
                s += f64::from(*c).powi(2);
            }
        }
        s.sqrt()
    }

    /// Shape 0, full ring: `count` columns of (inner, outer), one packet a
    /// column joining it to the next, the last one closed onto column 0,
    /// the fixed patch UVs, inner corners in `+0x04`'s colour.
    #[test]
    fn shape0_full_ring_closes_onto_column_zero() {
        let (b, d) = run(0, 10, &src(100, 200, 5, 6));
        assert_eq!(d.verts.len(), 20);
        assert_eq!(d.packets.len(), 10);
        for (k, v) in d.verts.iter().enumerate() {
            let want = if k % 2 == 0 { 100.0 } else { 200.0 };
            assert!((radius(*v, 2) - want).abs() < 2.0, "vertex {k} {v:?}");
            assert_eq!(v[2], if k % 2 == 0 { 5 } else { 6 });
        }
        for (k, p) in d.packets.iter().enumerate() {
            let next = (k + 1) % 10;
            assert_eq!(p.verts, [2 * next, 2 * next + 1, 2 * k, 2 * k + 1]);
            assert_eq!(p.uvs, [[0, 0xF0], [2, 0xF0], [0, 0xF2], [2, 0xF2]]);
            assert_eq!((p.clut, p.tpage), (DEFAULT_ARM_CLUT, DEFAULT_ARM_TPAGE));
            assert_eq!(p.rgb[0], [0x33, 0x22, 0x11]);
            assert_eq!(p.rgb[1], [0x66, 0x55, 0x44]);
        }
        // Group header and the zero tail.
        let n = b.bytes.len();
        assert!(b.bytes[n - 0x50..].iter().all(|&x| x == 0));
        assert!(b.written[n - 0x50..].iter().all(|&w| w));
    }

    /// `phase == 0` turns the ring by half a column: the first column sits
    /// at angle `-step/2 - 0x400`, not `-0x400`.
    #[test]
    fn shape0_zero_phase_turns_half_a_column() {
        let (_, d) = run(0, 4, &src(0, 0x1000, 0, 0));
        // step 0x400; column 0 at -0x200-0x400 = -0x600 -> (sin-table a,
        // cos-table b) = (cos, sin) of -135 degrees -> both negative.
        let v = d.verts[1];
        assert!(v[0] < 0 && v[1] < 0, "{v:?}");
        assert_eq!(v[0], v[1]);
    }

    /// `0 < phase < count` draws an open arc: `count + 1` columns and
    /// `phase` packets, with no closing packet.
    #[test]
    fn shape0_phase_below_count_is_an_open_arc() {
        let (_, d) = run(0, 8 | (3 << 8), &src(10, 20, 0, 0));
        assert_eq!(d.verts.len(), 18);
        assert_eq!(d.packets.len(), 3);
        assert_eq!(d.packets[2].verts, [6, 7, 4, 5]);
    }

    /// Mode `1` puts the ring in the XZ plane (the ground shadow's plane).
    #[test]
    fn mode1_is_the_xz_plane() {
        let (_, d) = run(1, 24, &src(0, 300, 0, 0));
        for v in &d.verts {
            assert_eq!(v[1], 0);
        }
        assert!(d.verts.iter().any(|v| v[2].abs() > 200));
    }

    /// Shape 1: eight vertex slots a column (three pairs + an extrapolated
    /// vertex 6), three packets a column in the crown pattern.
    #[test]
    fn shape1_crown_packets() {
        let (_, d) = run(0x08, 2 | (0x20 << 8), &src(0, 100, 0, 50));
        assert_eq!(d.verts.len(), 16);
        assert_eq!(d.packets.len(), 6);
        assert_eq!(d.packets[0].verts, [0, 1, 2, 3]);
        assert_eq!(d.packets[1].verts, [3, 2, 5, 4]);
        assert_eq!(d.packets[2].verts, [1, 3, 6, 5]);
        assert_eq!(d.packets[3].verts, [8, 9, 10, 11]);
        // Vertex 6 = v3 + (v3 - v2) / 2 on each component.
        let (v2, v3, v6) = (d.verts[2], d.verts[3], d.verts[6]);
        for c in 0..3 {
            let want = i32::from(v3[c]) + ((i32::from(v3[c]) - i32::from(v2[c])) >> 1);
            assert_eq!(i32::from(v6[c]), want);
        }
        // Outer / inner colours: t6 = +0x08, t7 = +0x04.
        assert_eq!(d.packets[0].rgb[0], [0x66, 0x55, 0x44]);
        assert_eq!(d.packets[0].rgb[2], [0x33, 0x22, 0x11]);
    }

    /// Shape 3 differs from 1 only in its packets: two degenerate quads.
    #[test]
    fn shape3_triangle_crown_packets() {
        let (_, d) = run(0x18, 5 | (8 << 8), &src(0, 100, 0, 0));
        assert_eq!(d.packets.len(), 15);
        assert_eq!(d.packets[0].verts, [1, 1, 2, 3]);
        assert_eq!(d.packets[1].verts, [2, 2, 3, 5]);
        assert_eq!(d.packets[2].verts, [1, 3, 6, 5]);
    }

    /// Shape 2: vertex 2 at the origin, one fan quad a column.
    #[test]
    fn shape2_star_fans_to_vertex_two() {
        let (_, d) = run(0x10, 4, &src(50, 100, 0, 0));
        assert_eq!(d.verts.len(), 24);
        assert_eq!(d.verts[2], [0, 0, 0]);
        assert_eq!(d.packets.len(), 4);
        for (k, p) in d.packets.iter().enumerate() {
            assert_eq!(p.verts, [6 * k, 2, 6 * k + 3, 6 * k + 4]);
        }
    }

    /// Texture mode 1 (`mode = 0x100`): four packets a column along the
    /// radius (inner, 1/4, 1/2, 3/4, outer = vertices 0, 2, 3, 4, 1), the
    /// UV / colour lanes interpolated across them, and the ring closed.
    #[test]
    fn texture_mode_subdivides_radially() {
        let mut s = src(0, 224, 0, 0);
        s[0xC..0xE].copy_from_slice(&0u16.to_le_bytes());
        s[0xE..0x10].copy_from_slice(&0u16.to_le_bytes());
        s[0x10..0x12].copy_from_slice(&64u16.to_le_bytes());
        s[0x12..0x14].copy_from_slice(&64u16.to_le_bytes());
        s[0x14..0x16].copy_from_slice(&0x0015u16.to_le_bytes());
        s[0x16..0x18].copy_from_slice(&0x7DC0u16.to_le_bytes());
        let (_, d) = run(0x100, 4, &s);
        assert_eq!(d.verts.len(), 20);
        assert_eq!(d.packets.len(), 16);
        assert_eq!(d.packets[0].verts, [0, 2, 5, 7]);
        assert_eq!(d.packets[1].verts, [2, 3, 7, 8]);
        assert_eq!(d.packets[2].verts, [3, 4, 8, 9]);
        assert_eq!(d.packets[3].verts, [4, 1, 9, 6]);
        assert_eq!(d.packets[15].verts, [19, 16, 4, 1]);
        for p in &d.packets {
            assert_eq!((p.clut, p.tpage), (0x7DC0, 0x0015));
        }
        // Inner edge at the rectangle centre (32, 32).
        assert_eq!(d.packets[0].uvs[0], [32, 32]);
        // The colour lanes run inner -> outer across the four packets.
        assert_eq!(d.packets[0].rgb[0], [0x33, 0x22, 0x11]);
        assert_eq!(d.packets[3].rgb[1], [0x66, 0x55, 0x44]);
        let mid = d.packets[2].rgb[0];
        assert_eq!(
            mid,
            [0x33 + (0x33 >> 1), 0x22 + (0x33 >> 1), 0x11 + (0x33 >> 1)]
        );
        // Radial vertex order: v2 is a quarter of the way out.
        let (v0, v1, v2) = (d.verts[0], d.verts[1], d.verts[2]);
        assert_eq!(v2[0], v0[0] + ((v1[0] - v0[0]) >> 2));
    }

    /// Shapes 4 / 5 draw three `rand()` values a column and jitter the
    /// radius by them.
    #[test]
    fn shapes_4_and_5_draw_three_rands_a_column() {
        for shape in [4u32, 5] {
            let mut calls = 0;
            let b = build(
                0x8010_0000,
                shape << 3,
                6,
                &src(100, 40, 0, 12),
                &AnalyticTrig::new(),
                || {
                    calls += 1;
                    7
                },
                0,
            )
            .unwrap();
            assert_eq!(calls, 18, "shape {shape}");
            let d = decode(&b);
            assert_eq!(d.packets.len(), 6);
        }
    }

    /// Shape `>= 8` has no arm.
    #[test]
    fn shape_eight_and_up_is_none() {
        assert!(build(0, 0x40, 4, &src(0, 1, 0, 0), &AnalyticTrig::new(), || 0, 0).is_none());
    }

    /// The ground shadow: a 24-column disc in the XZ plane, centre / rim
    /// colours from the plan, subtractive.
    #[test]
    fn ground_shadow_is_a_subtractive_xz_disc() {
        let m = ground_shadow_mesh(0x0040_4040, 0x0008_0808, 256);
        assert_eq!(m.indices.len(), 24 * 6);
        assert!(m.positions.iter().all(|p| p[1] == 0.0));
        assert!(m.cba_tsb.iter().all(
            |t| t[1] & legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT != 0 && (t[1] >> 5) & 3 == 2
        ));
        // Corner 0 is a centre vertex (radius 0) in the centre colour.
        assert_eq!(m.positions[0], [0.0, 0.0, 0.0]);
        // The colours go through the shadow's flag word's cue (`IR0 = 1`,
        // far colour black), which rounds each lane down by one.
        assert_eq!(m.colors[0], [0x3F, 0x3F, 0x3F]);
        assert_eq!(m.colors[1], [0x07, 0x07, 0x07]);
    }

    /// A move-VM node on the default arm yields a draw; a sprite-arm node
    /// does not.
    #[test]
    fn default_arm_node_yields_a_draw() {
        let mut s = ActorState {
            move_substate: DRAW_KIND_MULTI,
            field_9e: 0,
            field_72: 0x1000,
            field_74: 0xC900_0000,
            ..Default::default()
        };
        s.set_actor_u16(0x9C, 10);
        s.set_actor_u32(0xA0, 0x00FF_FFFF);
        s.set_actor_u32(0xA4, 0x0010_1010);
        s.set_actor_u16(0xB6, 16);
        s.set_actor_u16(0xBC, 0x1000);
        s.set_actor_u16(0xBE, 0x1000);
        let d = default_arm_draw(&s).expect("a draw");
        assert_eq!(d.mesh.indices.len(), 10 * 6);
        s.field_9e = 0x4000;
        assert!(default_arm_draw(&s).is_none());
    }
}
