//! Low-level MIPS R3000 instruction encoders (little-endian words) and the
//! `lui`/`ori` immediate-split helpers used by the routine builders.

use super::*;

pub(crate) const fn j(target: u32) -> u32 {
    (0x02 << 26) | ((target >> 2) & 0x03ff_ffff)
}
pub(crate) const fn jal(target: u32) -> u32 {
    (0x03 << 26) | ((target >> 2) & 0x03ff_ffff)
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
pub(crate) const fn addiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x09 << 26) | (rs << 21) | (rt << 16) | imm as u32
}
pub(crate) const fn sw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x2b << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn sh(rt: u32, rs: u32, off: u16) -> u32 {
    (0x29 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn jr(rs: u32) -> u32 {
    (rs << 21) | 0x08
}
pub(crate) const fn jalr(rs: u32) -> u32 {
    (rs << 21) | (RA << 11) | 0x09
}
pub(crate) const fn lbu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x24 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn andi(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0c << 26) | (rs << 21) | (rt << 16) | imm as u32
}
pub(crate) const fn sll(rd: u32, rt: u32, sa: u32) -> u32 {
    (rt << 16) | (rd << 11) | (sa << 6)
}
pub(crate) const fn addu(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x21
}
pub(crate) const fn slt(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x2a
}
pub(crate) const fn lw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x23 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn lhu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x25 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn bne(rs: u32, rt: u32, off: i16) -> u32 {
    (0x05 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
pub(crate) const fn beq(rs: u32, rt: u32, off: i16) -> u32 {
    (0x04 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
pub(crate) const fn slti(rt: u32, rs: u32, imm: i16) -> u32 {
    (0x0a << 26) | (rs << 21) | (rt << 16) | (imm as u16 as u32)
}
pub(crate) const fn lh(rt: u32, rs: u32, off: u16) -> u32 {
    (0x21 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn sltiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0b << 26) | (rs << 21) | (rt << 16) | imm as u32
}
pub(crate) const fn srl(rd: u32, rt: u32, sa: u32) -> u32 {
    (rt << 16) | (rd << 11) | (sa << 6) | 0x02
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
pub(crate) const fn bgez(rs: u32, off: i16) -> u32 {
    (0x01 << 26) | (rs << 21) | (0x01 << 16) | (off as u16 as u32)
}
pub(crate) const fn blez(rs: u32, off: i16) -> u32 {
    (0x06 << 26) | (rs << 21) | (off as u16 as u32)
}
pub(crate) const fn sb(rt: u32, rs: u32, off: u16) -> u32 {
    (0x28 << 26) | (rs << 21) | (rt << 16) | off as u32
}
pub(crate) const fn xori(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0e << 26) | (rs << 21) | (rt << 16) | imm as u32
}

/// High 16 bits to `lui` so a following signed-`lo` access reaches `va`.
pub(crate) const fn hi(va: u32) -> u16 {
    (va.wrapping_add(0x8000) >> 16) as u16
}
/// Low 16 bits of `va` (the signed offset half).
pub(crate) const fn lo(va: u32) -> u16 {
    (va & 0xffff) as u16
}
/// Plain high half of a 32-bit immediate (no sign correction - for `lui`+`ori`).
pub(crate) const fn imm_hi(v: u32) -> u16 {
    (v >> 16) as u16
}
pub(crate) const fn imm_lo(v: u32) -> u16 {
    (v & 0xffff) as u16
}
