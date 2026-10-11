//! Every place a code image forms one address, and how to make it form
//! another.
//!
//! A `ui_menu` / `system_text` string is reached straight from code, so
//! moving one means rewriting each instruction or word that produces its
//! address. [`scan`] finds those, in the forms the retail images use:
//!
//! - an aligned **word** equal to the address (a pointer-table slot, a record
//!   field such as a screen-element placement's payload pointer);
//! - a **`lui` pair**: `lui rX, %hi` completed by an `addiu` / `ori` / load /
//!   store on `rX`, followed down every path the way
//!   `scripts/ghidra-analysis/mips_walk.py` walks it (a branch forks, a
//!   `jal` delay slot still sees the register, a write to it ends the path);
//! - a **`$gp`-relative** `addiu` / `ori` / load / store, for an address in
//!   the small-data band.
//!
//! [`retarget`] rewrites one site. A word takes any address. A `lui` pair
//! whose high half serves only this address - the `lui rX; ... ; addiu rX,
//! rX, lo` shape with nothing else reading `rX` in between, no control flow
//! into or out of the gap, one completion on every path - is **private**, so
//! both halves are rewritten and the address can move anywhere. A shared
//! high half (the same `lui` completed for several addresses, or a load that
//! leaves it live) is kept: only the low half moves, so the new address must
//! have the same `%hi`. A `$gp` form must stay within the signed 16-bit
//! displacement of `$gp`.
//!
//! The scan over-approximates on purpose: a data word that happens to decode
//! as a `lui` adds a phantom reference, and a phantom reference only narrows
//! where a string may go (or pins it). A reference the scan missed would
//! leave code pointing at the old bytes, so every choice here errs toward
//! finding more.

use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Retail `$gp`: `lui gp,0x8008; addiu gp,gp,-0x4ce8` at `0x80026CA8` in
/// `SCUS_942.54` (see `scripts/ghidra-analysis/find-gp-relative-refs.py`).
/// Set once at boot and never reloaded, so overlays share it.
pub const RETAIL_GP: u32 = 0x8007_B318;

/// How a site supplies the low half of the address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoKind {
    /// `addiu rt, rs, lo` - signed.
    Addiu,
    /// `ori rt, rs, lo` - unsigned.
    Ori,
    /// A load or store `op rt, lo(rs)` - signed.
    Mem,
}

/// One instruction or word that forms an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefSite {
    /// An aligned word equal to the address, at image offset `off`.
    Word {
        /// Image offset of the word.
        off: usize,
    },
    /// A `lui` at image offset `lui` completed by the instruction at `lo`.
    Pair {
        /// Image offset of the `lui`.
        lui: usize,
        /// Image offset of the completing instruction.
        lo: usize,
        /// The completing instruction's kind.
        kind: LoKind,
        /// The high half serves this completion alone (see the module docs).
        private: bool,
    },
    /// A `$gp`-relative instruction at image offset `off`.
    Gp {
        /// Image offset of the instruction.
        off: usize,
        /// Its kind.
        kind: LoKind,
    },
}

/// `%hi` of `va` for a signed low half (`addiu` / load / store).
pub fn hi_signed(va: u32) -> u16 {
    (va.wrapping_add(0x8000) >> 16) as u16
}

fn word(image: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(image[off..off + 4].try_into().unwrap())
}

fn put(image: &mut [u8], off: usize, w: u32) {
    image[off..off + 4].copy_from_slice(&w.to_le_bytes());
}

/// `true` for a control-transfer instruction (branch, jump, `jr`, `jalr`).
fn is_cti(w: u32) -> bool {
    match w >> 26 {
        0x00 => matches!(w & 0x3F, 0x08 | 0x09),
        0x01..=0x07 => true,
        // COP branches (bc0f/bc0t ...): rs field 0x08.
        0x10..=0x13 => (w >> 21) & 0x1F == 0x08,
        _ => false,
    }
}

