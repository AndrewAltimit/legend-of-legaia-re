//! **Show Super Arts on the in-battle move list**: add the acting character's
//! five Super Arts to the Tactical-Arts list the Triangle button opens in
//! battle, which retail never draws at all.
//!
//! ## What retail does
//!
//! The in-battle arts list is rendered by `FUN_80034358` (`SCUS_942.54`, one
//! caller at `0x8003238C`). Read off the disassembly, not the C, it is a bare
//! `0..count` walk over the acting character's *learned* art ids:
//!
//! ```text
//! 800343c4  lbu   v0,0x74d(v0)   ; count      (A)  -> [sp+0x28], the loop bound
//! 8003440c  lw    v0,0x140(gp)   ; scroll     (the page offset the pager sets)
//! 80034414  addu  a2,s7,v0       ; entry = row + scroll
//! 80034418  sltu  v0,a2,t0       ; entry < count ?
//! 80034450  lbu   s2,0x74e(v0)   ; id         (B)  -> the id list at +0x74E..
//! 80034464  ...                  ; linear scan of DAT_80075EC4 (stride 0x14)
//! 80034470  bne   v1,v0,...      ;   require rec[+0] == character
//! 80034480  bne   v0,s2,...      ;   require rec[+1] == id
//! 8003474c  addiu s7,s7,0x1      ; miss  (C)  -> the row is silently consumed
//! ```
//!
//! `v0` at (A) is `0x80084140 + character*0x414`; the character index itself is
//! `gp+0x874` (`0` Vahn / `1` Noa / `2` Gala / `3` Terra), the same 0-based space
//! the arts-name table's `rec[+0]` byte uses. Rows per page is the drawable
//! height over the row pitch, `0x90 / 0x1C` = **5**.
//!
//! A Super Art is not in that walk and structurally cannot be. It has no row in
//! `DAT_80075EC4` (45 records, fifteen regular arts per character), it is never
//! entered as a combo (it is a find/replace over the finished action queue in
//! `FUN_801EF9E4`), and **no learned bit for one exists anywhere** - the id list
//! at `+0x74E..+0x75D` holds only regular-art ids, so retail has nowhere to
//! record that a Super Art was performed. That is why this feature shows all
//! five of the acting character's Super Arts unconditionally: "show", not
//! "learned". Gating on the chain arts being present would need a chain table
//! plus a per-row scan, which does not fit the dead space this uses.
//!
//! ## The four hooks
//!
//! | Site | VA | Stock word | Role |
//! |---|---|---|---|
//! | A count | `0x800343C4` | `0x9042074D` (`lbu v0,0x74d(v0)`) | return `count + 5` so the walk runs five more rows |
//! | B id | `0x80034450` | `0x9052074E` (`lbu s2,0x74e(v0)`) | synthesise id `0x40 + k` for the five added rows |
//! | C miss-draw | `0x8003474C` | `0x26F70001` (`addiu s7,s7,1`) | draw the Super Art's name on the table-scan miss |
//! | D pager | `0x801D3748` (PROT 0898) | `0x27BDFFE8` | page `0/5/10/15` instead of stopping at `10` |
//!
//! (A) and (B) add their five rows only for characters `0..=2`; Terra (`3`) has
//! no arts-name-table rows at all, so she keeps retail's empty list rather than
//! gaining five blank ones.
//!
//! The synthetic id space starts at [`SYN_ID_BASE`] because the retail table
//! uses ids `0x00..=0x10` only - [`plan`](SuperArtListInjection::plan) re-reads
//! the disc's own table and refuses if any record has landed in
//! `SYN_ID_BASE..SYN_ID_BASE+5`, so a synthetic row can never collide with a
//! real art's name.
//!
//! ### Why (C) carries the names instead of chasing them in RAM
//!
//! The runtime chase is real - `0x8004B6FC..0x8004B718` walks
//! `DAT_801C9360[slot]` -> `lw +0x58` -> the art-block pointer, then
//! `addiu s0,v0,0x4` - but that `+4`, plus a row origin that came from a
//! heuristic enumeration base, leaves the runtime row index **unmeasured**.
//! Carrying the fifteen names in dead space costs about 220 bytes and removes
//! both that unknown and the actor-slot to record-index mapping. Retail art
//! names are plain NUL-terminated ASCII (the regular rows draw
//! `record+0xC` -> `FUN_80036888`), so the glyph renderer draws the carried
//! blob identically.
//!
//! Register liveness at (C) - the delicate site - is what makes it cheap: `s2`
//! still holds the id on every miss path (the draw path that reuses `s2` never
//! runs), `s8`/`s6` still hold the row's x/y, `t0`/`t1`/`t2` are dead, and
//! `FUN_80036888` preserves `s0-s8` by ABI. The hook reuses the caller's own
//! frame slot `0x10(sp)` for the fifth argument exactly as retail's own call
//! site does, so it allocates no frame and saves no `ra` (retail's `ra` is
//! already spilled at `0x54(sp)` by the prologue).
//!
//! ### (D) is a replacement, not a detour
//!
//! `FUN_801D3748` is an 81-instruction leaf with exactly one caller
//! (`jal` at `0x801D21BC`) and no reference to its interior from anywhere -
//! every branch into its body comes from inside itself. So the whole body is
//! rewritten in place in the overlay, which costs no dead space at all. Retail
//! steps the page offset `0 -> 5 -> 10` and then closes the list; the
//! replacement steps while `scroll + 5 < effective count` (capped at
//! [`MAX_SCROLL`]), which is `0 -> 5 -> 10 -> 15` for a full fifteen-art list
//! plus five Super Arts. It also drops retail's "no arts at all -> do nothing"
//! early exit for characters `0..=2`, so a character who has learned nothing can
//! still open the list on its Super Arts page.
//!
//! ## Placement
//!
//! The three SCUS routines go in the verified-dead arena [`ARENA1_VA`] and the
//! name blob + offset table in the rodata gap [`SCUS_GAP_VA`] - the same
//! regions `--shiny-seru`, `--arts-ap-grant` / `--arts-ap-cost` and
//! `--delilas-challenge` contend over, so this toggle joins that
//! **mutually exclusive** set. The pager replacement lands in the overlay
//! itself and takes no arena bytes.
//!
//! ## Known cosmetic gap
//!
//! `FUN_801D3444`'s caption thresholds (`< 6`, `< 11`) stay retail, so the
//! Triangle prompt can read "View Hyper Arts list" on the added page where it
//! should read "View Next page". The list contents are correct; only that one
//! caption string is stale.
//!
//! No Sony bytes are embedded: the routines are the patcher's own code and the
//! fifteen names come from [`legaia_art::SUPER_ARTS`], the repo's own
//! walkthrough-sourced label table.

