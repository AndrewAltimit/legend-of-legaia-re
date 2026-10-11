//! The injected SCUS routines and data: the course seed routine and its refresh
//! plan, the roster, the seat / stream-map / reward routines, the course
//! descriptor, and the RAM-write probe.

use super::battle_ai::{AI_BLOCK_VA, GI_ARM_VA, assemble_ai_block, assemble_gi_arm};
use super::injection::{Write, scus_off};
use super::{
    CLONE_ID_OFFSET, COURSE_FLAG, COURSE_GLOBAL_VA, COURSE_WORD_VA, COURSE3_CLEAR_COINS,
    COURSE3_SEED_WORD, DELILAS_NAME_PTRS, DELILAS_PAIR_IDS, DELILAS_ROSTER_IDS, DELILAS_ROUNDS,
    DISPLAY_ROUTINE_VA, FLAG_CLEAR_FUNC_VA, FLAG_TEST_FUNC_VA, MENU_RESIDUE_VA,
    PRGERR_PRINT_GATE_NEW, PRGERR_PRINT_GATE_VA, REWARD_RETURN_VA, REWARD_ROUTINE_VA, ROSTER_VA,
    ROUTINE_VA, SEAT_HOOK_ORIG, SEAT_RETURN_VA, SEAT_ROUTINE_VA, SECOND_SEAT_ID, SEED_HOOK_ORIG,
    SEED_RETURN_VA, STREAM_HOOK_ORIG, STREAM_HOOK_VA, STREAM_RETURN_VA, STREAM_ROUTINE_VA,
    STREAM2_HOOK_ORIG, STREAM2_HOOK_VA, STREAM2_RETURN_VA, STREAM2_ROUTINE_VA, TEMPLATE_BYTES,
    TEMPLATE_VA, WORD_OFF_FROM_S1, assemble_display_routine,
};
use anyhow::{Result, bail};
use legaia_code_hooks::mips::*;

/// Assemble the seed routine: if [`COURSE_FLAG`] is set **and no retail
/// difficulty arm chose a course this entry** (the retail fresh-path seed
/// left the word at its no-flags default `1` - only the Delilas arm, which
/// clears all three course-unlock flags, produces that), store
/// [`COURSE3_SEED_WORD`] into the course word; clear the flag **whenever it
/// was set**, hijacked or not; always replay the displaced `lw v0,word` and
/// return to [`SEED_RETURN_VA`].
///
/// The word guard is the stale-flag scrub: `0x539` normally lives only for
/// the scripted warp flourish between the koin1 arm and this routine, but a
/// save state (or an interrupted enrollment) can freeze it set, and without
/// the guard every later dome entry - Beginner included - started the
/// Delilas course. With it, a retail arm's own seed (`0x101`/`0x111`/
/// `0x321 != 1`) wins and the stale flag is cleared as a side effect.
///
/// `s1` (the `0x80080000` base) is live across the detour and preserved by
/// the flag helpers (callee-saved); `s2 == 1` is the retail fresh-path
/// invariant this very function relies on for its own `sw s2,word` default
/// seed at `0x801CEB8C`, and the flag helpers touch only `v0`/`v1`/`a0`/
/// `a1`; `ra` is saved on `FUN_801CEA6C`'s own frame, so the `jal`s here are
/// free to clobber it. `v0`/`a0`/`t0` are scratch.
pub fn assemble_routine() -> Vec<u32> {
    const SKIP_SEED: usize = 10; // flag set but a retail arm seeded: clear only
    const RELOAD: usize = 12; // replay `lw` in the return jump's delay slot

    let words = vec![
        addiu(A0, ZERO, COURSE_FLAG),             // 0:  a0 = 0x539
        jal(FLAG_TEST_FUNC_VA),                   // 1:  v0 = flag_test(0x539)
        nop(),                                    // 2:  (branch delay)
        beq(V0, ZERO, (RELOAD - (3 + 1)) as i16), // 3:  flag clear -> RELOAD
        nop(),                                    // 4:  (branch delay)
        // Flag set: only seed course 3 when the retail flag seed left the
        // word at its no-course default (s2 == 1) - i.e. the Delilas arm ran.
        lw(T0, S1, WORD_OFF_FROM_S1), // 5:  t0 = seeded word
        addiu(A0, ZERO, COURSE_FLAG), // 6:  (load delay) a0 for the clear
        bne(T0, S2, (SKIP_SEED - (7 + 1)) as i16), // 7:  retail course chosen -> scrub only
        addiu(T0, ZERO, COURSE3_SEED_WORD as u16), // 8:  (branch delay) t0 = 0x131
        sw(T0, S1, WORD_OFF_FROM_S1), // 9:  word = 0x131 (course 3, round 0)
        // SKIP_SEED (idx 10): clear the request flag (one-shot / stale scrub).
        jal(FLAG_CLEAR_FUNC_VA), // 10: flag_clear(0x539)
        nop(),                   // 11: (branch delay)
        // RELOAD (idx 12): return to the decode, replaying the displaced
        // `lw v0,word` in the delay slot (the `nop` at SEED_RETURN_VA covers
        // the load delay before the decode's first `v0` read).
        j(SEED_RETURN_VA), // 12: back to the decode
        SEED_HOOK_ORIG,    // 13: (jump delay) lw v0,-0x4540(s1)
    ];
    debug_assert_eq!(words.len(), 14);
    debug_assert_eq!(words[RELOAD + 1], SEED_HOOK_ORIG);
    words
}

