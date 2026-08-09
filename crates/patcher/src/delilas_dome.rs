//! **Delilas Challenge** as a real Muscle Dome arena *course* - the code-hook
//! half of the feature (the arena overlay PROT 0977 side + a SCUS routine
//! cave). The casino-menu / warp half is [`crate::delilas_challenge`].
//!
//! ## Why the dome arena, and why a hook
//!
//! Live testing killed the earlier "launch a normal battle from the casino"
//! approach: koin1 is a town scene with no random encounters, so it never
//! installs the battle-effect / summon / player-magic asset residency, and a
//! spell cast (or opening the magic list) dereferences unloaded buffers and
//! freezes. That is exactly why the retail Muscle Dome disables magic - its
//! arena doesn't load those assets. And the battle mesh heap holds only about
//! one large boss's worth of *distinct* geometry, so three distinct Delilas
//! meshes at once overflow it (two already hang the load).
//!
//! Routing the challenge through the dome's own arena fixes all of that: the
//! dome loads one fighter + the round's bosses, disables magic by design, and
//! runs the fight in a real arena. A dome contest is a *course* - a fixed
//! roster fought one round at a time (the installer `FUN_801D1510` reads the
//! round's monster id from a course roster and seats it). So the Delilas
//! Challenge is a **new 2-round course**: **Che & Lu together (1v2), then Gi
//! (1v1)**. The double-team round is affordable precisely *because* it is a
//! dome round: only one player battle form is resident (a normal encounter
//! carries three), which frees the mesh-heap headroom the second distinct
//! boss needs.
//!
//! ## The seven edits (all in PROT 0977 + a SCUS cave; koin1 is separate)
//!
//! The arena reads its course from a packed word `_DAT_8007BAC0`; the init
//! `FUN_801CEA6C` seeds it from the course-unlock story flags on a fresh
//! contest (`0x536`->Beginner/course 0, `0x537`->Expert/1, `0x538`->Master/2;
//! `word = 0x101/0x111/0x321`, `course = ((word-1)&0xff)>>4`). A **4th course**
//! (course 3) needs a seed and a descriptor:
//!
//! 1. **Seed detour** ([`SEED_HOOK_VA`] `0x801CEBCC`, the `lw v0,word` reload
//!    just before the decode). Replaced with `j` into the cave routine, which
//!    tests the one-shot flag [`COURSE_FLAG`] (`0x539`, set by the koin1
//!    option): if set, it stores [`COURSE3_SEED_WORD`] (course 3, round 0,
//!    dome-marker bit) into the word and clears the flag, then replays the
//!    displaced `lw` and returns. Clearing the flag makes it one-shot - the
//!    word carries course 3 through the remaining rounds (the continuing-leg
//!    path), and a later normal dome entry reads `0x539` clear.
//! 2. **Course descriptor** for course 3 at [`COURSE3_DESC_VA`] (`0x801D1A20`
//!    = the descriptor table base `0x801D1A08` + `3*8`): `{i32 round_count=2;
//!    ptr first_round = cave roster}`. All four descriptor readers compute
//!    `base + course*8`, so writing the slot makes course 3 work everywhere
//!    with no per-reader hook.
//! 3. **Actor-template relocation.** `0x801D1A20` currently holds the hub
//!    actor template (24 bytes, referenced once at [`TEMPLATE_REF_LUI_VA`] /
//!    `+4`). Its 24 bytes are copied into the cave and that one `lui/addiu`
//!    pair is repointed there, freeing `0x801D1A20` for the descriptor.
//! 4. **Cave roster** ([`ROSTER_VA`]): two `{u32 name_ptr; u32 monster_id}`
//!    entries - Che (163), Gi (162) - reusing the dome's own Delilas name
//!    strings (resident with 0977) so the ROUND banner shows real names (the
//!    round-0 banner names Che; Lu is the surprise second seat).
//! 5. **Seed routine** ([`ROUTINE_VA`]): the seed test/clear above.
//! 6. **Second-seat detour** ([`SEAT_HOOK_VA`] `0x801D15A4`, the installer's
//!    `sb zero,1(v0)` that zeroes formation seat 1). The installer
//!    `FUN_801D1510` stages the round as raw id bytes at `0x8007BD0C..0F`
//!    (seat 0 = the roster id, seats 1..3 zeroed; battle setup counts the
//!    non-zero seats - the same `0x8007BD0D` cell the enemy-ally feature
//!    reads to detect a multi-enemy fight). The cave routine seats **Lu
//!    (164) in seat 1 when course 3, round 0**, else replays the zero store -
//!    turning round 0 into the Che & Lu double-team.
//! 7. **Reward detour** ([`REWARD_HOOK_VA`] `0x801D1118`, the settlement's
//!    payout-table load). `FUN_801D0F60` settles a contest: only when the
//!    cleared latch (`DAT_801D1ADC`, raised on *course exhausted AND
//!    survived*) is up does it add `*(0x801D1860 + course*0x40 +
//!    (round-1)*4)` to the winnings counter `0x80084440`. Course 3 indexes
//!    past the 3-course table into bytes that read 0 - the "0 tokens"
//!    live-test observation. The cave routine returns the table value
//!    normally but **[`COURSE3_CLEAR_COINS`] for course 3**; riding the
//!    stock cleared-latch gate means a loss still pays nothing (retail
//!    halves the accumulated counter on a loss, which is kept).
//!
//! The course-length clamp (`0x801CED28`) is Master-only (`bne course,2`), so
//! course 3 uses its descriptor's `round_count=2` verbatim - two rounds,
//! no clamp. The full-Master Seru grant (`round >= 13` at `0x801D111C..`)
//! can never trip at round 2. Everything lives in the loaded-and-preserved
//! SCUS rodata gap (the window every code-injection feature shares) +
//! same-size overwrites in the arena overlay; an unrecognized build is
//! refused, not corrupted.

