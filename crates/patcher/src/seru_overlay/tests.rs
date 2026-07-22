use super::*;

#[test]
fn sentinel_overlay_writes_the_sentinel() {
    let w = assemble_sentinel_overlay();
    assert_eq!(w.len(), 6);
    // v0 = 0x5E2D7ADE via lui+ori
    assert_eq!(w[0], lui(V0, 0x5E2D));
    assert_eq!(w[1], ori(V0, V0, 0x7ADE));
    // store to SENTINEL_ADDR (0x8007AF20): hi corrects for the +0x20 lo.
    assert_eq!(w[2], lui(V1, hi(SENTINEL_ADDR)));
    assert_eq!(w[3], sw(V0, V1, lo(SENTINEL_ADDR)));
    assert_eq!(w[4], jr(RA));
    assert_eq!(w[5], 0);
}

#[test]
fn loader_stub_calls_the_reader_then_the_overlay() {
    let lba = 0x0004_2A17u32;
    let sectors = 1u16;
    let displaced = [0x3c03_801du32, 0x2464_9070u32];
    let return_va = 0x801E_5A18u32;
    let s = assemble_loader_stub(lba, sectors, displaced, return_va);
    assert_eq!(s.len(), 18);

    // a0 = sectors, a1 = lba (lui+ori), a2 = DEST (lui+ori).
    assert_eq!(s[0], addiu(A0, ZERO, sectors));
    assert_eq!(s[1], lui(A1, 0x0004));
    assert_eq!(s[2], ori(A1, A1, 0x2A17));
    assert_eq!(s[3], lui(A2, imm_hi(DEST)));
    assert_eq!(s[4], ori(A2, A2, imm_lo(DEST)));

    // jal lands on the loader function.
    assert_eq!((s[5] & 0x03ff_ffff) << 2, LOADER_FN & 0x0fff_ffff);
    // FlushCache: li t2,0xA0 ; jalr t2 ; (delay) li t1,0x44.
    assert_eq!(s[7], addiu(T2, ZERO, BIOS_DISPATCH_A));
    assert_eq!(s[8], jalr(T2));
    assert_eq!(s[9], addiu(T1, ZERO, FLUSH_CACHE_FN));
    // jalr t0 calls the loaded overlay at DEST.
    assert_eq!(s[10], lui(T0, imm_hi(DEST)));
    assert_eq!(s[12], jalr(T0));
    // displaced pair replayed, then j back to the hook join.
    assert_eq!(s[14], displaced[0]);
    assert_eq!(s[15], displaced[1]);
    assert_eq!((s[16] & 0x03ff_ffff) << 2, return_va & 0x0fff_ffff);
}

#[test]
fn shop_stub_gates_on_sub_op_and_replays() {
    let s = assemble_shop_loader_stub(0x0004_2A17, 1);
    assert_eq!(s.len(), 21);
    // Gate: lbu t3,0(s6) ; bne t3,zero,->replay.
    assert_eq!(s[0], lbu(T3, S6, 0));
    assert_eq!(s[1] >> 26, 0x05, "bne opcode");
    // bne target = replay block (idx 17): off (words) = 17 - (1+1) = 15.
    let off = (s[1] & 0xffff) as i16;
    let target = (1 + 1) + off as i32; // branch idx+1 + off
    assert_eq!(target, 17, "bne skips to the replay block");
    // loader call + FlushCache + overlay call present.
    assert_eq!((s[8] & 0x03ff_ffff) << 2, LOADER_FN & 0x0fff_ffff);
    assert_eq!(s[10], addiu(T2, ZERO, BIOS_DISPATCH_A));
    assert_eq!(s[11], jalr(T2));
    assert_eq!(s[15], jalr(T0));
    // Replay the exact displaced pair, then jump back.
    assert_eq!(s[17], SHOP_DISPLACED[0]);
    assert_eq!(s[18], SHOP_DISPLACED[1]);
    assert_eq!((s[19] & 0x03ff_ffff) << 2, SHOP_RETURN_VA & 0x0fff_ffff);
    // Fits the gap window below the config blob.
    assert!(STUB_VA + (s.len() as u32) * 4 <= 0x8007_AF00);
}

