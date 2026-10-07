//! Overlay sub-assets and tables pinned by consumer, and the slot-B module walker.
//! Split out of `byte_account.rs`.

use super::*;

/// Sub-assets an overlay image carries at an offset this workspace has pinned.
///
/// The dump corpus is the parser for a code image's code and says nothing about
/// its data segment, so an asset sitting in that segment falls to the magic
/// sweep - which finds it, tags the claim `scan`, and thereby reports "found by
/// guessing" for something a module here already reads at a named constant.
/// That gap is a binding, not a format: the offsets below are the constants,
/// and the extents come from the TIM headers rather than from this table.
pub(super) fn claim_pinned_overlay_assets(buf: &[u8], sink: &mut Sink, prot_index: u32) {
    const MENU_OVERLAY: u32 = 899;
    if prot_index == MENU_OVERLAY {
        // The option-node list's extent is its own zero-word terminator, so
        // it is measured here rather than carried as a fixed row.
        if let Some(len) = crate::menu_windows::options_node_list_len(buf) {
            let off = crate::menu_windows::OPTIONS_NODE_LIST_OFFSET;
            sink.claim(
                off,
                off + len,
                OWNER_RECORD,
                "options row-descriptor list (menu_windows)",
            );
        }
        for (off, what) in [
            (
                crate::title_pak::OVERLAY_SAVE_MENU_TIM_OFFSET,
                "save-menu UI atlas",
            ),
            (crate::save_icon::PROT_ENTRY_OFFSET, "save-slot icon sheet"),
        ] {
            match crate::tim_scan::parse_at(buf, off) {
                Some(h) => sink.claim(
                    off,
                    (off + h.byte_len).min(buf.len()),
                    OWNER_TIM,
                    format!("{what}, {}x{} {}bpp", h.width, h.height, h.bpp),
                ),
                None => sink.note(format!("no TIM at the pinned {what} offset {off:#x}")),
            }
        }
    }
    for (off, len, owner, what) in pinned_overlay_tables(prot_index) {
        let end = off + len;
        if end > buf.len() {
            sink.note(format!(
                "pinned {what} at {off:#x} + {len} runs past this entry"
            ));
            continue;
        }
        sink.claim(off, end, owner, what);
    }
    if prot_index == 898 {
        claim_effect_proto_records(buf, sink);
        claim_subdraw_records(buf, sink);
        claim_unread_affinity_block(buf, sink);
        claim_battle_overlay_strings(buf, sink);
        claim_battle_jump_tables(buf, sink);
        claim_side_effect_banners(buf, sink);
    }
    if prot_index == STR_OVERLAY_PROT_INDEX {
        claim_str_overlay_tables(buf, sink);
    }
    if prot_index == WORLD_MAP_RENDER_PROT_INDEX {
        claim_world_map_prim_dispatch(buf, sink);
    }
    claim_consumer_pinned_tables(buf, sink, prot_index);
    if prot_index == ARENA_PROT_INDEX {
        claim_arena_course_ladder(buf, sink);
    }
    if prot_index == crate::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32 {
        claim_fishing_species_names(buf, sink);
    }
    if prot_index == crate::field_probe_tables::OVERLAY_PROT_INDEX {
        claim_field_probe_tables(buf, sink);
    }
    if prot_index == crate::other3_roster::OVERLAY_PROT_INDEX {
        claim_other3_roster(buf, sink);
    }
}

/// PROT entry of the Muscle Dome arena's door / init overlay.
const ARENA_PROT_INDEX: u32 = 977;
/// The arena's course descriptor table: three `{ i32 round_count; u32
/// first_round }` records (`legaia_engine_core::muscle_dome::COURSE_TABLE_VA`).
const ARENA_COURSE_TABLE_VA: u32 = 0x801D_1A08;
/// The per-`(course, round)` score table, sixteen `i32` cells per course
/// (`legaia_engine_core::muscle_dome::SCORE_TABLE_VA`).
const ARENA_SCORE_TABLE_VA: u32 = 0x801D_1860;
/// The two `lui` sites that form [`ARENA_SCORE_TABLE_VA`]: the settlement's
/// `DAT_801D1860 + course * 0x40 + (round - 1) * 4` read (`0x801D10E8`,
/// `sll v1,v1,0x6` on the course) and its sibling at `0x801D1234`.
const ARENA_SCORE_TABLE_SITES: [u32; 2] = [0x801D_10E8, 0x801D_1234];
const ARENA_COURSES: usize = 3;
const ARENA_MAX_ROUNDS: usize = 16;

/// The arena's course ladder, read the way `FUN_801D1510` and the settlement
/// read it: the score table (three rows of sixteen cells, the row stride the
/// consumer's `sll 6` states), every course's run of `{ u32 label_va; u32
/// monster_id }` round records reached through the descriptor table's
/// `first_round` pointers, and the label each round names - the opponent-name
/// pool at the head of the image, which is reached only through those
/// records. The parser of record is
/// `legaia_engine_core::muscle_dome::parse_course_ladder`; this mirrors its
/// validation (three courses of `1..=16` rounds, non-zero byte monster ids)
/// and claims nothing when any of it fails.
pub(super) fn claim_arena_course_ladder(buf: &[u8], sink: &mut Sink) {
    const SLOT_A: u32 = 0x801C_E818;
    let at = |va: u32| va.checked_sub(SLOT_A).map(|o| o as usize);
    let rd = |o: usize| legaia_bytes::u32_le(buf, o);
    let score_ok = ARENA_SCORE_TABLE_SITES
        .iter()
        .all(|&site| at(site).and_then(|o| lui_pair_address(buf, o)) == Some(ARENA_SCORE_TABLE_VA));
    let Some(table) = at(ARENA_COURSE_TABLE_VA) else {
        return;
    };
    let mut runs = Vec::new();
    for c in 0..ARENA_COURSES {
        let (Some(count), Some(first)) = (rd(table + c * 8), rd(table + c * 8 + 4)) else {
            return;
        };
        let count = count as usize;
        let Some(base) = at(first) else {
            sink.note("arena course ladder not claimed: a first_round pointer is below the image");
            return;
        };
        if count == 0 || count > ARENA_MAX_ROUNDS || base + count * 8 > buf.len() {
            sink.note("arena course ladder not claimed: a descriptor is out of range");
            return;
        }
        for r in 0..count {
            let id = rd(base + r * 8 + 4).unwrap_or(0);
            if id == 0 || id > 0xFF {
                sink.note("arena course ladder not claimed: a round's monster id is not a byte");
                return;
            }
        }
        runs.push((base, count));
    }
    if score_ok {
        let start = at(ARENA_SCORE_TABLE_VA).unwrap_or(0);
        sink.claim(
            start,
            start + ARENA_COURSES * ARENA_MAX_ROUNDS * 4,
            OWNER_RECORD,
            "arena score table, 3 courses x 16 i32 cells (FUN_801D1510 settlement)",
        );
    }
    let mut labels = 0usize;
    for (c, &(base, count)) in runs.iter().enumerate() {
        sink.claim(
            base,
            base + count * 8,
            OWNER_RECORD,
            format!("arena course {c}: {count} round record(s) (label_va, monster_id)"),
        );
        for r in 0..count {
            let Some(label) = rd(base + r * 8).and_then(at) else {
                continue;
            };
            let Some(len) = buf
                .get(label..)
                .and_then(|t| t.iter().position(|&b| b == 0))
            else {
                continue;
            };
            sink.claim(label, label + len + 1, OWNER_STRING, "arena round label");
            labels += 1;
        }
    }
    sink.note(format!(
        "arena course ladder: {} round record(s), {labels} label(s)",
        runs.iter().map(|&(_, n)| n).sum::<usize>()
    ));
}

