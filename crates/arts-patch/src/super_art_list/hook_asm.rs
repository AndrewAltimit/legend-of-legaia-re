//! The MIPS bodies the mod injects: the count, id, fill, performed-flag and
//! pager routines, assembled against the address map in the module root.

use super::{
    ARROW_BITS, ARROWS_COUNT_SHIFT, ART_BLOCK_PTR_OFF, BATTLE_MODE, CHAR_RECORD_BASE, CMD_TABLE_VA,
    GAME_MODE_VA, GLYPH_HI, GLYPH_LO_BASE, GP_CHARACTER, HOOK_PERFORMED_W0, HOOK_PERFORMED_W1,
    LEARNED_COUNT_OFF, LEARNED_LIST_OFF, MARKER_HI, MARKERS_SHIFT, MAX_SCROLL, PAGE_ROWS,
    PAGER_SOUND_FN_VA, PERFORMED_COUNT_SHIFT, PERFORMED_MASK, PERFORMED_OFF, SCAN_HIT_VA,
    SCRATCH_AP_OFF, SCRATCH_ARROWS_OFF, SCRATCH_NAME_OFF, STYLE_ART_END, STYLE_DEFAULT,
    STYLE_SUPER_END, SUPER_ARTS_PER_CHAR,
};
use legaia_code_hooks::mips::*;

// --- Routine assembly --------------------------------------------------------

/// Word offset of a PC-relative branch from `from` (the branch's own address)
/// to `to`. Branches reach ±128 KB, so every dead-space region can be a target.
fn br(from: u32, to: u32) -> i16 {
    let d = (to as i64 - (from as i64 + 4)) / 4;
    i16::try_from(d).expect("branch target in range")
}

/// (A) `count -> count + performed`. `v0` is the character record when the hook
/// fires, so the performed byte is read **before** the displaced `lbu`
/// replaces it with the learned count; the count field is its top three bits.
/// Gated on battle mode. `disp = [lbu v0,0x74d(v0), sra a2,a2,0x1f]`.
pub(crate) fn assemble_count(disp: [u32; 2], ret: u32) -> Vec<u32> {
    vec![
        lbu(T1, V0, PERFORMED_OFF),                    // 0  t1 = performed byte
        disp[0],                                       // 1  v0 = learned count
        lui(T3, hi(GAME_MODE_VA)),                     // 2
        lh(T3, T3, lo(GAME_MODE_VA)),                  // 3
        srl(T1, T1, PERFORMED_COUNT_SHIFT),            // 4  performed count
        addiu(T3, T3, (-(BATTLE_MODE as i32)) as u16), // 5  mode - 0x15
        bne(T3, ZERO, 2),                              // 6  not in battle -> skip
        nop(),                                         // 7  delay
        addu(V0, V0, T1),                              // 8  count += performed
        j(ret),                                        // 9
        disp[1],                                       // 10 delay: sra a2,a2,0x1f
    ]
}

