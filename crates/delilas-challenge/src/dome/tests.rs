use super::battle_ai::{
    AI_ARM_JOIN_VA, AI_BLOCK_CAPACITY, AI_BLOCK_ORIG_HEAD, AI_BLOCK_ORIG_HEAD2, AI_HOOK_ORIG,
    AI_HOOK_PREV_ORIG, AI_PERIOD_MASK, AI_SPECIAL_PHASE, AI_STOCK_RESUME_VA, GI_ARM_CAPACITY,
    GI_ARM_ORIG_HEAD, GI_ARM_ORIG_WORD2, GI_ARM_VA, GI_DIVIDE_SPELL, GI_FLAGS_VA, GI_ROUND_WORD,
    GI_SPORE_GAS_MIN_TURN, GI_SPORE_GAS_SPELL, assemble_ai_block, assemble_gi_arm,
};
use super::routines::{
    assemble_reward_routine, assemble_routine, assemble_seat_routine, assemble_stream_map_routine,
    assemble_stream2_map_routine, descriptor_bytes, plan_routine_refresh, roster_bytes,
    template_ref_words,
};
use super::*;

#[test]
fn seed_word_decodes_to_course_3_round_0() {
    // The init decodes as `course = ((word-1) & 0xff) >> 4` (mask THEN
    // shift, per 0x801CEBD8/0x801CEBE0) and `round = (word-1) & 0xf`.
    let word = COURSE3_SEED_WORD;
    let course = ((word - 1) & 0xff) >> 4;
    let round = (word - 1) & 0xf;
    assert_eq!(course, 3);
    assert_eq!(round, 0);
    // Bit 0x100 = the dome-contest marker `FUN_80046A20` tests at battle
    // exit (`_DAT_8007BAC0 & 0x100`); without it a dome wipe game-overs
    // and a win exits to the field. Every retail seed carries it.
    assert_ne!(word & 0x100, 0, "seed word must carry the dome marker bit");
}

#[test]
fn seed_routine_shape() {
    let r = assemble_routine();
    // The cave layout is packed: the routine must keep its 14-word slot.
    assert_eq!(r.len(), 14);
    // Opens by loading the flag id and testing it.
    assert_eq!(r[0], addiu(A0, ZERO, COURSE_FLAG));
    assert_eq!(r[1], jal(FLAG_TEST_FUNC_VA));
    // beq at idx 3 skips to RELOAD (idx 12): off = 12 - (3+1) = 8.
    assert_eq!(r[3], beq(V0, ZERO, 8));
    // The stale-flag guard: reload the retail-seeded word and only seed
    // course 3 when it is the no-course default (`s2 == 1`); a retail
    // arm's own seed wins and the branch skips to the flag clear.
    assert_eq!(r[5], lw(T0, S1, WORD_OFF_FROM_S1));
    assert_eq!(r[7], bne(T0, S2, 2));
    assert_eq!(r[8], addiu(T0, ZERO, COURSE3_SEED_WORD as u16));
    assert_eq!(r[9], sw(T0, S1, WORD_OFF_FROM_S1));
    // The flag clear runs on BOTH set-flag paths (one-shot + scrub).
    assert_eq!(r[10], jal(FLAG_CLEAR_FUNC_VA));
    // Ends by returning to the decode, replaying the displaced `lw` in
    // the jump's delay slot.
    assert_eq!(r[12], j(SEED_RETURN_VA));
    assert_eq!(r[13], SEED_HOOK_ORIG);
}

#[test]
fn routine_refresh_plans_only_when_stale() {
    // A minimal PS-X EXE shell whose text segment covers the cave window.
    let mut scus = vec![0u8; 0x80000];
    scus[0..8].copy_from_slice(b"PS-X EXE");
    scus[0x18..0x1C].copy_from_slice(&0x8001_0000u32.to_le_bytes()); // t_addr
    scus[0x1C..0x20].copy_from_slice(&0x7_0000u32.to_le_bytes()); // t_size
    let off = legaia_game_tables::item_names::file_offset_for_va(&scus, ROUTINE_VA).unwrap();
    let cur: Vec<u8> = assemble_routine()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    // Stale bytes (an older routine) -> a same-size in-place rewrite.
    let w = plan_routine_refresh(&scus).unwrap().expect("stale plans");
    assert_eq!(w.off, off);
    assert_eq!(w.bytes, cur);
    // Current bytes -> no write.
    scus[off..off + cur.len()].copy_from_slice(&cur);
    assert!(plan_routine_refresh(&scus).unwrap().is_none());
}

#[test]
fn descriptor_points_at_roster() {
    let d = descriptor_bytes();
    assert_eq!(
        u32::from_le_bytes(d[..4].try_into().unwrap()),
        DELILAS_ROUNDS
    );
    assert_eq!(u32::from_le_bytes(d[4..].try_into().unwrap()), ROSTER_VA);
}