use anyhow::{Result, bail};

use crate::mips::*;

/// PROT entry index of the Muscle Dome arena overlay ("other_game" / slot-A
/// base `0x801CE818`).
pub const ARENA_OVERLAY_PROT_INDEX: usize = 977;

/// Load base VA of the arena overlay; a VA inside it maps to PROT-entry file
/// offset `va - ARENA_BASE_VA`.
pub const ARENA_BASE_VA: u32 = 0x801C_E818;

/// One-shot SYSTEM flag the koin1 Delilas option sets to request course 3.
/// Sits just past the retail course-unlock flags (`0x536`/`0x537`/`0x538`);
/// the seed routine clears it after seeding, so it never persists into a
/// normal dome entry. Absent from the disc-wide field/motion flag censuses.
pub const COURSE_FLAG: u16 = 0x539;

/// Packed course/round word the seed routine writes for course 3, round 0:
/// `course = ((0x131-1)&0xff)>>4 = 3`, `round = (0x131-1)&0xf = 0`.
///
/// Bit `0x100` is **load-bearing**: it is the dome-contest marker the battle
/// exit selector `FUN_80046A20` tests (`_DAT_8007BAC0 & 0x100`) to route a
/// leg's end back to arena mode `0x18` instead of MAIN INIT - every retail
/// seed carries it (`0x101`/`0x111`/`0x321`). Without it a dome wipe falls
/// into the ordinary game-over gate (CARD continue screen) and a win exits to
/// the field instead of the between-leg hub - the exact live-test failure an
/// earlier `0x431` seed produced. Master's extra `0x200` bit is not copied
/// (only `0x321` has it; Beginner/Expert prove `0x100` alone suffices).
pub const COURSE3_SEED_WORD: u32 = 0x0000_0131;

/// Word global holding the packed course/round (`_DAT_8007BAC0`), addressed in
/// the init as `s1 - 0x4540` with `s1 = 0x80080000`.
pub const COURSE_WORD_VA: u32 = 0x8007_BAC0;
/// The signed `lw`/`sw` offset from `s1` (`0x80080000`) to [`COURSE_WORD_VA`]:
/// `-0x4540`, i.e. `0xBAC0` as the raw 16-bit field.
pub const WORD_OFF_FROM_S1: u16 = 0xBAC0;