/// The GPR an instruction writes, when a register walk must drop it
/// (mirrors `mips_walk.writes_reg`; over-approximates).
fn writes_reg(w: u32) -> Option<u32> {
    let op = w >> 26;
    let rt = (w >> 16) & 0x1F;
    match op {
        0x00 => {
            let funct = w & 0x3F;
            if matches!(funct, 0x08 | 0x0C | 0x0D | 0x11 | 0x13 | 0x18..=0x1B) {
                None
            } else {
                Some((w >> 11) & 0x1F)
            }
        }
        0x01 => ((rt & 0x1E) == 0x10).then_some(31),
        0x03 => Some(31),
        0x08..=0x0F => Some(rt),
        0x10..=0x13 => matches!((w >> 21) & 0x1F, 0x00 | 0x02).then_some(rt),
        0x20..=0x26 => Some(rt),
        _ => None,
    }
}

/// `true` when an instruction may read GPR `reg` as a source. Every field
/// that can name a source counts, so a store's data register and a branch's
/// operands do; over-approximates.
fn may_read(w: u32, reg: u32) -> bool {
    let op = w >> 26;
    let rs = (w >> 21) & 0x1F;
    let rt = (w >> 16) & 0x1F;
    match op {
        // `lui` has no source.
        0x0F => false,
        // `j` / `jal`.
        0x02 | 0x03 => false,
        // I-type ALU and loads read `rs` only.
        0x08..=0x0E | 0x20..=0x26 => rs == reg,
        _ => rs == reg || rt == reg,
    }
}

/// Every instruction index some control transfer can land on: the static
/// targets of branches and `j` / `jal`, plus every aligned word that is the
/// VA of an instruction in the image (a jump table's slot).
fn landing_sites(words: &[u32], base: u32) -> HashSet<usize> {
    let n = words.len();
    let mut out = HashSet::new();
    let end = base.wrapping_add((n * 4) as u32);
    for (i, &w) in words.iter().enumerate() {
        match w >> 26 {
            0x01 | 0x04..=0x07 => {
                let off = (w & 0xFFFF) as u16 as i16 as i64;
                let t = i as i64 + 1 + off;
                if (0..n as i64).contains(&t) {
                    out.insert(t as usize);
                }
            }
            0x02 | 0x03 => {
                let va =
                    (base.wrapping_add((i * 4) as u32) & 0xF000_0000) | ((w & 0x03FF_FFFF) << 2);
                if (base..end).contains(&va) {
                    out.insert(((va - base) / 4) as usize);
                }
            }
            _ => {}
        }
        if (base..end).contains(&w) && w.is_multiple_of(4) {
            out.insert(((w - base) / 4) as usize);
        }
    }
    out
}

/// Longest path a `lui` walk follows, and the most instructions it visits.
const WALK_STEPS: usize = 64;
const WALK_BUDGET: usize = 512;

/// Callee-saved registers survive a call: `s0`-`s7`, `gp`, `sp`, `s8`.
fn survives_call(reg: u32) -> bool {
    matches!(reg, 16..=23 | 28..=30)
}