/// (B) resolve the row's art id by merging the id-sorted learned list with the
/// character's AP-sorted, performed Super Arts.
///
/// Entered with `v0` = record + `entry`, `a2` = `entry`, `a0` = `0x80070000`
/// (the table-base `lui`, read again by the word after the hook - preserved) and
/// `v1` = the table's first byte (the displaced `sltiu` tests it - preserved).
/// Clobbers `t0`-`t9` and `s2`/`s5`, all dead or about to be written by retail
/// at this point; `ra` is dead (spilled in the prologue). Uses no `mult`/`div`.
///
/// A learned row lands on `PLAIN` with `t4` = its index into the id list and
/// replays the stock `lbu` there; a Super Art row branches (cross-region) to the
/// fill routine `fill_va` with `t5` = the character's `SUP` base, `t6` = the
/// Super's sorted index and `t8` = the character `* 4`.
///
/// `disp = [lbu s2,0x74e(v0), sltiu v1,v1,0x63]`.
pub(crate) fn assemble_id(
    disp: [u32; 2],
    base_va: u32,
    sup_va: u32,
    fill_va: u32,
    ret: u32,
) -> Vec<u32> {
    const LOOP: i32 = 20;
    const PICKSUP: i32 = 39;
    const NEXTK: i32 = 41;
    const PICKLRN: i32 = 43;
    const PLAIN0: i32 = 47;
    const PLAINI: i32 = 48;
    let at = |i: i32| base_va + (i as u32) * 4;
    let body = vec![
        subu(V0, V0, A2),                              // 0  v0 = the character record
        lui(T3, hi(GAME_MODE_VA)),                     // 1
        lh(T3, T3, lo(GAME_MODE_VA)),                  // 2
        lbu(T0, V0, PERFORMED_OFF),                    // 3  t0 = performed byte
        addiu(T3, T3, (-(BATTLE_MODE as i32)) as u16), // 4  mode - 0x15
        bne(T3, ZERO, (PLAIN0 - 6) as i16),            // 5  not in battle -> plain
        andi(T0, T0, u16::from(PERFORMED_MASK)),       // 6  delay: the mask
        beq(T0, ZERO, (PLAIN0 - 8) as i16),            // 7  nothing performed -> plain
        lw(T8, GP, GP_CHARACTER),                      // 8  delay: character
        lbu(T2, V0, LEARNED_COUNT_OFF),                // 9  t2 = learned count
        addiu(T9, V0, LEARNED_LIST_OFF),               // 10 t9 = the id list
        sll(T8, T8, 2),   // 11 t8 = character * 4 (F keys the command table with it)
        sll(T7, T8, 2),   // 12
        addu(T7, T7, T8), // 13 character * 20 = * 5 * SUP_STRIDE
        lui(T5, hi(sup_va)), // 14
        addiu(T5, T5, lo(sup_va)), // 15
        addu(T5, T5, T7), // 16 t5 = &SUP[character*5]
        or(T4, ZERO, ZERO), // 17 i = 0 (learned cursor)
        or(T6, ZERO, ZERO), // 18 k = 0 (Super cursor)
        or(T7, A2, ZERO), // 19 r = entry (rows to pass)
        // LOOP: pick the next merged row.
        sltiu(T3, T6, SUPER_ARTS_PER_CHAR as u16), // 20 k < 5 ?
        beq(T3, ZERO, (PICKLRN - 22) as i16),      // 21 no Super left -> learned
        sll(T3, T6, 2),                            // 22 delay: k * 4
        addu(T3, T3, T5),                          // 23 t3 = &SUP[k]
        lbu(T1, T3, 1),                            // 24 thr | bit << 5
        nop(),                                     // 25
        srl(T1, T1, PERFORMED_COUNT_SHIFT),        // 26 the Super's bit
        srlv(T1, T0, T1),                          // 27 mask >> bit
        andi(T1, T1, 1),                           // 28
        beq(T1, ZERO, (NEXTK - 30) as i16),        // 29 not performed -> next k
        sltu(T1, T4, T2),                          // 30 delay: i < learned count ?
        beq(T1, ZERO, (PICKSUP - 32) as i16),      // 31 learned exhausted -> Super
        lbu(T1, T3, 1),                            // 32 delay: thr | bit << 5
        addu(T3, T9, T4),                          // 33
        lbu(T3, T3, 0),                            // 34 t3 = list[i]
        andi(T1, T1, u16::from(PERFORMED_MASK)),   // 35 t1 = thr
        sltu(T1, T3, T1),                          // 36 list[i] < thr -> learned first
        bne(T1, ZERO, (PICKLRN - 38) as i16),      // 37
        nop(),                                     // 38
        // PICKSUP: the Super Art at k is the next row.
        beq(T7, ZERO, br(at(PICKSUP), fill_va)), // 39 r == 0 -> fill and draw it
        addiu(T7, T7, 0xFFFF),                   // 40 delay: r -= 1 (F ignores t7)
        // NEXTK:
        j(at(LOOP)),      // 41
        addiu(T6, T6, 1), // 42 delay: k += 1
        // PICKLRN: the learned art at i is the next row.
        beq(T7, ZERO, (PLAINI - 44) as i16), // 43 r == 0 -> it is this row
        addiu(T7, T7, 0xFFFF),               // 44 delay: r -= 1 (PLAINI ignores t7)
        j(at(LOOP)),                         // 45
        addiu(T4, T4, 1),                    // 46 delay: i += 1
        // PLAIN0: no Super Art rows at all - the learned index is the entry.
        or(T4, A2, ZERO), // 47
        // PLAINI: learned index i -> the stock load, re-based.
        addu(V0, V0, T4),              // 48
        lbu(S2, V0, LEARNED_LIST_OFF), // 49 == disp[0]
        j(ret),                        // 50
        disp[1],                       // 51 delay: sltiu v1,v1,0x63
    ];
    // The replayed stock load is the word after the PLAINI re-base; the check
    // once named index 51, which is the delay-slot `sltiu`, so every dev-profile
    // build tripped here while release builds (CI) compiled the check away.
    debug_assert_eq!(
        body[PLAINI as usize + 1],
        disp[0],
        "the learned path replays the stock load"
    );
    debug_assert_eq!(body.len(), PLAINI as usize + 4);
    body
}