/// Seed-detour site: the `lw v0,-0x4540(s1)` word reload in `FUN_801CEA6C`
/// immediately before the course/round decode. Replaced with `j ROUTINE_VA`.
pub const SEED_HOOK_VA: u32 = 0x801C_EBCC;
/// The stock instruction at [`SEED_HOOK_VA`] (`lw v0,-0x4540(s1)`) - the
/// displaced word the routine replays, and the recognized-build fingerprint.
pub const SEED_HOOK_ORIG: u32 = lw(V0, S1, WORD_OFF_FROM_S1);
/// Where the seed detour returns (the instruction after the displaced `lw`).
pub const SEED_RETURN_VA: u32 = 0x801C_EBD0;

/// Story-flag helpers the seed routine calls (shared with the field VM):
/// test (`0x7x`) returns non-zero in `v0` when the flag is set.
pub const FLAG_TEST_FUNC_VA: u32 = 0x8003_CE64;
/// Clear (`0x6x`).
pub const FLAG_CLEAR_FUNC_VA: u32 = 0x8003_CE34;

/// Course-descriptor slot for course 3 (`0x801D1A08 + 3*8`). Receives
/// `{i32 round_count; u32 first_round_ptr}`. Currently the hub actor template.
pub const COURSE3_DESC_VA: u32 = 0x801D_1A20;
/// Number of rounds in the Delilas course: Che & Lu (1v2), then Gi (1v1).
pub const DELILAS_ROUNDS: u32 = 2;