#[test]
fn ungated_stub_neuters_the_gate_branch_only() {
    let g = assemble_shop_loader_stub_gated(0x0004_2A17, 1, true);
    let u = assemble_shop_loader_stub_gated(0x0004_2A17, 1, false);
    // Same length + layout; only the gate branch (idx 1) differs.
    assert_eq!(g.len(), u.len());
    for i in 0..g.len() {
        if i == 1 {
            continue;
        }
        assert_eq!(g[i], u[i], "word {i} must be identical");
    }
    // Gated: bne t3,zero (rs = T3). Ungated: bne zero,zero (rs = ZERO, never taken).
    assert_eq!((g[1] >> 21) & 0x1f, T3);
    assert_eq!((u[1] >> 21) & 0x1f, ZERO);
    assert_eq!(
        u[1] >> 26,
        0x05,
        "still a bne (never-taken), so layout holds"
    );
    // Same branch displacement either way.
    assert_eq!(g[1] & 0xffff, u[1] & 0xffff);
}

#[test]
fn warp_trigger_mirrors_the_op_0x3e_idiom() {
    let sub_id = 7u16;
    let s = assemble_warp_trigger_stub(sub_id, false);
    assert_eq!(s.len(), 17);
    // Two housekeeping zeroing stores.
    assert_eq!(s[0], lui(AT, hi(WARP_HOUSEKEEP_VA)));
    assert_eq!(s[1], sw(ZERO, AT, lo(WARP_HOUSEKEEP_VA)));
    assert_eq!(s[2], lui(AT, hi(WINNINGS_VA)));
    assert_eq!(s[3], sw(ZERO, AT, lo(WINNINGS_VA)));
    // sub-id written before the mode (mirrors op-0x3E ordering).
    assert_eq!(s[4], addiu(V0, ZERO, sub_id));
    assert_eq!(s[6], sh(V0, AT, lo(SUBID_VA)));
    // mode index <- 0x18 (mode 24 OTHER INIT).
    assert_eq!(s[7], addiu(V0, ZERO, MODE_OTHER_INIT));
    assert_eq!(s[9], sh(V0, AT, lo(MODE_INDEX_VA)));
    // SysFlag setter call with a0 = 0xE.
    assert_eq!(s[10], addiu(A0, ZERO, SYSFLAG_WARP_ARG));
    assert_eq!((s[11] & 0x03ff_ffff) << 2, SYSFLAG_SET_FN & 0x0fff_ffff);
    // Replay the op-0x49 displaced pair, then j back to the dispatcher.
    assert_eq!(s[13], SHOP_DISPLACED[0]);
    assert_eq!(s[14], SHOP_DISPLACED[1]);
    assert_eq!((s[15] & 0x03ff_ffff) << 2, SHOP_RETURN_VA & 0x0fff_ffff);
    // Mode 24 (not 25): we request INIT, which loads + hands off to RUN itself.
    assert_eq!(MODE_OTHER_INIT, 0x18);
    // Fits the gap free window below the config blob at 0x8007AF00.
    assert!(STUB_VA + (s.len() as u32) * 4 <= 0x8007_AF00);
    // The warp globals resolve to the addresses pinned from overlay_0897.
    assert_eq!(MODE_INDEX_VA, 0x8007_B83C);
    assert_eq!(SUBID_VA, 0x8007_BA34);
    assert_eq!(WINNINGS_VA, 0x8008_4440);
    assert_eq!(WARP_HOUSEKEEP_VA, 0x8007_BAC0);
}

#[test]
fn warp_trigger_fire_once_guard() {
    let s = assemble_warp_trigger_stub(WARP_SUBID, true);
    // Prefix: read the fired flag, branch past the warp if already set.
    assert_eq!(s[0], lui(AT, hi(WARP_FIRED_VA)));
    assert_eq!(s[1], lhu(V0, AT, lo(WARP_FIRED_VA)));
    assert_eq!(s[2] >> 26, 0x05, "bne (skip warp if fired)");
    assert_eq!(s[4], addiu(V0, ZERO, 1), "set the fired flag");
    assert_eq!(s[6], sh(V0, AT, lo(WARP_FIRED_VA)));
    // The bne targets .replay: replay is the SHOP_DISPLACED pair near the end.
    let off = s[2] as u16 as i16 as i32;
    let target = 2 + 1 + off;
    assert_eq!(
        s[target as usize], SHOP_DISPLACED[0],
        "bne -> .replay (skip warp)"
    );
    // The warp arm still sets mode 0x18 and the sub-id.
    assert!(s.contains(&addiu(V0, ZERO, MODE_OTHER_INIT)));
    assert!(s.contains(&addiu(V0, ZERO, WARP_SUBID)));
    // Fire-once trigger + redirect both fit the gap window, no overlap.
    assert!(WARP_TRIGGER_VA + (s.len() as u32) * 4 <= WARP_REDIRECT_VA);
}

