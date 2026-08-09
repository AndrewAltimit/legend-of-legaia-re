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
//! dome loads one boss per round, disables magic by design, and runs the fight
//! in a real arena. A dome contest is a *course* - a fixed roster fought one
//! round at a time (the installer `FUN_801D1510` reads the round's monster id
//! from a course roster and seats it as the sole enemy). So the Delilas
//! Challenge is a **new 3-round course**: Gi -> Che -> Lu, one boss per round.
//!
//! ## The five edits (all in PROT 0977 + a SCUS cave; koin1 is separate)
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
//!    option): if set, it stores `0x431` (course 3, round 0) into the word and
//!    clears the flag, then replays the displaced `lw` and returns. Clearing
//!    the flag makes it one-shot - the word carries course 3 through the
//!    remaining rounds (the continuing-leg path), and a later normal dome
//!    entry reads `0x539` clear.
//! 2. **Course descriptor** for course 3 at [`COURSE3_DESC_VA`] (`0x801D1A20`
//!    = the descriptor table base `0x801D1A08` + `3*8`): `{i32 round_count=3;
//!    ptr first_round = cave roster}`. All four descriptor readers compute
//!    `base + course*8`, so writing the slot makes course 3 work everywhere
//!    with no per-reader hook.
//! 3. **Actor-template relocation.** `0x801D1A20` currently holds the hub
//!    actor template (24 bytes, referenced once at [`TEMPLATE_REF_LUI_VA`] /
//!    `+4`). Its 24 bytes are copied into the cave and that one `lui/addiu`
//!    pair is repointed there, freeing `0x801D1A20` for the descriptor.
//! 4. **Cave roster** ([`ROSTER_VA`]): three `{u32 name_ptr; u32 monster_id}`
//!    entries - Gi (162) / Che (163) / Lu (164) - reusing the dome's own
//!    Delilas name strings (`0x801CE8B8/0x801CE8C4/0x801CE8D0`, resident with
//!    0977) so the ROUND banner shows the right names.
//! 5. **Cave routine** ([`ROUTINE_VA`]): the seed test/clear above.
//!
//! The course-length clamp (`0x801CED28`) is Master-only (`bne course,2`), so
//! course 3 uses its descriptor's `round_count=3` verbatim - three rounds,
//! no clamp. Everything lives in the loaded-and-preserved SCUS rodata gap
//! (the window every code-injection feature shares) + same-size overwrites in
//! the arena overlay; an unrecognized build is refused, not corrupted.

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
/// Number of rounds in the Delilas course.
pub const DELILAS_ROUNDS: u32 = 3;

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

/// The three Delilas roster ids, in fight order (round 1..3): Gi, Che, Lu.
pub const DELILAS_ROSTER_IDS: [u32; 3] = [162, 163, 164];
/// The dome's own Delilas name-string VAs (resident with 0977), paired with
/// [`DELILAS_ROSTER_IDS`] so the ROUND banner shows "Gi/Che/Lu Delilas".
pub const DELILAS_NAME_PTRS: [u32; 3] = [0x801C_E8B8, 0x801C_E8C4, 0x801C_E8D0];

// --- SCUS routine cave (the preserved rodata gap 0x8007AB38..0x8007AF40) -----
// Placed in the free tail after the flee-EXP routine (0x8007AD00..0x8007AE00),
// so this composes with every other gap feature.

/// Load VA of the seed routine in the preserved SCUS gap.
pub const ROUTINE_VA: u32 = 0x8007_AE00;
/// Load VA of the relocated hub actor template (24 bytes).
pub const TEMPLATE_VA: u32 = 0x8007_AE40;
/// Load VA of the cave roster (3 x 8 bytes).
pub const ROSTER_VA: u32 = 0x8007_AE60;
/// One past the last cave byte used; must stay within the gap end.
pub const CAVE_END_VA: u32 = 0x8007_AE78;
/// End of the preserved zero gap (exclusive) - shared with the other features.
pub const GAP_END_VA: u32 = 0x8007_AF40;

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

/// The cave roster bytes: 3 x `{u32 name_ptr; u32 monster_id}`, little-endian.
pub fn roster_bytes() -> Vec<u8> {
    let mut v = Vec::with_capacity(24);
    for i in 0..3 {
        v.extend_from_slice(&DELILAS_NAME_PTRS[i].to_le_bytes());
        v.extend_from_slice(&DELILAS_ROSTER_IDS[i].to_le_bytes());
    }
    v
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
        // --- SCUS cave: routine + template + roster --------------------------
        let routine: Vec<u8> = assemble_routine()
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        if TEMPLATE_VA < ROUTINE_VA + routine.len() as u32 {
            bail!("dome seed routine overruns the template slot");
        }
        if CAVE_END_VA > GAP_END_VA {
            bail!("dome cave overruns the preserved gap end {GAP_END_VA:#x}");
        }
        let cave: [(u32, Vec<u8>); 3] = [
            (ROUTINE_VA, routine),
            (TEMPLATE_VA, TEMPLATE_BYTES.to_vec()),
            (ROSTER_VA, roster_bytes()),
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
        assert_eq!(b.len(), 24);
        for i in 0..3 {
            let name = u32::from_le_bytes(b[i * 8..i * 8 + 4].try_into().unwrap());
            let id = u32::from_le_bytes(b[i * 8 + 4..i * 8 + 8].try_into().unwrap());
            assert_eq!(name, DELILAS_NAME_PTRS[i]);
            assert_eq!(id, DELILAS_ROSTER_IDS[i]);
        }
        // Gi, Che, Lu order (the fight sequence).
        assert_eq!(DELILAS_ROSTER_IDS[0], 162);
        assert_eq!(DELILAS_ROSTER_IDS[2], 164);
    }

    #[test]
    fn template_repoint_targets_cave() {
        let w = template_ref_words();
        assert_eq!(w[0], lui(A0, hi(TEMPLATE_VA)));
        assert_eq!(w[1], addiu(A0, A0, lo(TEMPLATE_VA)));
    }

    #[test]
    fn cave_fits_the_gap() {
        assert!(ROUTINE_VA + assemble_routine().len() as u32 * 4 <= TEMPLATE_VA);
        assert!(TEMPLATE_VA + TEMPLATE_BYTES.len() as u32 <= ROSTER_VA);
        let roster_end = ROSTER_VA + roster_bytes().len() as u32;
        assert!(roster_end <= CAVE_END_VA);
        assert!(
            roster_end <= GAP_END_VA,
            "cave stays within the preserved gap"
        );
    }
}