/// Plan the same-size in-place cave rewrite that brings an already-patched
/// disc's resident seed routine up to [`assemble_routine`]'s current words,
/// or `None` when it is already current. Older builds shipped the routine
/// without the stale-[`COURSE_FLAG`] guard, and the apply layer's
/// idempotence check keys on the seed *hook*, which those builds already
/// have - so the upgrade must be planned separately.
pub fn plan_routine_refresh(scus: &[u8]) -> Result<Option<Write>> {
    let bytes: Vec<u8> = assemble_routine()
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let off = scus_off(scus, ROUTINE_VA)?;
    let Some(cur) = scus.get(off..off + bytes.len()) else {
        bail!("SCUS too short for the seed-routine cave");
    };
    if cur == bytes {
        return Ok(None);
    }
    Ok(Some(Write { off, bytes }))
}

/// The cave roster bytes: 2 x `{u32 name_ptr; u32 monster_id}`, little-endian.
pub fn roster_bytes() -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    for i in 0..DELILAS_ROSTER_IDS.len() {
        v.extend_from_slice(&DELILAS_NAME_PTRS[i].to_le_bytes());
        v.extend_from_slice(&DELILAS_ROSTER_IDS[i].to_le_bytes());
    }
    v
}

/// Assemble the second-seat routine: when course-3 round 0 is being
/// installed (course word == [`COURSE3_SEED_WORD`] exactly - round 1 carries
/// `0x132`), seat [`SECOND_SEAT_ID`] (Lu) in formation seat 1 instead of
/// zeroing it; every other course replays the displaced zero store. Entered
/// by `j` from [`SEAT_HOOK_VA`]; `v0` holds the formation-cell base, `t0..t4`
/// are dead, and `ra` is restored by the installer's own epilogue.
pub fn assemble_seat_routine() -> Vec<u32> {
    const DEF: usize = 9; // index of the default (replay) arm
    let words = vec![
        lui(T0, hi(COURSE_WORD_VA)),               // 0: t0 = 0x8008
        lw(T1, T0, lo(COURSE_WORD_VA)),            // 1: t1 = course word
        addiu(T2, ZERO, COURSE3_SEED_WORD as u16), // 2: t2 = 0x131 (load delay)
        bne(T1, T2, (DEF - (3 + 1)) as i16),       // 3: not round 0 -> DEF
        addiu(T3, ZERO, SECOND_SEAT_ID as u16),    // 4: t3 = Lu (delay, harmless)
        sb(T3, V0, 1),                             // 5: seat 1 = Lu
        lui(T4, hi(MENU_RESIDUE_VA)),              // 6: t4 = 0x8008
        j(SEAT_RETURN_VA),                         // 7:
        sw(ZERO, T4, lo(MENU_RESIDUE_VA)),         // 8: clear quit residue (delay)
        // DEF (idx 9): replay the displaced zero store.
        j(SEAT_RETURN_VA), // 9:
        SEAT_HOOK_ORIG,    // 10: sb zero,1(v0) (delay)
    ];
    debug_assert_eq!(words.len(), 11);
    debug_assert_eq!(words[DEF + 1], SEAT_HOOK_ORIG);
    words
}