use anyhow::{Result, bail};

use legaia_art::queue::Character;

use crate::mips::*;
use crate::shiny_seru::{
    ARENA1_END_VA, ARENA1_VA, Edit, OVERLAY_TABLE_RANGES, SCUS_GAP_END_VA, SCUS_GAP_VA,
    SCUS_TABLE_RANGES,
};
use crate::super_art_power::super_arts_for;

/// PROT entry of the battle-action overlay that hosts the list pager.
pub const OVERLAY_PROT_INDEX: usize = legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;
/// Load base of that overlay: a VA maps to raw-entry file offset `va - base`.
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// Super Arts per character, and therefore rows added to the list.
pub const SUPER_ARTS_PER_CHAR: usize = 5;
/// Characters with an arts list (`0` Vahn / `1` Noa / `2` Gala). Terra (`3`) has
/// no arts-name-table rows and is left at retail.
pub const ARTS_CHARACTERS: usize = 3;
/// Synthetic art id of the first added row. Retail's table uses `0x00..=0x10`,
/// so `0x40..=0x44` cannot collide; [`SuperArtListInjection::plan`] re-checks
/// that against the disc's own table.
pub const SYN_ID_BASE: u8 = 0x40;
/// Rows drawn per page (`drawable height 0x90 / row pitch 0x1C`).
pub const PAGE_ROWS: u32 = 5;
/// Highest page offset the replacement pager will step to. Fifteen regular arts
/// plus five Super Arts is twenty rows = four pages (`0/5/10/15`).
pub const MAX_SCROLL: u16 = 15;

// --- Hook sites (all byte-verified against the US build) ---------------------

/// (A) `lbu v0,0x74d(v0)` - the learned-art count that bounds the walk.
pub const HOOK_COUNT_VA: u32 = 0x8003_43C4;
pub(crate) const HOOK_COUNT_W0: u32 = 0x9042_074D;
/// `sra a2,a2,0x1f` - the displaced second word (`a2` is read again at
/// `0x800343E0`, so the routine has to replay it).
pub(crate) const HOOK_COUNT_W1: u32 = 0x0006_37C3;
const COUNT_RET_VA: u32 = HOOK_COUNT_VA + 8;

/// (B) `lbu s2,0x74e(v0)` - the per-row learned art id.
pub const HOOK_ID_VA: u32 = 0x8003_4450;
pub(crate) const HOOK_ID_W0: u32 = 0x9052_074E;
/// `sltiu v1,v1,0x63` - the displaced second word (the table-terminator test).
pub(crate) const HOOK_ID_W1: u32 = 0x2C63_0063;
const ID_RET_VA: u32 = HOOK_ID_VA + 8;

/// (C) `addiu s7,s7,0x1` - the row-consumed arm the table scan falls into on a
/// miss. **One word only**: the next word `0x80034750` is itself a jump target
/// (`j 0x80034750` from the hit path at `0x8003472C`), so it must stay put - the
/// hook's own delay slot re-executes it harmlessly and the routine returns to it.
pub const HOOK_DRAW_VA: u32 = 0x8003_474C;
pub(crate) const HOOK_DRAW_W0: u32 = 0x26F7_0001;
/// `lw t0,0x2c(sp)` at `HOOK_DRAW_VA + 4` - fingerprinted, never written.
pub(crate) const HOOK_DRAW_W1: u32 = 0x8FA8_002C;
const DRAW_RET_VA: u32 = HOOK_DRAW_VA + 4;

/// The proportional-text renderer the list draws every art name through:
/// `FUN_80036888(str, 0, 0, x, y)` with `y` in `0x10(sp)`.
pub const GLYPH_FN_VA: u32 = 0x8003_6888;
pub(crate) const GLYPH_FN_W0: u32 = 0x27BD_FFC0;

