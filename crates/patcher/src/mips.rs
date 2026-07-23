//! Shared MIPS R3000 instruction encoders (little-endian words), register-number
//! aliases, and the `lui`/`ori` immediate-split helpers used by every
//! hand-assembled routine builder in this crate (`enemy_ally`, `bonus_drop`,
//! `flee_exp`, `shiny_seru`, `seru_overlay`).
//!
//! These are the byte-for-byte-identical encoders that previously lived, copied,
//! in each of those modules. Each `const fn` returns the raw 32-bit instruction
//! word (host-endian `u32`; callers serialise with `to_le_bytes`). `imm`/`off`
//! are the raw 16-bit fields - already two's-complement for negative offsets
//! unless the signature takes a signed type (`beq`/`bne`/`slti`/`bgez`/`blez`,
//! which reinterpret the signed value into the low 16 bits).
//!
//! Two encoders are deliberately **not** hoisted here because a local copy
//! diverges and must keep emitting its exact bytes:
//! - `bonus_drop::slti` takes a `u16` immediate (this module's [`slti`] takes the
//!   ABI-correct signed `i16`); it stays local so its call site is unchanged.
//! - `bonus_drop::hi_lo` (a combined `(hi, lo)` helper) and `flee_exp::mv`
//!   (a `move` pseudo-op) are single-module conveniences, not duplicated
//!   encoders, so they stay where they are used.

use anyhow::Result;

// --- Register-number aliases (MIPS ABI indices) -----------------------------

/// `$zero` - always reads 0.
pub(crate) const ZERO: u32 = 0;
/// `$at` - assembler temporary; safe to clobber (never held live).
pub(crate) const AT: u32 = 1;
/// `$v0` - return value / scratch.
pub(crate) const V0: u32 = 2;
/// `$v1` - return value / scratch.
pub(crate) const V1: u32 = 3;
/// `$a0` - argument 0.
pub(crate) const A0: u32 = 4;
/// `$a1` - argument 1.
pub(crate) const A1: u32 = 5;
/// `$a2` - argument 2.
pub(crate) const A2: u32 = 6;
/// `$a3` - argument 3.
pub(crate) const A3: u32 = 7;
/// `$t0` - caller-saved temporary.
pub(crate) const T0: u32 = 8;
/// `$t1` - caller-saved temporary.
pub(crate) const T1: u32 = 9;
/// `$t2` - caller-saved temporary.
pub(crate) const T2: u32 = 10;
/// `$t3` - caller-saved temporary.
pub(crate) const T3: u32 = 11;
/// `$t4` - caller-saved temporary.
pub(crate) const T4: u32 = 12;
/// `$t5` - caller-saved temporary.
pub(crate) const T5: u32 = 13;
/// `$t6` - caller-saved temporary.
pub(crate) const T6: u32 = 14;
/// `$t7` - caller-saved temporary.
pub(crate) const T7: u32 = 15;
/// `$s0` - callee-saved register.
pub(crate) const S0: u32 = 16;
/// `$s1` - callee-saved register.
pub(crate) const S1: u32 = 17;
/// `$s2` - callee-saved register.
pub(crate) const S2: u32 = 18;
/// `$s3` - callee-saved register.
pub(crate) const S3: u32 = 19;
/// `$s4` - callee-saved register.
pub(crate) const S4: u32 = 20;
/// `$s5` - callee-saved register.
pub(crate) const S5: u32 = 21;
/// `$s6` - callee-saved register.
pub(crate) const S6: u32 = 22;
/// `$s7` - callee-saved register.
pub(crate) const S7: u32 = 23;
/// `$t8` - caller-saved temporary.
pub(crate) const T8: u32 = 24;
/// `$t9` - caller-saved temporary.
pub(crate) const T9: u32 = 25;
/// `$gp` - global pointer.
pub(crate) const GP: u32 = 28;
/// `$sp` - stack pointer.
pub(crate) const SP: u32 = 29;
/// `$ra` - return address.
pub(crate) const RA: u32 = 31;

