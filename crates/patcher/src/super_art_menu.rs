//! **Show Super Arts on the status screen's Moves page** - the menu-side half of
//! `--show-super-arts`. The pause menu's Status page (Left from the condition
//! page) lists a character's learned arts with name, AP, and for the selected
//! row its command arrows and a two-line description. This module lists the
//! character's *performed* Super Arts there too, in the same AP order as the
//! battle list, drawn by retail's own row code.
//!
//! ## What retail does
//!
//! The page is submenu 3 of the per-character panel renderer `FUN_801D33D8`
//! (menu overlay PROT 0899, base `0x801CE818`; rows `0x801D444C..0x801D4734`).
//! Per row `s6` (from the scroll offset) it reads the character record `t1`
//! (`0x18(sp)`, the save-record framing: count at `+0x185`, ids at `+0x186..`),
//! scans the arts-name table `DAT_80075EC4` for `(character, id)`, and draws
//! the hit `s2`: name `rec+0xC`, AP `rec+2` (halved under the record's `+0x6C0`
//! `0x800` flag), and for the selected row (`_DAT_8007BB98 & 0xFFF == s6`) the
//! `rec+8` glyph string as arrows via `FUN_8003C310` (same `0xFF style` markers
//! as the battle list) plus the `rec+0x10` description via `FUN_8003CD00`.
//!
//! ```text
//! 801d4440  lbu   t2,0x185(t2)   ; count -> [sp+0x2c], the "more rows" marker   (M1b)
//! 801d4454  lbu   v0,0x185(t1)   ; count -> the row bound `slt v0,s6,v0`         (M1)
//! 801d4480  addu  a0,t1,s6       ; a0 = record + row: the scan reads list[row]  (M2)
//! 801d4484  lbu   v0,0x0(s2)     ;   scan head (branch target from its own tail)
//! 801d44c8  lbu   v0,0x0(s2)     ;   found: rec[+0] must equal the character   <- Super rows enter here
//! 801da64c  lbu   s0,0x74d(v0)   ; the cursor bound the Up/Down mover gets       (M3)
//! 801da650  nop                  ;   (branch target from 0x801DA624)
//! ```
//!
//! ## The shape of the injection
//!
//! Four detours into the overlay itself, every routine hosted in 0899's own
//! reference-free dead space (`0x801E7FB0..0x801E83E0`, past what `--seru-trade`
//! uses of the same run) - so this half costs no SCUS bytes and composes with
//! every gap-based feature:
//!
//! - **M1 / M1b** add the performed count (record `+0x195`, top three bits) to
//!   the two learned-count reads.
//! - **M2** is the merge: the same id-sorted-list ⨯ AP-sorted-Supers walk hook
//!   (B) does in battle, over the same SCUS `SUP` records. A learned row lands
//!   back on the scan with `a0 = record + i` (the merged learned index); a Super
//!   row fills a **menu scratch record** - `+0` character, `+2` AP, `+8` the
//!   glyph buffer, `+0xC` a carried name, `+0x10` a carried description - sets
//!   `s2` to it and enters the found arm at `0x801D44C8`, whose own
//!   `rec[+0] == character` check the scratch record passes.
//! - **M3** adds the performed count to the cursor bound; the scroll follows the
//!   cursor with a fixed seven-row window and needs no count.
//!
//! The arrows are expanded from the same packed word the battle list carries
//! (`super_art_list::ARROWS_STRIDE`, three bits per arrow), with the same
//! colours: default, art-end style on every arrow a sub-art ends on, the
//! Miracle-Art orange on the Super's final arrow. Names are carried because the
//! battle art records the in-battle chase reads are not resident in the menu.
//! Descriptions read like retail's ("Arts. A reverse somersault|kick."):
//! `Super Arts. <first art>,|<rest of the chain>.` - and live in a second dead
//! run (`0x801E65F4..0x801E6B43`).
//!
//! Both runs are all-zero in the file, referenced by nothing in the overlay or
//! `SCUS_942.54` (literal word or `lui/addiu` pair), and read/write-watched
//! untouched across a pause-menu tour (Items, Magic, Equip, Status incl. the
//! Moves page with its cursor, Options, Save; the only writes are the overlay
//! loader's own copy at menu open).
//!
//! No Sony bytes are embedded: names and chain names come from
//! [`legaia_art::SUPER_ARTS`] and the disc's own arts-name table.

use anyhow::{Result, bail};

use crate::mips::*;
use crate::shiny_seru::Edit;
use crate::super_art_list::{
    ARROW_BITS, ARROWS_COUNT_SHIFT, GLYPH_BUF_BYTES, GLYPH_HI, GLYPH_LO_BASE, MARKER_HI,
    MARKERS_SHIFT, PERFORMED_COUNT_SHIFT, PERFORMED_MASK, STYLE_ART_END, STYLE_DEFAULT,
    STYLE_SUPER_END, SUPER_ARTS_PER_CHAR, SuperArtRow,
};

/// PROT entry of the menu overlay.
pub const MENU_PROT_INDEX: usize = 899;
/// Load base of the menu overlay: a VA maps to raw-entry file offset `va - base`.
pub const MENU_BASE_VA: u32 = 0x801C_E818;