/// (D) `FUN_801D3748`, the Triangle list pager, in the battle-action overlay.
pub const PAGER_VA: u32 = 0x801D_3748;
/// Original body length in words - the replacement must fit inside it.
pub const PAGER_WORDS: usize = 81;
pub(crate) const PAGER_W0: u32 = 0x27BD_FFE8;
/// `lw v0,0x598(a1)` - the pad-mask read that gates the whole routine.
pub(crate) const PAGER_MASK_W: u32 = 0x8CA2_0598;
/// `lbu v1,0x74d(v0)` - the pager's own read of the learned-art count.
pub(crate) const PAGER_COUNT_W: u32 = 0x9043_074D;
/// `addiu sp,sp,0x18` - the last word of the body.
pub(crate) const PAGER_LAST_W: u32 = 0x27BD_0018;
/// The list-open sound cue the pager tails into.
const PAGER_SOUND_FN_VA: u32 = 0x8003_19A8;

// Globals the routines touch, all read straight out of the retail code above.
/// `gp+0x874` - the acting character index (0-based).
const GP_CHARACTER: u16 = 0x874;
/// `gp+0x13C` - the text CLUT index the glyph renderer reads (`7` = a name).
const GP_TEXT_COLOR: u16 = 0x13C;
/// The colour retail selects before drawing an art name.
const NAME_TEXT_COLOR: u16 = 7;
/// The caller's frame slot holding the glyph renderer's fifth argument.
const GLYPH_Y_ARG_OFF: u16 = 0x10;
/// `[sp+0x28]` - where the walk caches the (patched) art count.
const SP_COUNT_OFF: u16 = 0x28;

// --- Routine assembly --------------------------------------------------------

/// (A) `count -> count + 5`, but only for a character that has an arts list.
/// Branch-free: `sltiu` yields `1`/`0`, `sll`+`addu` turns that into `5`/`0`.
/// `disp = [lbu v0,0x74d(v0), sra a2,a2,0x1f]`.
pub(crate) fn assemble_count(disp: [u32; 2], ret: u32) -> Vec<u32> {
    vec![
        disp[0],                               // 0 v0 = learned count
        lw(T0, GP, GP_CHARACTER),              // 1 t0 = character (covers v0's load delay)
        nop(),                                 // 2 load delay
        sltiu(T0, T0, ARTS_CHARACTERS as u16), // 3 t0 = 1 when character < 3
        sll(T1, T0, 2),                        // 4 t1 = 4 or 0
        addu(T1, T1, T0),                      // 5 t1 = 5 or 0
        addu(V0, V0, T1),                      // 6 count += 5 (or nothing, for Terra)
        j(ret),                                // 7
        disp[1],                               // 8 delay: sra a2,a2,0x1f (replay)
    ]
}

/// (B) synthesise the added rows' art ids. For row `entry` past the real count,
/// `id = SYN_ID_BASE + (entry - real_count)`; every other row keeps the byte the
/// stock `lbu` read. The stock load is always replayed (it is a **read**, never
/// a write - the highest added row reads `+0x761`, one byte past the id list and
/// into the equipment byte, and the value is discarded).
/// `disp = [lbu s2,0x74e(v0), sltiu v1,v1,0x63]`.
pub(crate) fn assemble_id(disp: [u32; 2], ret: u32) -> Vec<u32> {
    const DONE: i32 = 12;
    vec![
        disp[0],                               // 0  s2 = stock id byte
        lw(T0, GP, GP_CHARACTER),              // 1  t0 = character
        lw(T1, SP, SP_COUNT_OFF),              // 2  t1 = patched count
        sltiu(T0, T0, ARTS_CHARACTERS as u16), // 3  character < 3 ?
        sll(T2, T0, 2),                        // 4
        addu(T2, T2, T0),                      // 5  t2 = added rows (5 or 0)
        subu(T1, T1, T2),                      // 6  t1 = real learned count
        subu(T1, A2, T1),                      // 7  t1 = k = entry - real count
        sltu(T0, T1, T2),                      // 8  k < added rows ? (k < 0 wraps)
        beq(T0, ZERO, (DONE - 10) as i16),     // 9  no -> keep the stock id
        addiu(T1, T1, u16::from(SYN_ID_BASE)), // 10 delay: t1 = SYN_ID_BASE + k
        or(S2, T1, ZERO),                      // 11 s2 = the synthetic id
        j(ret),                                // 12 DONE
        disp[1],                               // 13 delay: sltiu v1,v1,0x63 (replay)
    ]
}