#[test]
fn dead_mode_trigger_and_loader_layout() {
    let trig = assemble_mode_request_trigger();
    let load = assemble_mode_init_loader_stub(0x0004_2A17, 1);
    // Trigger: stash the interrupted mode, request the dead mode, replay, return.
    assert_eq!(trig[1], lhu(V0, AT, lo(MODE_INDEX_VA)));
    assert_eq!(trig[3], sh(V0, AT, lo(ORIGIN_MODE_VA)));
    assert_eq!(trig[4], addiu(V0, ZERO, DEAD_MODE_INDEX));
    assert_eq!(trig[6], sh(V0, AT, lo(MODE_INDEX_VA)));
    assert_eq!(trig[7], SHOP_DISPLACED[0]);
    assert_eq!(trig[8], SHOP_DISPLACED[1]);
    assert_eq!((trig[9] & 0x03ff_ffff) << 2, SHOP_RETURN_VA & 0x0fff_ffff);
    // The trigger fits below the loader (no overlap at MODE_INIT_VA).
    assert!(TRIGGER_VA + (trig.len() as u32) * 4 <= MODE_INIT_VA);
    // Loader: sp frame saves ra; loads via FUN_8005E4D4; FlushCache; runs the
    // overlay; restores the stashed origin mode; restores ra; jr ra.
    assert_eq!(load[0], addiu(SP, SP, 0xFFF8));
    assert_eq!(load[1], sw(RA, SP, 4));
    assert_eq!((load[7] & 0x03ff_ffff) << 2, LOADER_FN & 0x0fff_ffff);
    assert_eq!(load[10], jalr(T2));
    assert_eq!(load[14], jalr(T0));
    assert_eq!(load[17], lhu(V0, AT, lo(ORIGIN_MODE_VA)));
    assert_eq!(load[19], sh(V0, AT, lo(MODE_INDEX_VA)));
    assert_eq!(load[20], lw(RA, SP, 4));
    assert_eq!(load[22], jr(RA));
    // Loader fits the gap window below the config blob at 0x8007AF00.
    assert!(MODE_INIT_VA + (load.len() as u32) * 4 <= 0x8007_AF00);
    // The dead mode's handler word lands inside the mode table.
    assert_eq!(dead_mode_handler_va(), MODE_TABLE_VA + 10 * 24 + 0x10);
}

#[test]
fn draw_overlay_layout() {
    let o = assemble_draw_overlay();
    // INIT (offset 0): hand off to mode 13, then return.
    assert_eq!(o[0], addiu(V0, ZERO, MAPDISP_MODE_INDEX));
    assert_eq!(o[2], sh(V0, AT, lo(MODE_INDEX_VA)));
    assert_eq!(o[3], jr(RA));
    // The string lives at word 5 and reads "SERU" in the first word.
    assert_eq!(o[5], u32::from_le_bytes(*b"SERU"));
    // TICK starts at offset 0x38 (word 14).
    let tick = (SLOT_A_TICK_OFFSET / 4) as usize;
    assert_eq!(tick, 14);
    assert_eq!(
        o[tick],
        addiu(SP, SP, 0xFFD8),
        "tick begins with the 0x28 sp frame (saves s0-s2 + ra)"
    );
    // The tick refreshes the pad, draws the box + native text + the seru list,
    // and exits via the return warp. (Checked by presence to stay robust to
    // layout shifts as the UI grows.)
    let body = &o[tick..];
    assert!(body.contains(&jal(PAD_POLL_FN)), "refreshes the pad");
    assert!(body.contains(&jal(BOX_FN)), "draws the native window box");
    assert!(body.contains(&jal(TEXT_DRAW_FN)), "draws native text");
    assert!(body.contains(&jal(MODE24_RETURN_FN)), "exit -> return warp");
    // Native text passes its y arg on the stack at sp+0x10 (o32 5th arg).
    assert!(body.contains(&sw(V0, SP, 0x10)), "y passed on the stack");
    // The title string ptr (SLOT_A_BASE + 5*4) is loaded for the title draw.
    let str_va = SLOT_A_BASE + 5 * 4;
    assert!(body.contains(&lui(A0, hi(str_va))) && body.contains(&addiu(A0, A0, lo(str_va))));
    // The seru list reads the count + indexes the spell-name pointer table.
    assert!(
        body.contains(&lbu(S1, AT, lo(SERU_COUNT_VA))),
        "reads seru count"
    );
    assert!(
        body.contains(&lui(T7, hi(SERU_NAME_PTRS))),
        "indexes the name table"
    );
    assert!(body.contains(&slt(T0, S0, S1)), "loops i < count");
    // The draw redirect (call_return_warp=false) does NOT call the return warp;
    // the overlay INIT requests the persistent mode instead.
    let r_draw = assemble_warp_init_redirect_opts(0x0004_2A17, 1, false);
    assert!(
        !r_draw.contains(&jal(MODE24_RETURN_FN)),
        "draw redirect skips return-warp"
    );
    let r_sent = assemble_warp_init_redirect_opts(0x0004_2A17, 1, true);
    assert!(
        r_sent.contains(&jal(MODE24_RETURN_FN)),
        "sentinel redirect keeps return-warp"
    );
}

