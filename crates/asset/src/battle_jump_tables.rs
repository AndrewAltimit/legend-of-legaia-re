//! The battle overlay's `switch` jump tables and its head string pool - the
//! rodata at the front of PROT `0898`, bound to the instructions that read it.
//!
//! The first `0xDF8` bytes of the battle overlay (link base `0x801CE818`) are
//! not one structure. They are a short C-string pool and then **twenty-two**
//! jump tables laid back to back, each one read by exactly one
//! `switch` dispatch in the overlay's code, with a zero word of alignment
//! padding between a few of them. None of the tables is named by a literal
//! word anywhere on the disc: every consumer forms the base with a
//! `lui $v0,0x801D` / `addiu $v0,$v0,lo` pair, bounds the index with a
//! `sltiu $v0,$vN,arms` + `beqz` to the default arm, scales it (`sll $v1,$v1,2`),
//! loads the word and `jr`s to it - so the table's extent is the `sltiu`
//! immediate times four, a measurement read off the consumer rather than a run
//! scanned out of the bytes.
//!
//! That is why an earlier reading of this region as **nine** tables was wrong:
//! it counted runs of in-image VA words, and adjacent tables with no padding
//! between them merge into one run. Twenty-two consumers form twenty-two
//! distinct bases, and the bases tile the region exactly: each table's
//! `base + 4*arms` is the next table's base or a zero pad word below it.
//!
//! Every constant here is a coordinate; the words themselves are read from the
//! caller's image. [`check`] re-derives each row from the image's own
//! instructions, so a row that stops matching the bytes fails loudly.
//!
//! Provenance: disassembly of PROT 0898 at its static base
//! (`crates/asset/data/static-overlays.toml`), each table's consumer located
//! with `scripts/ghidra-analysis/find-address-word-refs.py --range`
//! (`LUI` form). The arms of every table land inside the dumped extent that
//! also holds its dispatch `jr` (`scripts/ghidra-analysis/dump-extent-attribution.csv`,
//! asserted by `crates/asset/tests/battle_jump_tables_real.rs`).

/// Link base of the battle overlay (PROT 0898).
pub const OVERLAY_BASE_VA: u32 = crate::battle_ui_strings::OVERLAY_BASE_VA;

/// PROT extraction index of the battle overlay.
pub const OVERLAY_PROT_INDEX: u32 = 898;

/// One `switch` jump table and the dispatch that reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpTable {
    /// VA of word 0.
    pub va: u32,
    /// Arm count: the consumer's `sltiu` bound, so the extent is `4 * arms`.
    pub arms: u32,
    /// VA of the `lui` that forms the base (the `addiu` is the next word).
    pub base_site: u32,
    /// VA of the dispatch `jr`.
    pub jr: u32,
    /// The index the consumer bounds, as the instructions compute it: a
    /// register, or `$reg[+off]` for an `lbu` off a base register, or
    /// `(*addr)[+off]` for an `lbu` off a pointer the code loads from `addr`.
    pub index: &'static str,
}

impl JumpTable {
    /// File offset of word 0 in the image.
    pub const fn offset(&self) -> usize {
        (self.va - OVERLAY_BASE_VA) as usize
    }
    /// Byte length (`4 * arms`).
    pub const fn byte_len(&self) -> usize {
        self.arms as usize * 4
    }
}

const fn jt(va: u32, arms: u32, base_site: u32, jr: u32, index: &'static str) -> JumpTable {
    JumpTable {
        va,
        arms,
        base_site,
        jr,
        index,
    }
}