/// (C) draw an added row's name. Entered on the table-scan miss with `s2` = the
/// row's id, `s8`/`s6` = the row's x/y. Non-synthetic misses (including the
/// zero-command hit arm, which arrives with `s2 = 0`) fall straight through to
/// the replayed `addiu s7,s7,1`.
///
/// `offtab_va` is the 15-byte `character*5 + k` -> blob-offset table and
/// `blob_va` the NUL-terminated name blob.
pub(crate) fn assemble_draw(offtab_va: u32, blob_va: u32, disp_w0: u32, ret: u32) -> Vec<u32> {
    const RET_IDX: i32 = 24;
    vec![
        ori(T0, ZERO, u16::from(SYN_ID_BASE)), // 0  t0 = SYN_ID_BASE
        subu(T0, S2, T0),                      // 1  t0 = k = id - SYN_ID_BASE
        sltiu(T1, T0, SUPER_ARTS_PER_CHAR as u16), // 2  k < 5 ? (k < 0 wraps)
        beq(T1, ZERO, (RET_IDX - 4) as i16),   // 3  not an added row -> return
        lw(T1, GP, GP_CHARACTER),              // 4  delay: t1 = character
        nop(),                                 // 5  load delay
        sltiu(T2, T1, ARTS_CHARACTERS as u16), // 6  character < 3 ?
        beq(T2, ZERO, (RET_IDX - 8) as i16),   // 7  no -> return
        sll(T2, T1, 2),                        // 8  delay: character * 4
        addu(T1, T2, T1),                      // 9  character * 5
        addu(T0, T0, T1),                      // 10 index = character * 5 + k
        lui(T1, hi(offtab_va)),                // 11
        addu(T1, T1, T0),                      // 12
        lbu(T0, T1, lo(offtab_va)),            // 13 t0 = blob offset of the name
        lui(A0, hi(blob_va)),                  // 14 (load delay for t0)
        addiu(A0, A0, lo(blob_va)),            // 15
        addu(A0, A0, T0),                      // 16 a0 = the name
        ori(T1, ZERO, NAME_TEXT_COLOR),        // 17
        sw(T1, GP, GP_TEXT_COLOR),             // 18 same colour retail uses
        sw(S6, SP, GLYPH_Y_ARG_OFF),           // 19 fifth argument: y
        or(A1, ZERO, ZERO),                    // 20
        or(A2, ZERO, ZERO),                    // 21
        jal(GLYPH_FN_VA),                      // 22
        or(A3, S8, ZERO),                      // 23 delay: x
        j(ret),                                // 24 RET_IDX -> 0x80034750
        disp_w0,                               // 25 delay: addiu s7,s7,1 (replay)
    ]
}

/// (D) the whole replacement pager, assembled at [`PAGER_VA`]. Same prologue,
/// pad gate, character-record walk and open/close tail as retail; the page step
/// becomes "advance while another page exists", and the count it compares
/// against is the same `count + 5` (or `count`, for Terra) the renderer sees.
pub(crate) fn assemble_pager(base_va: u32) -> Vec<u32> {
    const TOGGLE: i32 = 39;
    const EXIT: i32 = 53;
    let exit_va = base_va + (EXIT as u32) * 4;
    let body = vec![
        addiu(SP, SP, 0xFFE8),                 // 0  frame
        sw(RA, SP, 0x10),                      // 1
        lui(V0, 0x8008),                       // 2
        addiu(A1, V0, 0x4140),                 // 3  a1 = 0x80084140
        lw(V0, A1, 0x598),                     // 4  v0 = the accepted pad mask
        nop(),                                 // 5
        and(A0, A0, V0),                       // 6
        beq(A0, ZERO, (EXIT - 8) as i16),      // 7  not our button -> return
        lui(V0, 0x8008),                       // 8  delay
        lw(V1, V0, 0xBB8C),                    // 9  v1 = character (gp+0x874)
        nop(),                                 // 10
        sll(V0, V1, 6),                        // 11
        addu(V0, V0, V1),                      // 12
        sll(V0, V0, 2),                        // 13
        addu(V0, V0, V1),                      // 14
        sll(V0, V0, 2),                        // 15 v0 = character * 0x414
        addu(V0, V0, A1),                      // 16 + 0x80084140
        lbu(A0, V0, 0x74D),                    // 17 a0 = learned count
        sltiu(V1, V1, ARTS_CHARACTERS as u16), // 18 character < 3 ? (load delay)
        sll(V0, V1, 2),                        // 19
        addu(V1, V0, V1),                      // 20 v1 = added rows (5 or 0)
        addu(A0, A0, V1),                      // 21 a0 = rows the list will draw
        beq(A0, ZERO, (EXIT - 23) as i16),     // 22 nothing to show -> return
        lui(V0, 0x801F),                       // 23 delay
        lbu(V0, V0, 0x4E09),                   // 24 v0 = the list-state flag
        nop(),                                 // 25
        andi(V0, V0, 1),                       // 26
        beq(V0, ZERO, (TOGGLE - 28) as i16),   // 27 list closed -> open it
        lui(A1, 0x8008),                       // 28 delay
        lw(V1, A1, 0xB458),                    // 29 v1 = page offset
        nop(),                                 // 30
        sltiu(V0, V1, MAX_SCROLL),             // 31 offset < MAX_SCROLL ?
        beq(V0, ZERO, (TOGGLE - 33) as i16),   // 32 no -> close
        addiu(V0, V1, PAGE_ROWS as u16),       // 33 delay: v0 = offset + 5
        sltu(V0, V0, A0),                      // 34 another page exists ?
        beq(V0, ZERO, (TOGGLE - 36) as i16),   // 35 no -> close
        addiu(V1, V1, PAGE_ROWS as u16),       // 36 delay: v1 = offset + 5
        j(exit_va),                            // 37
        sw(V1, A1, 0xB458),                    // 38 delay: advance one page
        // TOGGLE: retail's own open/close tail, unchanged.
        lui(V0, 0x801F),                   // 39
        lbu(V1, V0, 0x4E09),               // 40
        nop(),                             // 41
        addiu(V1, V1, 1),                  // 42
        andi(A0, V1, 0x81),                // 43
        andi(V1, V1, 1),                   // 44
        beq(V1, ZERO, (EXIT - 46) as i16), // 45 closed -> return
        sb(A0, V0, 0x4E09),                // 46 delay: store either way
        lui(V0, 0x801F),                   // 47
        sb(ZERO, V0, 0x4E08),              // 48
        lui(V0, 0x8008),                   // 49
        ori(A0, ZERO, 1),                  // 50
        jal(PAGER_SOUND_FN_VA),            // 51
        sw(ZERO, V0, 0xB458),              // 52 delay: open on page 0
        // EXIT:
        lw(RA, SP, 0x10),    // 53
        nop(),               // 54
        jr(RA),              // 55
        addiu(SP, SP, 0x18), // 56
    ];
    debug_assert_eq!(body.len(), EXIT as usize + 4);
    body
}