// --- Jumps / branches -------------------------------------------------------

/// `j target` - unconditional jump (drops the target's low 2 bits).
pub(crate) const fn j(target: u32) -> u32 {
    (0x02 << 26) | ((target >> 2) & 0x03ff_ffff)
}
/// `jal target` - jump-and-link (sets `$ra`).
pub(crate) const fn jal(target: u32) -> u32 {
    (0x03 << 26) | ((target >> 2) & 0x03ff_ffff)
}
/// `jr rs` - jump to the address in `rs`.
pub(crate) const fn jr(rs: u32) -> u32 {
    (rs << 21) | 0x08
}
/// `jalr rs` - jump-and-link to `rs`, link into `$ra`.
pub(crate) const fn jalr(rs: u32) -> u32 {
    (rs << 21) | (RA << 11) | 0x09
}
/// `nop` (encoded as `sll $zero,$zero,0` = all-zero).
pub(crate) const fn nop() -> u32 {
    0
}
/// `beq rs,rt,off` - branch if equal (PC-relative, `off` in words).
pub(crate) const fn beq(rs: u32, rt: u32, off: i16) -> u32 {
    (0x04 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
/// `bne rs,rt,off` - branch if not equal.
pub(crate) const fn bne(rs: u32, rt: u32, off: i16) -> u32 {
    (0x05 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
/// `bgez rs,off` - branch if `rs >= 0`.
pub(crate) const fn bgez(rs: u32, off: i16) -> u32 {
    (0x01 << 26) | (rs << 21) | (0x01 << 16) | (off as u16 as u32)
}
/// `blez rs,off` - branch if `rs <= 0`.
pub(crate) const fn blez(rs: u32, off: i16) -> u32 {
    (0x06 << 26) | (rs << 21) | (off as u16 as u32)
}

// --- Immediate ALU ----------------------------------------------------------

/// `lui rt,imm` - load `imm` into the high half of `rt`.
pub(crate) const fn lui(rt: u32, imm: u16) -> u32 {
    (0x0f << 26) | (rt << 16) | imm as u32
}
/// `ori rt,rs,imm` - bitwise OR immediate.
pub(crate) const fn ori(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0d << 26) | (rs << 21) | (rt << 16) | imm as u32
}
/// `andi rt,rs,imm` - bitwise AND immediate.
pub(crate) const fn andi(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0c << 26) | (rs << 21) | (rt << 16) | imm as u32
}
/// `xori rt,rs,imm` - bitwise XOR immediate.
pub(crate) const fn xori(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0e << 26) | (rs << 21) | (rt << 16) | imm as u32
}
/// `addiu rt,rs,imm` - add immediate (no overflow trap).
pub(crate) const fn addiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x09 << 26) | (rs << 21) | (rt << 16) | imm as u32
}
/// `slti rt,rs,imm` - set-less-than immediate (signed).
pub(crate) const fn slti(rt: u32, rs: u32, imm: i16) -> u32 {
    (0x0a << 26) | (rs << 21) | (rt << 16) | (imm as u16 as u32)
}
/// `sltiu rt,rs,imm` - set-less-than immediate (unsigned).
pub(crate) const fn sltiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0b << 26) | (rs << 21) | (rt << 16) | imm as u32
}

// --- Loads / stores ---------------------------------------------------------