#[test]
fn trade_handler_renders_per_owner_offer() {
    let h = assemble_trade_handler();
    // Fits the 0899 run-C dead region it's embedded in, and is hosted in that
    // overlay (VA inside the 0899 image window), not the SCUS gap.
    let end = TRADE_HANDLER_VA + (h.len() as u32) * 4;
    assert!(end <= TRADE_HANDLER_END && TRADE_HANDLER_VA >= SLOT_A_BASE);
    // Refreshes the pad, draws native text + the level number + the window box.
    assert!(h.contains(&jal(PAD_POLL_FN)), "refreshes the pad");
    assert!(h.contains(&jal(TEXT_DRAW_FN)), "draws native text");
    assert!(h.contains(&jal(NUMBER_FN)), "draws the LVL number");
    assert!(h.contains(&jal(BOX_FN)), "draws the window box (last)");
    assert!(
        h.contains(&jal(FINALIZE_FN)),
        "replays the menu finalize tail"
    );
    // Indexes the precomputed bucket schedule (live build, demo off) and the
    // spell-name pointer table.
    if !SERU_DEMO_FORCE_WANT {
        assert!(h.contains(&lw(T0, AT, lo(PLAY_TIME_VA))), "reads play-time");
        assert!(h.contains(&divu(T0, T1)), "divides into a bucket");
        assert!(
            h.contains(&andi(T0, T0, BUCKET_INDEX_MASK)),
            "wraps to BUCKET_COUNT"
        );
        assert!(
            h.contains(&addiu(T1, T1, lo(BUCKET_TABLE_VA))),
            "indexes the bucket table"
        );
    }
    assert!(
        h.contains(&lui(T7, hi(SERU_NAME_PTRS))),
        "indexes the spell-name table"
    );
    // Scans the four party records: slot < 4, record stride 0x414, per-record
    // seru count + id array.
    assert!(
        h.contains(&slti(T0, S0, PARTY_SLOT_COUNT)),
        "loops party slots"
    );
    assert!(
        h.contains(&addiu(T1, ZERO, CHAR_RECORD_STRIDE)),
        "uses the record stride"
    );
    assert!(h.contains(&slt(T0, S1, S5)), "loops each owner's seru list");
    // A bne on (rs=id reg T4, rt=want reg S3) skips non-matching ids (the branch
    // displacement is patched, so match on opcode + register fields).
    assert!(
        h.iter()
            .any(|&x| x >> 26 == 0x05 && (x >> 21) & 0x1f == T4 && (x >> 16) & 0x1f == S3),
        "compares each owned id against the want"
    );
    // Owner name comes from the record name field (+0x2A7).
    assert!(
        h.contains(&addiu(A0, S4, RECORD_NAME_OFFSET)),
        "draws the owner name from the record"
    );
    // ○ (CANCEL) in browse exits: an andi against the cancel mask + the flag clear.
    assert!(
        h.iter()
            .any(|&x| x >> 26 == 0x0c && (x & 0xffff) == HANDLER_CANCEL_MASK as u32),
        "○ exit tests the cancel mask"
    );
    assert!(
        h.contains(&sw(ZERO, AT, lo(TRADE_ACTIVE_VA))),
        "clears the active flag"
    );
    // Confirm sub-state: enters on ✕, draws the prompt + Yes/No, navigates yes/no.
    assert!(
        h.iter()
            .any(|&x| x >> 26 == 0x0c && (x & 0xffff) == PAD_CONFIRM_MASK as u32),
        "✕ edge tested (enter/confirm)"
    );
    assert!(
        h.contains(&addiu(A0, A0, lo(CONFIRM_PROMPT_STR_VA))),
        "draws the @Trade? prompt"
    );
    assert!(
        h.contains(&addiu(A0, A0, lo(CONFIRM_YES_STR_VA)))
            && h.contains(&addiu(A0, A0, lo(CONFIRM_NO_STR_VA))),
        "draws Yes + No"
    );
    assert!(
        h.contains(&sw(ZERO, AT, lo(TRADE_CONFIRM_VA))),
        "✕/○ in confirm clears the sub-state"
    );
    // The give-filter scans the owner's list for the give-back id (skip if owned).
    assert!(
        h.contains(&lw(T0, AT, lo(TRADE_GIVE_ID_VA))),
        "reads the give id for the filter/swap"
    );
    // The swap writes the spell list: sb to ids (+0x13D) and levels (+0x161).
    assert!(
        h.contains(&sb(T6, T4, lo(SERU_IDS_VA - CHAR_RECORD_BASE))),
        "swap writes the give id into the id array"
    );
    assert!(
        h.contains(&sb(S6, T4, SERU_LEVELS_OFFSET)),
        "swap writes the give level into the level array"
    );
    // On exit it slides the picker windows back in via the widget VM.
    assert!(
        h.contains(&jal(WIDGET_VM_FN)),
        "exit slides the picker back in"
    );
    assert!(
        h.contains(&addiu(A0, A0, lo(SLIDE_OPEN_SCRIPT_VA))),
        "exit runs the open script"
    );
    // The dispatch stub slides the picker away (Sell script) on Trade confirm.
    let d = assemble_trade_dispatch_stub();
    assert!(d.contains(&jal(WIDGET_VM_FN)), "Trade confirm slides away");
    assert!(
        d.contains(&addiu(A0, A0, lo(SLIDE_AWAY_SCRIPT_VA))),
        "confirm runs the Sell slide-away script"
    );
}