// --- Planning ----------------------------------------------------------------

/// A planned "show Super Arts" injection: every same-size write, plus the exact
/// landing addresses so an oracle can pin them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtListInjection {
    pub edits: Vec<Edit>,
    /// The fifteen names written into the blob, in `character * 5 + k` order.
    pub names: Vec<String>,
    pub count_va: u32,
    pub id_va: u32,
    pub draw_va: u32,
    pub offtab_va: u32,
    pub blob_va: u32,
}

fn words_to_bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Resolve a SCUS VA to a file offset in the image.
fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    legaia_asset::item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: can't resolve SCUS VA {va:#x}"))
}

/// Read a `u32` from `scus` at `va` and require it to be `expect`.
fn expect_scus(scus: &[u8], va: u32, expect: u32) -> Result<u32> {
    let off = scus_off(scus, va)?;
    let got = read_word(scus, off)?;
    if got != expect {
        bail!(
            "show-super-arts: SCUS {va:#x} = {got:#010x}, expected {expect:#010x} \
             (unrecognized build - nothing written)"
        );
    }
    Ok(got)
}

/// Read a `u32` from the overlay at `va` and require it to be `expect`.
fn expect_overlay(ov: &[u8], va: u32, expect: u32) -> Result<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let got = read_word(ov, off)?;
    if got != expect {
        bail!(
            "show-super-arts: PROT {OVERLAY_PROT_INDEX} {va:#x} = {got:#010x}, \
             expected {expect:#010x} (unrecognized build - nothing written)"
        );
    }
    Ok(got)
}

/// Refuse a region that overlaps a live data table, even if its bytes are zero.
fn assert_not_in_tables(va: u32, len: u32, ranges: &[(u32, u32)], what: &str) -> Result<()> {
    let end = va.saturating_add(len);
    for &(a, b) in ranges {
        if va < b && a < end {
            bail!(
                "show-super-arts {what} region {va:#x}..+{len} overlaps live table \
                 {a:#x}..{b:#x} - refusing"
            );
        }
    }
    Ok(())
}

