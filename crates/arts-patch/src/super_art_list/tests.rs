use super::hook_asm::{
    assemble_count, assemble_fill, assemble_id, assemble_pager, assemble_performed,
};
use super::rows::{SuperArtRow, arrow_code};
use super::*;
use crate::super_art_power::GRID_BIAS;
use anyhow::Result;
use legaia_art::queue::{Character, Command};
use legaia_code_hooks::arenas::{
    ARENA1_END_VA, ARENA1_VA, ARENA2_END_VA, ARENA2_VA, SCUS_GAP_END_VA, SCUS_GAP_VA, SLOT6_END_VA,
    SLOT6_VA,
};
use legaia_code_hooks::mips::*;

/// Decode a branch's word offset back out of an encoded instruction.
fn br_off(w: u32) -> i32 {
    ((w & 0xffff) as u16 as i16) as i32
}

/// Every store opcode, so a test can assert what a routine writes.
fn is_store(w: u32) -> bool {
    (0x28..=0x2b).contains(&(w >> 26))
}

/// The register an instruction writes, for the ones these routines use.
fn dest_reg(w: u32) -> Option<u32> {
    let op = w >> 26;
    match op {
        0 => match w & 0x3f {
            0x08 => None, // jr
            _ if w == 0 => None,
            _ => Some((w >> 11) & 0x1f),
        },
        0x02 | 0x04 | 0x05 | 0x06 | 0x07 | 0x28 | 0x29 | 0x2b => None,
        0x03 => Some(RA), // jal
        _ => Some((w >> 16) & 0x1f),
    }
}

const ID_VA: u32 = SCUS_GAP_VA;
const SCRATCH_VA: u32 = SCUS_GAP_VA + 0xD0;
const BUF_VA: u32 = SCRATCH_VA + 0x10;
const FILL_VA: u32 = ARENA1_VA;
const ARROWS_VA: u32 = ARENA1_VA + 0xC8; // past the 49-word fill routine
const COUNT_VA: u32 = ARENA2_VA;
const SUP_VA: u32 = SLOT6_VA;

fn id_routine() -> Vec<u32> {
    assemble_id([HOOK_ID_W0, HOOK_ID_W1], ID_VA, SUP_VA, FILL_VA, ID_RET_VA)
}

fn fill_routine() -> Vec<u32> {
    assemble_fill(FILL_VA, SUP_VA, ARROWS_VA, SCRATCH_VA)
}

// --- (A) the count hook --------------------------------------------------

#[test]
fn count_routine_reads_the_performed_byte_before_the_stock_load_clobbers_v0() {
    let r = assemble_count([HOOK_COUNT_W0, HOOK_COUNT_W1], COUNT_RET_VA);
    assert_eq!(r[0], lbu(T1, V0, PERFORMED_OFF), "v0 is still the record");
    assert_eq!(r[1], HOOK_COUNT_W0, "then the displaced count load");
    assert_eq!(r[3], lh(T3, T3, lo(GAME_MODE_VA)), "reads the game mode");
    assert_eq!(r[4], srl(T1, T1, 5), "count field = top three bits");
    assert_eq!(r[5], addiu(T3, T3, 0xFFEB), "mode - 0x15");
    let skip = 6 + 1 + br_off(r[6]);
    assert_eq!(skip, 9, "outside battle the add is skipped");
    assert_eq!(r[8], addu(V0, V0, T1), "count += performed");
    assert_eq!(r[9], j(COUNT_RET_VA));
    assert_eq!(r[10], HOOK_COUNT_W1, "replays sra a2,a2,0x1f");
    assert_eq!(r.len(), 11);
    assert!(
        !r.iter().copied().any(is_store),
        "no store in the count hook"
    );
    // hi/lo are live across (A) (`mult` at 0x800343A0, `mfhi` at
    // 0x800343D4): no mult/div, and only temporaries + v0 written.
    for (i, w) in r.iter().enumerate() {
        let special = (w >> 26) == 0 && (w & 0x3f) != 0;
        assert!(
            !(special && (0x18..=0x1b).contains(&(w & 0x3f))),
            "mult at {i}"
        );
        if let Some(d) = dest_reg(*w) {
            assert!(
                (T0..=T7).contains(&d) || d == V0 || d == A2,
                "instruction {i} writes r{d}"
            );
        }
    }
}

// --- (B) the id hook -----------------------------------------------------

#[test]
fn id_routine_gates_and_falls_back_to_the_stock_load() {
    let r = id_routine();
    assert_eq!(r[0], subu(V0, V0, A2), "v0 becomes the record base");
    assert_eq!(r[3], lbu(T0, V0, PERFORMED_OFF), "reads the performed byte");
    assert_eq!(r[6], andi(T0, T0, 0x1F), "keeps the mask bits");
    let plain0 = r.len() - 5;
    for i in [5usize, 7] {
        let target = i as i32 + 1 + br_off(r[i]);
        assert_eq!(target as usize, plain0, "guard at {i} lands on PLAIN0");
    }
    assert_eq!(r[plain0], or(T4, A2, ZERO), "PLAIN0: index = entry");
    assert_eq!(r[plain0 + 1], addu(V0, V0, T4));
    assert_eq!(r[plain0 + 2], HOOK_ID_W0, "then replays the stock load");
    assert_eq!(r[plain0 + 3], j(ID_RET_VA));
    assert_eq!(r[plain0 + 4], HOOK_ID_W1);
}