/// Every jump table in the overlay's head, in address order.
pub const JUMP_TABLES: [JumpTable; 22] = [
    jt(0x801C_E880, 50, 0x801D_38E8, 0x801D_3900, "$s8"),
    jt(0x801C_E948, 45, 0x801D_4DA0, 0x801D_4DB8, "$s8 - 5"),
    jt(0x801C_EA00, 10, 0x801D_59C0, 0x801D_59D8, "$s4"),
    jt(
        0x801C_EA28,
        8,
        0x801D_5DC0,
        0x801D_5DD8,
        "$s2[+0x1DB] - 0x11",
    ),
    jt(
        0x801C_EA48,
        8,
        0x801D_5FD4,
        0x801D_5FEC,
        "$s2[+0x1DB] - 0x11",
    ),
    jt(
        0x801C_EA68,
        8,
        0x801D_61FC,
        0x801D_6214,
        "$s2[+0x1DB] - 0x11",
    ),
    jt(
        0x801C_EA88,
        17,
        0x801D_72E8,
        0x801D_7300,
        "$s0[+0x1DB] - 0x1A",
    ),
    jt(
        0x801C_EAD0,
        20,
        0x801D_76CC,
        0x801D_76E4,
        "$s0[+0x1DB] - 0x1A",
    ),
    jt(
        0x801C_EB20,
        17,
        0x801D_7B2C,
        0x801D_7B44,
        "$s0[+0x1DB] - 0x1A",
    ),
    jt(0x801C_EB68, 80, 0x801D_8EA8, 0x801D_8EC0, "$s2 - 0x0A"),
    jt(0x801C_ECAC, 20, 0x801D_C1E4, 0x801D_C1FC, "$a1 & 0xFF"),
    jt(
        0x801C_ECFC,
        7,
        0x801D_E628,
        0x801D_E63C,
        "(*0x801C9358)[+0x1D]",
    ),
    jt(
        0x801C_ED44,
        256,
        0x801E_2A94,
        0x801E_2AAC,
        "(*0x8007BD24)[+0x07]",
    ),
    jt(0x801C_F144, 6, 0x801E_2D70, 0x801E_2D88, "$s3[+0x1DE]"),
    jt(
        0x801C_F15C,
        27,
        0x801E_7160,
        0x801E_7178,
        "$a0[+0x1DF] - 0x86",
    ),
    jt(
        0x801C_F1CC,
        179,
        0x801E_A9E4,
        0x801E_A9FC,
        "*(0x8007BD0C + $s7) - 4",
    ),
    jt(
        0x801C_F49C,
        5,
        0x801E_B548,
        0x801E_B560,
        "(*0x8007BD24)[+0x28A]",
    ),
    jt(
        0x801C_F4B4,
        6,
        0x801E_CBA4,
        0x801E_CBBC,
        "$v1[+0x1D9] - 0x0C",
    ),
    jt(
        0x801C_F4CC,
        8,
        0x801F_1324,
        0x801F_133C,
        "(*0x8007BD24)[+0x276] - 1",
    ),
    jt(
        0x801C_F4EC,
        32,
        0x801F_1F1C,
        0x801F_1F34,
        "$v0[+0x1DF] - 0x81",
    ),
    jt(0x801C_F56C, 32, 0x801F_21BC, 0x801F_21D4, "$v0[+0x01]"),
    jt(0x801C_F5EC, 9, 0x801F_39F8, 0x801F_3A10, "$v0[+0x1E8]"),
];

/// One NUL-terminated string in the head pool and the instruction that loads
/// its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadString {
    /// VA of the first byte.
    pub va: u32,
    /// VA of the `lui` of the `lui`/`addiu` pair that loads it (the `addiu`
    /// may sit a few words later).
    pub site: u32,
}

impl HeadString {
    /// File offset in the image.
    pub const fn offset(&self) -> usize {
        (self.va - OVERLAY_BASE_VA) as usize
    }
}

/// The head strings whose address a consumer forms. The first is handed to
/// the three call sites that pass it in `$a2`; the rest are loaded into `$a0`,
/// `$a1` or stored into a record's `+0x14` payload pointer.
pub const HEAD_STRINGS: [HeadString; 7] = [
    HeadString {
        va: 0x801C_E818,
        site: 0x801D_0F80,
    },
    HeadString {
        va: 0x801C_E840,
        site: 0x801D_3614,
    },
    HeadString {
        va: 0x801C_E860,
        site: 0x801D_371C,
    },
    HeadString {
        va: 0x801C_E878,
        site: 0x801D_40C4,
    },
    HeadString {
        va: 0x801C_ECA8,
        site: 0x801D_9E4C,
    },
    HeadString {
        va: 0x801C_ED18,
        site: 0x801E_3644,
    },
    HeadString {
        va: 0x801C_ED34,
        site: 0x801E_3C5C,
    },
];

fn word(image: &[u8], va: u32) -> Option<u32> {
    let off = va.checked_sub(OVERLAY_BASE_VA)? as usize;
    legaia_bytes::u32_le(image, off)
}