#[test]
fn roster_pairs_names_with_ids() {
    let b = roster_bytes();
    assert_eq!(b.len(), 16);
    for i in 0..2 {
        let name = u32::from_le_bytes(b[i * 8..i * 8 + 4].try_into().unwrap());
        let id = u32::from_le_bytes(b[i * 8 + 4..i * 8 + 8].try_into().unwrap());
        assert_eq!(name, DELILAS_NAME_PTRS[i]);
        assert_eq!(id, DELILAS_ROSTER_IDS[i]);
    }
    // Round 0 = Che (Lu joins via the seat detour), round 1 = Gi.
    assert_eq!(DELILAS_ROSTER_IDS[0], 163);
    assert_eq!(DELILAS_ROSTER_IDS[1], 162);
    assert_eq!(SECOND_SEAT_ID, 164);
}

#[test]
fn seat_routine_shape() {
    let r = assemble_seat_routine();
    assert_eq!(r.len(), 11);
    // One exact test: course word == 0x131 (course 3, round 0).
    assert_eq!(r[1], lw(T1, T0, lo(COURSE_WORD_VA)));
    assert_eq!(r[2], addiu(T2, ZERO, 0x131));
    assert_eq!(r[3], bne(T1, T2, 5)); // idx3 -> DEF(9)
    // Course-3 round-0 arm seats Lu in formation seat 1 and clears the
    // koin1 menu-selection residue (the quit-reclassification cell) in
    // the return delay slot.
    assert_eq!(r[5], sb(T3, V0, 1));
    assert_eq!(r[7], j(SEAT_RETURN_VA));
    assert_eq!(r[8], sw(ZERO, T4, lo(MENU_RESIDUE_VA)));
    // Default arm replays the displaced zero store.
    assert_eq!(r[9], j(SEAT_RETURN_VA));
    assert_eq!(r[10], SEAT_HOOK_ORIG);
}

#[test]
fn stream_map_routine_shape() {
    // Site A: remaps Lu (164) during round 0 only.
    let r = assemble_stream_map_routine();
    assert_eq!(r.len(), 10);
    assert_eq!(r[1], lw(V0, A1, lo(COURSE_WORD_VA)));
    assert_eq!(r[2], addiu(A1, A0, 0xFF5C)); // -164
    assert_eq!(r[3], addiu(V0, V0, 0xFECF)); // -0x131
    assert_eq!(r[4], or(V0, V0, A1));
    assert_eq!(r[5], bne(V0, ZERO, 2));
    assert_eq!(r[7], addiu(A0, A0, 27));
    assert_eq!(r[8], j(STREAM_RETURN_VA));
    assert_eq!(r[9], STREAM_HOOK_ORIG);

    // Site B: remaps Che (163), the id the pre-streamer always picks.
    // The hook sits one instruction early (delay-slot discipline - see
    // STREAM2_HOOK_VA) and returns to the stock conversion with the
    // displaced `ori` replayed in the return delay.
    let r2 = assemble_stream2_map_routine();
    assert_eq!(r2.len(), 10);
    assert_eq!(r2[2], addiu(A1, V1, 0xFF5D)); // -163
    assert_eq!(r2[7], addiu(V1, V1, 27));
    assert_eq!(r2[8], j(STREAM2_RETURN_VA));
    assert_eq!(r2[9], STREAM2_HOOK_ORIG);
    assert_eq!(STREAM2_RETURN_VA, STREAM2_HOOK_VA + 4);

    // Clone mapping is the same offset for both siblings.
    const {
        assert!(DELILAS_PAIR_IDS[0] as u32 + CLONE_ID_OFFSET == CLONE_IDS[0] as u32);
        assert!(DELILAS_PAIR_IDS[1] as u32 + CLONE_ID_OFFSET == CLONE_IDS[1] as u32);
    }
}

#[test]
fn template_repoint_targets_cave() {
    let w = template_ref_words();
    assert_eq!(w[0], lui(A0, hi(TEMPLATE_VA)));
    assert_eq!(w[1], addiu(A0, A0, lo(TEMPLATE_VA)));
}

#[test]
fn reward_routine_shape() {
    let r = assemble_reward_routine();
    assert_eq!(r.len(), 9);
    // Replays the displaced table load first (t1 = table value).
    assert_eq!(r[0], lw(T1, V0, 0));
    // Course-3 arm returns the flat clear payout (5000; the const block
    // pins that it fits a positive `addiu` immediate). The 5000 load
    // rides the branch delay slot (always executes); the fall-through
    // arm overwrites it with the stock table value.
    const {
        assert!(COURSE3_CLEAR_COINS == 5000);
        assert!(COURSE3_CLEAR_COINS <= 0x7FFF);
    }
    assert_eq!(r[5], addiu(V0, ZERO, COURSE3_CLEAR_COINS as u16));
    assert_eq!(r[6], addu(V0, T1, ZERO));
    assert_eq!(r[7], j(REWARD_RETURN_VA));
    // Branch offset: idx4 -> SKIP(7) = +2 (past the stock overwrite).
    assert_eq!(r[4], beq(T2, T3, 2));
}