#[test]
fn id_routine_preserves_what_the_stock_words_after_it_read() {
    // 0x80034458 `beq v1,zero` tests the displaced sltiu's v1 (loaded at
    // 0x8003444c), and 0x8003445c `addiu a1,a0,0x5ec4` reads a0. Neither
    // may be written on the learned path.
    for (i, w) in id_routine().iter().enumerate() {
        if *w == HOOK_ID_W1 {
            continue; // the replayed sltiu itself writes v1
        }
        if let Some(d) = dest_reg(*w) {
            assert_ne!(d, A0, "instruction {i} writes a0");
            assert_ne!(d, V1, "instruction {i} writes v1");
            assert!(
                (T0..=T9).contains(&d) || d == V0 || d == S2,
                "instruction {i} writes r{d}"
            );
        }
    }
}

#[test]
fn id_routine_branches_land_where_the_comments_say() {
    let r = id_routine();
    assert_eq!(r[41], j(ID_VA + 20 * 4), "NEXTK closes on LOOP");
    assert_eq!(r[45], j(ID_VA + 20 * 4), "PICKLRN's advance closes on LOOP");
    assert_eq!(21 + 1 + br_off(r[21]), 43, "no Super left -> PICKLRN");
    assert_eq!(29 + 1 + br_off(r[29]), 41, "not performed -> NEXTK");
    assert_eq!(31 + 1 + br_off(r[31]), 39, "learned exhausted -> PICKSUP");
    assert_eq!(37 + 1 + br_off(r[37]), 43, "learned first -> PICKLRN");
    assert_eq!(43 + 1 + br_off(r[43]), 48, "this learned row -> PLAINI");
    // The Super Art hit is a cross-region branch straight into (F); its delay
    // slot decrements r, which (F) never reads.
    let target = ID_VA as i64 + 39 * 4 + 4 + i64::from(br_off(r[39])) * 4;
    assert_eq!(target, FILL_VA as i64, "this Super row -> FILL");
    assert_eq!(r[40], addiu(T7, T7, 0xFFFF));
    assert_eq!(r[44], addiu(T7, T7, 0xFFFF), "PLAINI never reads t7 either");
    assert_eq!(r.len(), 52);
}

#[test]
fn id_routine_load_delays_are_covered() {
    let r = id_routine();
    // lh t3 (2) -> used at 4; lbu t0 (3) -> 6; lw t8 (8) -> 11; lbu t2 (9)
    // -> 30; lbu t1 (24) -> 26; lbu t1 (32) -> 35; lbu t3 (34) -> 36.
    for (load, first_use) in [
        (2usize, 4usize),
        (3, 6),
        (8, 11),
        (9, 30),
        (24, 26),
        (32, 35),
        (34, 36),
    ] {
        let dest = (r[load] >> 16) & 0x1f;
        let filler = r[load + 1];
        let (rs, rt) = ((filler >> 21) & 0x1f, (filler >> 16) & 0x1f);
        let is_branch = matches!(filler >> 26, 0x04 | 0x05);
        assert!(
            is_branch || (rs != dest && rt != dest),
            "delay slot after the load at {load} reads it"
        );
        assert!(first_use >= load + 2, "load at {load} used too early");
    }
}

#[test]
fn id_routine_never_moves_the_stack_pointer_or_stores() {
    for w in id_routine() {
        let is_addiu_sp = (w >> 26) == 0x09 && ((w >> 21) & 0x1f) == SP;
        assert!(!is_addiu_sp, "the id hook borrows retail's frame");
        assert!(!is_store(w), "the merge writes no memory");
    }
}