/// The character record in the panel renderer's framing (`0x80084708`-based
/// save record): count / ids / performed byte offsets.
pub const REC_COUNT_OFF: u16 = 0x185;
pub const REC_LIST_OFF: u16 = 0x186;
pub const REC_PERFORMED_OFF: u16 = 0x195;

// --- Hook sites (all byte-verified against the US build) ---------------------

/// (M1b) `lbu t2,0x185(t2)` + `nop` - the count the "more rows" marker compares
/// the scroll against.
pub const HOOK_MARK_VA: u32 = 0x801D_4440;
pub(crate) const HOOK_MARK_W0: u32 = 0x914A_0185;
const MARK_RET_VA: u32 = HOOK_MARK_VA + 8;
/// (M1) `lbu v0,0x185(t1)` + `nop` - the row bound.
pub const HOOK_BOUND_VA: u32 = 0x801D_4454;
pub(crate) const HOOK_BOUND_W0: u32 = 0x9122_0185;
const BOUND_RET_VA: u32 = HOOK_BOUND_VA + 8;
/// (M2) `addu a0,t1,s6` - one word; the next word is the scan head, a branch
/// target from the scan's own tail.
pub const HOOK_ROW_VA: u32 = 0x801D_4480;
pub(crate) const HOOK_ROW_W0: u32 = 0x0136_2021;
/// The scan head a learned row returns to.
pub const SCAN_HEAD_VA: u32 = 0x801D_4484;
pub(crate) const SCAN_HEAD_W: u32 = 0x9242_0000;
/// The found arm a Super row enters with `s2` = the scratch record.
pub const FOUND_VA: u32 = 0x801D_44C8;
pub(crate) const FOUND_W: u32 = 0x9242_0000;
/// (M3) `lbu s0,0x74d(v0)` - one word; the next word is a branch target.
pub const HOOK_CURSOR_VA: u32 = 0x801D_A64C;
pub(crate) const HOOK_CURSOR_W0: u32 = 0x9050_074D;
pub const CURSOR_RET_VA: u32 = HOOK_CURSOR_VA + 8;
pub(crate) const CURSOR_RET_W: u32 = 0x1600_0013;
/// The panel renderer's entry, fingerprinted so a re-based overlay is refused.
pub const PANEL_VA: u32 = 0x801D_33D8;
pub(crate) const PANEL_W0: u32 = 0x27BD_FF88;
/// The 0x80084140-based performed-byte offset the cursor hook sees.
const CUR_PERFORMED_OFF: u16 = 0x75D;

// --- Dead space ------------------------------------------------------------

/// Code, names, pointer tables, scratch record and glyph buffer: the tail of
/// the run `--seru-trade` shares, past its highest blob.
pub const MENU_RUN_VA: u32 = 0x801E_7FB0;
pub const MENU_RUN_END_VA: u32 = 0x801E_83E0;
/// The descriptions.
pub const MENU_DESC_VA: u32 = 0x801E_65F4;
pub const MENU_DESC_END_VA: u32 = 0x801E_6B43;

/// Scratch record: `+0` character, `+2` AP, `+8` glyph pointer, `+0xC` name,
/// `+0x10` description (retail's record stride is `0x14`).
pub const MENU_SCRATCH_BYTES: usize = 0x14;

// --- Routine assembly --------------------------------------------------------

/// (M1) `count -> count + performed` for the row bound. `disp` = the stock lbu.
pub(crate) fn assemble_bound(disp: u32, ret: u32) -> Vec<u32> {
    vec![
        lbu(T0, T1, REC_PERFORMED_OFF),     // 0  performed byte
        disp,                               // 1  v0 = learned count
        srl(T0, T0, PERFORMED_COUNT_SHIFT), // 2 (t0 delay covered by 1)
        addu(V0, V0, T0),                   // 3  (v0 delay covered by 2)
        j(ret),                             // 4
        nop(),                              // 5
    ]
}

/// (M1b) the same for the marker count in `t2` (which is also the base).
pub(crate) fn assemble_mark(disp: u32, ret: u32) -> Vec<u32> {
    vec![
        lbu(T0, T2, REC_PERFORMED_OFF),     // 0  performed byte
        disp,                               // 1  t2 = learned count
        srl(T0, T0, PERFORMED_COUNT_SHIFT), // 2
        addu(T2, T2, T0),                   // 3
        j(ret),                             // 4
        nop(),                              // 5
    ]
}

/// (M3) `count -> count + performed` for the cursor bound (`0x80084140`-based
/// record in `v0`). `disp` = the stock lbu.
pub(crate) fn assemble_cursor(disp: u32, ret: u32) -> Vec<u32> {
    vec![
        lbu(T0, V0, CUR_PERFORMED_OFF),     // 0
        disp,                               // 1  s0 = learned count
        srl(T0, T0, PERFORMED_COUNT_SHIFT), // 2
        addu(S0, S0, T0),                   // 3
        j(ret),                             // 4
        nop(),                              // 5
    ]
}