#[test]
fn cave_fits_the_gap() {
    assert!(ROUTINE_VA + assemble_routine().len() as u32 * 4 <= TEMPLATE_VA);
    assert!(TEMPLATE_VA + TEMPLATE_BYTES.len() as u32 <= ROSTER_VA);
    assert!(ROSTER_VA + roster_bytes().len() as u32 <= SEAT_ROUTINE_VA);
    assert_eq!(SEAT_ROUTINE_VA % 4, 0, "seat routine is a j target");
    assert!(SEAT_ROUTINE_VA + assemble_seat_routine().len() as u32 * 4 <= REWARD_ROUTINE_VA);
    assert_eq!(REWARD_ROUTINE_VA % 4, 0, "reward routine is a j target");
    assert!(REWARD_ROUTINE_VA + assemble_reward_routine().len() as u32 * 4 <= STREAM_ROUTINE_VA);
    assert_eq!(STREAM_ROUTINE_VA % 4, 0, "stream routine A is a j target");
    assert!(
        STREAM_ROUTINE_VA + assemble_stream_map_routine().len() as u32 * 4 <= STREAM2_ROUTINE_VA
    );
    assert_eq!(STREAM2_ROUTINE_VA % 4, 0, "stream routine B is a j target");
    let end = STREAM2_ROUTINE_VA + assemble_stream2_map_routine().len() as u32 * 4;
    assert!(end <= CAVE_END_VA);
    // The cave must stay below the live SsAPI I/O table at GAP_END_VA.
    const { assert!(CAVE_END_VA <= GAP_END_VA) }
}

#[test]
fn ai_block_shape() {
    let r = assemble_ai_block();
    assert!(r.len() * 4 <= AI_BLOCK_CAPACITY);
    // Entry replays the displaced ctx reload, so the stock-resume path
    // hands the retail arm the register it expects.
    assert_eq!(r[0], AI_HOOK_ORIG);
    // Course gate: exactly the Che & Lu round word (0x131) runs the
    // local retime; everything else is forwarded to the Gi cave (which
    // resumes the stock arm for non-course-3 contexts itself).
    assert_eq!(r[2], lw(A0, A3, lo(COURSE_WORD_VA)));
    assert_eq!(r[4], addiu(A0, A0, 0xFECF));
    assert_eq!(r[5], beq(A0, ZERO, 3)); // idx5 -> CHELU(9)
    assert_eq!(r[7], j(GI_ARM_VA));
    // The retime arm re-arms the Gi one-shots on every round-0 pick.
    assert_eq!(r[9], lui(A0, hi(GI_FLAGS_VA)));
    assert_eq!(r[10], sw(ZERO, A0, lo(GI_FLAGS_VA)));
    // Retime: (counter + {Che 0, Lu 2}) % 4 == 3 queues the special.
    assert_eq!(r[19], andi(A0, A0, AI_PERIOD_MASK));
    assert_eq!(r[20], addiu(A3, ZERO, AI_SPECIAL_PHASE));
    assert_eq!(r[21], bne(A0, A3, 6)); // idx21 -> NOSPEC(28)
    // Special queue mirrors the stock arm's stores.
    assert_eq!(r[24], sb(V0, S4, 0x1DE));
    assert_eq!(r[25], addiu(V1, V1, 0xFFD7));
    assert_eq!(r[26], j(AI_ARM_JOIN_VA));
    assert_eq!(r[27], sb(V1, S4, 0x1DF));
    assert_eq!(r[28], j(AI_ARM_JOIN_VA));
    assert_eq!(r.len(), 30);
    // Fingerprint sanity: the hook context words are what the battle
    // overlay really holds (pinned against the dump in plan()).
    assert_eq!(AI_HOOK_PREV_ORIG, lui(V0, 0x8008));
    assert_eq!(AI_BLOCK_ORIG_HEAD, 0x27BD_FFE0);
    assert_eq!(AI_BLOCK_ORIG_HEAD2, 0x3C03_8007);
}