/// Model of the merge (B) encodes: the row at `entry` given the id-sorted
/// learned list, the performed mask (bits = trigger rows) and the
/// AP-sorted Super records `(thr, trigger_row)`.
fn merged_row(learned: &[u8], mask: u8, sup: &[(u8, u8)], entry: usize) -> Result<Row, ()> {
    let (mut i, mut k, mut r) = (0usize, 0usize, entry);
    loop {
        let sup_next = loop {
            if k >= sup.len() {
                break None;
            }
            if mask >> sup[k].1 & 1 == 1 {
                break Some(k);
            }
            k += 1;
        };
        let take_super = match sup_next {
            None => false,
            Some(k) => i >= learned.len() || learned[i] >= sup[k].0,
        };
        if take_super {
            if r == 0 {
                return Ok(Row::Super(k));
            }
            r -= 1;
            k += 1;
        } else {
            if i >= learned.len() {
                return Err(());
            }
            if r == 0 {
                return Ok(Row::Learned(learned[i]));
            }
            r -= 1;
            i += 1;
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Row {
    Learned(u8),
    Super(usize),
}

#[test]
fn merge_model_interleaves_by_threshold() {
    // Vahn: learned Miracle(0), Burning Flare(1), Cyclone(4), Somersault(12);
    // Supers sorted by AP: k0 thr=1 (66 AP: before every hyper),
    // k1 thr=1 (60), k2 thr=1 (54), k3 thr=1 (54), k4 thr=1 (54).
    let learned = [0u8, 1, 4, 12];
    let sup = [(1u8, 4u8), (1, 0), (1, 1), (1, 2), (1, 3)];
    // Only Tri-Somersault (trigger row 0 = sorted k1) performed.
    let mask = 1 << 0;
    let rows: Vec<Row> = (0..5)
        .map(|e| merged_row(&learned, mask, &sup, e).unwrap())
        .collect();
    assert_eq!(
        rows,
        vec![
            Row::Learned(0),
            Row::Super(1),
            Row::Learned(1),
            Row::Learned(4),
            Row::Learned(12)
        ]
    );
    // Everything performed: Miracle, then all five Supers, then the rest.
    let rows: Vec<Row> = (0..9)
        .map(|e| merged_row(&learned, 0x1F, &sup, e).unwrap())
        .collect();
    assert_eq!(rows[0], Row::Learned(0));
    assert_eq!(
        rows[1..6],
        [
            Row::Super(0),
            Row::Super(1),
            Row::Super(2),
            Row::Super(3),
            Row::Super(4)
        ]
    );
    assert_eq!(
        rows[6..],
        [Row::Learned(1), Row::Learned(4), Row::Learned(12)]
    );
    // Nothing learned, one performed: the Super is row 0.
    // (sorted k0 carries trigger row 4, so mask bit 4 lights k0.)
    assert_eq!(merged_row(&[], 0x10, &sup, 0), Ok(Row::Super(0)));
    // Nothing performed: pure learned list.
    assert_eq!(merged_row(&learned, 0, &sup, 2), Ok(Row::Learned(4)));
}

// --- (F) the fill routine ------------------------------------------------

#[test]
fn fill_routine_chases_the_name_and_writes_only_scratch_and_buffer() {
    let r = fill_routine();
    assert_eq!(r[2], lbu(T1, T3, 0), "chain AP");
    assert_eq!(r[3], lhu(T7, T3, 2), "name offset | markers");
    assert_eq!(r[4], lui(T9, hi(CMD_TABLE_VA)));
    assert_eq!(r[5], addu(T9, T9, T8), "t8 = character * 4 from (B)");
    assert_eq!(r[6], lw(T9, T9, lo(CMD_TABLE_VA)), "the command-data block");
    assert_eq!(r[8], lw(T9, T9, ART_BLOCK_PTR_OFF), "the art-record array");
    assert_eq!(
        r[10],
        srl(T6, T7, MARKERS_SHIFT),
        "marker count off the top bits"
    );
    assert_eq!(r[11], andi(T7, T7, 0x1FFF), "name offset off the low bits");
    assert_eq!(r[12], addu(T9, T9, T7), "+ name offset");
    // The arrows are addressed relative to the SUP record (same order and
    // stride), so a Noa/Gala row never reads Vahn's arrows.
    let delta = ARROWS_VA.wrapping_sub(SUP_VA);
    assert_eq!(r[15], lui(T2, hi(delta)));
    assert_eq!(r[16], addu(T3, T3, T2));
    assert_eq!(
        r[17],
        lw(T3, T3, lo(delta)),
        "the packed word from &SUP[k] + delta"
    );
    assert_eq!(
        r[18],
        lw(T9, T5, SCRATCH_ARROWS_OFF),
        "the buffer is the scratch's own +8"
    );
    assert_eq!(r[19], srl(T4, T3, ARROWS_COUNT_SHIFT), "n off the top bits");
    assert_eq!(r[20], addu(T6, T6, T4), "glyph count = markers + n");
    assert_eq!(r[24], ori(T6, ZERO, 1), "rows start in the default style");
    // Style selection: default, art-end on an end bit, Super-end on the last.
    assert_eq!(r[26], andi(T7, T3, 4), "the end bit of the current arrow");
    assert_eq!(r[28], ori(T1, ZERO, 1));
    assert_eq!(r[30], ori(T1, ZERO, 6));
    assert_eq!(r[31], ori(T1, ZERO, 9), "Miracle orange on the final arrow");
    assert_eq!(27 + 1 + br_off(r[27]), 32, "not an end -> CMP");
    assert_eq!(29 + 1 + br_off(r[29]), 32, "an end, not last -> CMP");
    assert_eq!(32 + 1 + br_off(r[32]), 38, "same style -> GLYPH");
    assert_eq!(r[43], srl(T3, T3, 3), "three bits per arrow");
    assert_eq!(45 + 1 + br_off(r[45]), 25, "the glyph loop closes");
    assert_eq!(r[47], j(SCAN_HIT_VA), "then the hit arm");
    assert_eq!(
        r[48],
        addiu(S5, T5, SCRATCH_ARROWS_OFF),
        "s5 = scratch + 8 in the delay slot"
    );
    assert_eq!(r.len(), 49);
    let stores: Vec<u32> = r.iter().copied().filter(|&w| is_store(w)).collect();
    assert_eq!(stores[0], sb(T1, T5, SCRATCH_AP_OFF));
    assert_eq!(stores[1], sw(T9, T5, SCRATCH_NAME_OFF));
    assert_eq!(stores[2], sb(T6, T9, 0), "glyph count");
    assert_eq!(stores[3..5], [sb(T2, T9, 0), sb(T1, T9, 1)], "a marker");
    assert_eq!(
        stores[5..],
        [sb(T2, T9, 0), sb(T1, T9, 1)],
        "an arrow glyph"
    );
    assert_eq!(stores.len(), 7);
    // Load delays: lbu t1 (2) -> 13; lhu t7 (3) -> 10; lw t9 (6) -> 8;
    // lw t9 (8) -> 12; lw t3 (17) -> 19; lw t9 (18) -> 21.
    for (load, first_use) in [
        (2usize, 13usize),
        (3, 10),
        (6, 8),
        (8, 12),
        (17, 19),
        (18, 21),
    ] {
        let dest = (r[load] >> 16) & 0x1f;
        let filler = r[load + 1];
        let (rs, rt) = ((filler >> 21) & 0x1f, (filler >> 16) & 0x1f);
        assert!(
            rs != dest && rt != dest,
            "delay slot after the load at {load} reads it"
        );
        assert!(first_use >= load + 2);
    }
}

#[test]
fn packed_arrows_round_trip_through_the_expander_model() {
    // The expander: per arrow k, dir = word >> 3k & 3, end = word >> 3k & 4,
    // n = word >> 27; a marker whenever the style changes.
    let row = SuperArtRow {
        character: Character::Vahn,
        name: "Tri-Somersault",
        finisher: 0x2B,
        trigger_row: 0,
        sorted_index: 1,
        chain_ids: vec![12, 4, 12],
        chain_names: vec![],
        ap: 60,
        thr: 1,
        name_offset: 0x1614,
        input: vec![
            Command::Up,
            Command::Down,
            Command::Up,
            Command::Up,
            Command::Up,
            Command::Down,
            Command::Up,
        ],
        ends: vec![2, 4, 6],
    };
    let w = u32::from_le_bytes(row.packed_arrows());
    assert_eq!(w >> ARROWS_COUNT_SHIFT, 7);
    let mut dirs = Vec::new();
    let mut ends = Vec::new();
    for k in 0..7 {
        let f = (w >> (3 * k)) & 7;
        dirs.push(GLYPH_LO_BASE + (f & 3) as u8);
        if f & 4 != 0 {
            ends.push(k);
        }
    }
    // Retail's glyph codes: 0x81A8 Right / A9 Left / AA Up / AB Down.
    let want: Vec<u8> = "UDUUUDU"
        .chars()
        .map(|c| match c {
            'U' => 0xAA,
            'D' => 0xAB,
            'L' => 0xA9,
            _ => 0xA8,
        })
        .collect();
    assert_eq!(dirs, want);
    assert_eq!(ends, vec![2, 4, 6]);
    // blue blue yellow blue yellow blue orange: five style changes.
    assert_eq!(row.styles(), vec![1, 1, 6, 1, 6, 1, 9]);
    assert_eq!(row.marker_count(), 5);
    assert_eq!(
        row.glyph_string(),
        vec![
            12, 0x81, 0xAA, 0x81, 0xAB, 0xFF, 6, 0x81, 0xAA, 0xFF, 1, 0x81, 0xAA, 0xFF, 6, 0x81,
            0xAA, 0xFF, 1, 0x81, 0xAB, 0xFF, 9, 0x81, 0xAA
        ]
    );
    assert_eq!(
        row.sup_record(),
        [60, 1, 0x14, 0x16 | (5 << 5)],
        "markers ride the name offset's top bits"
    );
    assert_eq!(row.input_letters(), "UDUUUDU");
}

#[test]
fn arrow_codes_are_the_glyph_low_byte_offsets() {
    for (c, lo_) in [
        (Command::Right, 0xA8u8),
        (Command::Left, 0xA9),
        (Command::Up, 0xAA),
        (Command::Down, 0xAB),
    ] {
        assert_eq!(GLYPH_LO_BASE + arrow_code(c), lo_);
        let [hi_, lo2] = legaia_art::arts_table::command_to_glyph(c);
        assert_eq!(
            (hi_, lo2),
            (GLYPH_HI, lo_),
            "matches the arts-table decoder"
        );
    }
}

// --- (W) the performed hook -----------------------------------------------

#[test]
fn performed_routine_sets_one_bit_and_counts_it_once() {
    let r = assemble_performed(PERFORMED_RET_VA);
    assert_eq!(r[0], HOOK_PERFORMED_W0, "replays lui v1,0x801f");
    assert_eq!(r[1], HOOK_PERFORMED_W1, "replays addiu v0,zero,1");
    assert_eq!(
        r[2],
        srl(T3, T2, 4),
        "row = t2 >> 4, NOT a1 (the copy loop clobbers a1)"
    );
    assert_eq!(r[4], sllv(T3, T4, T3), "bit = 1 << row");
    assert_eq!(r[13], lbu(T2, T0, PERFORMED_OFF));
    assert_eq!(r[15], and(T4, T2, T3), "already performed?");
    assert_eq!(16 + 1 + br_off(r[16]), 20, "then straight to the return");
    assert_eq!(r[18], addiu(T2, T2, 0x20), "count field += 1");
    assert_eq!(r[19], sb(T2, T0, PERFORMED_OFF), "the only store");
    assert_eq!(r[20], j(PERFORMED_RET_VA), "back to the flag store");
    assert_eq!(r.len(), 22);
    let stores: Vec<u32> = r.iter().copied().filter(|&w| is_store(w)).collect();
    assert_eq!(stores.len(), 1);
    // Clobbers only t0-t4 besides the two replayed retail writes, and reads
    // t2 before it overwrites it.
    for (i, w) in r.iter().enumerate().skip(2) {
        if let Some(d) = dest_reg(*w) {
            assert!((T0..=T4).contains(&d), "instruction {i} writes r{d}");
        }
    }
    // Model: bit set once, count bumps once.
    let apply = |byte: u8, row: u8| -> u8 {
        let bit = 1u8 << row;
        if byte & bit != 0 {
            byte
        } else {
            (byte | bit) + 0x20
        }
    };
    let b = apply(0, 0);
    assert_eq!(b, 0x21);
    assert_eq!(apply(b, 0), 0x21, "performing it again changes nothing");
    assert_eq!(apply(b, 4), 0x51);
    assert_eq!(apply(0x51, 1) >> 5, 3);
}

// --- (D) the pager -------------------------------------------------------

#[test]
fn pager_fits_the_original_body_with_the_performed_routine_and_its_branches_land() {
    let r = assemble_pager(PAGER_VA);
    let w = assemble_performed(PERFORMED_RET_VA);
    assert!(
        r.len() + w.len() <= PAGER_WORDS,
        "{} + {} words",
        r.len(),
        w.len()
    );
    assert_eq!(r[0], addiu(SP, SP, 0xFFE8), "same frame as retail");
    assert_eq!(r[r.len() - 2], jr(RA));
    assert_eq!(r[r.len() - 1], addiu(SP, SP, 0x18));
    assert_eq!(
        r[17],
        lbu(V1, V0, PERFORMED_OFF),
        "reads the performed byte"
    );
    assert_eq!(
        r[18],
        lbu(A0, V0, LEARNED_COUNT_OFF),
        "and the learned count"
    );
    assert_eq!(r[19], srl(V1, V1, 5));
    assert_eq!(r[20], addu(A0, A0, V1), "rows = learned + performed");
    let exit = r.len() - 4;
    let toggle = 38usize;
    assert_eq!(r[toggle], lui(V0, 0x801F), "toggle arm starts at 38");
    for (i, want) in [
        (7usize, exit),
        (21, exit),
        (44, exit),
        (26, toggle),
        (31, toggle),
        (34, toggle),
    ] {
        let target = i as i32 + 1 + br_off(r[i]);
        assert_eq!(target as usize, want, "branch at {i}");
    }
    assert_eq!(r[36], j(PAGER_VA + exit as u32 * 4), "j lands on EXIT");
    assert_eq!(r[50], jal(PAGER_SOUND_FN_VA), "keeps the open cue");
}

#[test]
fn pager_steps_pages_while_another_page_exists() {
    let step = |offset: u16, rows: u32| -> Option<u16> {
        if offset < MAX_SCROLL && u32::from(offset) + PAGE_ROWS < rows {
            Some(offset + PAGE_ROWS as u16)
        } else {
            None
        }
    };
    assert_eq!(step(0, 21), Some(5));
    assert_eq!(step(15, 21), Some(20));
    assert_eq!(step(20, 21), None, "closes after the last page");
    assert_eq!(step(0, 2), None);
    assert_eq!(step(0, 6), Some(5));
    assert_eq!(step(5, 6), None);
    assert_eq!(step(0, 0), None);
}

// --- Table derivation ----------------------------------------------------

#[test]
fn chain_conversion_is_the_documented_constant_offset() {
    for s in legaia_art::SUPER_ARTS {
        for c in s.art_sequence() {
            assert!(
                c >= ART_CONSTANT_BIAS,
                "{}: chain constant {c:#x} is below the base",
                s.name
            );
        }
        assert!(
            s.art_sequence().len() >= 2,
            "{} would trigger on a single art",
            s.name
        );
        assert!(
            s.finisher >= GRID_BIAS,
            "{}: finisher below the grid",
            s.name
        );
    }
    assert_eq!(legaia_art::SUPER_ARTS.len(), 15);
}

#[test]
fn performed_byte_layout_holds_five_bits_and_a_count() {
    assert_eq!(PERFORMED_MASK, (1 << SUPER_ARTS_PER_CHAR) - 1);
    assert_eq!(PERFORMED_COUNT_SHIFT, SUPER_ARTS_PER_CHAR as u32);
    const { assert!(SUPER_ARTS_PER_CHAR < 1 << (8 - PERFORMED_COUNT_SHIFT)) };
    assert_eq!(
        PERFORMED_OFF,
        LEARNED_LIST_OFF + 15,
        "the sixteenth id slot"
    );
    // The packing carries the longest input plus its count in one word.
    const { assert!(MAX_INPUT_ARROWS as u32 * ARROW_BITS + 4 <= 32) };
    const { assert!(MAX_INPUT_ARROWS < 1 << (32 - ARROWS_COUNT_SHIFT)) };
    const { assert!(MAX_MARKERS < 1 << (16 - MARKERS_SHIFT)) };
    assert_eq!(GLYPH_BUF_BYTES, 29, "nine arrows + five markers");
}

/// The glyph string (F) builds for an input with the given art ends: the
/// same model `SuperArtRow::glyph_string` encodes.
fn expected_glyph_string(input: &[Command], ends: &[usize]) -> Vec<u8> {
    let row = SuperArtRow {
        character: Character::Vahn,
        name: "",
        finisher: 0x2B,
        trigger_row: 0,
        sorted_index: 0,
        chain_ids: vec![],
        chain_names: vec![],
        ap: 0,
        thr: 0,
        name_offset: 0,
        input: input.to_vec(),
        ends: ends.to_vec(),
    };
    row.glyph_string()
}

// --- Execution ------------------------------------------------------------

use legaia_code_hooks::mips_sim::Cpu;

/// One simulated character: which arts are learned, which Supers performed
/// (bits = trigger rows), and the AP-sorted SUP records `(ap, thr, row)`.
struct Scene {
    character: u32,
    learned: Vec<u8>,
    performed: u8,
    sup: Vec<(u8, u8, u8)>,
    arrows: Vec<Vec<Command>>,
    ends: Vec<Vec<usize>>,
}

fn cmd_va() -> u32 {
    0x8010_0000
}
fn art_array_va() -> u32 {
    0x8010_1000
}

/// Lay the scene out in a fresh CPU with every routine loaded at its test VA.
fn cpu_for(scene: &Scene, in_battle: bool) -> Cpu {
    let mut cpu = Cpu::new();
    cpu.load_words(ID_VA, &id_routine());
    cpu.load_words(FILL_VA, &fill_routine());
    cpu.load_words(
        COUNT_VA,
        &assemble_count([HOOK_COUNT_W0, HOOK_COUNT_W1], COUNT_RET_VA),
    );
    // The character record.
    let rec = CHAR_RECORD_BASE + scene.character * 0x414;
    cpu.wr8(
        rec + u32::from(LEARNED_COUNT_OFF),
        scene.learned.len() as u8,
    );
    for (i, id) in scene.learned.iter().enumerate() {
        cpu.wr8(rec + u32::from(LEARNED_LIST_OFF) + i as u32, *id);
    }
    let count = scene.performed.count_ones() as u8;
    cpu.wr8(
        rec + u32::from(PERFORMED_OFF),
        scene.performed | (count << PERFORMED_COUNT_SHIFT),
    );
    // Globals: game mode, character, command table -> art array.
    cpu.load(
        GAME_MODE_VA,
        &(if in_battle { BATTLE_MODE } else { 0x0C }).to_le_bytes(),
    );
    cpu.r[GP as usize] = 0x8007_0000;
    cpu.wr32(0x8007_0000 + u32::from(GP_CHARACTER), scene.character);
    cpu.wr32(CMD_TABLE_VA + scene.character * 4, cmd_va());
    cpu.wr32(cmd_va() + u32::from(ART_BLOCK_PTR_OFF), art_array_va());
    // The SUP records for every character (only this one's matter) and the
    // packed arrows, in AP-sorted order.
    for (k, &(ap, thr, row)) in scene.sup.iter().enumerate() {
        let name_offset = ART_BLOCK_BIAS
            + u32::from(0x2B + row - GRID_BIAS) * ART_RECORD_STRIDE
            + ART_NAME_FIELD_OFF;
        let at = SUP_VA + (scene.character * 5 + k as u32) * SUP_STRIDE;
        let arrows = &scene.arrows[k];
        let markers = expected_glyph_string(arrows, &scene.ends[k])[0] as u16 - arrows.len() as u16;
        let [lo_, hi_] = ((name_offset as u16) | (markers << MARKERS_SHIFT)).to_le_bytes();
        cpu.load(at, &[ap, thr | (row << PERFORMED_COUNT_SHIFT), lo_, hi_]);
        let mut w: u32 = (arrows.len() as u32) << ARROWS_COUNT_SHIFT;
        for (n, &c) in arrows.iter().enumerate() {
            let end = u32::from(scene.ends[k].contains(&n));
            w |= (u32::from(arrow_code(c)) | (end << 2)) << (n as u32 * ARROW_BITS);
        }
        cpu.load(
            ARROWS_VA + (scene.character * 5 + k as u32) * ARROWS_STRIDE,
            &w.to_le_bytes(),
        );
    }
    // Scratch record: +8 -> the glyph buffer.
    cpu.wr32(SCRATCH_VA + u32::from(SCRATCH_ARROWS_OFF), BUF_VA);
    cpu
}

/// Run hook (B) for `entry`; returns what the row resolved to.
fn run_row(scene: &Scene, entry: u32, in_battle: bool) -> Row {
    let mut cpu = cpu_for(scene, in_battle);
    let rec = CHAR_RECORD_BASE + scene.character * 0x414;
    cpu.r[V0 as usize] = rec + entry;
    cpu.r[A2 as usize] = entry;
    cpu.r[A0 as usize] = 0x8007_0000;
    cpu.r[V1 as usize] = 0; // rec[+0] of the table's first record (Vahn)
    cpu.pc = ID_VA;
    match cpu.run_until(&[ID_RET_VA, SCAN_HIT_VA]) {
        pc if pc == ID_RET_VA => {
            assert_eq!(
                cpu.r[V1 as usize], 1,
                "the replayed sltiu v1,v1,0x63 saw v1 = 0"
            );
            assert_eq!(cpu.r[A0 as usize], 0x8007_0000, "a0 preserved");
            Row::Learned(cpu.r[S2 as usize] as u8)
        }
        _ => {
            assert_eq!(cpu.r[S5 as usize], SCRATCH_VA + 8, "s5 = scratch + 8");
            // Which Super did the fill routine draw? Recover it from what it
            // wrote and check every field against the scene.
            let ap = cpu.rd8(SCRATCH_VA + u32::from(SCRATCH_AP_OFF));
            let name = cpu.rd32(SCRATCH_VA + u32::from(SCRATCH_NAME_OFF));
            let k = scene
                .sup
                .iter()
                .position(|&(a, _, row)| {
                    a == ap
                        && name
                            == art_array_va()
                                + ART_BLOCK_BIAS
                                + u32::from(0x2B + row - GRID_BIAS) * ART_RECORD_STRIDE
                                + ART_NAME_FIELD_OFF
                })
                .expect("scratch AP + name pair names one SUP record");
            let want = expected_glyph_string(&scene.arrows[k], &scene.ends[k]);
            let got: Vec<u8> = (0..want.len() as u32)
                .map(|i| cpu.rd8(BUF_VA + i))
                .collect();
            assert_eq!(
                got, want,
                "glyph string for Super {k} of character {}",
                scene.character
            );
            Row::Super(k)
        }
    }
}

fn vahn_scene(learned: &[u8], performed: u8) -> Scene {
    scene_for(0, learned, performed)
}

/// The same five Supers under any character index (arrows differ per
/// character so cross-character addressing is exercised, not just Vahn's).
fn scene_for(character: u32, learned: &[u8], performed: u8) -> Scene {
    // AP-sorted: Rolling Combo (66, row 4), Tri-Somersault (60, row 0),
    // Maximum Blow / Fire Tackle / Power Slash (54, rows 1..3). thr = 1 for
    // all (Vahn's first sub-66 art is Burning Flare, id 1, 50 AP).
    use Command::*;
    let mut sc = Scene {
        character,
        learned: learned.to_vec(),
        performed,
        sup: vec![(66, 1, 4), (60, 1, 0), (54, 1, 1), (54, 1, 2), (54, 1, 3)],
        arrows: vec![
            vec![Up, Down, Right, Left, Left, Down, Up, Up, Left],
            vec![Up, Down, Up, Up, Up, Down, Up],
            vec![Down, Right, Up, Down, Left, Left, Down],
            vec![Left, Right, Left, Left, Down, Right, Up],
            vec![Down, Right, Up, Down, Up, Down, Left],
        ],
        // Where each chain's arts end (`legaia_art::art_ends`): Rolling
        // Combo 3/5/8, the three-art seven-arrow chains 2/4/6.
        ends: vec![
            vec![3, 5, 8],
            vec![2, 4, 6],
            vec![2, 4, 6],
            vec![2, 4, 6],
            vec![2, 4, 6],
        ],
    };
    // Rotate the arrows per character so a wrong table base is visible.
    for _ in 0..character {
        sc.arrows.rotate_left(1);
        sc.ends.rotate_left(1);
    }
    sc
}

#[test]
fn executed_id_routine_agrees_with_the_merge_model_everywhere() {
    // Every subset of a five-art learned list x every performed mask x every
    // row: the assembled routine and the Rust model must resolve identically.
    let pool = [0u8, 1, 4, 12, 14];
    for lm in 0u8..32 {
        let learned: Vec<u8> = pool
            .iter()
            .enumerate()
            .filter(|(i, _)| lm >> i & 1 == 1)
            .map(|(_, &id)| id)
            .collect();
        for performed in 0u8..32 {
            let scene = vahn_scene(&learned, performed);
            let sup: Vec<(u8, u8)> = scene.sup.iter().map(|&(_, thr, row)| (thr, row)).collect();
            let total = learned.len() + performed.count_ones() as usize;
            for entry in 0..total {
                let want = merged_row(&learned, performed, &sup, entry).expect("model");
                let got = run_row(&scene, entry as u32, true);
                assert_eq!(
                    got, want,
                    "learned {learned:?} performed {performed:#07b} entry {entry}"
                );
            }
        }
    }
}

#[test]
fn executed_id_routine_is_plain_outside_battle_and_with_nothing_performed() {
    let learned = [0u8, 4, 12];
    for entry in 0..3u32 {
        let scene = vahn_scene(&learned, 0x1F);
        assert_eq!(
            run_row(&scene, entry, false),
            Row::Learned(learned[entry as usize]),
            "field / menu"
        );
        let scene = vahn_scene(&learned, 0);
        assert_eq!(
            run_row(&scene, entry, true),
            Row::Learned(learned[entry as usize]),
            "none performed"
        );
    }
}

#[test]
fn executed_count_routine_adds_the_performed_count_only_in_battle() {
    for (performed, in_battle, want) in [
        (0x1Fu8, true, 3 + 5u32),
        (0x05, true, 3 + 2),
        (0x1F, false, 3),
    ] {
        let scene = vahn_scene(&[0, 4, 12], performed);
        let mut cpu = cpu_for(&scene, in_battle);
        let rec = CHAR_RECORD_BASE;
        cpu.r[V0 as usize] = rec;
        cpu.r[A2 as usize] = 0xFFFF_FFF0; // sra a2,a2,0x1f replays on it
        cpu.pc = COUNT_VA;
        // The stock word inside the routine reads the count out of the record.
        cpu.run_until(&[COUNT_RET_VA]);
        assert_eq!(
            cpu.r[V0 as usize], want,
            "performed {performed:#x} battle {in_battle}"
        );
        assert_eq!(cpu.r[A2 as usize], 0xFFFF_FFFF, "sra a2,a2,0x1f replayed");
    }
}

#[test]
fn executed_performed_routine_records_each_super_once() {
    let mut cpu = Cpu::new();
    let w = assemble_performed(PERFORMED_RET_VA);
    const W_VA: u32 = 0x801D_3828;
    cpu.load_words(W_VA, &w);
    let rec = CHAR_RECORD_BASE + 2 * 0x414; // Gala
    let byte = |cpu: &Cpu| cpu.rd8(rec + u32::from(PERFORMED_OFF));
    for (row, want) in [
        (3u32, 0x28u8),
        (3, 0x28),
        (0, 0x49),
        (4, 0x79),
        (1, 0x9B),
        (2, 0xBF),
    ] {
        cpu.r[T5 as usize] = 2;
        cpu.r[T2 as usize] = row * 16; // sll t2,a1,0x4 at 0x801EFB04
        cpu.r[A1 as usize] = 0x2B; // what the copy loop leaves in a1: the finisher
        cpu.pc = W_VA;
        cpu.run_until(&[PERFORMED_RET_VA]);
        assert_eq!(byte(&cpu), want, "after performing row {row}");
        assert_eq!(
            cpu.r[V1 as usize], 0x801F_0000,
            "v1 as retail's lui left it"
        );
        assert_eq!(cpu.r[V0 as usize], 1, "v0 as retail's li left it");
    }
    // Another character's byte is untouched.
    assert_eq!(cpu.rd8(CHAR_RECORD_BASE + u32::from(PERFORMED_OFF)), 0);
}

// --- Placement -----------------------------------------------------------

#[test]
fn every_routine_and_table_fits_its_region() {
    let count = assemble_count([HOOK_COUNT_W0, HOOK_COUNT_W1], 0).len() * 4;
    let id = assemble_id([HOOK_ID_W0, HOOK_ID_W1], 0, 0, 0, 0).len() * 4;
    let fill = assemble_fill(0, 0, 0, 0).len() * 4;
    let n = ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR;
    let gap = id + SCRATCH_BYTES + GLYPH_BUF_BYTES;
    assert!(
        gap <= (SCUS_GAP_END_VA - SCUS_GAP_VA) as usize,
        "{gap} B in the gap"
    );
    let a1 = fill + n * ARROWS_STRIDE as usize;
    assert!(
        a1 <= (ARENA1_END_VA - ARENA1_VA) as usize,
        "{a1} B in arena 1"
    );
    assert!(
        count <= (ARENA2_END_VA - ARENA2_VA) as usize,
        "{count} B in arena 2"
    );
    assert!(n * SUP_STRIDE as usize <= (SLOT6_END_VA - SLOT6_VA) as usize);
    eprintln!(
        "count {count} + id {id} + fill {fill} = {} B of code, {} B of tables",
        count + id + fill,
        n * (SUP_STRIDE + ARROWS_STRIDE) as usize + SCRATCH_BYTES + GLYPH_BUF_BYTES
    );
}

#[test]
fn scratch_cursor_lines_up_with_the_fields_retail_reads() {
    // Retail's hit arm reads rec[+0xC] as `lw a0,0x4(s5)`, rec[+2] as
    // `lbu s0,-0x6(s5)` and rec[+8] as `lw v1,0x0(s5)` - all off s5 = rec+8.
    let s5 = SCRATCH_ARROWS_OFF as i32;
    assert_eq!(s5 + 4, SCRATCH_NAME_OFF as i32, "name at s5+4");
    assert_eq!(s5 - 6, SCRATCH_AP_OFF as i32, "AP at s5-6");
    assert!(SCRATCH_NAME_OFF as usize + 4 <= SCRATCH_BYTES);
}