/// The hub actor-template `lui`/`addiu` pair (`lui a0,0x801d` ; `addiu
/// a0,a0,0x1a20`) that materialises the template at [`COURSE3_DESC_VA`]. Both
/// are repointed at the relocated cave copy so the descriptor can take
/// `0x801D1A20`.
pub const TEMPLATE_REF_LUI_VA: u32 = 0x801C_EADC;
/// The stock `lui a0,0x801d` (recognized-build fingerprint).
pub const TEMPLATE_REF_LUI_ORIG: u32 = lui(A0, 0x801D);
/// The stock `addiu a0,a0,0x1a20` (recognized-build fingerprint).
pub const TEMPLATE_REF_ADDIU_ORIG: u32 = addiu(A0, A0, 0x1A20);
/// The 24-byte hub actor template copied verbatim into the cave. `+0x08` =
/// `0x801CF870` (the hub tick fn), which the copy preserves.
pub const TEMPLATE_BYTES: [u8; 24] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x70, 0xF8, 0x1C, 0x80, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The roster ids in fight order: round 0 = Che (joined by the second-seat
/// Lu), round 1 = Gi.
pub const DELILAS_ROSTER_IDS: [u32; 2] = [163, 162];
/// The dome's own Delilas name-string VAs (resident with 0977), paired with
/// [`DELILAS_ROSTER_IDS`]: Che (`0x801CE8C4`), Gi (`0x801CE8B8`). The round-0
/// banner names Che; Lu enters unannounced.
pub const DELILAS_NAME_PTRS: [u32; 2] = [0x801C_E8C4, 0x801C_E8B8];
/// The monster id the second-seat hook adds to round 0: Lu (164).
pub const SECOND_SEAT_ID: u32 = 164;
/// Coins added to the dome winnings counter (`0x80084440`) for a full clear
/// of the Delilas course.
pub const COURSE3_CLEAR_COINS: u32 = 5000;

// --- Second-seat hook (installer `FUN_801D1510`) -----------------------------

/// Second-seat detour site: the installer's `sb zero,1(v0)` (`v0` =
/// `0x8007BD0C`, the formation seat bytes) that zeroes seat 1. Replaced with
/// `j SEAT_ROUTINE_VA`; the following `sb zero,2(v0)` is its delay slot and
/// still executes.
pub const SEAT_HOOK_VA: u32 = 0x801D_15A4;
/// The stock instruction at [`SEAT_HOOK_VA`] (`sb zero,1(v0)`) - the displaced
/// store the routine replays on the non-Delilas path, and the build fingerprint.
pub const SEAT_HOOK_ORIG: u32 = sb(ZERO, V0, 1);
/// Where the seat detour returns (the `sb zero,3(v0)` after the delay slot).
pub const SEAT_RETURN_VA: u32 = 0x801D_15AC;

// --- Reward hook (settlement `FUN_801D0F60`) ---------------------------------

/// Reward detour site: the settlement's payout-table load `lw v0,0(v0)`
/// (`v0` = `0x801D1860 + course*0x40 + (round-1)*4`), reached only when the
/// cleared latch is up. Replaced with `j REWARD_ROUTINE_VA`; the following
/// `slti a1,a1,0xd` (the full-Master Seru gate) is its delay slot and still
/// executes.
pub const REWARD_HOOK_VA: u32 = 0x801D_1118;
/// The stock instruction at [`REWARD_HOOK_VA`] (`lw v0,0(v0)`) - the displaced
/// load the routine replays for retail courses, and the build fingerprint.
pub const REWARD_HOOK_ORIG: u32 = lw(V0, V0, 0);
/// Where the reward detour returns (the `addu v1,v1,v0` payout add).
pub const REWARD_RETURN_VA: u32 = 0x801D_1120;

/// Course global (`DAT_801D1A90`) read by the seat + reward routines.
pub const COURSE_GLOBAL_VA: u32 = 0x801D_1A90;
/// Round global (`DAT_801D1A94`), sibling of [`COURSE_GLOBAL_VA`].
pub const ROUND_GLOBAL_VA: u32 = 0x801D_1A94;

// --- SCUS routine cave (the preserved rodata gap 0x8007AB38..0x8007AF40) -----
// Placed in the free tail after the flee-EXP routine (0x8007AD00..0x8007AE00),
// so this composes with every other gap feature.

/// Load VA of the seed routine in the preserved SCUS gap.
pub const ROUTINE_VA: u32 = 0x8007_AE00;
/// Load VA of the relocated hub actor template (24 bytes).
pub const TEMPLATE_VA: u32 = 0x8007_AE40;
/// Load VA of the cave roster (2 x 8 bytes).
pub const ROSTER_VA: u32 = 0x8007_AE60;
/// Load VA of the second-seat routine (4-aligned - it is a `j` target).
pub const SEAT_ROUTINE_VA: u32 = 0x8007_AE70;
/// Load VA of the reward routine (4-aligned - it is a `j` target).
pub const REWARD_ROUTINE_VA: u32 = 0x8007_AEA4;
/// One past the last cave byte used; must stay within the gap end.
pub const CAVE_END_VA: u32 = 0x8007_AECC;
/// End of the usable zero window (exclusive): the SsAPI sound I/O register
/// table begins exactly at `0x8007AF00` and is read every frame (the
/// shiny-seru read-watch pinned it - see `shiny_seru::layout`), so the cave
/// must end below it even though the bytes scan as zero.
pub const GAP_END_VA: u32 = 0x8007_AF00;

/// Assemble the seed routine: if [`COURSE_FLAG`] is set, store
/// [`COURSE3_SEED_WORD`] into the course word and clear the flag; always
/// replay the displaced `lw v0,word` and return to [`SEED_RETURN_VA`].
///
/// `s1` (the `0x80080000` base) is live across the detour and preserved by the
/// flag helpers (callee-saved); `ra` is saved on `FUN_801CEA6C`'s own frame, so
/// the `jal`s here are free to clobber it. `v0`/`a0`/`t0` are scratch.
pub fn assemble_routine() -> Vec<u32> {
    const SKIP: usize = 11; // index of the replay `lw` (the flag-clear tail target)
    let flag_clear_skip = (SKIP as i32 - (3 + 1)) as i16;

    let words = vec![
        addiu(A0, ZERO, COURSE_FLAG),   // 0:  a0 = 0x539
        jal(FLAG_TEST_FUNC_VA),         // 1:  v0 = flag_test(0x539)
        nop(),                          // 2:  (branch delay)
        beq(V0, ZERO, flag_clear_skip), // 3:  flag clear -> SKIP (replay)
        nop(),                          // 4:  (branch delay)
        // flag set: word = 0x431 (course 3, round 0)
        lui(T0, imm_hi(COURSE3_SEED_WORD)),     // 5: \ t0 = 0x431
        ori(T0, T0, imm_lo(COURSE3_SEED_WORD)), // 6: /
        sw(T0, S1, WORD_OFF_FROM_S1),           // 7:  word (s1-0x4540) = 0x431
        addiu(A0, ZERO, COURSE_FLAG),           // 8:  a0 = 0x539
        jal(FLAG_CLEAR_FUNC_VA),                // 9:  flag_clear(0x539)  (one-shot)
        nop(),                                  // 10: (branch delay)
        // SKIP (idx 11): replay displaced `lw v0,word`, return.
        SEED_HOOK_ORIG,    // 11: lw v0,-0x4540(s1)
        j(SEED_RETURN_VA), // 12: back to the decode
        nop(),             // 13: (branch delay)
    ];
    debug_assert_eq!(words.len(), 14);
    debug_assert_eq!(words[SKIP], SEED_HOOK_ORIG);
    words
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

/// Assemble the second-seat routine: on course 3, round 0, store
/// [`SECOND_SEAT_ID`] into formation seat 1 (`v0` holds `0x8007BD0C` at the
/// hook); otherwise replay the displaced `sb zero,1(v0)`. Entered by `j` from
/// [`SEAT_HOOK_VA`] (whose delay slot, the seat-2 zero store, has already
/// executed); returns to [`SEAT_RETURN_VA`].
///
/// Register discipline: `v0`/`v1`/`a0`/`a1` are live across the detour (seat
/// base, `0x8008` half, monster id, `0x8008` half) and are not touched;
/// `t0..t4` are dead here. Loads respect the R3000 load-delay slot (each
/// loaded register is first read two or more instructions later).
pub fn assemble_seat_routine() -> Vec<u32> {
    const DEF: usize = 11; // index of the default (replay) arm
    let words = vec![
        lui(T0, hi(COURSE_GLOBAL_VA)),          // 0:  t0 = 0x801D....
        lw(T1, T0, lo(COURSE_GLOBAL_VA)),       // 1:  t1 = course
        lw(T2, T0, lo(ROUND_GLOBAL_VA)),        // 2:  t2 = round
        addiu(T3, ZERO, 3),                     // 3:  t3 = 3
        addiu(T4, ZERO, SECOND_SEAT_ID as u16), // 4:  t4 = Lu
        bne(T1, T3, (DEF - (5 + 1)) as i16),    // 5:  course != 3 -> DEF
        nop(),                                  // 6:  (branch delay)
        bne(T2, ZERO, (DEF - (7 + 1)) as i16),  // 7:  round != 0 -> DEF
        nop(),                                  // 8:  (branch delay)
        j(SEAT_RETURN_VA),                      // 9:  course 3, round 0:
        sb(T4, V0, 1),                          // 10: seat 1 = Lu (delay)
        // DEF (idx 11): replay the displaced zero store.
        j(SEAT_RETURN_VA), // 11:
        SEAT_HOOK_ORIG,    // 12: sb zero,1(v0) (delay)
    ];
    debug_assert_eq!(words.len(), 13);
    debug_assert_eq!(words[DEF + 1], SEAT_HOOK_ORIG);
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
    const STOCK: usize = 8; // index of the stock (table value) arm
    let words = vec![
        lw(T1, V0, 0),                               // 0: t1 = table value
        lui(T0, hi(COURSE_GLOBAL_VA)),               // 1: t0 = 0x801D....
        lw(T2, T0, lo(COURSE_GLOBAL_VA)),            // 2: t2 = course
        addiu(T3, ZERO, 3),                          // 3: t3 = 3
        bne(T2, T3, (STOCK - (4 + 1)) as i16),       // 4: course != 3 -> STOCK
        nop(),                                       // 5: (branch delay)
        j(REWARD_RETURN_VA),                         // 6: course 3:
        addiu(V0, ZERO, COURSE3_CLEAR_COINS as u16), // 7: v0 = 5000 (delay)
        // STOCK (idx 8): the retail table value.
        j(REWARD_RETURN_VA), // 8:
        addu(V0, T1, ZERO),  // 9: v0 = table value (delay)
    ];
    debug_assert_eq!(words.len(), 10);
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

/// One same-size write into a target image: `(file_offset, bytes)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    /// File offset within the target (`SCUS_942.54` or the arena overlay).
    pub off: usize,
    /// Little-endian bytes to write.
    pub bytes: Vec<u8>,
}

/// A planned Delilas-dome injection: the SCUS-cave writes (routine, relocated
/// template, roster) and the arena-overlay writes (seed detour, template
/// repoint, course-3 descriptor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomeInjection {
    /// Writes into `SCUS_942.54` (the routine + relocated template + roster).
    pub scus: Vec<Write>,
    /// Writes into the arena overlay PROT entry (detour + repoint + descriptor).
    pub overlay: Vec<Write>,
}

impl DomeInjection {
    /// Plan the injection. Fails (rather than corrupts) if the build isn't
    /// recognized: the SCUS cave must be all-zero dead space within the gap,
    /// and each overlay hook site must hold its known stock word.
    pub fn plan(scus: &[u8], overlay: &[u8]) -> Result<Self> {
        // --- SCUS cave: routines + template + roster -------------------------
        let words_to_bytes =
            |w: Vec<u32>| -> Vec<u8> { w.iter().flat_map(|w| w.to_le_bytes()).collect() };
        let routine = words_to_bytes(assemble_routine());
        let seat = words_to_bytes(assemble_seat_routine());
        let reward = words_to_bytes(assemble_reward_routine());
        if TEMPLATE_VA < ROUTINE_VA + routine.len() as u32 {
            bail!("dome seed routine overruns the template slot");
        }
        if SEAT_ROUTINE_VA < ROSTER_VA + roster_bytes().len() as u32 {
            bail!("dome roster overruns the seat-routine slot");
        }
        if REWARD_ROUTINE_VA < SEAT_ROUTINE_VA + seat.len() as u32 {
            bail!("dome seat routine overruns the reward-routine slot");
        }
        if CAVE_END_VA < REWARD_ROUTINE_VA + reward.len() as u32 {
            bail!("dome reward routine overruns the cave end");
        }
        if CAVE_END_VA > GAP_END_VA {
            bail!("dome cave overruns the preserved gap end {GAP_END_VA:#x}");
        }
        let cave: [(u32, Vec<u8>); 5] = [
            (ROUTINE_VA, routine),
            (TEMPLATE_VA, TEMPLATE_BYTES.to_vec()),
            (ROSTER_VA, roster_bytes()),
            (SEAT_ROUTINE_VA, seat),
            (REWARD_ROUTINE_VA, reward),
        ];
        let mut scus_writes = Vec::new();
        for (va, bytes) in cave {
            let off = scus_off(scus, va)?;
            let end = off + bytes.len();
            if scus.get(off..end).is_none_or(|w| w.iter().any(|&b| b != 0)) {
                bail!("dome cave region {va:#x} is not all-zero dead space - refusing to patch");
            }
            scus_writes.push(Write { off, bytes });
        }

        // --- Arena overlay: seed detour + template repoint + descriptor ------
        let mut overlay_writes = Vec::new();

        // Seed detour: verify the stock `lw`, replace with `j ROUTINE_VA`.
        let seed_off = overlay_off(SEED_HOOK_VA)?;
        expect_word(overlay, seed_off, SEED_HOOK_ORIG, "seed hook")?;
        overlay_writes.push(Write {
            off: seed_off,
            bytes: j(ROUTINE_VA).to_le_bytes().to_vec(),
        });

        // Template repoint: verify the stock lui+addiu, point at the cave copy.
        let lui_off = overlay_off(TEMPLATE_REF_LUI_VA)?;
        expect_word(overlay, lui_off, TEMPLATE_REF_LUI_ORIG, "template lui")?;
        expect_word(
            overlay,
            lui_off + 4,
            TEMPLATE_REF_ADDIU_ORIG,
            "template addiu",
        )?;
        let repoint = template_ref_words();
        overlay_writes.push(Write {
            off: lui_off,
            bytes: repoint.iter().flat_map(|w| w.to_le_bytes()).collect(),
        });

        // Course-3 descriptor at 0x801D1A20 (over the now-relocated template
        // head); zero the 16-byte template tail so no stale course-4 slot.
        let desc_off = overlay_off(COURSE3_DESC_VA)?;
        let mut desc = descriptor_bytes().to_vec();
        desc.extend_from_slice(&[0u8; 16]);
        // The bytes there are the stock template (non-zero); confirm they match
        // so a re-layout doesn't silently clobber live code.
        expect_bytes(
            overlay,
            desc_off,
            &TEMPLATE_BYTES,
            "descriptor slot (template)",
        )?;
        overlay_writes.push(Write {
            off: desc_off,
            bytes: desc,
        });

        // Second-seat detour: verify the installer's stock seat-1 zero store,
        // replace with `j SEAT_ROUTINE_VA` (its delay slot, the seat-2 zero
        // store, is untouched and still executes).
        let seat_off = overlay_off(SEAT_HOOK_VA)?;
        expect_word(overlay, seat_off, SEAT_HOOK_ORIG, "second-seat hook")?;
        overlay_writes.push(Write {
            off: seat_off,
            bytes: j(SEAT_ROUTINE_VA).to_le_bytes().to_vec(),
        });

        // Reward detour: verify the settlement's stock payout-table load,
        // replace with `j REWARD_ROUTINE_VA` (its delay slot, the Seru-gate
        // `slti`, is untouched and still executes).
        let reward_off = overlay_off(REWARD_HOOK_VA)?;
        expect_word(overlay, reward_off, REWARD_HOOK_ORIG, "reward hook")?;
        overlay_writes.push(Write {
            off: reward_off,
            bytes: j(REWARD_ROUTINE_VA).to_le_bytes().to_vec(),
        });

        Ok(Self {
            scus: scus_writes,
            overlay: overlay_writes,
        })
    }
}

/// Resolve a SCUS VA to its file offset within the `SCUS_942.54` image.
fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    legaia_asset::item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("cannot resolve SCUS VA {va:#x}"))
}