#[test]
fn gi_arm_shape() {
    let r = assemble_gi_arm();
    assert!(r.len() * 4 <= GI_ARM_CAPACITY);
    assert_eq!(r.len(), 47);
    // Exact-word gate (0x132) and the clone-seat gate: a clone pick
    // takes the join path (no specials), never the stock arm.
    assert_eq!(r[0], addiu(A0, A0, 0xFFFF)); // word-0x131 -> word-0x132
    assert_eq!(r[1], bne(A0, ZERO, 43)); // idx1 -> GSTOCK(45)
    assert_eq!(r[3], bne(S7, ZERO, 39)); // idx3 -> GNOSPEC(43)
    assert_eq!(r[4], lw(V1, A3, lo(GI_FLAGS_VA)));
    // Divide: HP strictly below half of max, one-shot via flags bit 0.
    assert_eq!(r[8], lhu(A0, S4, 0x14C));
    assert_eq!(r[9], lhu(A3, S4, 0x14E));
    assert_eq!(r[11], srl(A3, A3, 1));
    assert_eq!(r[12], sltu(A0, A0, A3));
    assert_eq!(r[15], ori(V1, V1, 1));
    assert_eq!(r[17], sw(V1, A3, lo(GI_FLAGS_VA)));
    assert_eq!(r[19], addiu(V1, ZERO, GI_DIVIDE_SPELL));
    // Spore Gas: counter floor test on both paths, one-shot via bit 1.
    assert_eq!(r[24], sltiu(A0, A0, GI_SPORE_GAS_MIN_TURN));
    assert_eq!(r[33], sltiu(A0, A0, GI_SPORE_GAS_MIN_TURN));
    assert_eq!(r[26], ori(V1, V1, 2));
    assert_eq!(r[35], ori(V1, V1, 2));
    assert_eq!(r[38], addiu(V1, ZERO, GI_SPORE_GAS_SPELL));
    // Post-Divide non-cast picks join (Blazing Slash locked out);
    // pre-Divide ones resume the stock cadence.
    assert_eq!(r[21], bne(A0, ZERO, 21)); // idx21 -> GNOSPEC(43)
    assert_eq!(r[25], bne(A0, ZERO, 17)); // idx25 -> GNOSPEC(43)
    assert_eq!(r[30], bne(A0, ZERO, 14)); // idx30 -> GSTOCK(45)
    assert_eq!(r[34], bne(A0, ZERO, 10)); // idx34 -> GSTOCK(45)
    // Queue mirrors the stock arm's stores; the two tails.
    assert_eq!(r[40], sb(V0, S4, 0x1DE));
    assert_eq!(r[41], j(AI_ARM_JOIN_VA));
    assert_eq!(r[42], sb(V1, S4, 0x1DF));
    assert_eq!(r[43], j(AI_ARM_JOIN_VA));
    assert_eq!(r[45], j(AI_STOCK_RESUME_VA));
    const {
        assert!(GI_ROUND_WORD == 0x132);
        // The flags cell is the display cave's data tail (word 8).
        assert!(GI_FLAGS_VA == DISPLAY_ROUTINE_VA + 32);
    }
    // The cave fingerprints match FUN_80050d40's real head.
    assert_eq!(GI_ARM_ORIG_HEAD, 0x3084_0FFF);
    assert_eq!(GI_ARM_ORIG_WORD2, 0x30A5_0FFF);
}

#[test]
fn display_routine_shape() {
    // The override fits the overwritten camera-preset body.
    let r = assemble_display_routine();
    assert!(r.len() * 4 <= DISPLAY_ROUTINE_CAPACITY);
    // Its home fingerprints match the SCUS camera preset's real words.
    assert_eq!(DISPLAY_HOME_ORIG_HEAD, 0x3C02_8008);
    assert_eq!(DISPLAY_HOME_ORIG_HEAD2, 0xAC40_40B8);
    // Course test rides the pre-shifted course<<6 still live in v1.
    assert_eq!(r[0], addiu(T0, ZERO, 0xC0));
    assert_eq!(r[1], bne(V1, T0, 3)); // idx1 -> SKIP(5)
    assert_eq!(r[2], DISPLAY_HOOK_ORIG); // replayed address add
    assert_eq!(r[4], addiu(V1, ZERO, COURSE3_CLEAR_COINS as u16));
    assert_eq!(r[5], DISPLAY_HOOK_DELAY_ORIG); // stock table read
    assert_eq!(r[3], j(DISPLAY_RETURN_VA));
    assert_eq!(r[6], j(DISPLAY_RETURN_VA));
    // The data tail: the Gi one-shot flags cell, initialised clear.
    assert_eq!(r[8], 0);
    assert_eq!(r.len() as u32, (GI_FLAGS_VA - DISPLAY_ROUTINE_VA) / 4 + 1);
    // Stock fingerprints match the arena overlay's real words.
    assert_eq!(DISPLAY_HOOK_ORIG, 0x0044_1021);
    assert_eq!(DISPLAY_HOOK_DELAY_ORIG, 0x8C43_0000);
}