/// Assemble the site-A stream-map routine: replay the displaced `id-1`
/// conversion, but add [`CLONE_ID_OFFSET`] first when the course word is
/// exactly [`COURSE3_SEED_WORD`] (round 0 - the only round that streams the
/// pair) **and** the id is Lu (site A only ever needs Lu: Che, the lower id,
/// is pre-streamed through site B). Both conditions fold into one zero test:
/// `(word - 0x131) | (id - 164) == 0`.
///
/// Register discipline at [`STREAM_HOOK_VA`]: `a0` = the formation id, dead
/// after the conversion; `v0` is overwritten by the first instruction at
/// [`STREAM_RETURN_VA`]; `a1` is written (never read) before its first use
/// at `0x80054538`. The `lw` load-delay slot is filled with the id compare.
pub fn assemble_stream_map_routine() -> Vec<u32> {
    const SKIP: usize = 8; // index of the replay conversion
    let neg_word = -(COURSE3_SEED_WORD as i32) as u32 as u16;
    let neg_lu = -(SECOND_SEAT_ID as i32) as u32 as u16;
    let words = vec![
        lui(A1, hi(COURSE_WORD_VA)),            // 0: a1 = 0x8008
        lw(V0, A1, lo(COURSE_WORD_VA)),         // 1: v0 = course word
        addiu(A1, A0, neg_lu),                  // 2: a1 = id - 164 (load delay)
        addiu(V0, V0, neg_word),                // 3: v0 = word - 0x131
        or(V0, V0, A1),                         // 4: 0 iff round 0 AND id == Lu
        bne(V0, ZERO, (SKIP - (5 + 1)) as i16), // 5: anything else -> SKIP
        nop(),                                  // 6: (branch delay)
        addiu(A0, A0, CLONE_ID_OFFSET as u16),  // 7: id -> clone slot id
        // SKIP (idx 8): replay the displaced conversion, return.
        j(STREAM_RETURN_VA), // 8:
        STREAM_HOOK_ORIG,    // 9: addiu v1,a0,-1 (delay)
    ];
    debug_assert_eq!(words.len(), 10);
    debug_assert_eq!(words[SKIP + 1], STREAM_HOOK_ORIG);
    words
}

/// Assemble the site-B stream-map routine - the pre-streamer's mirror of
/// [`assemble_stream_map_routine`]: the id lives in `v1` (site B only ever
/// needs Che, the lowest formation id, which the pre-streamer picks first),
/// and `v0`/`a1`/`at` are dead at the hook. Returns to the stock conversion
/// ([`STREAM2_RETURN_VA`]) with the displaced `ori` replayed in the return
/// delay, so the whole seek recomputes from the (possibly remapped) id.
pub fn assemble_stream2_map_routine() -> Vec<u32> {
    const SKIP: usize = 8; // index of the return jump
    let neg_word = -(COURSE3_SEED_WORD as i32) as u32 as u16;
    let neg_che = -(DELILAS_PAIR_IDS[0] as i32) as u32 as u16;
    let words = vec![
        lui(A1, hi(COURSE_WORD_VA)),            // 0: a1 = 0x8008
        lw(V0, A1, lo(COURSE_WORD_VA)),         // 1: v0 = course word
        addiu(A1, V1, neg_che),                 // 2: a1 = id - 163 (load delay)
        addiu(V0, V0, neg_word),                // 3: v0 = word - 0x131
        or(V0, V0, A1),                         // 4: 0 iff round 0 AND id == Che
        bne(V0, ZERO, (SKIP - (5 + 1)) as i16), // 5: anything else -> SKIP
        nop(),                                  // 6: (branch delay)
        addiu(V1, V1, CLONE_ID_OFFSET as u16),  // 7: id -> clone slot id
        // SKIP (idx 8): return to the stock conversion, replaying the
        // displaced `ori` in the delay slot.
        j(STREAM2_RETURN_VA), // 8:
        STREAM2_HOOK_ORIG,    // 9: ori a0,a0,0x2800 (delay)
    ];
    debug_assert_eq!(words.len(), 10);
    debug_assert_eq!(words[SKIP + 1], STREAM2_HOOK_ORIG);
    words
}

