//! In-shop seru-trade UI builders: the picker row-4 / reorder detour stubs,
//! the dispatch + entry stubs, and the full trade-screen handler (draw + input
//! + swap), all hosted in the menu overlay 0899 run-C dead region.

use super::*;

/// Words planted at [`PICKER_RENDER_VA`]: `j PICKER_TRIGGER_VA` then `nop`
/// (replacing the prologue head, which the stub replays).
pub fn picker_detour_words() -> [u32; 2] {
    [j(PICKER_TRIGGER_VA), nop()]
}
/// Words planted at [`TRADE_DISPATCH_VA`]: `j TRADE_DISPATCH_STUB_VA` then `nop`.
pub fn trade_dispatch_detour_words() -> [u32; 2] {
    [j(TRADE_DISPATCH_STUB_VA), nop()]
}
/// Words planted at [`ENTRY_VA`]: `j ENTRY_STUB_VA` then `nop`.
pub fn trade_entry_detour_words() -> [u32; 2] {
    [j(ENTRY_STUB_VA), nop()]
}
/// Reorder: the two words that repoint the body's row-2 text load to "@Trade"
/// (`lui a0,hi(TRADE_STR_VA); addiu a0,a0,lo(TRADE_STR_VA)`), so row 2 shows Trade.
pub fn row2_str_load_new() -> [u32; 2] {
    [lui(A0, hi(TRADE_STR_VA)), addiu(A0, A0, lo(TRADE_STR_VA))]
}
/// Reorder dispatch stub ([`TRADE_DISPATCH_STUB_VA`]), reached from the confirm
/// detour at [`TRADE_DISPATCH_VA`] (a0 = cursor): cursor 2 → enter the Trade
/// sub-mode ([`SUBSTATE_VA`] = [`TRADE_SUBMODE`]) and exit the dispatcher; cursor 3
/// → the original Quit action ([`QUIT_CODE_VA`]); cursor 0/1 → the Buy/Sell checks
/// ([`BUY_SELL_CHECK_VA`]).
pub fn assemble_trade_dispatch_stub() -> Vec<u32> {
    let mut w: Vec<u32> = Vec::new();
    w.push(addiu(T0, ZERO, 2)); // cursor 2 = Trade (after reorder)
    let b_trade = w.len();
    w.push(0); // beq a0,t0,.trade (patched)
    w.push(nop());
    w.push(addiu(T0, ZERO, 3)); // cursor 3 = Quit
    let b_quit = w.len();
    w.push(0); // beq a0,t0,.quit (patched)
    w.push(nop());
    w.push(j(BUY_SELL_CHECK_VA)); // cursor 0/1 -> Buy/Sell
    w.push(nop());
    let trade = w.len();
    // Slide the picker windows away (reuse the Sell transition) so the trade screen
    // gets the cleared space. Preserve ra across the call - we exit via `j TRADE_EXIT`
    // whose tail `jr ra` must still return to the menu tick.
    w.push(addiu(SP, SP, 0xFFF8)); // sp -= 8
    w.push(sw(RA, SP, 0));
    w.push(lui(A0, hi(SLIDE_AWAY_SCRIPT_VA)));
    w.push(addiu(A0, A0, lo(SLIDE_AWAY_SCRIPT_VA)));
    w.push(jal(WIDGET_VM_FN)); // FUN_801d6628(&DAT_801e4e54) - slide away
    w.push(nop());
    w.push(lw(RA, SP, 0));
    w.push(addiu(SP, SP, 8));
    // Reset the trade-screen state for this entry. All cells live in the same
    // 0x8007AExx page, so one `lui at` covers them. pad-prev = all-ones so the ✕ held
    // from confirming "Trade" in the picker isn't seen as a fresh press on frame 1.
    w.push(lui(AT, hi(TRADE_ACTIVE_VA)));
    w.push(addiu(T1, ZERO, SLIDE_START_OFF as u16));
    w.push(sw(T1, AT, lo(TRADE_SLIDE_DELTA_VA))); // slide = off-screen start
    w.push(sw(ZERO, AT, lo(TRADE_CURSOR_VA))); // line cursor = 0
    w.push(sw(ZERO, AT, lo(TRADE_CONFIRM_VA))); // confirm sub-state = 0 (browsing)
    w.push(sw(ZERO, AT, lo(TRADE_YESNO_VA))); // yes/no = 0 (Yes)
    w.push(addiu(T1, ZERO, 0xFFFF));
    w.push(sw(T1, AT, lo(TRADE_PAD_PREV_VA))); // pad prev = all-held (no frame-1 edges)
    w.push(addiu(T1, ZERO, 1));
    w.push(sw(T1, AT, lo(TRADE_ACTIVE_VA))); // TRADE_ACTIVE = 1
    w.push(j(TRADE_EXIT_VA)); // exit dispatcher; the entry detour catches the flag
    w.push(nop());
    let quit = w.len();
    w.push(j(QUIT_CODE_VA)); // original Quit action (sound + exit)
    w.push(nop());
    w[b_trade] = beq(A0, T0, (trade as i32 - (b_trade as i32 + 1)) as i16);
    w[b_quit] = beq(A0, T0, (quit as i32 - (b_quit as i32 + 1)) as i16);
    debug_assert!(
        TRADE_DISPATCH_STUB_VA + (w.len() as u32) * 4 <= ROW4_STUB_VA,
        "dispatch stub overruns into the row-4 stub (0899 run-C layout)"
    );
    w
}
/// `FUN_801dafd4` entry stub ([`ENTRY_STUB_VA`]): if the picker sub-state
/// ([`SUBSTATE_VA`]) is the Trade sub-mode, jump to the trade handler; otherwise
/// replay the displaced prologue and rejoin the function body at [`ENTRY_RETURN_VA`].
pub fn assemble_trade_entry_stub() -> Vec<u32> {
    let mut w: Vec<u32> = vec![
        lui(AT, hi(TRADE_ACTIVE_VA)),
        lw(V0, AT, lo(TRADE_ACTIVE_VA)), // v0 = TRADE_ACTIVE flag
        nop(),                           // R3000 load delay slot
    ];
    let b = w.len();
    w.push(0); // bne v0,zero,.trade (patched)
    w.push(nop());
    w.push(ENTRY_DISPLACED[0]); // addiu sp,sp,-0x20
    w.push(ENTRY_DISPLACED[1]); // sw s1,0x14(sp)
    w.push(j(ENTRY_RETURN_VA)); // back into FUN_801dafd4
    w.push(nop());
    let trade = w.len();
    w.push(j(TRADE_HANDLER_VA));
    w.push(nop());
    w[b] = bne(V0, ZERO, (trade as i32 - (b as i32 + 1)) as i16);
    w
}
/// In-shop trade-screen handler ([`TRADE_HANDLER_VA`]), invoked (via the entry
/// detour) in place of `FUN_801dafd4` while the picker sub-state is the Trade
/// sub-mode. Runs in mode 0x17 with the shop fully intact.
///
/// Renders the **want-a-type / offer-a-partner** offer (see
/// [`legaia_asset::seru_trade`]): it reads the current `(want, give)` pair from the
/// precomputed [`BUCKET_TABLE_VA`] indexed by `(play_time / `[`RESEED_PERIOD_FRAMES`]`)
/// & `[`BUCKET_INDEX_MASK`], draws the give-back seru as a reward header, then scans
/// the four party records - for each member that owns the wanted seru it draws one
/// selectable line `want_name  owner_name  LVL n` (so the same wanted type held by
/// two members lists once per owner, matching `expand_offers`). Finally the native
/// window box, and on ○ it clears the active flag to return to the picker.
///
/// DRAW ORDER MATTERS: text is emitted FIRST, the opaque window box LAST. The native
/// box (`FUN_8002C69C`) and the renderer's own pass both put a later-submitted prim
/// at a DEEPER OT slot, so a box drawn after the text lands *behind* it - exactly the
/// fix used for the in-body row-4 label. Drawing the box first buries every glyph
/// under the blue fill (verified: blank box in a VRAM dump).
///
/// Register budget (all callee-saved, restored on exit): s0 = party slot, s1 = seru
/// index within the owner, s2 = current row y, s3 = wanted id, s4 = current record
/// base, s5 = that record's seru count. The native draw callees preserve s-regs, so
/// loop state survives across them; the give id lives in a scratch reg only until the
/// header draw (no call between reading it and using it).
pub fn assemble_trade_handler() -> Vec<u32> {
    // Absolute VA of word index `i` (the loops `j` to fixed gap VAs, not PC-relative).
    let va = |i: usize| TRADE_HANDLER_VA + (i as u32) * 4;
    // Compute `id * 0xC` into T6 (the spell-name-table stride) from `id` in `src`.
    let id_times_12 = |w: &mut Vec<u32>, src: u32| {
        w.push(sll(T6, src, 2)); // id*4
        w.push(sll(T7, T6, 1)); // id*8
        w.push(addu(T6, T7, T6)); // id*12
    };

    // Prologue: 0x38 frame. sp+0x10 is the native draw 5th-arg (y) build slot; saves
    // ra + s0..s5 above it.
    let mut w: Vec<u32> = vec![
        addiu(SP, SP, 0xFFC8), // sp -= 0x38
        sw(RA, SP, 0x2C),
        sw(S0, SP, 0x14),
        sw(S1, SP, 0x18),
        sw(S2, SP, 0x1C),
        sw(S3, SP, 0x20),
        sw(S4, SP, 0x24),
        sw(S5, SP, 0x28),
        sw(S7, SP, 0x30), // s7 = slide x-offset (held across the frame)
        sw(S6, SP, 0x34), // s6 = give_level (header display + future swap)
        jal(PAD_POLL_FN), // refresh PAD_CUR
        nop(),
    ];

    // --- slide-in: s7 = the box/text x-offset, stepped from SLIDE_START_OFF -> 0 ---
    w.push(lui(AT, hi(TRADE_SLIDE_DELTA_VA)));
    w.push(lw(S7, AT, lo(TRADE_SLIDE_DELTA_VA)));
    w.push(nop()); // load-delay before the branch reads s7
    let slid_b = w.len();
    w.push(0); // bgez s7,.slid (settled at >=0 -> skip stepping) (patched)
    w.push(nop());
    w.push(addiu(S7, S7, SLIDE_STEP)); // step toward 0
    w.push(lui(AT, hi(TRADE_SLIDE_DELTA_VA)));
    w.push(sw(S7, AT, lo(TRADE_SLIDE_DELTA_VA)));
    let slid = w.len();
    w[slid_b] = bgez(S7, (slid as i32 - (slid_b as i32 + 1)) as i16);

    // --- current offer: want -> s3, give -> t5, give_level -> s6 ---
    if SERU_DEMO_FORCE_WANT {
        // DEV: force a fixed (want, give, level) so a save owning SERU_DEMO_BASE_ID lists.
        w.push(addiu(S3, ZERO, SERU_DEMO_BASE_ID)); // want
        w.push(addiu(T5, ZERO, SERU_DEMO_BASE_ID + 1)); // give
        w.push(addiu(S6, ZERO, 7)); // give_level (mid of 4..=9)
    } else {
        // bucket = (play_time / RESEED_PERIOD_FRAMES) & (BUCKET_COUNT-1); entry = bucket*3.
        w.push(lui(AT, hi(PLAY_TIME_VA)));
        w.push(lw(T0, AT, lo(PLAY_TIME_VA)));
        w.push(addiu(T1, ZERO, RESEED_PERIOD_FRAMES)); // (fills the lw load-delay)
        w.push(divu(T0, T1));
        w.push(mflo(T0)); // t0 = bucket
        w.push(andi(T0, T0, BUCKET_INDEX_MASK)); // % BUCKET_COUNT
        w.push(sll(T1, T0, 1)); // bucket*2
        w.push(addu(T0, T1, T0)); // bucket*3 (3-byte entries: want,give,give_level)
        w.push(lui(T1, hi(BUCKET_TABLE_VA)));
        w.push(addiu(T1, T1, lo(BUCKET_TABLE_VA)));
        w.push(addu(T1, T1, T0));
        w.push(lbu(S3, T1, 0)); // want
        w.push(lbu(T5, T1, 1)); // give
        w.push(lbu(S6, T1, 2)); // give_level (fills the t5 load-delay)
        w.push(nop()); // load-delay (s6 used by the header level draw)
    }
    // stash give id so the per-owner loop can skip owners who already own it
    w.push(lui(AT, hi(TRADE_GIVE_ID_VA)));
    w.push(sw(T5, AT, lo(TRADE_GIVE_ID_VA)));

    // --- reward header: the give-back seru name at (x=0x30, y=0x34) ---
    id_times_12(&mut w, T5);
    w.push(lui(T7, hi(SERU_NAME_PTRS))); // a0 = *(SERU_NAME_PTRS + give*0xC)
    w.push(addiu(T7, T7, lo(SERU_NAME_PTRS)));
    w.push(addu(T7, T7, T6));
    w.push(lw(A0, T7, 0));
    w.push(addiu(V0, ZERO, ROW_HEADER_Y)); // y (fills the lw load-delay before a0's use)
    w.push(sw(V0, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, COL_HEADER_X)); // x + slide offset
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // reward level (the bucket's fixed give-back level, shown so the player sees the
    // trade's value): FUN_80034b78(s6, 1, COL_LEVEL_X + slide, ROW_HEADER_Y) - aligns
    // under the per-owner level column.
    w.push(addu(A0, ZERO, S6)); // value = give_level
    w.push(addiu(A1, ZERO, 1)); // min_digits
    w.push(addiu(A2, S7, COL_LEVEL_X)); // x + slide offset
    w.push(addiu(A3, ZERO, ROW_HEADER_Y)); // y
    w.push(jal(NUMBER_FN));
    w.push(nop());

    // --- per-owner lines: for slot in 0..4, for j in 0..count: if ids[j]==want ---
    w.push(addiu(S0, ZERO, 0)); // slot = 0
    w.push(addiu(S2, ZERO, ROW_FIRST_Y)); // first row y
    let slotloop = w.len();
    w.push(slti(T0, S0, PARTY_SLOT_COUNT)); // slot < 4 ?
    let done_b = w.len();
    w.push(0); // beq t0,zero,.done (patched)
    w.push(nop());
    // record base s4 = CHAR_RECORD_BASE + slot*0x414
    w.push(addiu(T1, ZERO, CHAR_RECORD_STRIDE));
    w.push(multu(S0, T1));
    w.push(mflo(T2));
    w.push(lui(T3, hi(CHAR_RECORD_BASE)));
    w.push(addiu(T3, T3, lo(CHAR_RECORD_BASE)));
    w.push(addu(S4, T3, T2));
    w.push(lbu(S5, S4, lo(SERU_COUNT_VA - CHAR_RECORD_BASE))); // s5 = count (+0x13C)
    w.push(addiu(S1, ZERO, 0)); // j = 0 (fills the lbu load-delay)
    let seruloop = w.len();
    w.push(slt(T0, S1, S5)); // j < count ?
    let nextslot_b = w.len();
    w.push(0); // beq t0,zero,.nextslot (patched)
    w.push(nop());
    // id = *(s4 + 0x13D + j)
    w.push(addu(T1, S4, S1));
    w.push(lbu(T4, T1, lo(SERU_IDS_VA - CHAR_RECORD_BASE)));
    w.push(sll(T6, S3, 2)); // (fills the lbu load-delay) want*4, reused by render (1)
    let skip_b = w.len();
    w.push(0); // bne t4,s3,.skip (patched)
    w.push(nop());
    // MATCH: skip this owner if they ALREADY own the give-back seru (pointless trade).
    // Scan k=0..count for the give id; if found, jump to .nextslot (no line drawn).
    // Uses t0/t1/t2 only - t6 still holds want*4 from the bne delay slot for the render.
    w.push(lui(AT, hi(TRADE_GIVE_ID_VA)));
    w.push(lw(T0, AT, lo(TRADE_GIVE_ID_VA))); // t0 = give id
    w.push(addiu(T1, ZERO, 0)); // t1 = k = 0 (fills the lw load-delay)
    let gloop = w.len();
    w.push(slt(T2, T1, S5)); // k < count ?
    let gdone_b = w.len();
    w.push(0); // beq t2,zero,.gnotfound (give not owned -> draw)
    w.push(nop());
    w.push(addu(T2, S4, T1));
    w.push(lbu(T2, T2, lo(SERU_IDS_VA - CHAR_RECORD_BASE))); // ids[k]
    w.push(nop()); // load-delay before the beq reads t2
    let gskip_b = w.len();
    w.push(0); // beq t2,t0,.gskip (owns give -> skip owner)
    w.push(nop());
    w.push(addiu(T1, T1, 1)); // k++
    w.push(j(va(gloop)));
    w.push(nop());
    let gskip = w.len();
    w.push(0); // j .nextslot (skip owner) - patched once nextslot is known
    w.push(nop());
    let gnotfound = w.len();
    w[gdone_b] = beq(T2, ZERO, (gnotfound as i32 - (gdone_b as i32 + 1)) as i16);
    w[gskip_b] = beq(T2, T0, (gskip as i32 - (gskip_b as i32 + 1)) as i16);
    // .gnotfound: render the line.
    // MATCH (1): want spell name at x=0x40 (t6 already = want*4 from the delay slot).
    w.push(sll(T7, T6, 1)); // want*8
    w.push(addu(T6, T7, T6)); // want*12
    w.push(lui(T7, hi(SERU_NAME_PTRS)));
    w.push(addiu(T7, T7, lo(SERU_NAME_PTRS)));
    w.push(addu(T7, T7, T6));
    w.push(lw(A0, T7, 0));
    w.push(sw(S2, SP, 0x10)); // y (fills the lw load-delay)
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, COL_WANT_X)); // x + slide offset
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // (2) owner name: a0 = s4 + record name offset (+0x2A7).
    w.push(addiu(A0, S4, RECORD_NAME_OFFSET));
    w.push(sw(S2, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, COL_OWNER_X)); // x + slide offset
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // (3) level number: FUN_80034b78(*(s4+0x161+j), 1, COL_LEVEL_X + slide, y=s2).
    w.push(addu(T1, S4, S1));
    w.push(lbu(A0, T1, SERU_LEVELS_OFFSET)); // a0 = level value
    w.push(addiu(A1, ZERO, 1)); // min_digits (fills the lbu load-delay)
    w.push(addiu(A2, S7, COL_LEVEL_X)); // x + slide offset
    w.push(addu(A3, ZERO, S2)); // y
    w.push(jal(NUMBER_FN));
    w.push(nop());
    // stash (record base, want-index) when this is the selected line, so the swap on
    // ✕-Yes writes the right owner without a re-scan. cur_line = (s2 - ROW_FIRST_Y)>>4
    // compared to the (last-frame-clamped) cursor cell.
    w.push(lui(AT, hi(TRADE_CURSOR_VA)));
    w.push(lw(T0, AT, lo(TRADE_CURSOR_VA))); // t0 = cursor
    w.push(addiu(T1, S2, (0u16).wrapping_sub(ROW_FIRST_Y))); // (fills t0 load-delay)
    w.push(srl(T1, T1, 4)); // t1 = cur_line
    let stash_b = w.len();
    w.push(0); // bne t1,t0,.nostash (patched)
    w.push(nop());
    w.push(lui(AT, hi(TRADE_SEL_BASE_VA)));
    w.push(sw(S4, AT, lo(TRADE_SEL_BASE_VA))); // selected owner record base
    w.push(sw(S1, AT, lo(TRADE_SEL_J_VA))); // selected want-index (same gap page)
    let nostash = w.len();
    w[stash_b] = bne(T1, T0, (nostash as i32 - (stash_b as i32 + 1)) as i16);
    w.push(addiu(S2, S2, ROW_STEP_Y)); // advance the row
    let skip = w.len();
    w.push(addiu(S1, S1, 1)); // j++
    w.push(j(va(seruloop)));
    w.push(nop());
    let nextslot = w.len();
    w.push(addiu(S0, S0, 1)); // slot++
    w.push(j(va(slotloop)));
    w.push(nop());
    w[gskip] = j(va(nextslot)); // give-owned -> skip this owner
    let done = w.len();
    w[done_b] = beq(T0, ZERO, (done as i32 - (done_b as i32 + 1)) as i16);
    w[nextslot_b] = beq(T0, ZERO, (nextslot as i32 - (nextslot_b as i32 + 1)) as i16);
    w[skip_b] = bne(T4, S3, (skip as i32 - (skip_b as i32 + 1)) as i16);

    // --- input + cursor: browse the owner lines (NOT the header), or pick Yes/No ---
    // line count N = (s2 - ROW_FIRST_Y) >> 4 (kept in t0); line cursor in t1.
    w.push(addiu(T0, S2, (0u16).wrapping_sub(ROW_FIRST_Y)));
    w.push(srl(T0, T0, 4)); // t0 = N
    w.push(lui(AT, hi(TRADE_CURSOR_VA)));
    w.push(lw(T1, AT, lo(TRADE_CURSOR_VA))); // t1 = line cursor
    w.push(lui(AT, hi(PAD_CUR_VA)));
    w.push(lw(T2, AT, lo(PAD_CUR_VA))); // t2 = pad cur
    w.push(lui(AT, hi(TRADE_PAD_PREV_VA)));
    w.push(lw(T3, AT, lo(TRADE_PAD_PREV_VA))); // t3 = pad prev (last frame)
    w.push(lui(AT, hi(TRADE_PAD_PREV_VA)));
    w.push(sw(T2, AT, lo(TRADE_PAD_PREV_VA))); // prev = cur (fills the t3 load-delay)

    // Run `body` only on a fresh press of `mask` (held now in t2, not last frame in
    // t3). Uses t5/t6 as scratch (free for `body` after the guard branches).
    let edge = |w: &mut Vec<u32>, mask: u16, body: &dyn Fn(&mut Vec<u32>)| {
        w.push(andi(T5, T2, mask));
        let b1 = w.len();
        w.push(0);
        w.push(nop());
        w.push(andi(T6, T3, mask));
        let b2 = w.len();
        w.push(0);
        w.push(nop());
        body(w);
        let done = w.len();
        w[b1] = beq(T5, ZERO, (done as i32 - (b1 as i32 + 1)) as i16);
        w[b2] = bne(T6, ZERO, (done as i32 - (b2 as i32 + 1)) as i16);
    };

    // confirm sub-state -> t7; branch to .browse when 0.
    w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
    w.push(lw(T7, AT, lo(TRADE_CONFIRM_VA)));
    w.push(nop());
    let browse_b = w.len();
    w.push(0); // beq t7,zero,.browse (patched)
    w.push(nop());

    // === CONFIRM input: Left=Yes(0) / Right=No(1); ✕ or ○ leaves the prompt ===
    edge(&mut w, PAD_LEFT_MASK, &|w| {
        w.push(lui(AT, hi(TRADE_YESNO_VA)));
        w.push(sw(ZERO, AT, lo(TRADE_YESNO_VA))); // yesno = 0 (Yes)
    });
    edge(&mut w, PAD_RIGHT_MASK, &|w| {
        w.push(addiu(T6, ZERO, 1));
        w.push(lui(AT, hi(TRADE_YESNO_VA)));
        w.push(sw(T6, AT, lo(TRADE_YESNO_VA))); // yesno = 1 (No)
    });
    // ✕ resolves: on Yes, perform the swap on the selected owner; ○ cancels. Either
    // way clear the confirm sub-state -> back to browsing.
    edge(&mut w, PAD_CONFIRM_MASK, &|w| {
        // skip the swap unless yesno == 0 (Yes)
        w.push(lui(AT, hi(TRADE_YESNO_VA)));
        w.push(lw(T4, AT, lo(TRADE_YESNO_VA)));
        w.push(nop()); // load-delay
        let noswap_b = w.len();
        w.push(0); // bne t4,zero,.noswap (No)
        w.push(nop());
        // SWAP: on the stashed selected owner, replace the wanted seru with the
        // give-back at give_level (s6). We already filtered owners who own the
        // give-back, so this is always the in-place replace (count unchanged), matching
        // engine apply_trade. ids[j] @ +0x13D, levels[j] @ +0x161 are bytes.
        w.push(lui(AT, hi(TRADE_SEL_BASE_VA)));
        w.push(lw(T4, AT, lo(TRADE_SEL_BASE_VA))); // t4 = owner record base
        w.push(lui(AT, hi(TRADE_SEL_J_VA)));
        w.push(lw(T5, AT, lo(TRADE_SEL_J_VA))); // t5 = want index
        w.push(lui(AT, hi(TRADE_GIVE_ID_VA)));
        w.push(lw(T6, AT, lo(TRADE_GIVE_ID_VA))); // t6 = give id
        w.push(addu(T4, T4, T5)); // base + j
        w.push(sb(T6, T4, lo(SERU_IDS_VA - CHAR_RECORD_BASE))); // ids[j] = give id (+0x13D)
        w.push(sb(S6, T4, SERU_LEVELS_OFFSET)); // levels[j] = give_level (+0x161)
        let noswap = w.len();
        w[noswap_b] = bne(T4, ZERO, (noswap as i32 - (noswap_b as i32 + 1)) as i16);
        // clear the confirm sub-state (both Yes and No return to browsing)
        w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
        w.push(sw(ZERO, AT, lo(TRADE_CONFIRM_VA)));
    });
    edge(&mut w, HANDLER_CANCEL_MASK, &|w| {
        w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
        w.push(sw(ZERO, AT, lo(TRADE_CONFIRM_VA)));
    });
    let conf_done_b = w.len();
    w.push(0); // j .after_input (patched, absolute)
    w.push(nop());

    // === BROWSE input (.browse): Up/Down move the line cursor; ✕ enters confirm;
    // ○ exits the trade screen ===
    let browse = w.len();
    w[browse_b] = beq(T7, ZERO, (browse as i32 - (browse_b as i32 + 1)) as i16);
    edge(&mut w, PAD_UP_MASK, &|w| {
        w.push(addiu(T1, T1, 0xFFFF)); // cursor-- (-1)
    });
    edge(&mut w, PAD_DOWN_MASK, &|w| {
        w.push(addiu(T1, T1, 1)); // cursor++
    });
    edge(&mut w, PAD_CONFIRM_MASK, &|w| {
        // only enter confirm if there's a line to confirm (N > 0, kept in t0)
        let sk = w.len();
        w.push(0); // blez t0,.skipenter
        w.push(nop());
        w.push(addiu(T6, ZERO, 1));
        w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
        w.push(sw(T6, AT, lo(TRADE_CONFIRM_VA))); // enter confirm
        let done = w.len();
        w[sk] = blez(T0, (done as i32 - (sk as i32 + 1)) as i16);
    });
    // ○ edge -> exit (jump to .do_exit, far ahead).
    w.push(andi(T5, T2, HANDLER_CANCEL_MASK));
    let ox_b = w.len();
    w.push(0); // beq t5,zero,.noox
    w.push(nop());
    w.push(andi(T6, T3, HANDLER_CANCEL_MASK));
    let ox_b2 = w.len();
    w.push(0); // bne t6,zero,.noox
    w.push(nop());
    let exit_jb = w.len();
    w.push(0); // j .do_exit (patched, absolute)
    w.push(nop());
    let noox = w.len();
    w[ox_b] = beq(T5, ZERO, (noox as i32 - (ox_b as i32 + 1)) as i16);
    w[ox_b2] = bne(T6, ZERO, (noox as i32 - (ox_b2 as i32 + 1)) as i16);

    // .after_input: clamp the line cursor to [0, N) and store it.
    let after_input = w.len();
    w[conf_done_b] = j(va(after_input));
    let low_b = w.len();
    w.push(0); // bgez t1,.nolow
    w.push(nop());
    w.push(addu(T1, ZERO, ZERO)); // cursor = 0
    let nolow = w.len();
    w[low_b] = bgez(T1, (nolow as i32 - (low_b as i32 + 1)) as i16);
    w.push(slt(T5, T1, T0)); // cursor < N ?
    let high_b = w.len();
    w.push(0); // bne t5,zero,.nohigh
    w.push(nop());
    w.push(addiu(T1, T0, 0xFFFF)); // cursor = N-1
    let nohigh = w.len();
    w[high_b] = bne(T5, ZERO, (nohigh as i32 - (high_b as i32 + 1)) as i16);
    w.push(lui(AT, hi(TRADE_CURSOR_VA)));
    w.push(sw(T1, AT, lo(TRADE_CURSOR_VA)));
    // line cursor sprite at the selected line (skip if no lines).
    let hl_b = w.len();
    w.push(0); // blez t0,.nohl (N <= 0)
    w.push(nop());
    w.push(sll(T5, T1, 4)); // cursor * ROW_STEP_Y
    w.push(addiu(T5, T5, ROW_FIRST_Y)); // + first row y
    w.push(addiu(A0, ZERO, 0)); // slot 0 (standard menu cursor)
    w.push(addiu(A1, ZERO, 1)); // mode 1 (animated)
    w.push(addiu(A2, S7, CURSOR_X)); // x + slide offset
    w.push(addu(A3, ZERO, T5)); // y
    w.push(jal(CURSOR_DRAW_FN));
    w.push(nop());
    let nohl = w.len();
    w[hl_b] = blez(T0, (nohl as i32 - (hl_b as i32 + 1)) as i16);

    // confirm prompt (only in the confirm sub-state): "@Trade?" + Yes / No + cursor.
    w.push(lui(AT, hi(TRADE_CONFIRM_VA)));
    w.push(lw(T7, AT, lo(TRADE_CONFIRM_VA))); // reload (it may have just changed)
    w.push(nop());
    let prompt_b = w.len();
    w.push(0); // beq t7,zero,.noprompt (patched)
    w.push(nop());
    // "@Trade?" at (COL_HEADER_X + slide, PROMPT_Y)
    w.push(lui(A0, hi(CONFIRM_PROMPT_STR_VA)));
    w.push(addiu(A0, A0, lo(CONFIRM_PROMPT_STR_VA)));
    w.push(addiu(V0, ZERO, PROMPT_Y));
    w.push(sw(V0, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, COL_HEADER_X));
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // "@Yes" at (YES_X + slide, CHOICE_Y)
    w.push(lui(A0, hi(CONFIRM_YES_STR_VA)));
    w.push(addiu(A0, A0, lo(CONFIRM_YES_STR_VA)));
    w.push(addiu(V0, ZERO, CHOICE_Y));
    w.push(sw(V0, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, YES_X));
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // "@No" at (NO_X + slide, CHOICE_Y)
    w.push(lui(A0, hi(CONFIRM_NO_STR_VA)));
    w.push(addiu(A0, A0, lo(CONFIRM_NO_STR_VA)));
    w.push(addiu(V0, ZERO, CHOICE_Y));
    w.push(sw(V0, SP, 0x10));
    w.push(addiu(A1, ZERO, 0));
    w.push(addiu(A2, ZERO, 0));
    w.push(addiu(A3, S7, NO_X));
    w.push(jal(TEXT_DRAW_FN));
    w.push(nop());
    // yes/no cursor: x = (yesno ? NO_X : YES_X) - CHOICE_CURSOR_DX + slide.
    w.push(lui(AT, hi(TRADE_YESNO_VA)));
    w.push(lw(T4, AT, lo(TRADE_YESNO_VA)));
    w.push(addiu(A2, S7, YES_X - CHOICE_CURSOR_DX)); // default Yes (fills load-delay)
    let ynx_b = w.len();
    w.push(0); // beq t4,zero,.yesx
    w.push(nop());
    w.push(addiu(A2, S7, NO_X - CHOICE_CURSOR_DX)); // No
    let yesx = w.len();
    w[ynx_b] = beq(T4, ZERO, (yesx as i32 - (ynx_b as i32 + 1)) as i16);
    w.push(addiu(A0, ZERO, 0));
    w.push(addiu(A1, ZERO, 1));
    w.push(addiu(A3, ZERO, CHOICE_Y));
    w.push(jal(CURSOR_DRAW_FN));
    w.push(nop());
    let noprompt = w.len();
    w[prompt_b] = beq(T7, ZERO, (noprompt as i32 - (prompt_b as i32 + 1)) as i16);

    // native window box LAST (behind the text). Force the standard skin first
    // (gp[+0x14c]) so it doesn't inherit the brown name-plate skin after the slide.
    w.push(addiu(T0, ZERO, WINDOW_SKIN_STD));
    w.push(sw(T0, GP, WINDOW_SKIN_OFF));
    w.push(addiu(A0, S7, BOX_X)); // x + slide
    w.push(addiu(A1, ZERO, BOX_Y));
    w.push(addiu(A2, ZERO, BOX_W));
    w.push(addiu(A3, ZERO, BOX_H_PX));
    w.push(jal(BOX_FN));
    w.push(nop());
    let fin_jb = w.len();
    w.push(0); // j .finalize (patched, absolute)
    w.push(nop());

    // .do_exit (○ in browse): clear TRADE_ACTIVE + slide the picker windows back in.
    let do_exit = w.len();
    w[exit_jb] = j(va(do_exit));
    w.push(lui(AT, hi(TRADE_ACTIVE_VA)));
    w.push(sw(ZERO, AT, lo(TRADE_ACTIVE_VA)));
    w.push(lui(A0, hi(SLIDE_OPEN_SCRIPT_VA)));
    w.push(addiu(A0, A0, lo(SLIDE_OPEN_SCRIPT_VA)));
    w.push(jal(WIDGET_VM_FN)); // FUN_801d6628(&DAT_801e4e38) - slide back in
    w.push(nop());

    // .finalize: per-frame finalize tail + epilogue.
    let finalize = w.len();
    w[fin_jb] = j(va(finalize));
    w.push(jal(FINALIZE_FN));
    w.push(nop());
    w.push(lw(RA, SP, 0x2C));
    w.push(lw(S0, SP, 0x14));
    w.push(lw(S1, SP, 0x18));
    w.push(lw(S2, SP, 0x1C));
    w.push(lw(S3, SP, 0x20));
    w.push(lw(S4, SP, 0x24));
    w.push(lw(S5, SP, 0x28));
    w.push(lw(S7, SP, 0x30));
    w.push(lw(S6, SP, 0x34));
    w.push(addiu(SP, SP, 0x38));
    w.push(jr(RA)); // return to the menu tick (FUN_801dc6b4)
    w.push(nop());
    debug_assert!(
        TRADE_HANDLER_VA + (w.len() as u32) * 4 <= ENTRY_STUB_VA,
        "trade handler overruns into the entry stub (0899 run-C layout)"
    );
    w
}
/// Words planted at [`ROW4_DETOUR_VA`]: `j ROW4_STUB_VA` then `nop`.
pub fn row4_detour_words() -> [u32; 2] {
    [j(ROW4_STUB_VA), nop()]
}
/// Words planted at [`ROW4_DETOUR_VA`]: `j ROW4_STUB_VA` then `nop`.
///
/// Draws + highlights the fourth "Trade" picker row from INSIDE the renderer body
/// (right after the Quit text draw), so the glyphs link at the same OT depth as
/// Buy/Sell/Quit - in front of the box background. At the detour site `s0` = the
/// Quit row's y, `s2` = the text x, `s1` = the picker context (`param_1`); the
/// native callees preserve them, and the function's `ra` was already stack-saved
/// at the prologue, so the stub may `jal` freely. The Trade row sits at `s0+0xe`,
/// highlighted (mirroring the Quit-row logic) when the cursor is on index 3. Then
/// it replays the two displaced words and rejoins the Quit-highlight at
/// [`ROW4_RETURN_VA`].
pub fn assemble_row4_draw_stub() -> Vec<u32> {
    assemble_row4_draw_stub_str(TRADE_STR_VA)
}
/// As [`assemble_row4_draw_stub`], with the 4th-row label address selectable. The
/// standalone native-row build draws "@Trade" ([`TRADE_STR_VA`]); the full in-shop
/// build reorders to Buy/Sell/Trade/Quit by swapping the body's row-2 string to
/// "@Trade" and drawing "@Quit" ([`QUIT_STR_VA`]) here at row 3.
pub fn assemble_row4_draw_stub_str(str_va: u32) -> Vec<u32> {
    // draw the row-4 label at (a3 = x = s2, y = s0 + 0xe), matching the body's call.
    let mut w: Vec<u32> = vec![
        addiu(T0, S0, 0x0e),       // t0 = 4th-row y = Quit y + 0xe
        sw(T0, SP, 0x10),          // 5th arg (y) on the stack
        addiu(A3, S2, 0),          // a3 = x (text x, = body s2)
        addiu(A1, ZERO, 0),        // a1 = 0
        addiu(A2, ZERO, 0),        // a2 = 0
        lui(A0, hi(str_va)),       //  \ a0 = &label
        addiu(A0, A0, lo(str_va)), //  /
        jal(TEXT_DRAW_FN),
        nop(),
        // highlight row 3 (mirror of the Quit-row highlight with index 3, y=s0+0xe)
        lui(V0, hi(CURSOR_VA)),
        lw(A1, V0, lo(CURSOR_VA)), // a1 = DAT_801e46bc
        nop(),                     // R3000 load-delay slot (a1 not ready until now)
        andi(V0, A1, 0x4000),      // ✕ cancel?
    ];
    let b_cancel = w.len();
    w.push(0); // bne v0,zero,.done (patched)
    w.push(nop());
    w.push(andi(V0, A1, 0x2000)); // ○ confirm?
    let b_nav = w.len();
    w.push(0); // beq v0,zero,.nav (patched)
    w.push(addiu(A0, ZERO, 0)); // (delay) a0 = 0 (slot)
    // confirm branch: a1 = ((DAT & 0x1000)==0) << 2
    w.push(andi(A1, A1, 0x1000));
    w.push(sltiu(A1, A1, 1));
    let j_hl = w.len();
    w.push(0); // j .hl (patched)
    w.push(sll(A1, A1, 2)); // (delay)
    // .nav: if cursor != 3 -> .done; else a1 = (DAT>>0xc ^ 1) & 1
    let nav = w.len();
    w.push(andi(V1, A1, 0xfff));
    w.push(addiu(V0, ZERO, 3));
    let b_skip = w.len();
    w.push(0); // bne v1,v0,.done (patched)
    w.push(srl(A1, A1, 0xc)); // (delay)
    w.push(xori(A1, A1, 1));
    w.push(andi(A1, A1, 1));
    // .hl: FUN_8002b994(0, a1=mode, a2=x, a3=y4)
    let hl = w.len();
    w.push(lh(A2, S1, 0x0a)); // a2 = cursor x (raw, like the Quit highlight)
    w.push(addiu(A3, S0, 0x0e)); // a3 = y4 = s0 + 0xe
    w.push(jal(HIGHLIGHT_FN));
    w.push(nop());
    // .done: replay the displaced words, rejoin the Quit-highlight at +8.
    let done = w.len();
    w.push(ROW4_DISPLACED[0]); // lui v0,0x801e
    w.push(ROW4_DISPLACED[1]); // lw a1,0x46bc(v0)
    w.push(j(ROW4_RETURN_VA));
    w.push(nop());
    // resolve branches / jump
    w[b_cancel] = bne(V0, ZERO, (done as i32 - (b_cancel as i32 + 1)) as i16);
    w[b_nav] = beq(V0, ZERO, (nav as i32 - (b_nav as i32 + 1)) as i16);
    w[b_skip] = bne(V1, V0, (done as i32 - (b_skip as i32 + 1)) as i16);
    w[j_hl] = j(ROW4_STUB_VA + (hl as u32) * 4);
    debug_assert!(
        (w.len() as u32) * 4 <= (TRADE_STR_VA - ROW4_STUB_VA),
        "row-4 stub overruns the reserved code window before TRADE_STR_VA"
    );
    w
}
/// Stub for the shop-picker quiet-frame trigger ([`PICKER_TRIGGER_VA`]).
///
/// Runs at the head of `FUN_801d4868` each frame the Buy/Sell/Quit choice is on
/// screen. On SQUARE it arms the mode-24 warp (sub-id + mode 0x18 + the
/// minigame-active sysflag), mirroring the op-0x3E door-warp minus the casino
/// housekeeping zeros; `ra`/`a0` (live at entry, not yet saved by the prologue)
/// are preserved across the sysflag `jal`. Then it replays the two displaced
/// prologue words and rejoins the picker at [`PICKER_RETURN_VA`]. Fixed at 24
/// words (0x60 bytes) to fit the gap slot before the redirect.
pub fn assemble_picker_trade_detour_stub(sub_id: u16) -> Vec<u32> {
    let mut w: Vec<u32> = Vec::new();
    // if ((pad & SQUARE) == 0) -> .replay
    w.push(lui(AT, hi(PAD_CUR_VA)));
    w.push(lw(T0, AT, lo(PAD_CUR_VA)));
    w.push(andi(T1, T0, PICKER_TRIGGER_MASK));
    let skipb = w.len();
    w.push(0); // beq T1,ZERO,.replay (patched)
    w.push(nop());
    // SQUARE held: arm the warp. Preserve ra + a0 across the sysflag jal (sp here
    // is the caller's; the prologue's own -0x28 happens later in .replay).
    w.push(addiu(SP, SP, 0xFFF8)); // sp -= 8
    w.push(sw(RA, SP, 0));
    w.push(sw(A0, SP, 4));
    w.push(addiu(V0, ZERO, sub_id)); //  \ _DAT_8007BA34 = sub_id
    w.push(lui(AT, hi(SUBID_VA))); //  |
    w.push(sh(V0, AT, lo(SUBID_VA))); //  /
    w.push(addiu(V0, ZERO, MODE_OTHER_INIT)); //  \ _DAT_8007B83C = 0x18 (mode 24)
    w.push(lui(AT, hi(MODE_INDEX_VA))); //  |
    w.push(sh(V0, AT, lo(MODE_INDEX_VA))); //  /
    w.push(addiu(A0, ZERO, SYSFLAG_WARP_ARG)); //  a0 = 0xE
    w.push(jal(SYSFLAG_SET_FN)); //  func_0x8003CE08(0xE)
    w.push(nop()); //  (delay)
    w.push(lw(RA, SP, 0));
    w.push(lw(A0, SP, 4));
    w.push(addiu(SP, SP, 8)); // sp += 8
    // .replay: the picker prologue head, then back into the body at +8.
    let replay = w.len();
    w.push(PICKER_DISPLACED[0]); // addiu sp,sp,-0x28
    w.push(PICKER_DISPLACED[1]); // sw s1,0x1c(sp)
    w.push(j(PICKER_RETURN_VA));
    w.push(nop()); // (delay)
    w[skipb] = beq(T1, ZERO, (replay as i32 - (skipb as i32 + 1)) as i16);
    debug_assert_eq!(w.len(), 24, "picker stub must be 24 words (0x60 bytes)");
    w
}