/// (M2) the merge and, for a Super row, the scratch fill + arrows.
///
/// Entered from `0x801D4480` with `t1` = the character record, `s6` = the row,
/// `s2` = the arts-table base. Clobbers `t0..t9`, `v0`, `v1`, `a0`; `a1..a3`
/// are re-set by the found arm before use. A learned row returns to the scan
/// head with `a0 = t1 + i`; a Super row returns to the found arm with `s2` =
/// the scratch record.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble_row(
    base_va: u32,
    sup_va: u32,
    arrows_va: u32,
    scratch_va: u32,
    buf_va: u32,
    names_va: u32,
    descs_va: u32,
) -> Vec<u32> {
    // Layout of the routine: merge (0..MERGE_END), PLAIN, FILL, expander.
    const LOOP: i32 = 15;
    const PICKSUP: i32 = 34;
    const NEXTK: i32 = 36;
    const PICKLRN: i32 = 38;
    const PLAIN: i32 = 42;
    const FILL: i32 = 45;
    let delta = arrows_va.wrapping_sub(sup_va);
    let at = |i: i32| base_va + (i as u32) * 4;
    let mut body = vec![
        lw(T8, SP, 0x1c),                        // 0  t8 = character
        lbu(T0, T1, REC_PERFORMED_OFF),          // 1  performed byte
        lbu(T2, T1, REC_COUNT_OFF),              // 2  learned count
        addiu(T9, T1, REC_LIST_OFF),             // 3  &list
        sll(T7, T8, 2),                          // 4
        addu(T7, T7, T8),                        // 5  character * 5
        sll(T7, T7, 2),                          // 6  * SUP_STRIDE
        lui(T5, hi(sup_va)),                     // 7
        addiu(T5, T5, lo(sup_va)),               // 8
        addu(T5, T5, T7),                        // 9  t5 = &SUP[character*5]
        andi(T0, T0, u16::from(PERFORMED_MASK)), // 10 the mask
        or(T4, ZERO, ZERO),                      // 11 i = 0
        or(T6, ZERO, ZERO),                      // 12 k = 0
        or(T7, S6, ZERO),                        // 13 r = row
        nop(),                                   // 14
        // LOOP
        sltiu(T3, T6, SUPER_ARTS_PER_CHAR as u16), // 15 k < 5 ?
        beq(T3, ZERO, (PICKLRN - 17) as i16),      // 16 no Super left -> learned
        sll(T3, T6, 2),                            // 17 delay
        addu(T3, T3, T5),                          // 18 t3 = &SUP[k]
        lbu(V1, T3, 1),                            // 19 thr | bit << 5
        nop(),                                     // 20
        srl(V1, V1, PERFORMED_COUNT_SHIFT),        // 21 the Super's bit
        srlv(V1, T0, V1),                          // 22 mask >> bit
        andi(V1, V1, 1),                           // 23
        beq(V1, ZERO, (NEXTK - 25) as i16),        // 24 not performed -> next k
        sltu(V1, T4, T2),                          // 25 delay: i < count ?
        beq(V1, ZERO, (PICKSUP - 27) as i16),      // 26 learned exhausted -> Super
        lbu(V1, T3, 1),                            // 27 delay: thr | bit << 5
        addu(V0, T9, T4),                          // 28
        lbu(V0, V0, 0),                            // 29 list[i]
        andi(V1, V1, u16::from(PERFORMED_MASK)),   // 30 thr
        sltu(V1, V0, V1),                          // 31 list[i] < thr -> learned first
        bne(V1, ZERO, (PICKLRN - 33) as i16),      // 32
        nop(),                                     // 33
        // PICKSUP
        beq(T7, ZERO, (FILL - 35) as i16), // 34 r == 0 -> this Super
        addiu(T7, T7, 0xFFFF),             // 35 delay: r -= 1
        // NEXTK
        j(at(LOOP)),      // 36
        addiu(T6, T6, 1), // 37 delay: k += 1
        // PICKLRN
        beq(T7, ZERO, (PLAIN - 39) as i16), // 38 r == 0 -> this learned
        addiu(T7, T7, 0xFFFF),              // 39 delay: r -= 1
        j(at(LOOP)),                        // 40
        addiu(T4, T4, 1),                   // 41 delay: i += 1
        // PLAIN: a learned row - the scan reads list[i].
        addu(A0, T1, T4), // 42 a0 = record + i
        j(SCAN_HEAD_VA),  // 43
        nop(),            // 44
        // FILL: the Super at k of this character.
        sll(T3, T6, 2),                // 45
        addu(T3, T3, T5),              // 46 t3 = &SUP[k]
        lbu(V0, T3, 0),                // 47 chain AP
        lhu(T7, T3, 2),                // 48 name offset | markers << 13
        lui(T5, hi(scratch_va)),       // 49
        addiu(T5, T5, lo(scratch_va)), // 50 t5 = scratch
        sb(T8, T5, 0),                 // 51 scratch[+0] = character
        sb(V0, T5, 2),                 // 52 scratch[+2] = AP
        // Table index = character*5 + k, x4 for the pointer tables.
        sll(T4, T8, 2),                          // 53
        addu(T4, T4, T8),                        // 54 character * 5
        addu(T4, T4, T6),                        // 55 + k
        sll(T4, T4, 2),                          // 56 * 4
        lui(V0, hi(names_va)),                   // 57
        addu(V0, V0, T4),                        // 58
        lw(V0, V0, lo(names_va)),                // 59 the name pointer
        lui(V1, hi(descs_va)),                   // 60
        addu(V1, V1, T4),                        // 61
        lw(V1, V1, lo(descs_va)),                // 62 the description pointer
        sw(V0, T5, 0xC),                         // 63 scratch[+0xC] = name
        sw(V1, T5, 0x10),                        // 64 scratch[+0x10] = description
        srl(T6, T7, MARKERS_SHIFT),              // 65 t6 = marker count
        lui(T2, hi(delta)),                      // 66
        addu(T3, T3, T2),                        // 67 &SUP[k] + hi(delta)
        lw(T3, T3, lo(delta)),                   // 68 t3 = the packed arrows word
        lui(T9, hi(buf_va)),                     // 69
        addiu(T9, T9, lo(buf_va)),               // 70 t9 = the glyph buffer
        srl(T4, T3, ARROWS_COUNT_SHIFT),         // 71 t4 = arrow count n
        addu(T6, T6, T4),                        // 72 glyph count = markers + n
        sb(T6, T9, 0),                           // 73 buf[0]
        addiu(T9, T9, 1),                        // 74
        or(T8, ZERO, ZERO),                      // 75 k = 0
        ori(T6, ZERO, u16::from(STYLE_DEFAULT)), // 76 current style
    ];
    // Expander loop, same shape as the battle fill routine.
    let l = body.len() as i32; // LOOP2 index
    let cmp = l + 7;
    let glyph = l + 13;
    body.extend_from_slice(&[
        addiu(T8, T8, 1),                          // l+0  k += 1
        andi(T7, T3, 4),                           // l+1  an art ends here ?
        beq(T7, ZERO, (cmp - (l + 3)) as i16),     // l+2  no -> default style
        ori(T1, ZERO, u16::from(STYLE_DEFAULT)),   // l+3  delay: want = default
        bne(T8, T4, (cmp - (l + 5)) as i16),       // l+4  not the last arrow ->
        ori(T1, ZERO, u16::from(STYLE_ART_END)),   // l+5  delay: want = art end
        ori(T1, ZERO, u16::from(STYLE_SUPER_END)), // l+6  the final arrow
        // CMP
        beq(T1, T6, (glyph - (l + 8)) as i16), // l+7  same style -> no marker
        ori(T2, ZERO, u16::from(MARKER_HI)),   // l+8  delay
        sb(T2, T9, 0),                         // l+9
        sb(T1, T9, 1),                         // l+10
        addiu(T9, T9, 2),                      // l+11
        or(T6, T1, ZERO),                      // l+12 current = want
        // GLYPH
        andi(T1, T3, 3),                         // l+13 dir 0..3
        addiu(T1, T1, u16::from(GLYPH_LO_BASE)), // l+14
        ori(T2, ZERO, u16::from(GLYPH_HI)),      // l+15
        sb(T2, T9, 0),                           // l+16
        sb(T1, T9, 1),                           // l+17
        srl(T3, T3, ARROW_BITS),                 // l+18 next packed arrow
        sltu(T2, T8, T4),                        // l+19 k < n ?
        bne(T2, ZERO, (l - (l + 21)) as i16),    // l+20 -> LOOP2
        addiu(T9, T9, 2),                        // l+21 delay
        // Done: the record cursor is the scratch record; enter the found arm.
        // t1 = the character record must be intact for the found arm's
        // `lw t1,0x1c(sp)`? It reloads t1 itself, so t1 is free here.
        j(FOUND_VA),      // l+22
        or(S2, T5, ZERO), // l+23 delay: s2 = scratch
    ]);
    debug_assert_eq!(base_va % 4, 0);
    body
}