/// `lb rt,off(rs)` - load byte, sign-extended.
pub(crate) const fn lb(rt: u32, rs: u32, off: u16) -> u32 {
    (0x20 << 26) | (rs << 21) | (rt << 16) | off as u32
}
/// `lbu rt,off(rs)` - load byte, zero-extended.
pub(crate) const fn lbu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x24 << 26) | (rs << 21) | (rt << 16) | off as u32
}
/// `lh rt,off(rs)` - load halfword, sign-extended.
pub(crate) const fn lh(rt: u32, rs: u32, off: u16) -> u32 {
    (0x21 << 26) | (rs << 21) | (rt << 16) | off as u32
}
/// `lhu rt,off(rs)` - load halfword, zero-extended.
pub(crate) const fn lhu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x25 << 26) | (rs << 21) | (rt << 16) | off as u32
}
/// `lw rt,off(rs)` - load word.
pub(crate) const fn lw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x23 << 26) | (rs << 21) | (rt << 16) | off as u32
}
/// `sb rt,off(rs)` - store byte.
pub(crate) const fn sb(rt: u32, rs: u32, off: u16) -> u32 {
    (0x28 << 26) | (rs << 21) | (rt << 16) | off as u32
}
/// `sh rt,off(rs)` - store halfword.
pub(crate) const fn sh(rt: u32, rs: u32, off: u16) -> u32 {
    (0x29 << 26) | (rs << 21) | (rt << 16) | off as u32
}
/// `sw rt,off(rs)` - store word.
pub(crate) const fn sw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x2b << 26) | (rs << 21) | (rt << 16) | off as u32
}

// --- Register ALU / shifts / mul-div ----------------------------------------

/// `addu rd,rs,rt` - add unsigned (no overflow trap).
pub(crate) const fn addu(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x21
}
/// `slt rd,rs,rt` - set-less-than (signed).
pub(crate) const fn slt(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x2a
}
/// `sltu rd,rs,rt` - set-less-than (unsigned).
pub(crate) const fn sltu(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x2b
}
/// `sll rd,rt,sa` - shift left logical by a constant amount.
pub(crate) const fn sll(rd: u32, rt: u32, sa: u32) -> u32 {
    (rt << 16) | (rd << 11) | (sa << 6)
}
/// `srl rd,rt,sa` - shift right logical by a constant amount.
pub(crate) const fn srl(rd: u32, rt: u32, sa: u32) -> u32 {
    (rt << 16) | (rd << 11) | (sa << 6) | 0x02
}
/// `srlv rd,rt,rs` - shift right logical by a variable amount (`rs`).
pub(crate) const fn srlv(rd: u32, rt: u32, rs: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x06
}
/// `multu rs,rt` - unsigned multiply into `hi`/`lo`.
pub(crate) const fn multu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x19
}
/// `divu rs,rt` - unsigned divide (`lo` = quotient, `hi` = remainder).
pub(crate) const fn divu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x1b
}
/// `mfhi rd` - move from the `hi` result register.
pub(crate) const fn mfhi(rd: u32) -> u32 {
    (rd << 11) | 0x10
}
/// `mflo rd` - move from the `lo` result register.
pub(crate) const fn mflo(rd: u32) -> u32 {
    (rd << 11) | 0x12
}

// --- Immediate-split helpers ------------------------------------------------

/// Low 16 bits of a VA (the signed offset half for `lw`/`lhu`/`sh`/`addiu` off a
/// `lui` high half).
pub(crate) const fn lo(va: u32) -> u16 {
    (va & 0xffff) as u16
}
/// High 16 bits a `lui` must load so a following signed-`lo` access reaches `va`
/// (the `+0x8000` corrects for the low half's sign extension).
pub(crate) const fn hi(va: u32) -> u16 {
    (va.wrapping_add(0x8000) >> 16) as u16
}
/// Plain high half of a 32-bit immediate (no sign correction - for `lui`+`ori`).
pub(crate) const fn imm_hi(v: u32) -> u16 {
    (v >> 16) as u16
}
/// Plain low half of a 32-bit immediate (for `lui`+`ori`).
pub(crate) const fn imm_lo(v: u32) -> u16 {
    (v & 0xffff) as u16
}

// --- Shared byte reader -----------------------------------------------------

/// Read a little-endian `u32` at file offset `off`, or a bounded error if the
/// buffer is too short. Shared by the routine planners that fingerprint a hook
/// site before detouring it.
pub(crate) fn read_word(buf: &[u8], off: usize) -> Result<u32> {
    let b = buf
        .get(off..off + 4)
        .ok_or_else(|| anyhow::anyhow!("buffer too short at {off:#x}"))?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}