/// (F) fill the scratch record for the Super Art (B) picked and enter the draw.
///
/// Entered from (B) with `t5` = `&SUP[character*5]`, `t6` = the Super's sorted
/// index `k`, `t8` = the character `* 4`. Writes the scratch record's `+2` AP
/// and `+0xC` name pointer (chased through `DAT_801C9360[character] -> +0x58 ->
/// + name offset`), expands the Super's packed arrows into the glyph buffer the
/// scratch record's own `+8` points at, then jumps to the scan's hit arm with
/// `s5` = scratch + 8.
///
/// The glyph string is `[n + markers]` then, per arrow, a `0xFF style` marker
/// whenever the style changes and the arrow glyph: default style, the
/// art-end style on an arrow a sub-art ends on, the Super-end style on the
/// final arrow - so Tri-Somersault reads blue blue yellow blue yellow blue orange.
/// The packed word is consumed three bits at a time (`dir dir end`), the count
/// having been read off its top bits first.
///
/// The packed arrows are addressed **relative to the SUP record**: both tables
/// share the `character*5 + k` order and the 4-byte stride, so
/// `&arrows[row] = &SUP[row] + (arrows_va - sup_va)` - which is what keeps a
/// Noa or Gala row from reading Vahn's arrows.
pub(crate) fn assemble_fill(
    base_va: u32,
    sup_va: u32,
    arrows_va: u32,
    scratch_va: u32,
) -> Vec<u32> {
    const LOOP: i32 = 25;
    const CMP: i32 = 32;
    const GLYPH: i32 = 38;
    let delta = arrows_va.wrapping_sub(sup_va);
    let body = vec![
        sll(T3, T6, 2),                          // 0
        addu(T3, T3, T5),                        // 1  t3 = &SUP[k]
        lbu(T1, T3, 0),                          // 2  t1 = chain AP
        lhu(T7, T3, 2),                          // 3  t7 = name offset | markers << 13
        lui(T9, hi(CMD_TABLE_VA)),               // 4
        addu(T9, T9, T8),                        // 5  + character * 4
        lw(T9, T9, lo(CMD_TABLE_VA)),            // 6  the command-data block
        lui(T5, hi(scratch_va)),                 // 7  (delay filler)
        lw(T9, T9, ART_BLOCK_PTR_OFF),           // 8  the art-record array
        addiu(T5, T5, lo(scratch_va)),           // 9  t5 = scratch (filler)
        srl(T6, T7, MARKERS_SHIFT),              // 10 t6 = marker count
        andi(T7, T7, (1 << MARKERS_SHIFT) - 1),  // 11 t7 = name offset
        addu(T9, T9, T7),                        // 12 t9 = the name
        sb(T1, T5, SCRATCH_AP_OFF),              // 13 scratch[+2]   = AP
        sw(T9, T5, SCRATCH_NAME_OFF),            // 14 scratch[+0xC] = name
        lui(T2, hi(delta)),                      // 15
        addu(T3, T3, T2),                        // 16 &SUP[k] + hi(delta)
        lw(T3, T3, lo(delta)),                   // 17 t3 = the packed arrows word
        lw(T9, T5, SCRATCH_ARROWS_OFF),          // 18 t9 = the glyph buffer
        srl(T4, T3, ARROWS_COUNT_SHIFT),         // 19 t4 = arrow count n
        addu(T6, T6, T4),                        // 20 glyph count = markers + n
        sb(T6, T9, 0),                           // 21 buf[0]
        addiu(T9, T9, 1),                        // 22
        or(T8, ZERO, ZERO),                      // 23 k = 0
        ori(T6, ZERO, u16::from(STYLE_DEFAULT)), // 24 current style
        // LOOP: k counts the arrow being drawn, one-based, from here on.
        addiu(T8, T8, 1),                          // 25 k += 1
        andi(T7, T3, 4),                           // 26 an art ends here ?
        beq(T7, ZERO, (CMP - 28) as i16),          // 27 no -> default style
        ori(T1, ZERO, u16::from(STYLE_DEFAULT)),   // 28 delay: want = default
        bne(T8, T4, (CMP - 30) as i16),            // 29 not the last arrow ->
        ori(T1, ZERO, u16::from(STYLE_ART_END)),   // 30 delay: want = art end
        ori(T1, ZERO, u16::from(STYLE_SUPER_END)), // 31 the final arrow
        // CMP: emit a marker only when the style changes.
        beq(T1, T6, (GLYPH - 33) as i16), // 32 same style -> no marker
        ori(T2, ZERO, u16::from(MARKER_HI)), // 33 delay
        sb(T2, T9, 0),                    // 34
        sb(T1, T9, 1),                    // 35
        addiu(T9, T9, 2),                 // 36
        or(T6, T1, ZERO),                 // 37 current = want
        // GLYPH: the arrow itself.
        andi(T1, T3, 3),                         // 38 dir 0..3
        addiu(T1, T1, u16::from(GLYPH_LO_BASE)), // 39 low byte
        ori(T2, ZERO, u16::from(GLYPH_HI)),      // 40
        sb(T2, T9, 0),                           // 41
        sb(T1, T9, 1),                           // 42
        srl(T3, T3, ARROW_BITS),                 // 43 next packed arrow
        sltu(T2, T8, T4),                        // 44 k < n ?
        bne(T2, ZERO, (LOOP - 46) as i16),       // 45
        addiu(T9, T9, 2),                        // 46 delay
        j(SCAN_HIT_VA),                          // 47 draw it
        addiu(S5, T5, SCRATCH_ARROWS_OFF),       // 48 delay: s5 = scratch + 8
    ];
    debug_assert_eq!(base_va % 4, 0);
    body
}