/// Assemble the reward routine: replay the payout-table load, but return
/// [`COURSE3_CLEAR_COINS`] when the settling course is 3. Entered by `j` from
/// [`REWARD_HOOK_VA`] (whose delay slot, the `slti` Seru gate, has already
/// executed); returns to [`REWARD_RETURN_VA`], where the stock code adds `v0`
/// into the winnings counter. Reached only on the cleared-latch path, so a
/// lost or abandoned contest never pays.
///
/// Register discipline: `v0` = table address in, payout out; `v1` (counter
/// value) / `a1` (Seru gate) / `a2` (state base) are live and untouched;
/// `t0..t3` are dead. Load-delay respected as in [`assemble_seat_routine`].
pub fn assemble_reward_routine() -> Vec<u32> {
    const SKIP: usize = 7; // branch target: past the stock-value overwrite
    let words = vec![
        lw(T1, V0, 0),                               // 0: t1 = table value
        lui(T0, hi(COURSE_GLOBAL_VA)),               // 1: t0 = 0x801D....
        lw(T2, T0, lo(COURSE_GLOBAL_VA)),            // 2: t2 = course
        addiu(T3, ZERO, 3),                          // 3: t3 = 3
        beq(T2, T3, (SKIP - (4 + 1)) as i16),        // 4: course 3 -> keep 5000
        addiu(V0, ZERO, COURSE3_CLEAR_COINS as u16), // 5: v0 = 5000 (delay, always)
        addu(V0, T1, ZERO),                          // 6: stock: v0 = table value
        // SKIP (idx 7):
        j(REWARD_RETURN_VA), // 7:
        nop(),               // 8: (delay)
    ];
    debug_assert_eq!(words.len(), 9);
    words
}

/// The course-3 descriptor bytes: `{i32 round_count=3; u32 first_round=ROSTER_VA}`.
pub fn descriptor_bytes() -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&DELILAS_ROUNDS.to_le_bytes());
    b[4..].copy_from_slice(&ROSTER_VA.to_le_bytes());
    b
}

/// The two words that repoint the template `lui`/`addiu` at the cave copy.
pub fn template_ref_words() -> [u32; 2] {
    [lui(A0, hi(TEMPLATE_VA)), addiu(A0, A0, lo(TEMPLATE_VA))]
}

/// The SCUS-side half of the injection as `(VA, bytes)` pairs, for
/// RAM-installing it in an emulator probe: a library save state predates the
/// patched disc, so the always-resident SCUS image in RAM lacks the cave and
/// hooks, while the arena/battle overlay halves stream in from the patched
/// `--iso` disc on their own. Mirrors exactly the `scus` writes [`DomeInjection::plan`]
/// produces (cave regions, the two stream-map hook `j`s, the PRG ERR print
/// gate, the AI-retime block) - no disc needed because every word is static.
pub fn probe_ram_writes() -> Vec<(u32, Vec<u8>)> {
    let words = |w: Vec<u32>| -> Vec<u8> { w.iter().flat_map(|w| w.to_le_bytes()).collect() };
    vec![
        (AI_BLOCK_VA, words(assemble_ai_block())),
        (GI_ARM_VA, words(assemble_gi_arm())),
        (DISPLAY_ROUTINE_VA, words(assemble_display_routine())),
        (ROUTINE_VA, words(assemble_routine())),
        (TEMPLATE_VA, TEMPLATE_BYTES.to_vec()),
        (ROSTER_VA, roster_bytes()),
        (SEAT_ROUTINE_VA, words(assemble_seat_routine())),
        (REWARD_ROUTINE_VA, words(assemble_reward_routine())),
        (STREAM_ROUTINE_VA, words(assemble_stream_map_routine())),
        (STREAM2_ROUTINE_VA, words(assemble_stream2_map_routine())),
        (STREAM_HOOK_VA, j(STREAM_ROUTINE_VA).to_le_bytes().to_vec()),
        (
            STREAM2_HOOK_VA,
            j(STREAM2_ROUTINE_VA).to_le_bytes().to_vec(),
        ),
        (
            PRGERR_PRINT_GATE_VA,
            PRGERR_PRINT_GATE_NEW.to_le_bytes().to_vec(),
        ),
    ]
}