// --- Strings ------------------------------------------------------------------

/// The description a Super Art row shows: retail's phrasing ("Arts. A reverse
/// somersault|kick.") with the chain spelled out - `Super Arts. <first>,|<rest>.`
pub fn description(row: &SuperArtRow) -> String {
    let names = &row.chain_names;
    match names.len() {
        0 => "Super Arts.".to_string(),
        1 => format!("Super Arts.|{}.", names[0]),
        _ => format!("Super Arts. {},|{}.", names[0], names[1..].join(", ")),
    }
}

/// Bytes the text renderers accept: ASCII, `|` = line break, NUL-terminated.
fn text_bytes(s: &str) -> Result<Vec<u8>> {
    if !s.is_ascii() {
        bail!("show-super-arts: menu text {s:?} is not ASCII");
    }
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    Ok(v)
}

// --- Planning ------------------------------------------------------------------

/// The planned menu-side edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtMenuInjection {
    pub edits: Vec<Edit>,
    pub bound_va: u32,
    pub mark_va: u32,
    pub row_va: u32,
    pub cursor_va: u32,
    pub scratch_va: u32,
    pub buf_va: u32,
    pub names_va: u32,
    pub descs_va: u32,
    pub run_used: u32,
    pub desc_used: u32,
}

fn expect_menu(ov: &[u8], va: u32, expect: u32) -> Result<u32> {
    let off = (va - MENU_BASE_VA) as usize;
    let got = read_word(ov, off)?;
    if got != expect {
        bail!(
            "show-super-arts: PROT {MENU_PROT_INDEX} {va:#x} = {got:#010x}, expected \
             {expect:#010x} (unrecognized build - nothing written)"
        );
    }
    Ok(got)
}