/// (W) record the Super Art the applier just matched, in place of the match
/// arm's first two words. `t5` = the character; the matched trigger-table row
/// arrives as `t2 = row * 16` (the replace-row offset `sll t2,a1,0x4` at
/// `0x801EFB04`, which the copy loop never writes - `a1` itself is reused by
/// that loop for the bytes it copies, so at the arm it is the last replacement
/// byte, not the row). Sets the row's bit in the record's performed byte and
/// bumps its count field once, then returns to the flag store with `v0`/`v1`
/// as retail left them.
pub(crate) fn assemble_performed(ret: u32) -> Vec<u32> {
    let body = vec![
        lui(V1, 0x801F),                           // 0  replay
        addiu(V0, ZERO, 1),                        // 1  replay
        srl(T3, T2, 4),                            // 2  row = t2 >> 4
        ori(T4, ZERO, 1),                          // 3
        sllv(T3, T4, T3),                          // 4  t3 = the row's bit
        sll(T0, T5, 6),                            // 5
        addu(T0, T0, T5),                          // 6  * 65
        sll(T0, T0, 2),                            // 7  * 260
        addu(T0, T0, T5),                          // 8  * 261
        sll(T0, T0, 2),                            // 9  * 0x414
        lui(T1, hi(CHAR_RECORD_BASE)),             // 10
        addiu(T1, T1, lo(CHAR_RECORD_BASE)),       // 11
        addu(T0, T0, T1),                          // 12 t0 = the character record
        lbu(T2, T0, PERFORMED_OFF),                // 13 t2 = performed byte
        nop(),                                     // 14 load delay
        and(T4, T2, T3),                           // 15 already set ?
        bne(T4, ZERO, 3),                          // 16 yes -> nothing to do
        or(T2, T2, T3),                            // 17 delay: set the bit
        addiu(T2, T2, 1 << PERFORMED_COUNT_SHIFT), // 18 count += 1
        sb(T2, T0, PERFORMED_OFF),                 // 19
        j(ret),                                    // 20
        nop(),                                     // 21
    ];
    debug_assert_eq!(body[0], HOOK_PERFORMED_W0);
    debug_assert_eq!(body[1], HOOK_PERFORMED_W1);
    body
}