/// The species names the fishing species table's `+0x00` pointers name - the
/// banner string `FUN_801D4004` prints for a hooked fish. The table itself is
/// a [`pinned_overlay_tables`] row; its pool is the head of the image, which
/// nothing else reaches.
pub(super) fn claim_fishing_species_names(buf: &[u8], sink: &mut Sink) {
    use crate::fishing_species as fish;
    let Some(species) = fish::parse(buf) else {
        return;
    };
    for sp in &species {
        let Some(name) = sp.name(buf) else {
            continue;
        };
        let off = (sp.name_ptr_va - fish::FISHING_OVERLAY_BASE_VA) as usize;
        sink.claim(
            off,
            off + name.len() + 1,
            OWNER_STRING,
            "fishing species name (fishing_species)",
        );
    }
}

/// Where a consumer-pinned table's count comes from.
#[derive(Clone, Copy)]
pub(super) enum PinnedCount {
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
pub(super) struct ConsumerPinnedTable {
    prot: u32,
    base_va: u32,
    stride: usize,
    count: usize,
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
pub(super) const CONSUMER_PINNED_TABLES: [ConsumerPinnedTable; 13] = [
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
        count: crate::fishing_species::SPECIES_COUNT,
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
    // ([`crate::spell_anim_pairs::OPENING_SHOT_VA`]). The index is a spell id
    // with no upper bound check; the next formed address is the status-guard
    // mask table at `0x801F672C` (`0x801F07DC`), which closes eighty-four
    // bytes - ids `0x25..0x79`. A higher id reads the guard masks.
    ConsumerPinnedTable {
        prot: 898,
        base_va: crate::spell_anim_pairs::OPENING_SHOT_VA,
        stride: 1,
        count: 0x54,
        forms: &[(0x801E_A558, crate::spell_anim_pairs::OPENING_SHOT_VA)],
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
pub(super) fn lui_pair_address(buf: &[u8], at: usize) -> Option<u32> {
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
pub(super) fn claim_consumer_pinned_tables(buf: &[u8], sink: &mut Sink, prot_index: u32) {
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

/// The field overlay's three probe-offset tables at the head of its data
/// segment, each bound to the `lui` pairs that form its base
/// ([`crate::field_probe_tables`]). Claimed only when
/// [`crate::field_probe_tables::check`] re-derives every base from this
/// image's own instructions.
pub(super) fn claim_field_probe_tables(buf: &[u8], sink: &mut Sink) {
    use crate::field_probe_tables as fpt;
    let errs = fpt::check(buf);
    if !errs.is_empty() {
        sink.note(format!(
            "field probe tables not claimed: {} row(s) disagree with this image ({})",
            errs.len(),
            errs[0]
        ));
        return;
    }
    for (va, rows, sites) in fpt::TABLES {
        let off = (va - fpt::OVERLAY_BASE_VA) as usize;
        sink.claim(
            off,
            off + rows * fpt::ROW_BYTES,
            OWNER_RECORD,
            format!(
                "field probe table {va:#010x}, {rows} x (dx, dz) rows, formed at {} site(s)",
                sites.len()
            ),
        );
    }
}

/// The `OTHER3` dev module's 81-record selection roster (PROT `0974`).
///
/// Three quarters of that entry is one fixed-stride table of NUL-padded
/// labels, and a shape test can only call the whole thing `ascii_text` - the
/// stride is in the drawing loop's index arithmetic, not in the bytes
/// ([`crate::other3_roster`]). Claiming each record at the stride covers its
/// padding too, because the stride is what the loop advances by.
pub(super) fn claim_other3_roster(buf: &[u8], sink: &mut Sink) {
    use crate::other3_roster as roster;
    if roster::records(buf).is_none() {
        sink.note("no OTHER3 roster at the pinned offset in PROT 0974");
        return;
    }
    for i in 0..roster::RECORD_COUNT {
        let Some((off, len)) = roster::record_extent(i) else {
            break;
        };
        sink.claim(
            off,
            (off + len).min(buf.len()),
            OWNER_STRING,
            format!("OTHER3 roster label {i} (other3_roster)"),
        );
    }
    sink.note(format!(
        "{} roster labels on a {:#x} stride at {:#010x} (other3_roster)",
        roster::RECORD_COUNT,
        roster::RECORD_STRIDE,
        roster::ROSTER_VA
    ));
}

/// PROT index of the STR/MDEC cutscene overlay.
pub(super) const STR_OVERLAY_PROT_INDEX: u32 = 970;

/// The STR/MDEC overlay's two data-segment structures: the per-`fmv_id`
/// dispatch table with the movie paths it points at, and the compressed blob
/// the VLC lookup table is unpacked from.
///
/// Both are consumed by modules in this workspace at pinned constants
/// ([`crate::fmv_dispatch`], `legaia_mdec::strv2_table`) and neither was
/// claimed: the dispatch table and its path strings read as `plausible_mips` /
/// `ascii_text` residue, and the blob - the second-largest overlay residue run
/// on the disc - read as `mixed`, which is what a compressed stream looks like
/// to a shape test.
///
/// The blob's extent is **measured**, not assumed: `unpack_lz_tracked` walks
/// the control bytes to the `0xFF 0xFF` terminator and reports what it
/// consumed, the same way [`take_lzs`] measures an LZS span. What is left of
/// the entry's last sector past that terminator is the builder's sector
/// buffer, and takes the same one-sector-wide slack rule the streaming classes
/// take.
pub(super) fn claim_str_overlay_tables(buf: &[u8], sink: &mut Sink) {
    use crate::fmv_dispatch as fmv;
    use legaia_mdec::strv2_table as vlc;

    let base = fmv::STR_OVERLAY_BASE_VA;
    let table_off = (fmv::FMV_TABLE_VA - base) as usize;
    let table_len = fmv::FMV_SLOT_COUNT * fmv::SLOT_STRIDE;
    match fmv::FmvTable::from_str_overlay(buf) {
        Some(_) => {
            sink.claim(
                table_off,
                (table_off + table_len).min(buf.len()),
                OWNER_TOC,
                format!(
                    "FMV dispatch table, {} x {} bytes (fmv_dispatch)",
                    fmv::FMV_SLOT_COUNT,
                    fmv::SLOT_STRIDE
                ),
            );
            // Each slot's `+0x00` is a pointer to its ISO9660 movie path; the
            // strings sit in the image's own head pool, so the extents come
            // from the pointers plus a NUL scan rather than from a table.
            for i in 0..fmv::FMV_SLOT_COUNT {
                let at = table_off + i * fmv::SLOT_STRIDE;
                let Some(w) = buf.get(at..at + 4) else { break };
                let ptr = u32::from_le_bytes(w.try_into().unwrap());
                let Some(off) = ptr.checked_sub(base).map(|o| o as usize) else {
                    continue;
                };
                let Some(tail) = buf.get(off..) else { continue };
                let Some(len) = tail.iter().position(|&b| b == 0) else {
                    continue;
                };
                sink.claim(
                    off,
                    off + len + 1,
                    OWNER_STRING,
                    format!("movie path for fmv_id {i} (fmv_dispatch)"),
                );
            }
        }
        None => sink.note("no FMV dispatch table at the pinned offset in PROT 0970"),
    }

    // The two MDEC command packets the table upload sends, each checked by
    // its own header word before it is claimed.
    for (va, header, what) in [
        (
            fmv::MDEC_QUANT_PACKET_VA,
            fmv::MDEC_QUANT_PACKET_HEADER,
            "MDEC quant-table packet: header + luma + chroma matrices (fmv_dispatch)",
        ),
        (
            fmv::MDEC_IDCT_PACKET_VA,
            fmv::MDEC_IDCT_PACKET_HEADER,
            "MDEC IDCT-table packet: header + 64-halfword matrix (fmv_dispatch)",
        ),
    ] {
        let off = (va - base) as usize;
        if legaia_bytes::u32_le(buf, off) == Some(header)
            && off + fmv::MDEC_PACKET_BYTES <= buf.len()
        {
            sink.claim(off, off + fmv::MDEC_PACKET_BYTES, OWNER_RECORD, what);
        } else {
            sink.note(format!(
                "no MDEC packet header {header:#010x} at {va:#010x}"
            ));
        }
    }

    claim_str_dead_vlc_table(buf, sink, base);

    let src = (vlc::STRV2_PACKED_VA - base) as usize;
    match buf.get(src..).map(vlc::unpack_lz_tracked) {
        Some(Ok((table, consumed))) => {
            let end = (src + consumed).min(buf.len());
            sink.claim(
                src,
                end,
                OWNER_LZS,
                format!(
                    "STRv2 VLC table source, mode-switched LZ77 -> {} bytes at {:#010x} \
                     (legaia_mdec::strv2_table, FUN_801f1a00)",
                    table.len(),
                    vlc::STRV2_TABLE_VA
                ),
            );
            // Past the terminator, inside the entry's last sector: the
            // builder's buffer, the same slack the streaming classes declare.
            claim_last_sector_slack(buf, sink, end, "slack past the VLC blob terminator");
        }
        _ => sink.note("the VLC blob at the pinned offset does not terminate"),
    }
}

/// First word of the STR overlay's second, unreferenced AC VLC lookup table.
pub(super) const STR_DEAD_VLC_VA: u32 = 0x801D_0E9C;
/// One past its last word: the start of the overlay's uninitialised data
/// region (the `cutscene_str` row of `static-overlays.toml`).
pub(super) const STR_DEAD_VLC_END_VA: u32 = 0x801D_199C;

/// The STR overlay's 2816-byte AC VLC lookup table at [`STR_DEAD_VLC_VA`],
/// directly above the MDEC / DMA register-pointer block, which **nothing
/// reads**: no word, `jal`, `j`, branch or `lui` pair in any image names an
/// address in it (`find-address-word-refs.py --prot`), no `lui` + load or
/// `gp`-relative access reaches it (`find-gp-relative-refs.py --prot`), and
/// the register block's own accesses stop at its last pointer, `0x801D0E98`.
/// The overlay's decoder reads the separate `0x11000`-byte table
/// `FUN_801F1A00` unpacks at `0x801E0A00` instead
/// (`legaia_mdec::strv2_table`). So this is initialised data the link carried
/// and no code uses - claimed as dead data under that name, not as a
/// structure anything consumes.
///
/// The claim is shape-checked before it is made: every word is either zero or
/// an entry `(len << 26) | (run << 10) | level` whose bits `16..26` are clear
/// and whose length field is a plausible code length - the MPEG-1 AC run /
/// level form (`0x1000_0401` is a 4-bit code for run 1, level 1).
pub(super) fn claim_str_dead_vlc_table(buf: &[u8], sink: &mut Sink, base: u32) {
    let (start, end) = (
        (STR_DEAD_VLC_VA - base) as usize,
        (STR_DEAD_VLC_END_VA - base) as usize,
    );
    let Some(words) = buf.get(start..end) else {
        sink.note("the STR overlay is too short for its dead VLC table");
        return;
    };
    let (mut zero, mut entries) = (0usize, 0usize);
    for w in words
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
    {
        if w == 0 {
            zero += 1;
        } else if w & 0x03FF_0000 == 0 && (2..=17).contains(&(w >> 26)) {
            entries += 1;
        } else {
            sink.note(format!(
                "no AC VLC table at {STR_DEAD_VLC_VA:#010x}: word {w:#010x} is not a run/level entry"
            ));
            return;
        }
    }
    sink.claim(
        start,
        end,
        OWNER_RECORD,
        format!(
            "unreferenced AC VLC lookup table, {entries} run/level entries + {zero} empty              slots: dead data, no image forms an address in it"
        ),
    );
}

/// The battle overlay's head: twenty-two `switch` jump tables and the C
/// strings in front of them, each bound to the instruction pair that forms its
/// address ([`crate::battle_jump_tables`]).
///
/// A table's extent is its consumer's `sltiu` bound times four - read off the
/// dispatch, not scanned out of the bytes - so the claims are structural. They
/// are made only when [`crate::battle_jump_tables::check`] re-derives every row
/// from this image's own instructions; an image that disagrees gets a note and
/// no claim.
pub(super) fn claim_battle_jump_tables(buf: &[u8], sink: &mut Sink) {
    use crate::battle_jump_tables as bjt;
    let errs = bjt::check(buf);
    if !errs.is_empty() {
        sink.note(format!(
            "battle jump tables not claimed: {} row(s) disagree with this image ({})",
            errs.len(),
            errs[0]
        ));
        return;
    }
    for t in &bjt::JUMP_TABLES {
        sink.claim(
            t.offset(),
            t.offset() + t.byte_len(),
            OWNER_TOC,
            format!(
                "jump table, {} arms on {} (jr {:#010x})",
                t.arms, t.index, t.jr
            ),
        );
    }
    for s in &bjt::HEAD_STRINGS {
        let off = s.offset();
        if let Some(len) = buf.get(off..).and_then(|t| t.iter().position(|&b| b == 0)) {
            sink.claim(
                off,
                off + len + 1,
                OWNER_STRING,
                format!("head string, address formed at {:#010x}", s.site),
            );
        }
    }
}

/// The banner strings the Seru-magic side-effect table names.
///
/// Each 8-byte `[element][band]` record's `+4` word is the banner-string
/// pointer the stager `FUN_801F3D3C` copies to `0x800775B4`
/// ([`crate::seru_side_effect`]), so the table - already claimed at its
/// pinned offset - is the consumer that pins each string's start, and the
/// string's own NUL pins its end. A pointer outside the image claims nothing.
pub(super) fn claim_side_effect_banners(buf: &[u8], sink: &mut Sink) {
    use crate::seru_side_effect as seru;
    let base = seru::OVERLAY_LINK_BASE;
    let mut n = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    for r in 0..seru::SIDE_EFFECT_ELEMENTS * seru::SIDE_EFFECT_BANDS {
        let at = seru::SIDE_EFFECT_TABLE_FILE_OFFSET + r * seru::SIDE_EFFECT_RECORD_STRIDE + 4;
        let Some(w) = buf
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        else {
            continue;
        };
        let Some(off) = w.checked_sub(base).map(|o| o as usize) else {
            continue;
        };
        if off >= buf.len() || !seen.insert(off) {
            continue;
        }
        let Some(len) = buf[off..].iter().position(|&b| b == 0) else {
            continue;
        };
        sink.claim(
            off,
            off + len + 1,
            OWNER_STRING,
            format!("Seru side-effect banner, record {r} +4"),
        );
        n += 1;
    }
    if n > 0 {
        sink.note(format!(
            "{n} Seru side-effect banner string(s) named by the side-effect table's +4 words"
        ));
    }
}

/// The battle overlay's NUL-terminated UI strings, whose extents no fixed
/// `count * stride` row can express.
///
/// Every address here is a `pub const` (or a pointer read from one), so this is
/// the same kind of binding as [`pinned_overlay_tables`] - what differs is only
/// that a C string's length is in its own bytes rather than in a table, so the
/// extent is NUL-scanned from the pinned start instead of computed. Without
/// that these read as `ascii_text` residue while the parser that consumes them
/// names the exact byte they start at.
pub(super) fn claim_battle_overlay_strings(buf: &[u8], sink: &mut Sink) {
    use crate::{battle_ui_strings as bui, muscle_dome as dome};
    let base = bui::OVERLAY_BASE_VA;
    let claim_cstr = |off: usize, what: String, sink: &mut Sink| {
        let Some(tail) = buf.get(off..) else { return };
        // A string with no terminator in the image is not a string - claim
        // nothing rather than run to the end of the entry.
        let Some(len) = tail.iter().position(|&b| b == 0) else {
            sink.note(format!("{what}: no NUL terminator at {off:#x}"));
            return;
        };
        sink.claim(off, off + len + 1, OWNER_STRING, what);
    };
    for (va, label) in bui::OVERLAY_LABELS {
        let Some(off) = va.checked_sub(base).map(|o| o as usize) else {
            continue;
        };
        claim_cstr(
            off,
            format!("battle UI label {label:?} (battle_ui_strings)"),
            sink,
        );
    }
    for (i, off) in dome::victory_message_offsets(buf).into_iter().enumerate() {
        claim_cstr(
            off,
            format!("muscle-dome victory message {i} (muscle_dome)"),
            sink,
        );
    }
}

/// The battle overlay's **effect-prototype record pool** - the bytes the
/// `0x801F6324` pointer table points INTO, as distinct from the table itself.
///
/// The table has a pinned constant and a row in [`pinned_overlay_tables`], so
/// the 61 pointers were credited while the 54 unique records they name were
/// not: the pool read as one unbroken `low_entropy` residue run, which is how a
/// fully decoded structure looks when only its index is claimed.
/// [`crate::move_power::parse_effect_proto_records`] already decodes it to
/// `[i16 model_sel][u16 reserved][move-VM bytecode]` part records
/// ([`move-power.md`](../../../docs/formats/move-power.md)); this walks the same
/// offsets and claims each record's extent.
///
/// A record ends where the next one begins - the pool is packed, with the last
/// record bounded by the table itself rather than by the end of the entry, which
/// is the one place `parse_records_at`'s generic bound is too generous for a
/// byte claim.
pub(super) fn claim_effect_proto_records(buf: &[u8], sink: &mut Sink) {
    use crate::move_power as mp;
    let Some(aux) = mp::EffectAuxTables::parse(buf) else {
        sink.note("no effect-prototype table: PROT 0898 structural guard failed");
        return;
    };
    let table = mp::EFFECT_PROTO_TABLE_FILE_OFFSET;
    let mut offs: Vec<usize> = (0..mp::EFFECT_AUX_TABLE_LEN as u8)
        .filter_map(|i| aux.proto_record_offset(i))
        .filter(|&f| f + 4 <= table)
        .collect();
    offs.sort_unstable();
    offs.dedup();
    for (i, &f) in offs.iter().enumerate() {
        let end = offs.get(i + 1).copied().unwrap_or(table);
        let model_sel = i16::from_le_bytes([buf[f], buf[f + 1]]);
        sink.claim(
            f,
            end,
            OWNER_RECORD,
            format!("move-FX part record, model_sel {model_sel} (move_power)"),
        );
    }
    sink.note(format!(
        "{} unique effect-prototype record(s) behind the {}-entry \
         0x801F6324 table (move_power::parse_effect_proto_records)",
        offs.len(),
        mp::EFFECT_AUX_TABLE_LEN
    ));
}

/// The battle overlay's **sub-draw record pool** - the step records the
/// `0x801F4D34` pointer table names, as distinct from the table itself (the
/// same shape as [`claim_effect_proto_records`]). Each record's extent is the
/// consumer's own `3 + 2*count` read
/// ([`crate::muscle_dome::subdraw_record_extents`]); the word padding between
/// records stays residue.
pub(super) fn claim_subdraw_records(buf: &[u8], sink: &mut Sink) {
    let recs = crate::muscle_dome::subdraw_record_extents(buf);
    for &(off, len) in &recs {
        sink.claim(
            off,
            off + len,
            OWNER_RECORD,
            format!(
                "battle HUD sub-draw step, {} element pair(s) (muscle_dome)",
                (len - 3) / 2
            ),
        );
    }
    sink.note(format!(
        "{} unique sub-draw record(s) behind the {}-entry 0x801F4D34 table \
         (muscle_dome::subdraw_record_extents)",
        recs.len(),
        crate::muscle_dome::SUBDRAW_PTR_TABLE_LEN
    ));
}

/// PROT entry of the world-map render overlay (slot B, base `0x801F69D8`).
pub(super) const WORLD_MAP_RENDER_PROT_INDEX: u32 = 901;

/// Slot-B link base the world-map render overlay loads at.
const SLOT_B_BASE: u32 = 0x801F_69D8;

/// The per-prim dispatch row SCUS's `FUN_80043390` switches to while the
/// world-map overlay is resident: it forms `0x801F8968` itself (`lui
/// s4,0x8020` / `addiu s4,s4,-0x7698` at `0x800435F4..F8`), indexes it by the
/// prim group's `flags >> 1`, and adds no alpha offset on this branch, so one
/// twenty-slot row is the whole table (`docs/subsystems/world-map.md`).
const WORLD_MAP_PRIM_DISPATCH_VA: u32 = 0x801F_8968;

/// Slots in one dispatch row.
const PRIM_DISPATCH_SLOTS: usize = 20;

/// The four lit low-mode dispatchers slots `8..12` share with the SCUS
/// table at `0x8007657C`.
const PRIM_DISPATCH_LOW_MODE: [u32; 4] = [0x8004_409C, 0x8004_423C, 0x8004_4434, 0x8004_45B0];

/// Claim the world-map render overlay's per-prim dispatch row
/// ([`WORLD_MAP_PRIM_DISPATCH_VA`]), after a shape check: slots `0..8` are
/// zero (the index space below the lit rows, not padding), slots `8..12` are
/// the SCUS low-mode quartet, and slots `12..20` - the eight untextured and
/// textured high-mode emit leaves - each point into this image's own code.
/// The consumer is the executable's, so the row cannot be read off a `lui`
/// pair in this image the way [`CONSUMER_PINNED_TABLES`] rows are.
pub(super) fn claim_world_map_prim_dispatch(buf: &[u8], sink: &mut Sink) {
    let start = (WORLD_MAP_PRIM_DISPATCH_VA - SLOT_B_BASE) as usize;
    let end = start + 4 * PRIM_DISPATCH_SLOTS;
    let words: Option<Vec<u32>> = (start..end)
        .step_by(4)
        .map(|o| legaia_bytes::u32_le(buf, o))
        .collect();
    let Some(words) = words else {
        return;
    };
    let own_top = SLOT_B_BASE + start as u32;
    let ok = words[..8].iter().all(|&w| w == 0)
        && words[8..12] == PRIM_DISPATCH_LOW_MODE
        && words[12..]
            .iter()
            .all(|&w| (SLOT_B_BASE..own_top).contains(&w) && w % 4 == 0);
    if !ok {
        sink.note("no world-map prim dispatch row at 0x801F8968: shape check failed");
        return;
    }
    sink.claim(
        start,
        end,
        OWNER_TOC,
        "per-prim dispatch row read by FUN_80043390 (slots 8..11 SCUS, 12..19 this image)",
    );
}

/// The battle overlay's unread second affinity block
/// ([`crate::element_affinity::UNREAD_AFFINITY_BLOCK_VA`]) - claimed as dead
/// data under that name, the way [`claim_str_dead_vlc_table`] claims the STR
/// overlay's unreferenced table, and only after a shape check: it must end
/// exactly where the summon power-percent table begins and every byte must be
/// a plausible percentage (`1..=200`).
pub(super) fn claim_unread_affinity_block(buf: &[u8], sink: &mut Sink) {
    use crate::element_affinity as elem;
    let start = elem::UNREAD_AFFINITY_BLOCK_FILE_OFFSET;
    let len = elem::ELEMENT_COUNT * elem::ELEMENT_COUNT;
    if start + len != elem::SUMMON_POWER_PCT_FILE_OFFSET
        || start != elem::AFFINITY_MATRIX_FILE_OFFSET + len
    {
        sink.note("the unread affinity block's constants no longer tile the gap");
        return;
    }
    let Some(block) = buf.get(start..start + len) else {
        return;
    };
    if !block.iter().all(|b| (1..=200).contains(b)) {
        sink.note("no percentage block between the affinity matrix and the summon table");
        return;
    }
    sink.claim(
        start,
        start + len,
        OWNER_RECORD,
        "unread second 8x8 affinity block, dead data (element_affinity)",
    );
}

/// Data-segment tables an overlay image carries at an offset a parser in this
/// workspace already reads, with the extent that parser's own `count * stride`.
///
/// Every row is a **binding**, not a discovery: each offset and each length is
/// a `pub const` of the module named in the detail, so nothing here is a new
/// claim about the disc and nothing here can be tuned to buy percentage points
/// - widening a row means widening the parser that reads it. The rows are
///   asserted against the parsers' constants by the disc-gated tests, so a
///   parser that re-pins a table moves this table with it or the test fails.
///
/// What this closes is the gap the sweep kept reporting as unwalked format: an
/// overlay's code is credited from the dump corpus and its data segment from
/// nothing, so a table with a named constant and a decoded record layout ranked
/// beside a format nobody had opened.
pub fn pinned_overlay_tables(prot_index: u32) -> Vec<(usize, usize, &'static str, &'static str)> {
    use crate::{
        baka_opponents as baka, battle_attack_camera_table as atkcam, battle_camera_table as camh,
        battle_ui_strings as bui, dance_art, dance_cast, dance_chart, element_affinity as elem,
        menu_windows as menu, minigame_art as art, minigame_slot_scene as slot, move_power as mp,
        muscle_dome as dome, seru_side_effect as seru, slot_payout as payout,
    };
    use crate::{fishing_exchange as fex, fishing_species as fish};
    const SLOT_A: u32 = 0x801C_E818;
    let at = |va: u32| (va - SLOT_A) as usize;
    match prot_index {
        898 => vec![
            (
                at(dome::DECK_TABLE_VA),
                dome::HAND_SLOTS,
                OWNER_RECORD,
                "muscle-dome deck move-index table (muscle_dome)",
            ),
            (
                at(dome::HAND_SPRITE_TABLE_VA),
                dome::HAND_SLOTS,
                OWNER_RECORD,
                "muscle-dome hand sprite-id table (muscle_dome)",
            ),
            (
                at(bui::RASERU_LABEL_TABLE_VA),
                (bui::RASERU_LABEL_MAX as usize + 1) * bui::RASERU_LABEL_STRIDE as usize,
                OWNER_STRING,
                "Ra-Seru magic-command labels (battle_ui_strings)",
            ),
            (
                camh::CAMERA_HEIGHT_FILE_OFFSET,
                camh::CAMERA_HEIGHT_LEN * 2,
                OWNER_RECORD,
                "battle camera-height table (battle_camera_table)",
            ),
            (
                at(dome::SUBDRAW_PTR_TABLE_VA),
                dome::SUBDRAW_PTR_TABLE_LEN * 4,
                OWNER_TOC,
                "muscle-dome sub-draw record pointers (muscle_dome)",
            ),
            (
                at(dome::VICTORY_MSG_TABLE_VA),
                dome::VICTORY_MSG_TABLE_LEN * 4,
                OWNER_TOC,
                "muscle-dome victory-message pointers (muscle_dome)",
            ),
            (
                atkcam::ATTACK_CAMERA_FILE_OFFSET,
                atkcam::ATTACK_CAMERA_LEN,
                OWNER_RECORD,
                "per-art attack-camera tracks (battle_attack_camera_table)",
            ),
            (
                mp::MOVE_ID_INDEX_MAP_FILE_OFFSET,
                mp::MOVE_ID_INDEX_MAP_LEN,
                OWNER_RECORD,
                "move-id to record-index map (move_power)",
            ),
            (
                mp::MOVE_POWER_TABLE_FILE_OFFSET,
                mp::MOVE_POWER_TABLE_LEN * mp::MOVE_POWER_RECORD_STRIDE,
                OWNER_RECORD,
                "move power + behaviour table (move_power)",
            ),
            (
                mp::IMPACT_EFFECT_TABLE_FILE_OFFSET,
                mp::IMPACT_EFFECT_TABLE_LEN * 4,
                OWNER_RECORD,
                "impact-effect config table (move_power)",
            ),
            (
                elem::AFFINITY_MATRIX_FILE_OFFSET,
                elem::ELEMENT_COUNT * elem::ELEMENT_COUNT,
                OWNER_RECORD,
                "element-affinity matrix (element_affinity)",
            ),
            (
                elem::SUMMON_POWER_PCT_FILE_OFFSET,
                elem::SUMMON_POWER_PCT_ROWS * elem::ELEMENT_COUNT,
                OWNER_RECORD,
                "summon power-percent table (element_affinity)",
            ),
            (
                elem::CHARACTER_ELEMENTS_FILE_OFFSET,
                elem::CHARACTER_ELEMENTS_LEN,
                OWNER_RECORD,
                "per-character element table (element_affinity)",
            ),
            (
                mp::EFFECT_PROTO_TABLE_FILE_OFFSET,
                mp::EFFECT_AUX_TABLE_LEN * 4,
                OWNER_TOC,
                "move effect-prototype pointers (move_power)",
            ),
            (
                mp::EFFECT_CLUT_TABLE_FILE_OFFSET,
                mp::EFFECT_AUX_TABLE_LEN,
                OWNER_RECORD,
                "move effect CLUT ids (move_power)",
            ),
            (
                mp::CUE_GROUP_TABLE_FILE_OFFSET,
                mp::CUE_GROUP_TABLE_LEN * mp::CUE_GROUP_STRIDE,
                OWNER_RECORD,
                "move cue-group table (move_power)",
            ),
            (
                seru::SIDE_EFFECT_TABLE_FILE_OFFSET,
                seru::SIDE_EFFECT_ELEMENTS
                    * seru::SIDE_EFFECT_BANDS
                    * seru::SIDE_EFFECT_RECORD_STRIDE,
                OWNER_RECORD,
                "Seru-magic side-effect table (seru_side_effect)",
            ),
        ],
        899 => vec![
            (
                menu::EQUIP_BROWSE_MAP_OFFSET,
                menu::EQUIP_BROWSE_MAP_LEN,
                OWNER_RECORD,
                "equip browse-row to equip-byte map (menu_windows)",
            ),
            (
                menu::EQUIP_BROWSE_MAP_OFFSET + menu::EQUIP_BROWSE_MAP_LEN,
                1,
                OWNER_PAD,
                "pad byte between the browse map and the equip mask",
            ),
            (
                menu::CHARACTER_EQUIP_MASK_OFFSET,
                menu::CHARACTER_EQUIP_MASK_LEN,
                OWNER_RECORD,
                "per-character equip mask (menu_windows)",
            ),
            (
                menu::SLOT_PICTOGRAM_OFFSET,
                menu::SLOT_PICTOGRAM_LEN * 2,
                OWNER_RECORD,
                "equip slot pictogram ids (menu_windows)",
            ),
            (
                menu::MENU_WINDOW_TABLE_OFFSET,
                menu::MENU_WINDOW_COUNT * menu::MENU_WINDOW_RECORD_STRIDE,
                OWNER_RECORD,
                "pause-menu window descriptor table (menu_windows)",
            ),
            (
                menu::OPTIONS_LAYOUT_OFFSET,
                menu::OPTIONS_LAYOUT_ROWS * menu::OPTIONS_LAYOUT_STRIDE,
                OWNER_RECORD,
                "options display-layout table (menu_windows)",
            ),
            (
                menu::PRIZE_TABLE_OFFSET,
                menu::PRIZE_TABLE_BLOCKS * menu::PRIZE_BLOCK_BYTES,
                OWNER_RECORD,
                "casino prize table (menu_windows)",
            ),
        ],
        975 => vec![
            (
                slot::MESSAGE_TABLE_OFFSET,
                slot::MESSAGE_COUNT * slot::MESSAGE_STRIDE,
                OWNER_RECORD,
                "slot-machine message table (minigame_slot_scene)",
            ),
            (
                payout::SLOT_PAYOUT_FILE_OFFSET,
                payout::SLOT_SYMBOL_COUNT,
                OWNER_RECORD,
                "per-symbol payout ladder (slot_payout)",
            ),
            (
                slot::PAYLINE_TABLE_OFFSET,
                slot::PAYLINE_COUNT * 16,
                OWNER_RECORD,
                "payline geometry table (minigame_slot_scene)",
            ),
            (
                slot::MARQUEE_TABLE_OFFSET,
                slot::MARQUEE_COUNT * 16,
                OWNER_RECORD,
                "marquee cell table (minigame_slot_scene)",
            ),
            (
                slot::MEDALLION_TABLE_OFFSET,
                slot::LAMP_COUNT * slot::LAMP_RECORD_STRIDE,
                OWNER_RECORD,
                "payline medallion positions (minigame_slot_scene)",
            ),
            (
                slot::LAMP_TABLE_OFFSET,
                slot::LAMP_COUNT * slot::LAMP_RECORD_STRIDE,
                OWNER_RECORD,
                "payline lamp positions (minigame_slot_scene)",
            ),
            (
                art::SLOT_HUD_TABLE_OFFSET,
                art::SLOT_HUD_RECORDS * art::SLOT_HUD_STRIDE,
                OWNER_RECORD,
                "slot-machine HUD sprite records (minigame_art)",
            ),
        ],
        976 => vec![
            (
                baka::HUD_WIDGET_TABLE_FILE_OFFSET,
                baka::HUD_WIDGET_COUNT * baka::HUD_WIDGET_STRIDE,
                OWNER_RECORD,
                "Baka Fighter HUD widget table (baka_opponents)",
            ),
            (
                baka::ACTOR_PROTOTYPE_TABLE_FILE_OFFSET,
                baka::ACTOR_PROTOTYPE_COUNT * baka::ACTOR_PROTOTYPE_STRIDE,
                OWNER_RECORD,
                "Baka Fighter actor prototypes (baka_opponents)",
            ),
            (
                baka::OPPONENT_TABLE_FILE_OFFSET,
                baka::OPPONENT_COUNT * baka::OPPONENT_RECORD_STRIDE,
                OWNER_RECORD,
                "Baka Fighter opponent roster (baka_opponents)",
            ),
            (
                (baka::ACTION_PTR_TABLE_VA - SLOT_A) as usize,
                baka::OPPONENT_COUNT * 4,
                OWNER_TOC,
                "Baka Fighter per-fighter action-table pointers (baka_opponents)",
            ),
            (
                baka::BLIT_RECT_TABLE_FILE_OFFSET,
                baka::BLIT_RECT_COUNT * baka::BLIT_RECT_STRIDE,
                OWNER_RECORD,
                "Baka Fighter blit source rects (baka_opponents)",
            ),
        ],
        980 => vec![
            (
                (dance_art::WIDGET_TABLE_VA - SLOT_A) as usize,
                dance_art::WIDGET_COUNT * dance_art::WIDGET_STRIDE,
                OWNER_RECORD,
                "dance widget table (dance_art)",
            ),
            (
                (dance_cast::KIND_TABLE_VA - SLOT_A) as usize,
                dance_cast::KIND_COUNT * dance_cast::KIND_STRIDE,
                OWNER_RECORD,
                "dance cast kind table (dance_cast)",
            ),
            (
                dance_chart::DANCE_CHART_FILE_OFFSET,
                dance_chart::DANCE_CHART_ROWS * dance_chart::BEATS_PER_ROW,
                OWNER_RECORD,
                "dance step chart (dance_chart)",
            ),
            (
                (dance_chart::DANCE_BONUS_VA - SLOT_A) as usize,
                dance_chart::DANCE_SKILL_ROWS * dance_chart::DANCE_BONUS_LANES * 4,
                OWNER_RECORD,
                "dance sequence-bonus table (dance_chart)",
            ),
            (
                (dance_chart::DANCE_TRIANGLE_SCHEDULE_VA - SLOT_A) as usize,
                dance_chart::DANCE_SKILL_ROWS * dance_chart::DANCE_SCHEDULE_SLOTS * 4,
                OWNER_RECORD,
                "dance AI triangle schedule (dance_chart)",
            ),
            (
                (dance_cast::SPAWN_QUALIFIER_VA - SLOT_A) as usize,
                3 * 0x10,
                OWNER_RECORD,
                "dance qualifier spawn table (dance_cast)",
            ),
            (
                (dance_cast::SPAWN_FINALS_VA - SLOT_A) as usize,
                3 * 0x10,
                OWNER_RECORD,
                "dance finals spawn table (dance_cast)",
            ),
            (
                (dance_cast::SPAWN_FREEPLAY_VA - SLOT_A) as usize,
                6 * 0x10,
                OWNER_RECORD,
                "dance free-play spawn table (dance_cast)",
            ),
        ],
        972 => vec![
            (
                fish::SPECIES_TABLE_FILE_OFFSET,
                fish::SPECIES_COUNT * fish::SPECIES_RECORD_STRIDE,
                OWNER_RECORD,
                "fishing species table (fishing_species)",
            ),
            (
                (fish::SPAWN_TABLE_VA_PAGE0 - SLOT_A) as usize,
                fish::SPAWN_RODS * fish::SPAWN_BANDS * 4,
                OWNER_RECORD,
                "fishing venue-0 spawn table (fishing_species)",
            ),
            (
                (fish::SPAWN_TABLE_VA_PAGE1 - SLOT_A) as usize,
                fish::SPAWN_RODS * fish::SPAWN_BANDS * 4,
                OWNER_RECORD,
                "fishing venue-1 spawn table (fishing_species)",
            ),
            (
                (fish::CADENCE_TEMPLATE_VA - SLOT_A) as usize,
                fish::CADENCE_TEMPLATE_COUNT * fish::CADENCE_TEMPLATE_STRIDE,
                OWNER_RECORD,
                "fishing reel-cadence templates (fishing_species)",
            ),
            (
                (crate::fishing_sprites::FISHING_SPRITE_TABLE_VA - SLOT_A) as usize,
                crate::fishing_sprites::FISHING_SPRITE_COUNT
                    * crate::fishing_sprites::FISHING_SPRITE_STRIDE,
                OWNER_RECORD,
                "fishing HUD sprite records (fishing_sprites)",
            ),
            (
                (fex::EXCHANGE_TABLE_VA_PAGE0 - SLOT_A) as usize,
                fex::EXCHANGE_ROWS * fex::EXCHANGE_ROW_STRIDE,
                OWNER_RECORD,
                "fishing point-exchange page 0 (fishing_exchange)",
            ),
            (
                (fex::EXCHANGE_TABLE_VA_PAGE1 - SLOT_A) as usize,
                fex::EXCHANGE_ROWS * fex::EXCHANGE_ROW_STRIDE,
                OWNER_RECORD,
                "fishing point-exchange page 1 (fishing_exchange)",
            ),
        ],
        _ => Vec::new(),
    }
}

/// A slot-B module image: the dump corpus for its code, plus the image's own
/// structural regions - the head jump table and the spawn-record band.
///
/// The record claims need no dump directory: both ends of every one of them are
/// addresses the module's own code computes and hands to `FUN_80021B04` /
/// `FUN_80050ED4`. See [`crate::slot_b_module`] and
/// [`docs/formats/slot-b-module-layout.md`](https://andrewaltimit.github.io/legend-of-legaia-re/formats/slot-b-module-layout.html).
pub(super) fn walk_slot_b_module(buf: &[u8], sink: &mut Sink, opts: &AccountOptions) {
    // The record walk is cut at the inherited tail for the same reason the
    // band-level measurement is: a spawn call site up there belongs to the
    // donor whose bytes those are, and so does the record pointer it forms.
    let layout =
        crate::slot_b_module::parse_with_tail(buf, base_for(opts), inherited_tail_start(buf, opts));
    if let Some(h) = layout.head_table.clone() {
        sink.claim(
            h.start,
            h.end,
            OWNER_TOC,
            format!("head jump table, {} arms", (h.end - h.start) / 4),
        );
    }
    for (i, r) in layout.records.iter().enumerate() {
        sink.claim(
            r.start,
            r.end,
            OWNER_RECORD,
            format!("spawn record {i} (model_sel {})", r.model_sel),
        );
    }
    // Records above the highest consumer-credited one. Their starts come from
    // the move-VM program walk rather than from a pointer, so the reason line
    // says which evidence the claim rests on.
    for (i, r) in layout.chained_records.iter().enumerate() {
        sink.claim(
            r.start,
            r.end,
            OWNER_RECORD,
            format!(
                "spawn record {} (model_sel {}, chained from the record below by \
                 its program's terminator)",
                layout.records.len() + i,
                r.model_sel
            ),
        );
    }
    sink.note(format!(
        "slot-B module: {} framed functions, code ends at {:#x}; {} spawn sites, \
         {} records claimed ({} bytes) + {} chained ({} bytes){}",
        layout.functions.len(),
        layout.code_end(),
        layout.spawn_sites,
        layout.records.len(),
        layout.record_bytes(),
        layout.chained_records.len(),
        layout
            .chained_records
            .iter()
            .map(crate::slot_b_module::RecordSpan::len)
            .sum::<usize>(),
        match layout.unbounded_record {
            Some(o) => format!(
                "; the highest record at {o:#x} has no boundary above it and \
                 its program does not terminate, so it stays residue"
            ),
            None => String::new(),
        }
    ));
    // The dump corpus is the parser for the code half, and it is optional here
    // - `--funcs` absent still gives the structural regions.
    if opts.funcs_dir.is_some() {
        walk_overlay_code(buf, sink, opts);
    }
}
