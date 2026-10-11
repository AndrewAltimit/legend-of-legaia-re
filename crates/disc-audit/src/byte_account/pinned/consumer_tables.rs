//! Tables whose extent is bound to the consumer instruction that walks them:
//! the row table, the `lui` pair resolver and the claim pass over it.

use super::*;

/// Where a consumer-pinned table's count comes from.
#[derive(Clone, Copy)]
pub(in crate::byte_account) enum PinnedCount {
    /// The loop that walks the table states it: the word at `site` is an
    /// `slti` / `sltiu` (or `li`) whose immediate is the count.
    Loop { site: u32 },
    /// The index's domain is another table's row count, named by a constant
    /// of the module that parses that table.
    Domain { what: &'static str },
    /// Nothing bounds the index; the table runs to the next address the image
    /// forms, `next` (formed by the `lui` at `site`), and that distance must be
    /// a whole number of elements - the runtime-index rule's layout test, for
    /// a table whose field reads sit too far from the index arithmetic for
    /// that rule's straight-line scan.
    Layout { next: u32, site: u32 },
    /// A counted loop whose index starts at the `li` at `first` and runs while
    /// the `sltiu` / `slti` at `bound` holds: the count is the difference of
    /// the two immediates. The index is subtracted back to zero before the
    /// table is addressed, so neither immediate alone is the count.
    Range { first: u32, bound: u32 },
}

/// One table whose base, stride and count are each read off a consumer
/// instruction rather than off the bytes. See [`claim_consumer_pinned_tables`].
pub(in crate::byte_account) struct ConsumerPinnedTable {
    pub(super) prot: u32,
    pub(super) base_va: u32,
    pub(super) stride: usize,
    pub(super) count: usize,
    /// `(lui site, address formed)`: the `lui` at `site` and the first
    /// following instruction that addresses through its register (within four
    /// words) together form `address` - the table base or one of its fields.
    forms: &'static [(u32, u32)],
    count_from: PinnedCount,
    what: &'static str,
}

/// Tables the generic array rules cannot size, each pinned by the
/// instructions that consume it and re-checked against this image's own words
/// before it is claimed.
pub(in crate::byte_account) const CONSUMER_PINNED_TABLES: [ConsumerPinnedTable; 15] = [
    // DEBUG MODE's variable-monitor rows: the `FUN_8001C93C` row layout
    // (`+0x00` kind, `+0x04` y, `+0x08` value pointer, `+0x0E` label, `+0x24`
    // name table), walked inline by the menu loop at `0x801CEBC0`. The kind
    // read `lh v1,-0x770(at)` (`lui at` at `0x801CEC1C`, indexed by the loop's
    // `s3 += 0x28`) forms the base; the row-y pointer `s2` starts at `+4`
    // (`0x801CEBA8`), bumped `addiu s2,s2,0x28`; the loop bound is `sltiu
    // v0,s0,0x16` at `0x801CECE0`.
    ConsumerPinnedTable {
        prot: 971,
        base_va: 0x801C_F890,
        stride: 0x28,
        count: 22,
        forms: &[(0x801C_EC1C, 0x801C_F890), (0x801C_EBA8, 0x801C_F894)],
        count_from: PinnedCount::Loop { site: 0x801C_ECE0 },
        what: "DEBUG MODE value-monitor rows (FUN_8001C93C layout)",
    },
    // The fishing overlay's per-species eight-byte records: `lw` of the hooked
    // species id `0x801D91CC`, `sll 3`, `addu` with the base formed at
    // `0x801D52CC` / `0x801D5360` / `0x801D5414`, then halfword reads at
    // `+0`, `+2`, `+4`, `+6`. The index is the species id, whose domain is
    // the ten-row species table; the tenth record ends exactly on the `HIT`
    // string at `0x801D8584`.
    ConsumerPinnedTable {
        prot: 972,
        base_va: 0x801D_8534,
        stride: 8,
        count: legaia_minigame_formats::fishing_species::SPECIES_COUNT,
        forms: &[
            (0x801D_52CC, 0x801D_8534),
            (0x801D_5360, 0x801D_8534),
            (0x801D_5414, 0x801D_8534),
        ],
        count_from: PinnedCount::Domain {
            what: "the hooked species id (fishing_species::SPECIES_COUNT rows)",
        },
        what: "fishing per-species motion records",
    },
    // The contest hub's twenty-byte sprite records: `FUN_801D050C` and
    // `FUN_801D08EC` index them `(a2 & 0x3FF) * 20` off the base formed at
    // `0x801D0544` / `0x801D093C`, and read every field - `+0x00` word,
    // `+0x04` / `+0x06` halfwords, `+0x08..+0x13` bytes - most of them over a
    // hundred words below the `addu`, past the reach of the runtime-index
    // rule's straight-line scan. The fixed accesses the image makes into the
    // table land on the same fields at the same widths: `sb 0xC3(s4)` /
    // `sb 0xC7(s4)` at `0x801CF538` (element 9, `+0x0F` / `+0x13`),
    // `sh 0xBA(v1)` and `sb 8(s3)` in `FUN_801D1308` (element 9, `+0x06` /
    // `+0x08`), and the direct byte globals at elements 1 and 4 (`+0x08`,
    // `+0x0B`). The next address the image forms, `0x801D1860`, closes
    // seventeen whole records.
    ConsumerPinnedTable {
        prot: 977,
        base_va: 0x801D_170C,
        stride: 0x14,
        count: 17,
        forms: &[
            (0x801C_F530, 0x801D_170C),
            (0x801D_0544, 0x801D_170C),
            (0x801D_093C, 0x801D_170C),
            (0x801D_13D0, 0x801D_170C),
        ],
        count_from: PinnedCount::Layout {
            next: 0x801D_1860,
            site: 0x801D_10E8,
        },
        what: "contest hub sprite records",
    },
    // The dance overlay's per-dancer motion scripts: `FUN_801D0640` forms the
    // base at `0x801D0674`, indexes it `actor[+0x50] << 7` (one `0x80`-byte
    // row per script) plus `actor[+0x9C] * 2`, and reads `(clip, frames)`
    // halfword pairs until a negative clip rewinds the cursor. Nothing bounds
    // `+0x50`; the next address the image forms, `0x801D46CC`
    // (`0x801D2630`), closes four whole rows.
    ConsumerPinnedTable {
        prot: 980,
        base_va: 0x801D_44CC,
        stride: 0x80,
        count: 4,
        forms: &[(0x801D_0674, 0x801D_44CC)],
        count_from: PinnedCount::Layout {
            next: 0x801D_46CC,
            site: 0x801D_2630,
        },
        what: "dance motion-script rows ((clip, frames) pairs, -1 rewinds)",
    },
    // The first of the two step-index lists `FUN_801CF470` picks between on
    // `_DAT_801D514C` (`0x801CF5F8..0x801CF60C`): words read `lw` at
    // `index * 4`, ending on a `-1`. The other list's base, `0x801D4488`, is
    // the next address the image forms and closes eighteen whole words.
    ConsumerPinnedTable {
        prot: 980,
        base_va: 0x801D_4440,
        stride: 4,
        count: 18,
        forms: &[(0x801C_F5FC, 0x801D_4440)],
        count_from: PinnedCount::Layout {
            next: 0x801D_4488,
            site: 0x801C_F608,
        },
        what: "dance step-index list A (u32, -1 terminated)",
    },
    // The Baka Fighter developer dump `FUN_801D553C` (retail's `ot5stat.txt`)
    // walks its fighter-code labels with a pointer it keeps in a stack slot:
    // the base is formed at `0x801D5588` (and again at `0x801D56DC`), stored
    // at `sp+0x32C`, bumped `addiu v1,v1,0x10` per pass, and the pass count
    // is `sltiu v0,s7,0x11` at `0x801D5754` - seventeen sixteen-byte labels.
    // The pointer-bump rule cannot see a pointer that lives in memory.
    ConsumerPinnedTable {
        prot: 976,
        base_va: 0x801D_B7A8,
        stride: 0x10,
        count: 17,
        forms: &[(0x801D_5588, 0x801D_B7A8), (0x801D_56DC, 0x801D_B7A8)],
        count_from: PinnedCount::Loop { site: 0x801D_5754 },
        what: "Baka Fighter dev-dump fighter labels (FUN_801D553C)",
    },
    // `FUN_801D2A28`'s per-index score words, added into `_DAT_801DBED8`:
    // the index is `_DAT_801DBEC8` clamped by `slti v0,a3,0x14` at
    // `0x801D2A34` (and `li a3,0x13`), so twenty words from the base formed
    // at `0x801D2A48`.
    ConsumerPinnedTable {
        prot: 976,
        base_va: 0x801D_70C4,
        stride: 4,
        count: 20,
        forms: &[(0x801D_2A48, 0x801D_70C4)],
        count_from: PinnedCount::Loop { site: 0x801D_2A34 },
        what: "Baka Fighter per-index score words (FUN_801D2A28)",
    },
    // The save menu's twelve-byte sprite cells: `FUN_801E3FF0` addresses
    // `a0 * 12` (`sll v1,a0,1` / `addu` / `sll v1,v1,2`) off the base it forms
    // at `0x801E4028` and reads `+6` / `+8` as the cell's size into a
    // `0x2C` textured quad. The index is a caller immediate with no bound; the
    // next formed address, the quad records at `0x801E50A8`, closes eight
    // whole cells.
    ConsumerPinnedTable {
        prot: 899,
        base_va: 0x801E_5048,
        stride: 12,
        count: 8,
        forms: &[(0x801E_4028, 0x801E_5048), (0x801E_2298, 0x801E_5048)],
        count_from: PinnedCount::Layout {
            next: 0x801E_50A8,
            site: 0x801E_2F1C,
        },
        what: "save-menu sprite cells (FUN_801E3FF0)",
    },
    // The save menu's twenty-byte quad records: `FUN_801E2EE4` addresses
    // `(a3 & 0x3FF) * 20` off the base it forms at `0x801E2F1C` and reads
    // `+0x0C`, `+0x0F` and `+0x13` into a `0x3C` gouraud-textured quad - the
    // same record shape the contest hub's sprite table uses. The next formed
    // address, the card read buffer at `0x801E5120` (`0x801DFAA0`), closes
    // six whole records.
    ConsumerPinnedTable {
        prot: 899,
        base_va: 0x801E_50A8,
        stride: 0x14,
        count: 6,
        forms: &[(0x801E_2F1C, 0x801E_50A8)],
        count_from: PinnedCount::Layout {
            next: 0x801E_5120,
            site: 0x801D_FAA0,
        },
        what: "save-menu quad records (FUN_801E2EE4)",
    },
    // The battle overlay's two per-command scalar tables, each five bytes,
    // indexed by `(cmd - 0x0C) mod 5` that `FUN_801EC3E4` computes once and
    // keeps at `sp+0x18`: `addiu a1,a1,-0xC`, the `0x66666667` reciprocal
    // divide by five, and `a1 - 5 * q` (`0x801EC588..0x801EC5C0`). The
    // defender-side scalar is read once (`0x801EC678`), the attacker-side
    // one twice (`0x801ECE9C`, `0x801ED308`). Each is followed by three
    // zero bytes of word alignment before the next table.
    ConsumerPinnedTable {
        prot: 898,
        base_va: 0x801F_64E4,
        stride: 1,
        count: 5,
        forms: &[(0x801E_C678, 0x801F_64E4)],
        count_from: PinnedCount::Domain {
            what: "(queue command - 0x0C) mod 5, FUN_801EC3E4 0x801EC588..0x801EC5C0",
        },
        what: "defender-side per-command scalar (FUN_801EC3E4)",
    },
    ConsumerPinnedTable {
        prot: 898,
        base_va: 0x801F_64EC,
        stride: 1,
        count: 5,
        forms: &[(0x801E_CE9C, 0x801F_64EC), (0x801E_D308, 0x801F_64EC)],
        count_from: PinnedCount::Domain {
            what: "(queue command - 0x0C) mod 5, FUN_801EC3E4 0x801EC588..0x801EC5C0",
        },
        what: "attacker-side per-command scalar (FUN_801EC3E4)",
    },
    // The Miracle Art trigger rows `FUN_801EED1C` copies into the action
    // queue: base formed at `0x801EF4E8`, row `(char_id - 1) << 4`, sixteen
    // bytes per row (`sltiu v0,v0,0x10` at `0x801EF520`). The next address
    // the image forms is the Super Art `find` table at `0x801F6524`
    // (`0x801EFA38`), which closes three whole rows - Vahn, Noa, Gala
    // ([`art-data.md`](../../../docs/formats/art-data.md)).
    ConsumerPinnedTable {
        prot: 898,
        base_va: 0x801F_64F4,
        stride: 0x10,
        count: 3,
        forms: &[(0x801E_F4E8, 0x801F_64F4)],
        count_from: PinnedCount::Layout {
            next: 0x801F_6524,
            site: 0x801E_FA38,
        },
        what: "Miracle Art trigger rows (FUN_801EED1C)",
    },
    // The Super Art `replace` strings `FUN_801EF9E4` writes over a matched
    // queue tail: base formed at `0x801EFA58`, element `char * 0x50 +
    // entry * 0x10` with `entry < 5` (`slti v0,a1,0x5` at `0x801EFBE0`), so
    // five sixteen-byte strings per character. The next formed address, the
    // opening-shot table at `0x801F66D8` (`0x801EA558`), closes fifteen.
    ConsumerPinnedTable {
        prot: 898,
        base_va: 0x801F_65E8,
        stride: 0x10,
        count: 15,
        forms: &[(0x801E_FA58, 0x801F_65E8)],
        count_from: PinnedCount::Layout {
            next: 0x801F_66D8,
            site: 0x801E_A558,
        },
        what: "Super Art replace strings, 3 characters x 5 (FUN_801EF9E4)",
    },
    // The monster cast pick's opening-shot bytes, `0x801F66D8 + id - 0x25`
    // ([`legaia_game_tables::spell_anim_pairs::OPENING_SHOT_VA`]). The index is a spell id
    // with no upper bound check; the next formed address is the status-guard
    // mask table at `0x801F672C` (`0x801F07DC`), which closes eighty-four
    // bytes - ids `0x25..0x79`. A higher id reads the guard masks.
    ConsumerPinnedTable {
        prot: 898,
        base_va: legaia_game_tables::spell_anim_pairs::OPENING_SHOT_VA,
        stride: 1,
        count: 0x54,
        forms: &[(
            0x801E_A558,
            legaia_game_tables::spell_anim_pairs::OPENING_SHOT_VA,
        )],
        count_from: PinnedCount::Layout {
            next: 0x801F_672C,
            site: 0x801F_07DC,
        },
        what: "monster cast opening-shot bytes (spell_anim_pairs)",
    },
    // The four direction commands' status-guard masks, `lh` at
    // `0x801F672C + (cmd - 0x0C) * 2` in `FUN_801F0450`'s command loop:
    // `li s2,0xC` at `0x801F07BC`, `sltiu v0,v0,0x10` at `0x801F0A04`.
    ConsumerPinnedTable {
        prot: 898,
        base_va: 0x801F_672C,
        stride: 2,
        count: 4,
        forms: &[(0x801F_07DC, 0x801F_672C)],
        count_from: PinnedCount::Range {
            first: 0x801F_07BC,
            bound: 0x801F_0A04,
        },
        what: "direction-command status-guard masks (FUN_801F0450)",
    },
];

/// The address the `lui` at file offset `at` forms with the first following
/// instruction (within four words) that addresses through the same register:
/// an `addiu`, a load or a store. `None` when the word is not a `lui`.
pub(in crate::byte_account) fn lui_pair_address(buf: &[u8], at: usize) -> Option<u32> {
    let w = legaia_bytes::u32_le(buf, at)?;
    if w >> 26 != 0x0F {
        return None;
    }
    let reg = (w >> 16) & 0x1F;
    let hi = (w & 0xFFFF) << 16;
    for k in 1..=4 {
        let x = legaia_bytes::u32_le(buf, at + 4 * k)?;
        let (op, rs) = (x >> 26, (x >> 21) & 0x1F);
        if rs == reg && matches!(op, 0x09 | 0x20..=0x26 | 0x28..=0x2B) {
            return Some(hi.wrapping_add((x & 0xFFFF) as i16 as i32 as u32));
        }
    }
    None
}

/// Claim each [`CONSUMER_PINNED_TABLES`] row of this entry, after checking
/// every instruction the row cites: each `lui` site must still form its
/// address, and a loop-stated count must still be the immediate at its site.
/// A row that disagrees with the image gets a note and no claim.
pub(in crate::byte_account) fn claim_consumer_pinned_tables(
    buf: &[u8],
    sink: &mut Sink,
    prot_index: u32,
) {
    const SLOT_A: u32 = 0x801C_E818;
    for t in CONSUMER_PINNED_TABLES
        .iter()
        .filter(|t| t.prot == prot_index)
    {
        let at = |va: u32| (va - SLOT_A) as usize;
        let forms_ok = t
            .forms
            .iter()
            .all(|&(site, va)| lui_pair_address(buf, at(site)) == Some(va));
        let count_ok = match t.count_from {
            PinnedCount::Loop { site } => legaia_bytes::u32_le(buf, at(site)).is_some_and(|w| {
                matches!(w >> 26, 0x09..=0x0B) && (w & 0xFFFF) as usize == t.count
            }),
            PinnedCount::Domain { .. } => true,
            PinnedCount::Range { first, bound } => {
                let imm = |site: u32, ops: &[u32]| {
                    legaia_bytes::u32_le(buf, at(site))
                        .filter(|w| ops.contains(&(w >> 26)))
                        .map(|w| (w & 0xFFFF) as usize)
                };
                // `li` is `addiu rt,zero,imm` / `ori rt,zero,imm`.
                matches!(
                    (imm(first, &[0x09, 0x0D]), imm(bound, &[0x0A, 0x0B])),
                    (Some(a), Some(b)) if b > a && b - a == t.count
                )
            }
            PinnedCount::Layout { next, site } => {
                lui_pair_address(buf, at(site)) == Some(next)
                    && next > t.base_va
                    && (next - t.base_va) as usize == t.count * t.stride
            }
        };
        let (start, end) = (at(t.base_va), at(t.base_va) + t.count * t.stride);
        if !forms_ok || !count_ok || end > buf.len() {
            sink.note(format!(
                "{} at {:#010x} not claimed: a cited instruction no longer reads as stated",
                t.what, t.base_va
            ));
            continue;
        }
        let count_why = match t.count_from {
            PinnedCount::Loop { site } => format!("loop bound at {site:#010x}"),
            PinnedCount::Domain { what } => format!("index domain: {what}"),
            PinnedCount::Range { first, bound } => {
                format!("loop from {first:#010x} to the bound at {bound:#010x}")
            }
            PinnedCount::Layout { next, .. } => {
                format!("whole records to the next formed address {next:#010x}")
            }
        };
        sink.claim(
            start,
            end,
            OWNER_RECORD,
            format!(
                "{}, {} x {:#x} bytes ({count_why}; base formed at {} site(s))",
                t.what,
                t.count,
                t.stride,
                t.forms.len()
            ),
        );
    }
}