/// (D) the whole replacement pager, assembled at [`PAGER_VA`]. Same prologue,
/// pad gate, character-record walk and open/close tail as retail; the page step
/// becomes "advance while another page exists", and the row count it compares
/// against is `learned + performed`, read from the same record byte the
/// renderer's hooks read.
pub(crate) fn assemble_pager(base_va: u32) -> Vec<u32> {
    const TOGGLE: i32 = 38;
    const EXIT: i32 = 52;
    let exit_va = base_va + (EXIT as u32) * 4;
    let body = vec![
        addiu(SP, SP, 0xFFE8),               // 0  frame
        sw(RA, SP, 0x10),                    // 1
        lui(V0, 0x8008),                     // 2
        addiu(A1, V0, 0x4140),               // 3  a1 = 0x80084140
        lw(V0, A1, 0x598),                   // 4  v0 = the accepted pad mask
        nop(),                               // 5
        and(A0, A0, V0),                     // 6
        beq(A0, ZERO, (EXIT - 8) as i16),    // 7  not our button -> return
        lui(V0, 0x8008),                     // 8  delay
        lw(V1, V0, 0xBB8C),                  // 9  v1 = character (gp+0x874)
        nop(),                               // 10
        sll(V0, V1, 6),                      // 11
        addu(V0, V0, V1),                    // 12
        sll(V0, V0, 2),                      // 13
        addu(V0, V0, V1),                    // 14
        sll(V0, V0, 2),                      // 15 v0 = character * 0x414
        addu(V0, V0, A1),                    // 16 + 0x80084140
        lbu(V1, V0, PERFORMED_OFF),          // 17 v1 = performed byte
        lbu(A0, V0, LEARNED_COUNT_OFF),      // 18 a0 = learned count
        srl(V1, V1, PERFORMED_COUNT_SHIFT),  // 19 performed count
        addu(A0, A0, V1),                    // 20 a0 = rows the list will draw
        beq(A0, ZERO, (EXIT - 22) as i16),   // 21 nothing to show -> return
        lui(V0, 0x801F),                     // 22 delay
        lbu(V0, V0, 0x4E09),                 // 23 v0 = the list-state flag
        nop(),                               // 24
        andi(V0, V0, 1),                     // 25
        beq(V0, ZERO, (TOGGLE - 27) as i16), // 26 list closed -> open it
        lui(A1, 0x8008),                     // 27 delay
        lw(V1, A1, 0xB458),                  // 28 v1 = page offset
        nop(),                               // 29
        sltiu(V0, V1, MAX_SCROLL),           // 30 offset < MAX_SCROLL ?
        beq(V0, ZERO, (TOGGLE - 32) as i16), // 31 no -> close
        addiu(V0, V1, PAGE_ROWS as u16),     // 32 delay: v0 = offset + 5
        sltu(V0, V0, A0),                    // 33 another page exists ?
        beq(V0, ZERO, (TOGGLE - 35) as i16), // 34 no -> close
        addiu(V1, V1, PAGE_ROWS as u16),     // 35 delay: v1 = offset + 5
        j(exit_va),                          // 36
        sw(V1, A1, 0xB458),                  // 37 delay: advance one page
        // TOGGLE: retail's own open/close tail, unchanged.
        lui(V0, 0x801F),                   // 38
        lbu(V1, V0, 0x4E09),               // 39
        nop(),                             // 40
        addiu(V1, V1, 1),                  // 41
        andi(A0, V1, 0x81),                // 42
        andi(V1, V1, 1),                   // 43
        beq(V1, ZERO, (EXIT - 45) as i16), // 44 closed -> return
        sb(A0, V0, 0x4E09),                // 45 delay: store either way
        lui(V0, 0x801F),                   // 46
        sb(ZERO, V0, 0x4E08),              // 47
        lui(V0, 0x8008),                   // 48
        ori(A0, ZERO, 1),                  // 49
        jal(PAGER_SOUND_FN_VA),            // 50
        sw(ZERO, V0, 0xB458),              // 51 delay: open on page 0
        // EXIT:
        lw(RA, SP, 0x10),    // 52
        nop(),               // 53
        jr(RA),              // 54
        addiu(SP, SP, 0x18), // 55
    ];
    debug_assert_eq!(body.len(), EXIT as usize + 4);
    body
}