/// Every completion of the `lui` at word index `at`, down every path.
fn completions(words: &[u32], base: u32, at: usize) -> Vec<(usize, LoKind, u32)> {
    let lui = words[at];
    let reg = (lui >> 16) & 0x1F;
    let hi = (lui & 0xFFFF) << 16;
    let n = words.len();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    // (instruction index, steps taken)
    let mut stack = vec![(at + 1, 0usize)];
    let mut budget = WALK_BUDGET;
    let complete = |i: usize, out: &mut Vec<(usize, LoKind, u32)>| {
        let u = words[i];
        let op = u >> 26;
        if (u >> 21) & 0x1F != reg {
            return;
        }
        let imm = u & 0xFFFF;
        let signed = hi.wrapping_add(imm as u16 as i16 as i32 as u32);
        match op {
            0x09 => out.push((i, LoKind::Addiu, signed)),
            0x0D => out.push((i, LoKind::Ori, hi | imm)),
            0x20..=0x26 | 0x28..=0x2B | 0x2E | 0x32 | 0x3A => out.push((i, LoKind::Mem, signed)),
            _ => {}
        }
    };
    while let Some((mut i, mut steps)) = stack.pop() {
        loop {
            if i >= n || steps >= WALK_STEPS || budget == 0 || !seen.insert(i) {
                break;
            }
            budget -= 1;
            steps += 1;
            let u = words[i];
            complete(i, &mut out);
            if is_cti(u) {
                // The delay slot executes on both paths.
                if i + 1 < n {
                    complete(i + 1, &mut out);
                }
                let slot_writes = i + 1 < n && writes_reg(words[i + 1]) == Some(reg);
                let op = u >> 26;
                if slot_writes {
                    break;
                }
                match op {
                    0x01 | 0x04..=0x07 => {
                        let off = (u & 0xFFFF) as u16 as i16 as i64;
                        let t = i as i64 + 1 + off;
                        if (0..n as i64).contains(&t) {
                            stack.push((t as usize, steps));
                        }
                        // Link forms (`bltzal`/`bgezal`) clobber `ra` only.
                        if reg == 31 && op == 0x01 {
                            break;
                        }
                        i += 2;
                        continue;
                    }
                    // `j`: the word after is reached from elsewhere, so the
                    // walk continues at the target only.
                    0x02 => {
                        let pc = base.wrapping_add((i * 4) as u32);
                        let va = (pc & 0xF000_0000) | ((u & 0x03FF_FFFF) << 2);
                        let t = va.wrapping_sub(base) as usize / 4;
                        if va >= base && t < n {
                            i = t;
                            continue;
                        }
                        break;
                    }
                    // `jal` / `jalr`: the callee may clobber a caller-saved
                    // register; the walk continues past the call only for a
                    // callee-saved one.
                    0x03 => {
                        if survives_call(reg) && reg != 31 {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    0x00 if u & 0x3F == 0x09 => {
                        if survives_call(reg) && reg != 31 {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    // `jr`: the path ends.
                    _ => break,
                }
            }
            if writes_reg(u) == Some(reg) {
                break;
            }
            i += 1;
        }
    }
    out
}

/// Every site in `image` (loaded at `base`) forming an address in
/// `lo..hi`, keyed by the address. `gp` enables the `$gp`-relative forms.
pub fn scan(
    image: &[u8],
    base: u32,
    gp: Option<u32>,
    lo: u32,
    hi: u32,
) -> BTreeMap<u32, Vec<RefSite>> {
    let words: Vec<u32> = image
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    let landing = landing_sites(&words, base);
    let mut out: BTreeMap<u32, Vec<RefSite>> = BTreeMap::new();
    let in_range = |a: u32| (lo..hi).contains(&a);
    for (i, &w) in words.iter().enumerate() {
        if in_range(w) {
            out.entry(w).or_default().push(RefSite::Word { off: i * 4 });
        }
        let op = w >> 26;
        if op == 0x0F && (w >> 16) & 0x1F != 0 {
            let reg = (w >> 16) & 0x1F;
            let comps = completions(&words, base, i);
            // Private: one completion in all, an `addiu` / `ori` that
            // overwrites the register, reached straight down with nothing
            // reading the register, no transfer leaving the gap before the
            // completion's own delay-slot position, nothing landing inside
            // it, and the `lui` itself not sitting in a delay slot.
            let private = comps.len() == 1 && {
                let (j, kind, _) = comps[0];
                let u = words[j];
                j > i
                    && kind != LoKind::Mem
                    && (u >> 16) & 0x1F == reg
                    && (i == 0 || !is_cti(words[i - 1]))
                    && (i + 1..j)
                        .all(|m| !may_read(words[m], reg) && writes_reg(words[m]) != Some(reg))
                    && (i + 1..j.saturating_sub(1)).all(|m| !is_cti(words[m]))
                    && (i + 1..=j).all(|m| !landing.contains(&m))
            };
            for (j, kind, addr) in comps {
                if in_range(addr) {
                    out.entry(addr).or_default().push(RefSite::Pair {
                        lui: i * 4,
                        lo: j * 4,
                        kind,
                        private,
                    });
                }
            }
        }
        if let Some(gp) = gp
            && (w >> 21) & 0x1F == 28
        {
            let kind = match op {
                0x09 => Some(LoKind::Addiu),
                0x0D => Some(LoKind::Ori),
                0x20..=0x26 | 0x28..=0x2B | 0x2E => Some(LoKind::Mem),
                _ => None,
            };
            if let Some(kind) = kind {
                let imm = w & 0xFFFF;
                let a = match kind {
                    LoKind::Ori => gp | imm,
                    _ => gp.wrapping_add(imm as u16 as i16 as i32 as u32),
                };
                if in_range(a) {
                    out.entry(a)
                        .or_default()
                        .push(RefSite::Gp { off: i * 4, kind });
                }
            }
        }
    }
    for v in out.values_mut() {
        let mut seen = BTreeSet::new();
        v.retain(|s| seen.insert(format!("{s:?}")));
    }
    out
}

/// The address `site` forms in `image` as it now reads (a check that a
/// rewrite landed): `None` when the bytes no longer read as the site.
pub fn site_address(image: &[u8], site: &RefSite, gp: u32) -> Option<u32> {
    let rd = |off: usize| image.get(off..off + 4).map(|_| word(image, off));
    match *site {
        RefSite::Word { off } => rd(off),
        RefSite::Pair { lui, lo, kind, .. } => {
            let (l, c) = (rd(lui)?, rd(lo)?);
            if l >> 26 != 0x0F {
                return None;
            }
            let hi = (l & 0xFFFF) << 16;
            let imm = c & 0xFFFF;
            Some(match kind {
                LoKind::Ori => hi | imm,
                _ => hi.wrapping_add(imm as u16 as i16 as i32 as u32),
            })
        }
        RefSite::Gp { off, kind } => {
            let imm = rd(off)? & 0xFFFF;
            Some(match kind {
                LoKind::Ori => gp | imm,
                _ => gp.wrapping_add(imm as u16 as i16 as i32 as u32),
            })
        }
    }
}

/// `true` when `site`, which forms `old`, can be made to form `new`.
pub fn can_retarget(site: &RefSite, old: u32, new: u32, gp: u32) -> bool {
    match *site {
        RefSite::Word { .. } => true,
        RefSite::Pair {
            kind,
            private: true,
            ..
        } if kind != LoKind::Mem => true,
        RefSite::Pair { kind, .. } => match kind {
            LoKind::Ori => old >> 16 == new >> 16,
            _ => hi_signed(old) == hi_signed(new),
        },
        RefSite::Gp { kind, .. } => {
            let d = new.wrapping_sub(gp) as i32;
            match kind {
                LoKind::Ori => new >> 16 == gp >> 16 && new & 0xFFFF >= gp & 0xFFFF,
                _ => (-0x8000..0x8000).contains(&d),
            }
        }
    }
}

/// Rewrite `site` to form `new` (it forms `old` today). Returns the image
/// ranges written, or `None` when [`can_retarget`] says it cannot, or the
/// bytes no longer read as the site (nothing is written then).
pub fn retarget(
    image: &mut [u8],
    site: &RefSite,
    old: u32,
    new: u32,
    gp: u32,
) -> Option<Vec<(usize, usize)>> {
    if !can_retarget(site, old, new, gp) {
        return None;
    }
    let lo_imm = |_: LoKind, addr: u32| addr & 0xFFFF;
    match *site {
        RefSite::Word { off } => {
            if word(image, off) != old {
                return None;
            }
            put(image, off, new);
            Some(vec![(off, 4)])
        }
        RefSite::Pair {
            lui,
            lo,
            kind,
            private,
        } => {
            let lw = word(image, lui);
            let cw = word(image, lo);
            let old_hi = match kind {
                LoKind::Ori => (old >> 16) as u16,
                _ => hi_signed(old),
            };
            if lw >> 26 != 0x0F
                || (lw & 0xFFFF) as u16 != old_hi
                || cw & 0xFFFF != lo_imm(kind, old)
            {
                return None;
            }
            let new_hi = match kind {
                LoKind::Ori => (new >> 16) as u16,
                _ => hi_signed(new),
            };
            let mut written = vec![];
            if new_hi != old_hi {
                if !private {
                    return None;
                }
                put(image, lui, (lw & 0xFFFF_0000) | new_hi as u32);
                written.push((lui, 4));
            }
            put(image, lo, (cw & 0xFFFF_0000) | lo_imm(kind, new));
            written.push((lo, 4));
            Some(written)
        }
        RefSite::Gp { off, kind } => {
            let w = word(image, off);
            let disp = match kind {
                LoKind::Ori => new & 0xFFFF,
                _ => new.wrapping_sub(gp) & 0xFFFF,
            };
            put(image, off, (w & 0xFFFF_0000) | disp);
            Some(vec![(off, 4)])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(ws: &[u32]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn private_pair_retargets_both_halves() {
        // lui a0,0x801d ; jal 0 ; addiu a0,a0,-0x1630  -> 0x801CE9D0
        let mut img = bytes(&[0x3C04_801D, 0x0C00_0000, 0x2484_E9D0, 0x03E0_0008, 0]);
        let refs = scan(&img, 0x8010_0000, None, 0x801C_0000, 0x801F_0000);
        let sites = &refs[&0x801C_E9D0];
        assert_eq!(sites.len(), 1);
        assert!(matches!(sites[0], RefSite::Pair { private: true, .. }));
        let w = retarget(&mut img, &sites[0], 0x801C_E9D0, 0x801E_D340, 0).unwrap();
        assert_eq!(w.len(), 2);
        let again = scan(&img, 0x8010_0000, None, 0x801C_0000, 0x801F_0000);
        assert!(again.contains_key(&0x801E_D340));
    }

    #[test]
    fn shared_high_half_moves_only_within_its_hi() {
        // lui v0,0x801d ; addiu a0,v0,-0x1630 ; addiu a1,v0,-0x1620
        let mut img = bytes(&[0x3C02_801D, 0x2444_E9D0, 0x2445_E9E0, 0x03E0_0008, 0]);
        let refs = scan(&img, 0x8010_0000, None, 0x801C_0000, 0x801F_0000);
        let s = refs[&0x801C_E9D0][0];
        assert!(matches!(s, RefSite::Pair { private: false, .. }));
        assert!(!can_retarget(&s, 0x801C_E9D0, 0x801E_D340, 0));
        assert!(can_retarget(&s, 0x801C_E9D0, 0x801C_E9F0, 0));
        retarget(&mut img, &s, 0x801C_E9D0, 0x801C_E9F0, 0).unwrap();
        let again = scan(&img, 0x8010_0000, None, 0x801C_0000, 0x801F_0000);
        assert!(again.contains_key(&0x801C_E9F0));
        assert!(again.contains_key(&0x801C_E9E0));
    }

    #[test]
    fn a_landing_site_inside_the_gap_makes_a_pair_shared() {
        // lui a0,hi ; nop ; addiu a0,a0,lo ; ... ; beq zero,zero,-3 (lands on the nop)
        let img = bytes(&[0x3C04_801D, 0, 0x2484_E9D0, 0x03E0_0008, 0, 0x1000_FFFC, 0]);
        let refs = scan(&img, 0x8010_0000, None, 0x801C_0000, 0x801F_0000);
        assert!(matches!(
            refs[&0x801C_E9D0][0],
            RefSite::Pair { private: false, .. }
        ));
    }

    #[test]
    fn gp_forms_are_found_and_bounded() {
        // addiu a0,gp,0x340 -> gp+0x340
        let mut img = bytes(&[0x2784_0340]);
        let refs = scan(&img, 0x8010_0000, Some(RETAIL_GP), 0x8007_0000, 0x8008_0000);
        let s = refs[&(RETAIL_GP + 0x340)][0];
        assert!(can_retarget(
            &s,
            RETAIL_GP + 0x340,
            RETAIL_GP - 0x100,
            RETAIL_GP
        ));
        assert!(!can_retarget(
            &s,
            RETAIL_GP + 0x340,
            RETAIL_GP + 0x9000,
            RETAIL_GP
        ));
        retarget(
            &mut img,
            &s,
            RETAIL_GP + 0x340,
            RETAIL_GP - 0x100,
            RETAIL_GP,
        )
        .unwrap();
        assert!(
            scan(&img, 0x8010_0000, Some(RETAIL_GP), 0x8007_0000, 0x8008_0000)
                .contains_key(&(RETAIL_GP - 0x100))
        );
    }

    #[test]
    fn a_completion_after_a_branch_is_followed_on_both_paths() {
        // lui a0,hi ; beq v0,zero,+2 ; nop ; addiu a1,a0,lo1 ; jr ra ; nop ; addiu a2,a0,lo2
        let img = bytes(&[
            0x3C04_801D,
            0x1040_0004,
            0,
            0x2485_E9D0,
            0x03E0_0008,
            0,
            0x2486_E9E0,
            0x03E0_0008,
            0,
        ]);
        let refs = scan(&img, 0x8010_0000, None, 0x801C_0000, 0x801F_0000);
        assert!(refs.contains_key(&0x801C_E9D0));
        assert!(refs.contains_key(&0x801C_E9E0));
    }
}
