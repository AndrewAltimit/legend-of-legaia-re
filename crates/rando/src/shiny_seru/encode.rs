//! MIPS R3000 instruction encoders (little-endian) and register aliases used to
//! hand-assemble the shiny-Seru detour routines.

// --- MIPS R3000 encoders (little-endian) -----------------------------------

pub(crate) const ZERO: u32 = 0;
pub(crate) const AT: u32 = 1; // assembler temp - safe to clobber (never held live)
pub(crate) const V0: u32 = 2;
pub(crate) const V1: u32 = 3;
pub(crate) const A0: u32 = 4;
pub(crate) const A1: u32 = 5;
pub(crate) const A2: u32 = 6;
pub(crate) const T0: u32 = 8;
pub(crate) const T1: u32 = 9;
pub(crate) const T2: u32 = 10;
pub(crate) const T3: u32 = 11;
pub(crate) const T4: u32 = 12;
pub(crate) const T5: u32 = 13;
pub(crate) const T6: u32 = 14;
pub(crate) const T7: u32 = 15;
pub(crate) const S1: u32 = 17; // live actor pointer in FUN_8004a908 (compared, never written)
pub(crate) const T8: u32 = 24;
pub(crate) const SP: u32 = 29; // stack pointer (the banner detour keeps the caller's sp)
pub(crate) const T9: u32 = 25;

pub(crate) const fn j(t: u32) -> u32 {
    (0x02 << 26) | ((t >> 2) & 0x03ff_ffff)
}
pub(crate) const fn jal(t: u32) -> u32 {
    (0x03 << 26) | ((t >> 2) & 0x03ff_ffff)
}
pub(crate) const fn nop() -> u32 {
    0
}
pub(crate) const fn lui(rt: u32, imm: u16) -> u32 {
    (0x0f << 26) | (rt << 16) | imm as u32
}
pub(crate) const fn ori(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0d << 26) | (rs << 21) | (rt << 16) | imm as u32
}
pub(crate) const fn andi(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0c << 26) | (rs << 21) | (rt << 16) | imm as u32
}
pub(crate) const fn addiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x09 << 26) | (rs << 21) | (rt << 16) | imm as u32
}
pub(crate) const fn lhu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x25 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn lw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x23 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn lbu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x24 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn sh(rt: u32, rs: u32, off: u16) -> u32 {
    (0x29 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn sb(rt: u32, rs: u32, off: u16) -> u32 {
    (0x28 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn sw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x2b << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn sltiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0b << 26) | (rs << 21) | (rt << 16) | imm as u32
}
pub(crate) const fn beq(rs: u32, rt: u32, off: i16) -> u32 {
    (0x04 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
pub(crate) const fn bne(rs: u32, rt: u32, off: i16) -> u32 {
    (0x05 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
pub(crate) const fn addu(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x21
}
pub(crate) const fn srl(rd: u32, rt: u32, sa: u32) -> u32 {
    (rt << 16) | (rd << 11) | (sa << 6) | 0x02
}
pub(crate) const fn srlv(rd: u32, rt: u32, rs: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x06
}
pub(crate) const fn multu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x19
}
pub(crate) const fn divu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x1b
}
pub(crate) const fn mflo(rd: u32) -> u32 {
    (rd << 11) | 0x12
}
pub(crate) const fn mfhi(rd: u32) -> u32 {
    (rd << 11) | 0x10
}
pub(crate) const fn lo(va: u32) -> u16 {
    (va & 0xffff) as u16
}
pub(crate) const fn hi(va: u32) -> u16 {
    (va.wrapping_add(0x8000) >> 16) as u16
}