/// `lui` / `addiu` halves of `va` as the assembler emits them: the `addiu`
/// immediate is sign-extended, so the `lui` half carries the borrow.
fn hi_lo(va: u32) -> (u16, u16) {
    let lo = va as u16;
    let hi = (va.wrapping_sub((lo as i16) as i32 as u32) >> 16) as u16;
    (hi, lo)
}

/// Does a `lui rt,hi` at `site` feed an `addiu rt,rt,lo` within the next
/// `window` words?
fn forms_address(image: &[u8], site: u32, va: u32, window: u32) -> bool {
    let (hi, lo) = hi_lo(va);
    let Some(lui) = word(image, site) else {
        return false;
    };
    if lui >> 26 != 0x0F || lui as u16 != hi {
        return false;
    }
    let rt = (lui >> 16) & 0x1F;
    (1..=window).any(|k| {
        word(image, site + 4 * k).is_some_and(|w| {
            w >> 26 == 0x09 && (w >> 21) & 0x1F == rt && (w >> 16) & 0x1F == rt && w as u16 == lo
        })
    })
}

/// Re-derive every row from the image's own instructions.
///
/// For a jump table: the `lui`/`addiu` pair at `base_site` forms `va`; a
/// `sltiu` with immediate `arms` sits within the eight words before it; a `jr`
/// sits at `jr`, at most eight words after it; and every arm is an in-image
/// VA. For a string: its pair forms `va` and the bytes from there hold a NUL.
/// Returns one line per disagreement, empty when the image is the retail one.
pub fn check(image: &[u8]) -> Vec<String> {
    let mut errs = Vec::new();
    let end_va = OVERLAY_BASE_VA + image.len() as u32;
    for t in &JUMP_TABLES {
        if !forms_address(image, t.base_site, t.va, 1) {
            errs.push(format!(
                "{:#010x}: no lui/addiu pair at {:#010x}",
                t.va, t.base_site
            ));
        }
        let bound = (1..=8).any(|k| {
            word(image, t.base_site - 4 * k)
                .is_some_and(|w| w >> 26 == 0x0B && (w & 0xFFFF) == t.arms)
        });
        if !bound {
            errs.push(format!(
                "{:#010x}: no sltiu {} before {:#010x}",
                t.va, t.arms, t.base_site
            ));
        }
        let jr_ok = t.jr > t.base_site
            && t.jr <= t.base_site + 32
            && word(image, t.jr).is_some_and(|w| w & 0xFC1F_FFFF == 0x0000_0008);
        if !jr_ok {
            errs.push(format!("{:#010x}: no jr at {:#010x}", t.va, t.jr));
        }
        for i in 0..t.arms {
            match word(image, t.va + 4 * i) {
                Some(a) if (OVERLAY_BASE_VA..end_va).contains(&a) => {}
                other => errs.push(format!(
                    "{:#010x}: arm {i} = {other:x?} is not in the image",
                    t.va
                )),
            }
        }
    }
    for s in &HEAD_STRINGS {
        if !forms_address(image, s.site, s.va, 8) {
            errs.push(format!(
                "string {:#010x}: no lui/addiu pair at {:#010x}",
                s.va, s.site
            ));
        }
        if image.get(s.offset()..).is_none_or(|t| !t.contains(&0)) {
            errs.push(format!("string {:#010x}: no NUL terminator", s.va));
        }
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_tile_the_head_with_at_most_one_pad_word_between() {
        for pair in JUMP_TABLES.windows(2) {
            let end = pair[0].va + 4 * pair[0].arms;
            let gap = pair[1].va.checked_sub(end).expect("tables overlap");
            // Two strings sit between ECA8 and ED44; everywhere else the
            // tables abut or leave one alignment word.
            assert!(
                gap <= 4
                    || (pair[0].va, pair[1].va) == (0x801C_EB68, 0x801C_ECAC)
                    || (pair[0].va, pair[1].va) == (0x801C_ECFC, 0x801C_ED44),
                "{:#x} -> {:#x}: gap {gap}",
                pair[0].va,
                pair[1].va
            );
        }
    }

    #[test]
    fn hi_lo_carries_the_sign_borrow() {
        // 0x801CE880 = lui 0x801D + addiu -0x1780.
        assert_eq!(hi_lo(0x801C_E880), (0x801D, (-0x1780i16) as u16));
        assert_eq!(hi_lo(0x801C_F1CC), (0x801D, (-0x0E34i16) as u16));
    }
}