#[test]
fn trade_0899_layout_is_disjoint() {
    // Every seru-trade piece lives in the 0899 run-C dead region; none touch the
    // SCUS gap. Assert they are all inside run-C and pairwise non-overlapping.
    let mut spans: Vec<(&str, u32, u32)> = vec![
        (
            "handler",
            TRADE_HANDLER_VA,
            (assemble_trade_handler().len() as u32) * 4,
        ),
        (
            "entry",
            ENTRY_STUB_VA,
            (assemble_trade_entry_stub().len() as u32) * 4,
        ),
        (
            "dispatch",
            TRADE_DISPATCH_STUB_VA,
            (assemble_trade_dispatch_stub().len() as u32) * 4,
        ),
        (
            "row4",
            ROW4_STUB_VA,
            (assemble_row4_draw_stub_str(QUIT_STR_VA).len() as u32) * 4,
        ),
        ("@trade", TRADE_STR_VA, TRADE_STR.len() as u32),
        ("title", TITLE_STR_VA, TITLE_STR.len() as u32),
        (
            "prompt",
            CONFIRM_PROMPT_STR_VA,
            CONFIRM_PROMPT_STR.len() as u32,
        ),
        ("yes", CONFIRM_YES_STR_VA, CONFIRM_YES_STR.len() as u32),
        ("no", CONFIRM_NO_STR_VA, CONFIRM_NO_STR.len() as u32),
        ("table", BUCKET_TABLE_VA, BUCKET_TABLE_LEN as u32),
        ("cells", TRADE_ACTIVE_VA, 0x24),
    ];
    // All within run-C (handler base .. end), and entirely in 0899 (>= SLOT_A_BASE).
    for &(name, va, len) in &spans {
        assert!(va >= SLOT_A_BASE, "{name} not in 0899");
        assert!(va >= TRADE_HANDLER_VA, "{name} below run-C");
        assert!(va + len <= TRADE_HANDLER_END, "{name} past run-C end");
    }
    spans.sort_by_key(|s| s.1);
    for w in spans.windows(2) {
        assert!(
            w[0].1 + w[0].2 <= w[1].1,
            "0899 overlap: {} [{:#x}+{:#x}) vs {} [{:#x})",
            w[0].0,
            w[0].1,
            w[0].2,
            w[1].0,
            w[1].1,
        );
    }
}