/// Confirm a hosted span really is all-zero dead space in this image.
fn assert_zero(scus: &[u8], va: u32, len: usize) -> Result<()> {
    let off = scus_off(scus, va)?;
    let region = scus
        .get(off..off + len)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: {va:#x}..+{len} past end of SCUS"))?;
    if region.iter().any(|&b| b != 0) {
        bail!(
            "show-super-arts: {va:#x}..+{len} is not all-zero dead space \
             (build / collision) - refusing"
        );
    }
    Ok(())
}

/// The fifteen Super Art names in `character * 5 + k` order.
pub fn super_art_names() -> Vec<String> {
    let mut out = Vec::with_capacity(ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR);
    for ch in Character::all() {
        for s in super_arts_for(ch) {
            out.push(s.name.to_string());
        }
    }
    out
}

/// Build the offset table + NUL-terminated blob for `names`.
fn build_blob(names: &[String]) -> Result<(Vec<u8>, Vec<u8>)> {
    let want = ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR;
    if names.len() != want {
        bail!(
            "show-super-arts: expected {want} Super Art names, got {} - refusing",
            names.len()
        );
    }
    let mut offsets = Vec::with_capacity(want);
    let mut blob: Vec<u8> = Vec::new();
    for n in names {
        if n.is_empty() || !n.is_ascii() {
            bail!("show-super-arts: Super Art name {n:?} is empty or non-ASCII - refusing");
        }
        let off = u8::try_from(blob.len())
            .map_err(|_| anyhow::anyhow!("show-super-arts: name blob exceeds 255 bytes"))?;
        offsets.push(off);
        blob.extend_from_slice(n.as_bytes());
        blob.push(0);
    }
    Ok((offsets, blob))
}

impl SuperArtListInjection {
    /// Plan every edit. Needs the `SCUS_942.54` image (hook sites, arts-name
    /// table, arena host) and the raw PROT 0898 entry (the pager). Refuses -
    /// without touching anything - if the build isn't the recognized US layout,
    /// a hosted region isn't dead space, the synthetic id space collides with a
    /// real art row, or a routine overruns its arena.
    pub fn plan(scus: &[u8], ov0898: &[u8]) -> Result<Self> {
        // Fingerprint every site first; capture the words the routines replay.
        let count_disp = [
            expect_scus(scus, HOOK_COUNT_VA, HOOK_COUNT_W0)?,
            expect_scus(scus, HOOK_COUNT_VA + 4, HOOK_COUNT_W1)?,
        ];
        let id_disp = [
            expect_scus(scus, HOOK_ID_VA, HOOK_ID_W0)?,
            expect_scus(scus, HOOK_ID_VA + 4, HOOK_ID_W1)?,
        ];
        let draw_w0 = expect_scus(scus, HOOK_DRAW_VA, HOOK_DRAW_W0)?;
        // Never written, but the hook's correctness depends on it staying put:
        // the hit path jumps straight to it.
        expect_scus(scus, HOOK_DRAW_VA + 4, HOOK_DRAW_W1)?;
        expect_scus(scus, GLYPH_FN_VA, GLYPH_FN_W0)?;
        expect_overlay(ov0898, PAGER_VA, PAGER_W0)?;
        expect_overlay(ov0898, PAGER_VA + 0x10, PAGER_MASK_W)?;
        expect_overlay(ov0898, PAGER_VA + 0x44, PAGER_COUNT_W)?;
        expect_overlay(ov0898, PAGER_VA + 0x12C, jal(PAGER_SOUND_FN_VA))?;
        expect_overlay(
            ov0898,
            PAGER_VA + (PAGER_WORDS as u32 - 1) * 4,
            PAGER_LAST_W,
        )?;
        assert_not_in_tables(
            PAGER_VA,
            (PAGER_WORDS * 4) as u32,
            OVERLAY_TABLE_RANGES,
            "pager",
        )?;

        // The synthetic ids must be unreachable by any real arts-table row, or a
        // Super Art row would draw a regular art's name instead of falling
        // through to the injected draw.
        let rows = legaia_art::arts_table::raw_records_from_scus(scus)
            .ok_or_else(|| anyhow::anyhow!("show-super-arts: parse the SCUS arts-name table"))?;
        let syn = SYN_ID_BASE..SYN_ID_BASE.saturating_add(SUPER_ARTS_PER_CHAR as u8);
        if let Some(bad) = rows.iter().find(|r| syn.contains(&r.index)) {
            bail!(
                "show-super-arts: arts-name table row (character {:?}, id {:#x}) lands in the \
                 synthetic id range {:#x}..{:#x} - refusing",
                bad.character,
                bad.index,
                syn.start,
                syn.end
            );
        }

        // Data first: the offset table then the blob, both in the rodata gap.
        let names = super_art_names();
        let (offsets, blob) = build_blob(&names)?;
        let offtab_va = SCUS_GAP_VA;
        let blob_va = offtab_va + offsets.len().next_multiple_of(4) as u32;
        let gap_end = blob_va + blob.len() as u32;
        if gap_end > SCUS_GAP_END_VA {
            bail!(
                "show-super-arts: name table + blob ({} B) overrun the SCUS gap \
                 {SCUS_GAP_VA:#x}..{SCUS_GAP_END_VA:#x}",
                gap_end - offtab_va
            );
        }

        // Routines, laid end to end in arena 1.
        let count = assemble_count(count_disp, COUNT_RET_VA);
        let count_va = ARENA1_VA;
        let id = assemble_id(id_disp, ID_RET_VA);
        let id_va = count_va + (count.len() * 4) as u32;
        let draw = assemble_draw(offtab_va, blob_va, draw_w0, DRAW_RET_VA);
        let draw_va = id_va + (id.len() * 4) as u32;
        let arena1_end = draw_va + (draw.len() * 4) as u32;
        for (va, what) in [(count_va, "count"), (id_va, "id"), (draw_va, "draw")] {
            if va & 3 != 0 {
                bail!("show-super-arts: {what} routine VA {va:#x} is not 4-byte aligned");
            }
        }
        if arena1_end > ARENA1_END_VA {
            bail!(
                "show-super-arts: the three routines ({} B) overrun arena 1 \
                 {ARENA1_VA:#x}..{ARENA1_END_VA:#x}",
                arena1_end - ARENA1_VA
            );
        }
        for (va, len, what) in [
            (ARENA1_VA, arena1_end - ARENA1_VA, "routine"),
            (offtab_va, gap_end - offtab_va, "name blob"),
        ] {
            assert_not_in_tables(va, len, SCUS_TABLE_RANGES, what)?;
            assert_zero(scus, va, len as usize)?;
        }

        // The pager replacement, nop-padded out to the original body length so
        // no stale retail instruction survives behind it.
        let mut pager = assemble_pager(PAGER_VA);
        if pager.len() > PAGER_WORDS {
            bail!(
                "show-super-arts: the replacement pager ({} words) does not fit \
                 FUN_801D3748 ({PAGER_WORDS} words)",
                pager.len()
            );
        }
        pager.resize(PAGER_WORDS, nop());

        let detour = |target_va: u32| -> Vec<u8> { words_to_bytes(&[j(target_va), nop()]) };
        let edits = vec![
            // Detours over the renderer's three sites (A and B take two words,
            // C takes one - see HOOK_DRAW_VA).
            Edit {
                prot_index: None,
                file_off: scus_off(scus, HOOK_COUNT_VA)?,
                bytes: detour(count_va),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(scus, HOOK_ID_VA)?,
                bytes: detour(id_va),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(scus, HOOK_DRAW_VA)?,
                bytes: words_to_bytes(&[j(draw_va)]),
            },
            // Routines + data into the verified-dead SCUS regions.
            Edit {
                prot_index: None,
                file_off: scus_off(scus, count_va)?,
                bytes: words_to_bytes(&count),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(scus, id_va)?,
                bytes: words_to_bytes(&id),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(scus, draw_va)?,
                bytes: words_to_bytes(&draw),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(scus, offtab_va)?,
                bytes: offsets,
            },
            Edit {
                prot_index: None,
                file_off: scus_off(scus, blob_va)?,
                bytes: blob,
            },
            // The pager, replaced whole inside the overlay.
            Edit {
                prot_index: Some(OVERLAY_PROT_INDEX),
                file_off: (PAGER_VA - OVERLAY_BASE_VA) as usize,
                bytes: words_to_bytes(&pager),
            },
        ];

        Ok(Self {
            edits,
            names,
            count_va,
            id_va,
            draw_va,
            offtab_va,
            blob_va,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a branch's word offset back out of an encoded instruction.
    fn br_off(w: u32) -> i32 {
        ((w & 0xffff) as u16 as i16) as i32
    }

    #[test]
    fn count_routine_adds_the_super_art_rows_only_for_an_arts_character() {
        let disp = [HOOK_COUNT_W0, HOOK_COUNT_W1];
        let r = assemble_count(disp, COUNT_RET_VA);
        assert_eq!(r[0], HOOK_COUNT_W0, "replays the displaced count load");
        assert_eq!(r[1], lw(T0, GP, GP_CHARACTER), "reads the character index");
        assert_eq!(r[2], nop(), "load-delay slot after the character read");
        assert_eq!(r[3], sltiu(T0, T0, 3), "character < 3");
        // sll+addu turns the 1/0 predicate into 5/0 with no branch.
        assert_eq!(r[4], sll(T1, T0, 2));
        assert_eq!(r[5], addu(T1, T1, T0));
        assert_eq!(r[6], addu(V0, V0, T1));
        assert_eq!(r[7], j(COUNT_RET_VA));
        assert_eq!(
            r[8], HOOK_COUNT_W1,
            "replays sra a2,a2,0x1f in the delay slot"
        );
        assert_eq!(r.len(), 9);
    }

    #[test]
    fn count_routine_predicate_is_five_or_zero() {
        // Model the branch-free arithmetic the routine performs.
        for ch in 0u32..4 {
            let pred = u32::from(ch < ARTS_CHARACTERS as u32);
            assert_eq!((pred << 2) + pred, if ch < 3 { 5 } else { 0 });
        }
    }

    #[test]
    fn id_routine_synthesises_only_past_the_real_count() {
        let disp = [HOOK_ID_W0, HOOK_ID_W1];
        let r = assemble_id(disp, ID_RET_VA);
        assert_eq!(r[0], HOOK_ID_W0, "the stock id load is always replayed");
        assert_eq!(r[2], lw(T1, SP, SP_COUNT_OFF), "reads the cached count");
        assert_eq!(r[7], subu(T1, A2, T1), "k = entry - real count");
        assert_eq!(r[8], sltu(T0, T1, T2), "unsigned, so a negative k fails");
        assert_eq!(
            r[10],
            addiu(T1, T1, 0x40),
            "SYN_ID_BASE + k in the delay slot"
        );
        assert_eq!(r[11], or(S2, T1, ZERO));
        assert_eq!(r[12], j(ID_RET_VA));
        assert_eq!(r[13], HOOK_ID_W1, "replays sltiu v1,v1,0x63");
        // The skip branch has to land exactly on the `j`.
        let target = 9 + 1 + br_off(r[9]);
        assert_eq!(target, 12, "beq skips straight to the return");
        assert_eq!(r.len(), 14);
        // Nothing in the routine writes memory: the character record is read-only.
        for w in &r {
            let op = w >> 26;
            assert!(!(0x28..=0x2b).contains(&op), "no store in the id routine");
        }
    }

    #[test]
    fn id_routine_never_writes_the_character_record() {
        // Structural restatement of the rule the hook must not break: the only
        // memory op touching the record is the stock `lbu`, a load.
        assert_eq!(HOOK_ID_W0 >> 26, 0x24, "stock word is lbu (a load)");
    }

    #[test]
    fn draw_routine_returns_to_the_untouched_next_word() {
        let r = assemble_draw(SCUS_GAP_VA, SCUS_GAP_VA + 16, HOOK_DRAW_W0, DRAW_RET_VA);
        assert_eq!(r[0], ori(T0, ZERO, 0x40));
        assert_eq!(r[1], subu(T0, S2, T0), "k from s2, the surviving id");
        assert_eq!(r[2], sltiu(T1, T0, 5));
        assert_eq!(r[5], nop(), "load-delay slot after the character read");
        assert_eq!(r[13], lbu(T0, T1, lo(SCUS_GAP_VA)), "offset-table load");
        assert_eq!(
            r[14],
            lui(A0, hi(SCUS_GAP_VA + 16)),
            "covers the load delay"
        );
        assert_eq!(
            r[19],
            sw(S6, SP, GLYPH_Y_ARG_OFF),
            "y through the caller frame"
        );
        assert_eq!(r[22], jal(GLYPH_FN_VA));
        assert_eq!(r[23], or(A3, S8, ZERO), "x in the delay slot");
        assert_eq!(r[24], j(DRAW_RET_VA), "returns to 0x80034750, not past it");
        assert_eq!(r[25], HOOK_DRAW_W0, "replays addiu s7,s7,1");
        assert_eq!(r.len(), 26);
        // Both guards must skip to the `j`, never into the call.
        for i in [3usize, 7] {
            let target = i as i32 + 1 + br_off(r[i]);
            assert_eq!(target, 24, "guard at {i} lands on the return");
        }
        assert_eq!(DRAW_RET_VA, HOOK_DRAW_VA + 4, "one-word detour only");
    }

    #[test]
    fn draw_routine_uses_no_frame_of_its_own() {
        let r = assemble_draw(SCUS_GAP_VA, SCUS_GAP_VA + 16, HOOK_DRAW_W0, DRAW_RET_VA);
        // No `addiu sp,sp,imm` anywhere: the hook borrows retail's frame, so
        // the caller's `[sp+0x28]` / `[sp+0x2c]` locals stay addressable.
        for w in &r {
            let is_addiu_sp = (w >> 26) == 0x09 && ((w >> 21) & 0x1f) == SP;
            assert!(!is_addiu_sp, "the draw hook must not move sp");
        }
    }

    #[test]
    fn pager_fits_the_original_body_and_its_branches_land() {
        let r = assemble_pager(PAGER_VA);
        assert!(r.len() <= PAGER_WORDS, "{} words", r.len());
        assert_eq!(r[0], addiu(SP, SP, 0xFFE8), "same frame as retail");
        assert_eq!(r[r.len() - 2], jr(RA));
        assert_eq!(r[r.len() - 1], addiu(SP, SP, 0x18));
        let exit = r.len() - 4;
        let toggle = 39usize;
        assert_eq!(r[toggle], lui(V0, 0x801F), "toggle arm starts at 39");
        for (i, want) in [
            (7usize, exit),
            (22, exit),
            (45, exit),
            (27, toggle),
            (32, toggle),
            (35, toggle),
        ] {
            let target = i as i32 + 1 + br_off(r[i]);
            assert_eq!(target as usize, want, "branch at {i}");
        }
        assert_eq!(r[37], j(PAGER_VA + exit as u32 * 4), "j lands on EXIT");
        assert_eq!(r[51], jal(PAGER_SOUND_FN_VA), "keeps the open cue");
    }

    #[test]
    fn pager_steps_pages_while_another_page_exists() {
        // The comparison the routine encodes: advance iff offset < MAX_SCROLL
        // and offset + PAGE_ROWS < rows.
        let step = |offset: u16, rows: u32| -> Option<u16> {
            if offset < MAX_SCROLL && u32::from(offset) + PAGE_ROWS < rows {
                Some(offset + PAGE_ROWS as u16)
            } else {
                None
            }
        };
        // Fifteen learned arts + five Super Arts = four pages.
        assert_eq!(step(0, 20), Some(5));
        assert_eq!(step(5, 20), Some(10));
        assert_eq!(step(10, 20), Some(15));
        assert_eq!(step(15, 20), None, "closes after the last page");
        // Nothing learned: the Super Arts page is the only page.
        assert_eq!(step(0, 5), None);
        // One learned art: two pages.
        assert_eq!(step(0, 6), Some(5));
        assert_eq!(step(5, 6), None);
        // Terra keeps retail's empty list.
        assert_eq!(step(0, 0), None);
    }

    #[test]
    fn pager_delay_slots_are_harmless_when_the_branch_is_taken() {
        let r = assemble_pager(PAGER_VA);
        // Each conditional branch's delay slot writes a register the taken arm
        // re-initialises before use (v0 at TOGGLE, v1 at TOGGLE, a1 unused).
        assert_eq!(r[8], lui(V0, 0x8008));
        assert_eq!(r[23], lui(V0, 0x801F));
        assert_eq!(r[28], lui(A1, 0x8008));
        assert_eq!(r[33], addiu(V0, V1, 5));
        assert_eq!(r[36], addiu(V1, V1, 5));
        assert_eq!(
            r[46],
            sb(A0, V0, 0x4E09),
            "the store is unconditional in retail too"
        );
    }

    #[test]
    fn blob_layout_is_deterministic_and_fits_the_gap() {
        let names = super_art_names();
        assert_eq!(names.len(), ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR);
        let (offsets, blob) = build_blob(&names).expect("blob");
        assert_eq!(offsets.len(), 15);
        assert_eq!(offsets[0], 0);
        // Every offset addresses the start of its own NUL-terminated name.
        for (i, &off) in offsets.iter().enumerate() {
            let end = blob[off as usize..]
                .iter()
                .position(|&b| b == 0)
                .expect("terminator");
            assert_eq!(&blob[off as usize..off as usize + end], names[i].as_bytes());
        }
        let total = offsets.len().next_multiple_of(4) + blob.len();
        assert!(
            total <= (SCUS_GAP_END_VA - SCUS_GAP_VA) as usize,
            "{total} B in a {} B gap",
            SCUS_GAP_END_VA - SCUS_GAP_VA
        );
    }

    #[test]
    fn build_blob_rejects_a_bad_name_set() {
        assert!(build_blob(&[]).is_err(), "wrong count");
        let mut names = super_art_names();
        names[0] = String::new();
        assert!(build_blob(&names).is_err(), "empty name");
        let mut names = super_art_names();
        names[3] = "Fire\u{2013}Tackle".to_string();
        assert!(build_blob(&names).is_err(), "non-ASCII name");
    }

    #[test]
    fn routines_fit_arena_one() {
        let count = assemble_count([HOOK_COUNT_W0, HOOK_COUNT_W1], COUNT_RET_VA);
        let id = assemble_id([HOOK_ID_W0, HOOK_ID_W1], ID_RET_VA);
        let draw = assemble_draw(SCUS_GAP_VA, SCUS_GAP_VA + 16, HOOK_DRAW_W0, DRAW_RET_VA);
        let total = (count.len() + id.len() + draw.len()) * 4;
        assert!(
            total <= (ARENA1_END_VA - ARENA1_VA) as usize,
            "{total} B in a {} B arena",
            ARENA1_END_VA - ARENA1_VA
        );
    }
}