fn assert_zero_menu(ov: &[u8], va: u32, len: usize) -> Result<()> {
    let off = (va - MENU_BASE_VA) as usize;
    let region = ov
        .get(off..off + len)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: {va:#x}..+{len} past end of PROT 0899"))?;
    if region.iter().any(|&b| b != 0) {
        bail!("show-super-arts: PROT 0899 {va:#x}..+{len} is not all-zero dead space - refusing");
    }
    Ok(())
}

fn words_to_bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// A bump allocator over one dead run.
struct Run {
    cursor: u32,
    end: u32,
    name: &'static str,
}

impl Run {
    fn take(&mut self, len: u32, align: u32, what: &str) -> Result<u32> {
        let at = self.cursor.next_multiple_of(align);
        if at + len > self.end {
            bail!(
                "show-super-arts: {what} ({len} B) overruns {} at {:#x} (end {:#x})",
                self.name,
                at,
                self.end
            );
        }
        self.cursor = at + len;
        Ok(at)
    }
}

impl SuperArtMenuInjection {
    /// Plan the menu-side edits against the raw PROT 0899 entry, for the rows
    /// the SCUS plan derived and the SCUS addresses its tables landed at.
    pub fn plan(ov0899: &[u8], rows: &[SuperArtRow], sup_va: u32, arrows_va: u32) -> Result<Self> {
        expect_menu(ov0899, PANEL_VA, PANEL_W0)?;
        let bound_disp = expect_menu(ov0899, HOOK_BOUND_VA, HOOK_BOUND_W0)?;
        expect_menu(ov0899, HOOK_BOUND_VA + 4, 0)?;
        let mark_disp = expect_menu(ov0899, HOOK_MARK_VA, HOOK_MARK_W0)?;
        expect_menu(ov0899, HOOK_MARK_VA + 4, 0)?;
        expect_menu(ov0899, HOOK_ROW_VA, HOOK_ROW_W0)?;
        expect_menu(ov0899, SCAN_HEAD_VA, SCAN_HEAD_W)?;
        expect_menu(ov0899, FOUND_VA, FOUND_W)?;
        let cursor_disp = expect_menu(ov0899, HOOK_CURSOR_VA, HOOK_CURSOR_W0)?;
        expect_menu(ov0899, HOOK_CURSOR_VA + 4, 0)?;
        expect_menu(ov0899, CURSOR_RET_VA, CURSOR_RET_W)?;
        if rows.len() != 3 * SUPER_ARTS_PER_CHAR {
            bail!(
                "show-super-arts: menu plan wants 15 rows, got {}",
                rows.len()
            );
        }

        // Strings first, so the pointer tables can be built.
        let mut run = Run {
            cursor: MENU_RUN_VA,
            end: MENU_RUN_END_VA,
            name: "the 0899 code run",
        };
        let mut desc_run = Run {
            cursor: MENU_DESC_VA,
            end: MENU_DESC_END_VA,
            name: "the 0899 description run",
        };
        let mut name_blob = Vec::new();
        let mut desc_blob = Vec::new();
        let mut name_ptrs = Vec::with_capacity(rows.len());
        let mut desc_ptrs = Vec::with_capacity(rows.len());
        // Routines are placed first (aligned), then the tables.
        let bound = assemble_bound(bound_disp, BOUND_RET_VA);
        let mark = assemble_mark(mark_disp, MARK_RET_VA);
        let cursor = assemble_cursor(cursor_disp, CURSOR_RET_VA);
        let row_len = (assemble_row(0, 0, 0, 0, 0, 0, 0).len() * 4) as u32;
        let bound_va = run.take((bound.len() * 4) as u32, 4, "the bound routine")?;
        let mark_va = run.take((mark.len() * 4) as u32, 4, "the marker routine")?;
        let cursor_va = run.take((cursor.len() * 4) as u32, 4, "the cursor routine")?;
        let row_va = run.take(row_len, 4, "the row routine")?;
        let scratch_va = run.take(MENU_SCRATCH_BYTES as u32, 4, "the scratch record")?;
        let buf_va = run.take(GLYPH_BUF_BYTES as u32, 4, "the glyph buffer")?;
        let names_va = run.take((rows.len() * 4) as u32, 4, "the name pointer table")?;
        let descs_va = run.take((rows.len() * 4) as u32, 4, "the description pointer table")?;
        let name_blob_va = run.take(0, 1, "the names")?;
        for r in rows {
            let bytes = text_bytes(r.name)?;
            name_ptrs.push(name_blob_va + name_blob.len() as u32);
            name_blob.extend_from_slice(&bytes);
        }
        run.take(name_blob.len() as u32, 1, "the names")?;
        let desc_blob_va = desc_run.take(0, 1, "the descriptions")?;
        for r in rows {
            let bytes = text_bytes(&description(r))?;
            desc_ptrs.push(desc_blob_va + desc_blob.len() as u32);
            desc_blob.extend_from_slice(&bytes);
        }
        desc_run.take(desc_blob.len() as u32, 1, "the descriptions")?;

        assert_zero_menu(ov0899, MENU_RUN_VA, (run.cursor - MENU_RUN_VA) as usize)?;
        assert_zero_menu(
            ov0899,
            MENU_DESC_VA,
            (desc_run.cursor - MENU_DESC_VA) as usize,
        )?;

        let row = assemble_row(
            row_va, sup_va, arrows_va, scratch_va, buf_va, names_va, descs_va,
        );
        debug_assert_eq!((row.len() * 4) as u32, row_len);
        let mut scratch = vec![0u8; MENU_SCRATCH_BYTES];
        scratch[8..12].copy_from_slice(&buf_va.to_le_bytes());
        let name_table: Vec<u8> = name_ptrs.iter().flat_map(|p| p.to_le_bytes()).collect();
        let desc_table: Vec<u8> = desc_ptrs.iter().flat_map(|p| p.to_le_bytes()).collect();

        let ed = |va: u32, bytes: Vec<u8>| Edit {
            prot_index: Some(MENU_PROT_INDEX),
            file_off: (va - MENU_BASE_VA) as usize,
            bytes,
        };
        let detour2 = |target: u32| words_to_bytes(&[j(target), nop()]);
        let edits = vec![
            ed(HOOK_BOUND_VA, detour2(bound_va)),
            ed(HOOK_MARK_VA, detour2(mark_va)),
            ed(HOOK_ROW_VA, words_to_bytes(&[j(row_va)])),
            ed(HOOK_CURSOR_VA, words_to_bytes(&[j(cursor_va)])),
            ed(bound_va, words_to_bytes(&bound)),
            ed(mark_va, words_to_bytes(&mark)),
            ed(cursor_va, words_to_bytes(&cursor)),
            ed(row_va, words_to_bytes(&row)),
            ed(scratch_va, scratch),
            ed(names_va, name_table),
            ed(descs_va, desc_table),
            ed(name_blob_va, name_blob),
            ed(desc_blob_va, desc_blob),
        ];
        Ok(Self {
            edits,
            bound_va,
            mark_va,
            row_va,
            cursor_va,
            scratch_va,
            buf_va,
            names_va,
            descs_va,
            run_used: run.cursor - MENU_RUN_VA,
            desc_used: desc_run.cursor - MENU_DESC_VA,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn br_off(w: u32) -> i32 {
        ((w & 0xffff) as u16 as i16) as i32
    }

    #[test]
    fn count_hooks_add_the_performed_count_and_return_past_their_two_words() {
        let b = assemble_bound(HOOK_BOUND_W0, BOUND_RET_VA);
        assert_eq!(
            b[0],
            lbu(T0, T1, 0x195),
            "performed byte off the same record"
        );
        assert_eq!(b[1], HOOK_BOUND_W0, "then the stock count load");
        assert_eq!(b[3], addu(V0, V0, T0));
        assert_eq!(b[4], j(0x801D_445C), "returns onto slt v0,s6,v0");
        let m = assemble_mark(HOOK_MARK_W0, MARK_RET_VA);
        assert_eq!(
            m[0],
            lbu(T0, T2, 0x195),
            "reads the byte before t2 is replaced"
        );
        assert_eq!(m[1], HOOK_MARK_W0);
        assert_eq!(m[3], addu(T2, T2, T0));
        assert_eq!(m[4], j(0x801D_4448), "returns onto sw t2,0x2c(sp)");
        let c = assemble_cursor(HOOK_CURSOR_W0, CURSOR_RET_VA);
        assert_eq!(c[0], lbu(T0, V0, 0x75D), "0x80084140-based record here");
        assert_eq!(c[1], HOOK_CURSOR_W0);
        assert_eq!(c[3], addu(S0, S0, T0));
        assert_eq!(c[4], j(0x801D_A654), "returns onto bnez s0");
        for r in [&b, &m, &c] {
            assert_eq!(r.len(), 6);
        }
    }

    #[test]
    fn row_routine_branches_land_where_the_comments_say() {
        let r = assemble_row(
            0x801E_8000,
            0x8007_8A88,
            0x8007_AEC0,
            0x801E_8200,
            0x801E_8220,
            0x801E_8240,
            0x801E_8280,
        );
        assert_eq!(
            r[0],
            lw(T8, SP, 0x1c),
            "the character from the renderer's frame"
        );
        assert_eq!(16 + 1 + br_off(r[16]), 38, "no Super left -> PICKLRN");
        assert_eq!(24 + 1 + br_off(r[24]), 36, "not performed -> NEXTK");
        assert_eq!(26 + 1 + br_off(r[26]), 34, "learned exhausted -> PICKSUP");
        assert_eq!(32 + 1 + br_off(r[32]), 38, "learned first -> PICKLRN");
        assert_eq!(34 + 1 + br_off(r[34]), 45, "this Super -> FILL");
        assert_eq!(38 + 1 + br_off(r[38]), 42, "this learned -> PLAIN");
        assert_eq!(r[36], j(0x801E_8000 + 15 * 4), "NEXTK closes on LOOP");
        assert_eq!(
            r[40],
            j(0x801E_8000 + 15 * 4),
            "PICKLRN's advance closes on LOOP"
        );
        assert_eq!(r[42], addu(A0, T1, T4), "PLAIN: a0 = record + i");
        assert_eq!(r[43], j(SCAN_HEAD_VA));
        assert_eq!(
            r[51],
            sb(T8, T5, 0),
            "scratch[+0] = character, what the found arm tests"
        );
        assert_eq!(r[52], sb(V0, T5, 2), "AP");
        assert_eq!(r[63], sw(V0, T5, 0xC), "name");
        assert_eq!(r[64], sw(V1, T5, 0x10), "description");
        assert_eq!(
            r[68],
            lw(T3, T3, lo(0x8007_AEC0u32.wrapping_sub(0x8007_8A88))),
            "arrows via the SUP delta"
        );
        let n = r.len();
        assert_eq!(r[n - 2], j(FOUND_VA), "a Super row enters the found arm");
        assert_eq!(r[n - 1], or(S2, T5, ZERO), "with s2 = the scratch record");
        // The expander loop closes on itself.
        let l = 77i32;
        assert_eq!(l + 20 + 1 + br_off(r[l as usize + 20]), l);
        assert_eq!(
            l + 2 + 1 + br_off(r[l as usize + 2]),
            l + 7,
            "not an end -> CMP"
        );
        assert_eq!(
            l + 7 + 1 + br_off(r[l as usize + 7]),
            l + 13,
            "same style -> GLYPH"
        );
    }

    use crate::mips_sim::Cpu;
    use crate::super_art_list::{ARROWS_STRIDE, SUP_STRIDE, arrow_code};
    use legaia_art::queue::Command;

    const ROW_VA: u32 = 0x801E_8000;
    const SUP_VA: u32 = 0x8007_8A88;
    const ARROWS_VA: u32 = 0x8007_AEC0;
    const SCRATCH_VA: u32 = 0x801E_8200;
    const BUF_VA: u32 = 0x801E_8220;
    const NAMES_VA: u32 = 0x801E_8240;
    const DESCS_VA: u32 = 0x801E_8280;
    const REC_BASE: u32 = 0x8008_4708;

    struct Scene {
        character: u32,
        learned: Vec<u8>,
        performed: u8,
        sup: Vec<(u8, u8, u8)>,
        arrows: Vec<Vec<Command>>,
        ends: Vec<Vec<usize>>,
    }

    fn scene(character: u32, learned: &[u8], performed: u8) -> Scene {
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
            ends: vec![
                vec![3, 5, 8],
                vec![2, 4, 6],
                vec![2, 4, 6],
                vec![2, 4, 6],
                vec![2, 4, 6],
            ],
        };
        for _ in 0..character {
            sc.arrows.rotate_left(1);
            sc.ends.rotate_left(1);
        }
        sc
    }

    fn glyphs(input: &[Command], ends: &[usize]) -> Vec<u8> {
        let row = SuperArtRow {
            character: legaia_art::queue::Character::Vahn,
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

    #[derive(Debug, PartialEq, Eq)]
    enum Row {
        Learned(u32),
        Super(usize),
    }

    fn run_row(sc: &Scene, row: u32) -> Row {
        let mut cpu = Cpu::new();
        cpu.load_words(
            ROW_VA,
            &assemble_row(
                ROW_VA, SUP_VA, ARROWS_VA, SCRATCH_VA, BUF_VA, NAMES_VA, DESCS_VA,
            ),
        );
        let rec = REC_BASE + sc.character * 0x414;
        cpu.wr8(rec + u32::from(REC_COUNT_OFF), sc.learned.len() as u8);
        for (i, id) in sc.learned.iter().enumerate() {
            cpu.wr8(rec + u32::from(REC_LIST_OFF) + i as u32, *id);
        }
        cpu.wr8(
            rec + u32::from(REC_PERFORMED_OFF),
            sc.performed | ((sc.performed.count_ones() as u8) << PERFORMED_COUNT_SHIFT),
        );
        for (k, &(ap, thr, trow)) in sc.sup.iter().enumerate() {
            let idx = sc.character * 5 + k as u32;
            let markers = glyphs(&sc.arrows[k], &sc.ends[k])[0] as u16 - sc.arrows[k].len() as u16;
            let [lo_, hi_] = (0x1614u16 | (markers << MARKERS_SHIFT)).to_le_bytes();
            cpu.load(
                SUP_VA + idx * SUP_STRIDE,
                &[ap, thr | (trow << PERFORMED_COUNT_SHIFT), lo_, hi_],
            );
            let mut w: u32 = (sc.arrows[k].len() as u32) << ARROWS_COUNT_SHIFT;
            for (n, &c) in sc.arrows[k].iter().enumerate() {
                let end = u32::from(sc.ends[k].contains(&n));
                w |= (u32::from(arrow_code(c)) | (end << 2)) << (n as u32 * ARROW_BITS);
            }
            cpu.load(ARROWS_VA + idx * ARROWS_STRIDE, &w.to_le_bytes());
            cpu.wr32(NAMES_VA + idx * 4, 0x8100_0000 + idx * 0x100);
            cpu.wr32(DESCS_VA + idx * 4, 0x8200_0000 + idx * 0x100);
        }
        cpu.wr32(SCRATCH_VA + 8, BUF_VA);
        // The renderer's frame: 0x1c(sp) = character.
        cpu.r[SP as usize] = 0x801F_FF00;
        cpu.wr32(0x801F_FF00 + 0x1c, sc.character);
        cpu.r[T1 as usize] = rec;
        cpu.r[S6 as usize] = row;
        cpu.r[S2 as usize] = 0x8007_5EC4;
        cpu.pc = ROW_VA;
        match cpu.run_until(&[SCAN_HEAD_VA, FOUND_VA]) {
            pc if pc == SCAN_HEAD_VA => {
                assert_eq!(
                    cpu.r[S2 as usize], 0x8007_5EC4,
                    "s2 untouched on the learned path"
                );
                Row::Learned(cpu.r[A0 as usize] - rec)
            }
            _ => {
                assert_eq!(cpu.r[S2 as usize], SCRATCH_VA, "s2 = the scratch record");
                assert_eq!(
                    cpu.rd8(SCRATCH_VA),
                    sc.character as u8,
                    "scratch[+0] = character"
                );
                let name = cpu.rd32(SCRATCH_VA + 0xC);
                let idx = (name - 0x8100_0000) / 0x100;
                assert_eq!(
                    cpu.rd32(SCRATCH_VA + 0x10),
                    0x8200_0000 + idx * 0x100,
                    "description of the same row"
                );
                let k = (idx - sc.character * 5) as usize;
                assert_eq!(cpu.rd8(SCRATCH_VA + 2), sc.sup[k].0, "AP");
                let want = glyphs(&sc.arrows[k], &sc.ends[k]);
                let got: Vec<u8> = (0..want.len() as u32)
                    .map(|i| cpu.rd8(BUF_VA + i))
                    .collect();
                assert_eq!(got, want, "glyph string");
                Row::Super(k)
            }
        }
    }

    /// The same merge model as the battle list's.
    fn model(learned: &[u8], mask: u8, sup: &[(u8, u8)], entry: usize) -> Row {
        let (mut i, mut k, mut r) = (0usize, 0usize, entry);
        loop {
            let next = loop {
                if k >= sup.len() {
                    break None;
                }
                if mask >> sup[k].1 & 1 == 1 {
                    break Some(k);
                }
                k += 1;
            };
            let take_super = match next {
                None => false,
                Some(k) => i >= learned.len() || learned[i] >= sup[k].0,
            };
            if take_super {
                if r == 0 {
                    return Row::Super(k);
                }
                r -= 1;
                k += 1;
            } else {
                if r == 0 {
                    return Row::Learned(i as u32);
                }
                r -= 1;
                i += 1;
            }
        }
    }

    #[test]
    fn executed_row_routine_agrees_with_the_merge_model_for_every_character() {
        let pool = [0u8, 1, 4, 12, 14];
        for character in 0u32..3 {
            for lm in 0u8..32 {
                let learned: Vec<u8> = pool
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| lm >> i & 1 == 1)
                    .map(|(_, &id)| id)
                    .collect();
                for performed in 0u8..32 {
                    let sc = scene(character, &learned, performed);
                    let sup: Vec<(u8, u8)> =
                        sc.sup.iter().map(|&(_, thr, row)| (thr, row)).collect();
                    let total = learned.len() + performed.count_ones() as usize;
                    for row in 0..total {
                        let want = model(&learned, performed, &sup, row);
                        let got = run_row(&sc, row as u32);
                        assert_eq!(
                            got, want,
                            "char {character} learned {learned:?} performed {performed:#07b} row {row}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn executed_count_hooks_add_the_performed_count() {
        let mut cpu = Cpu::new();
        let rec = REC_BASE + 0x414;
        cpu.wr8(rec + 0x185, 9);
        cpu.wr8(rec + 0x195, 0x1F | (5 << 5));
        cpu.load_words(0x801E_8000, &assemble_bound(HOOK_BOUND_W0, BOUND_RET_VA));
        cpu.r[T1 as usize] = rec;
        cpu.pc = 0x801E_8000;
        cpu.run_until(&[BOUND_RET_VA]);
        assert_eq!(cpu.r[V0 as usize], 14, "bound = learned + performed");
        let mut cpu = Cpu::new();
        cpu.wr8(rec + 0x185, 9);
        cpu.wr8(rec + 0x195, 0x03 | (2 << 5));
        cpu.load_words(0x801E_8000, &assemble_mark(HOOK_MARK_W0, MARK_RET_VA));
        cpu.r[T2 as usize] = rec;
        cpu.pc = 0x801E_8000;
        cpu.run_until(&[MARK_RET_VA]);
        assert_eq!(cpu.r[T2 as usize], 11);
        let mut cpu = Cpu::new();
        let rec2 = 0x8008_4140 + 2 * 0x414;
        cpu.wr8(rec2 + 0x74D, 5);
        cpu.wr8(rec2 + 0x75D, 0x11 | (2 << 5));
        cpu.load_words(0x801E_8000, &assemble_cursor(HOOK_CURSOR_W0, CURSOR_RET_VA));
        cpu.r[V0 as usize] = rec2;
        cpu.pc = 0x801E_8000;
        cpu.run_until(&[CURSOR_RET_VA]);
        assert_eq!(cpu.r[S0 as usize], 7);
    }

    #[test]
    fn descriptions_read_like_retails_and_stay_ascii() {
        let row = SuperArtRow {
            character: legaia_art::queue::Character::Vahn,
            name: "Tri-Somersault",
            finisher: 0x2B,
            trigger_row: 0,
            sorted_index: 1,
            chain_ids: vec![12, 4, 12],
            chain_names: vec!["Somersault".into(), "Cyclone".into(), "Somersault".into()],
            ap: 60,
            thr: 1,
            name_offset: 0x1614,
            input: vec![],
            ends: vec![],
        };
        assert_eq!(
            description(&row),
            "Super Arts. Somersault,|Cyclone, Somersault."
        );
        assert_eq!(text_bytes("Super Arts.").unwrap(), b"Super Arts.\0");
        assert!(text_bytes("caf\u{e9}").is_err());
    }
}