#[test]
fn warp_init_redirect_layout_and_branch() {
    let r = assemble_warp_init_redirect(0x0004_2A17, 1);
    assert_eq!(r.len(), 30);
    // sub-id read + compare against our sub-id.
    assert_eq!(r[1], lhu(V0, AT, lo(WARP_SUBID_VA)));
    assert_eq!(r[2], addiu(T0, ZERO, WARP_SUBID));
    // beq to the .ours block (index 9): offset words = 9 - (3+1) = 5.
    assert_eq!(r[3] >> 26, 0x04, "beq opcode");
    assert_eq!(
        (1 + 3 + (r[3] as u16 as i16) as i32),
        9,
        "beq targets .ours"
    );
    // .default: original loader, then rejoin into FUN_80025980.
    assert_eq!((r[5] & 0x03ff_ffff) << 2, OVERLAY_LOADER_A_FN & 0x0fff_ffff);
    assert_eq!((r[7] & 0x03ff_ffff) << 2, WARP_INIT_REJOIN_VA & 0x0fff_ffff);
    // .ours: load to slot A, FlushCache, run overlay, return-warp, epilogue.
    assert_eq!((r[14] & 0x03ff_ffff) << 2, LOADER_FN & 0x0fff_ffff);
    assert_eq!(r[12], lui(A2, imm_hi(SLOT_A_BASE)));
    assert_eq!(r[17], jalr(T2));
    assert_eq!(r[21], jalr(T3));
    assert_eq!((r[23] & 0x03ff_ffff) << 2, MODE24_RETURN_FN & 0x0fff_ffff);
    assert_eq!(r[25], lw(RA, SP, WARP_INIT_RA_OFF));
    assert_eq!(r[28], jr(RA));
    // Fits the gap window below the config blob.
    assert!(WARP_REDIRECT_VA + (r.len() as u32) * 4 <= 0x8007_AF00);
    // Detour words jump to the redirect; displaced guard matches the build.
    assert_eq!(
        (warp_init_detour_words()[0] & 0x03ff_ffff) << 2,
        WARP_REDIRECT_VA & 0x0fff_ffff
    );
    assert_eq!(WARP_INIT_REJOIN_VA, WARP_INIT_DETOUR_VA + 8);
}

#[test]
fn shop_hook_file_offset_is_in_the_field_overlay() {
    // The hook VA maps linearly from the overlay base.
    assert_eq!(SHOP_HOOK_VA - SHOP_OVERLAY_BASE, 0x12190);
    assert_eq!(SHOP_RETURN_VA, SHOP_HOOK_VA + 8);
}

#[test]
fn detour_jumps_to_the_stub() {
    let d = detour_words();
    assert_eq!((d[0] & 0x03ff_ffff) << 2, STUB_VA & 0x0fff_ffff);
    assert_eq!(d[1], 0);
}

#[test]
fn stub_fits_the_gap_free_window() {
    // The stub at 0x8007AE00 must stay below the config blob at 0x8007AF00.
    let s = assemble_loader_stub(0, 1, [0, 0], 0);
    let end = STUB_VA + (s.len() as u32) * 4;
    assert!(end <= 0x8007_AF00, "stub overruns into the config blob");
    // ...and the sentinel cell sits in the reserved tail after the blob.
    assert!((0x8007_AF18..0x8007_AF40).contains(&SENTINEL_ADDR));
}

#[test]
fn sectors_for_rounds_up() {
    assert_eq!(sectors_for(1), 1);
    assert_eq!(sectors_for(2048), 1);
    assert_eq!(sectors_for(2049), 2);
    assert_eq!(sectors_for(0), 0);
}