/// Resolve an arena-overlay VA to its raw PROT-entry file offset.
fn overlay_off(va: u32) -> Result<usize> {
    va.checked_sub(ARENA_BASE_VA)
        .map(|d| d as usize)
        .ok_or_else(|| anyhow::anyhow!("overlay VA {va:#x} below base {ARENA_BASE_VA:#x}"))
}

fn expect_word(buf: &[u8], off: usize, want: u32, what: &str) -> Result<()> {
    let got = read_word(buf, off)?;
    if got != want {
        bail!("{what} at file +{off:#x} = {got:#010x}, expected {want:#010x} (unrecognized build)");
    }
    Ok(())
}

fn expect_bytes(buf: &[u8], off: usize, want: &[u8], what: &str) -> Result<()> {
    let got = buf
        .get(off..off + want.len())
        .ok_or_else(|| anyhow::anyhow!("{what} at +{off:#x} out of bounds"))?;
    if got != want {
        bail!("{what} at file +{off:#x} does not match the known build - refusing to patch");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
        assert_eq!(r.len(), 14);
        // Opens by loading the flag id and testing it.
        assert_eq!(r[0], addiu(A0, ZERO, COURSE_FLAG));
        assert_eq!(r[1], jal(FLAG_TEST_FUNC_VA));
        // The flag-clear branch target is the replay `lw`.
        assert_eq!(r[11], SEED_HOOK_ORIG);
        // beq at idx 3 skips to idx 11 (SKIP): off = 11 - (3+1) = 7.
        assert_eq!(r[3], beq(V0, ZERO, 7));
        // Ends by returning to the decode.
        assert_eq!(r[12], j(SEED_RETURN_VA));
        // Seeds the course-3 word and clears the flag on the set path.
        assert_eq!(r[7], sw(T0, S1, WORD_OFF_FROM_S1));
        assert_eq!(r[9], jal(FLAG_CLEAR_FUNC_VA));
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
        // Che first (with the surprise Lu seat), Gi as the closer.
        assert_eq!(DELILAS_ROSTER_IDS[0], 163);
        assert_eq!(DELILAS_ROSTER_IDS[1], 162);
        assert_eq!(SECOND_SEAT_ID, 164);
    }

    #[test]
    fn template_repoint_targets_cave() {
        let w = template_ref_words();
        assert_eq!(w[0], lui(A0, hi(TEMPLATE_VA)));
        assert_eq!(w[1], addiu(A0, A0, lo(TEMPLATE_VA)));
    }

    #[test]
    fn seat_routine_shape() {
        let r = assemble_seat_routine();
        assert_eq!(r.len(), 13);
        // Reads course + round off the arena globals.
        assert_eq!(r[1], lw(T1, T0, lo(COURSE_GLOBAL_VA)));
        assert_eq!(r[2], lw(T2, T0, lo(ROUND_GLOBAL_VA)));
        // Course-3 round-0 arm seats Lu into formation seat 1; both `j`
        // delay slots store into seat 1 (`1(v0)`), never elsewhere.
        assert_eq!(r[9], j(SEAT_RETURN_VA));
        assert_eq!(r[10], sb(T4, V0, 1));
        // Default arm replays the displaced stock zero store.
        assert_eq!(r[11], j(SEAT_RETURN_VA));
        assert_eq!(r[12], SEAT_HOOK_ORIG);
        // Branch offsets: idx5 -> DEF(11) = +5, idx7 -> DEF(11) = +3.
        assert_eq!(r[5], bne(T1, T3, 5));
        assert_eq!(r[7], bne(T2, ZERO, 3));
    }

    #[test]
    fn reward_routine_shape() {
        let r = assemble_reward_routine();
        assert_eq!(r.len(), 10);
        // Replays the displaced table load first (t1 = table value).
        assert_eq!(r[0], lw(T1, V0, 0));
        // Course-3 arm returns the flat clear payout (5000; the const block
        // pins that it fits a positive `addiu` immediate).
        const {
            assert!(COURSE3_CLEAR_COINS == 5000);
            assert!(COURSE3_CLEAR_COINS <= 0x7FFF);
        }
        assert_eq!(r[6], j(REWARD_RETURN_VA));
        assert_eq!(r[7], addiu(V0, ZERO, COURSE3_CLEAR_COINS as u16));
        // Stock arm returns the table value.
        assert_eq!(r[8], j(REWARD_RETURN_VA));
        assert_eq!(r[9], addu(V0, T1, ZERO));
        // Branch offset: idx4 -> STOCK(8) = +3.
        assert_eq!(r[4], bne(T2, T3, 3));
    }

    #[test]
    fn cave_fits_the_gap() {
        assert!(ROUTINE_VA + assemble_routine().len() as u32 * 4 <= TEMPLATE_VA);
        assert!(TEMPLATE_VA + TEMPLATE_BYTES.len() as u32 <= ROSTER_VA);
        assert!(ROSTER_VA + roster_bytes().len() as u32 <= SEAT_ROUTINE_VA);
        assert_eq!(SEAT_ROUTINE_VA % 4, 0, "seat routine is a j target");
        assert!(SEAT_ROUTINE_VA + assemble_seat_routine().len() as u32 * 4 <= REWARD_ROUTINE_VA);
        assert_eq!(REWARD_ROUTINE_VA % 4, 0, "reward routine is a j target");
        let end = REWARD_ROUTINE_VA + assemble_reward_routine().len() as u32 * 4;
        assert!(end <= CAVE_END_VA);
        // The cave must stay below the live SsAPI I/O table at GAP_END_VA.
        const { assert!(CAVE_END_VA <= GAP_END_VA) }
    }
}
