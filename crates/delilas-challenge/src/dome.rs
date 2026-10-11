//! **Delilas Challenge** as a real Muscle Dome arena *course* - the code-hook
//! half of the feature (the arena overlay PROT 0977 side + a SCUS routine
//! cave). The casino-menu / warp half is [`crate::enrollment`].
//!
//! ## Why the dome arena, and why a hook
//!
//! Live testing killed the earlier "launch a normal battle from the casino"
//! approach: koin1 is a town scene with no random encounters, so it never
//! installs the battle-effect / summon / player-magic asset residency, and a
//! spell cast (or opening the magic list) dereferences unloaded buffers and
//! freezes. That is exactly why the retail Muscle Dome disables magic - its
//! arena doesn't load those assets. And the battle heap's distinct-monster
//! budget (~145 KB) cannot seat two full Delilas blocks at once.
//!
//! Routing the challenge through the dome's own arena fixes the residency
//! problem, and the slim-clone stream (below) fixes the heap one. A dome
//! contest is a *course* - a fixed
//! roster fought one round at a time (the installer `FUN_801D1510` reads the
//! round's monster id from a course roster and seats it as the sole enemy).
//! So the Delilas Challenge is a **2-round course**: Che & Lu together
//! (1v2), then Gi (1v1).
//!
//! ## How the double-team round fits the battle heap
//!
//! Two distinct Delilas blocks (163-166 KB of pre-texture bytes) overshoot
//! the measured ~145 KB distinct-monster budget by ~20-25 KB - a naive 1v2
//! froze at the round-1 load (`docs/subsystems/battle.md`, heap-budget
//! section: the failed malloc returns NULL and the loader copies through it
//! unchecked). Neither VRAM nor the AI is involved, so the fix is to shrink
//! what the loader *streams*, without touching what the game *is*:
//!
//! - **Slim clones.** `legaia_actor_formats::monster_archive::slim_castables`
//!   rebuilds Che's and Lu's blocks minus their generic-AI castable spell
//!   entries (the ~5-6 KB packed keyframe streams the AI picker can roll;
//!   mesh, stats, name, reactions, approach/special entries, and the two
//!   `agl=0xFF` choreography entries all survive byte-identical). Entry
//!   count + index space are preserved (dropped slots alias the basic
//!   attack): the engine addresses animations by raw entry index, so the
//!   specials must stay at their retail indices. The slim pair costs
//!   ~133 KB - under budget. The clones are written into
//!   [`CLONE_IDS`] (190/191), two archive slots **no formation, encounter,
//!   or dome roster on the disc ever references**; the originals at 163/164
//!   are never modified, so the ravine duels and the Master course keep
//!   every move.
//! - **Stream-map detour.** The formation still seats the *real* ids
//!   163/164 - the bespoke attack-attack-special AI arm (`case 0xa2..0xa4`
//!   keys on the formation cell), the name, and every id-keyed table stay
//!   genuine. A cave routine hooked at the monster streamer's single
//!   id-to-slot-offset site ([`STREAM_HOOK_VA`]) adds [`CLONE_ID_OFFSET`]
//!   to the id **only for the archive fetch**, and only while the course
//!   word says the Delilas course is running - a ravine duel or Master
//!   round streams the untouched originals.
//!
//! The second seat itself comes from a detour at the installer's seat-1 zero
//! store ([`SEAT_HOOK_VA`]): round 0 seats Lu (164) beside the roster's Che.
//!
//! ## The course edits (PROT 0977 + the SCUS cave; koin1 is separate)
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
//!    option): if set **and the retail flag seed left the word at its
//!    no-course default `1`** (only the Delilas arm, which clears all three
//!    course-unlock flags, produces that - a retail difficulty arm's own
//!    seed `0x101`/`0x111`/`0x321` wins the guard), it stores
//!    [`COURSE3_SEED_WORD`] (course 3, round 0, dome-marker bit) into the
//!    word; either way it clears the flag, then replays the displaced `lw`
//!    and returns. Clearing on both paths makes the flag one-shot *and*
//!    scrubs a stale one - a save state frozen during the warp flourish
//!    otherwise redirected every later dome entry (Beginner included) into
//!    the Delilas course. The word carries course 3 through the remaining
//!    rounds (the continuing-leg path), and a later normal dome entry reads
//!    `0x539` clear.
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
//!    entries - Che (163) for round 0 (the seat detour adds Lu beside her)
//!    and Gi (162) for round 1 - reusing the dome's own Delilas name strings
//!    (resident with 0977) so the ROUND banner shows the right names.
//! 5. **Seed routine** ([`ROUTINE_VA`]): the seed test/clear above.
//! 6. **Reward detour** ([`REWARD_HOOK_VA`] `0x801D1118`, the settlement's
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
//! same-size overwrites in the arena overlay and one same-size SCUS code
//! hook ([`STREAM_HOOK_VA`]); an unrecognized build is refused, not
//! corrupted. The slim-clone archive slots are written by the apply layer
//! (`legaia_patcher::apply`), which builds them from the user's own disc.

use legaia_code_hooks::mips::*;

mod battle_ai;
mod injection;
mod routines;

pub use battle_ai::{
    AI_ARM_JOIN_VA, AI_BLOCK_CAPACITY, AI_BLOCK_ORIG_HEAD, AI_BLOCK_ORIG_HEAD2, AI_BLOCK_VA,
    AI_HOOK_ORIG, AI_HOOK_PREV_ORIG, AI_HOOK_VA, AI_PERIOD_MASK, AI_SPECIAL_PHASE,
    AI_STOCK_RESUME_VA, GI_ARM_CAPACITY, GI_ARM_ORIG_HEAD, GI_ARM_ORIG_WORD2, GI_ARM_VA,
    GI_DIVIDE_SPELL, GI_FLAGS_VA, GI_ROUND_WORD, GI_SPORE_GAS_MIN_TURN, GI_SPORE_GAS_SPELL,
    assemble_ai_block, assemble_gi_arm,
};
pub use injection::{DomeInjection, Write};
pub use routines::{
    assemble_reward_routine, assemble_routine, assemble_seat_routine, assemble_stream_map_routine,
    assemble_stream2_map_routine, descriptor_bytes, plan_routine_refresh, probe_ram_writes,
    roster_bytes, template_ref_words,
};

/// PROT entry index of the Muscle Dome arena overlay ("other_game" / slot-A
/// base `0x801CE818`).
pub const ARENA_OVERLAY_PROT_INDEX: usize = 977;

/// Load base VA of the arena overlay; a VA inside it maps to PROT-entry file
/// offset `va - ARENA_BASE_VA`.
pub const ARENA_BASE_VA: u32 = 0x801C_E818;

/// One-shot SYSTEM flag the koin1 Delilas option sets to request course 3.
/// Sits just past the retail course-unlock flags (`0x536`/`0x537`/`0x538`);
/// the seed routine clears it whenever it finds it set - after seeding on a
/// Delilas entry, and as a scrub on a retail-class entry that inherited a
/// stale flag (e.g. from a save state frozen during the warp flourish) -
/// so it never persists into a normal dome entry. Absent from the
/// disc-wide field/motion flag censuses.
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

/// The Delilas roster ids, in fight order: round 0 = Che (Lu joins via the
/// seat detour), round 1 = Gi.
pub const DELILAS_ROSTER_IDS: [u32; 2] = [163, 162];
/// The dome's own Delilas name-string VAs (resident with 0977), paired with
/// [`DELILAS_ROSTER_IDS`] so the ROUND banner shows "Che/Gi Delilas".
pub const DELILAS_NAME_PTRS: [u32; 2] = [0x801C_E8C4, 0x801C_E8B8];
/// The monster id the seat detour adds beside the roster's Che in round 0.
pub const SECOND_SEAT_ID: u32 = 164;
/// Coins added to the dome winnings counter (`0x80084440`) for a full clear
/// of the Delilas course.
pub const COURSE3_CLEAR_COINS: u32 = 5000;

// --- Seat hook (installer `FUN_801D1510`) ------------------------------------

/// Second-seat detour site: the installer's `sb zero,1(v0)` that zeroes
/// formation seat 1 (`0x8007BD0D`). The installer stages the round as raw id
/// bytes at `0x8007BD0C..0F` (seat 0 = the roster id, seats 1..3 zeroed;
/// battle setup counts the non-zero seats). Replaced with
/// `j SEAT_ROUTINE_VA`.
pub const SEAT_HOOK_VA: u32 = 0x801D_15A4;
/// The stock instruction at [`SEAT_HOOK_VA`] (`sb zero,1(v0)`) - the displaced
/// store the routine replays on the non-Delilas path, and the build fingerprint.
pub const SEAT_HOOK_ORIG: u32 = sb(ZERO, V0, 1);
/// Where the seat detour returns (the `sb zero,3(v0)` after the delay slot).
pub const SEAT_RETURN_VA: u32 = 0x801D_15AC;

// --- Stream-map hook (monster streamer `FUN_800542C8`) -----------------------

/// Offset added to a Delilas pair id (163/164) to reach its slim-clone
/// archive slot: `163 -> 190`, `164 -> 191` (same offset for both, so one
/// `addiu` remaps either).
pub const CLONE_ID_OFFSET: u32 = 27;
/// The slim-clone archive slots. Battle-unreachable on the retail disc: no
/// encounter formation, scripted-battle row, or dome-course roster references
/// ids 190/191 (full-disc formation census + the three retail dome rosters),
/// and they are outside the `--unused-enemies` pool.
pub const CLONE_IDS: [u16; 2] = [190, 191];
/// The two original ids the stream map redirects (Che, Lu).
pub const DELILAS_PAIR_IDS: [u16; 2] = [163, 164];

/// Per-sibling slim policy, probe-traced against the shipped course.
///
/// Streamed special-move modules stage block entries by **raw index**
/// (Lu's Plasma Strike, action `0x7B`, stages `14 -> 12 -> 13`; Che's,
/// action `0x7A`, stages `10 -> 11`), and an aliased stand-in never
/// satisfies the module's completion wait - the caster loops the approach
/// run forever. So each sibling's choreography entries are `protected`.
/// Lu additionally force-drops entry 11 (a `0x23` special her own special
/// does NOT stage) to pay for the protected pair: the reclaimed bytes keep
/// the in-battle transient allocs (damage popups, effect instances) from
/// starving at `[163,164]`. Her rollable keeper stays a REAL castable
/// (entry 7, `0x0D`): promoting the never-rolled choreography entry 12 to
/// rollable was tried and wedges her first generic cast (the whole round
/// order idles forever) - choreography entries are not standalone casts.
///
/// Returns `(protected_entry_indices, extra_drop_indices)` for a pair id.
pub fn slim_policy(src_id: u16) -> (&'static [usize], &'static [usize]) {
    match src_id {
        164 => (&[12, 13], &[11]),
        _ => (&[], &[]),
    }
}

/// Stream-map detour site A: the id-to-slot-offset conversion in the monster
/// streamer `FUN_800542C8` (`addiu v1,a0,-0x1` at `0x8005451C`, feeding
/// `(id-1)*5 << 14` = `(id-1)*0x14000`). `a0` holds the formation id here
/// and is dead after; the hook's own delay slot (the staging-pointer store
/// at `0x80054520`) is unrelated and still executes. Replaced with
/// `j STREAM_ROUTINE_VA`. This is a **SCUS** site, not an overlay one.
///
/// The battle load streams monsters through **two** paths: the first
/// (lowest-id) enemy is pre-streamed by `FUN_80054A6C` during party setup
/// (its scan loop reads the enemy formation cells, seeks `(id-1)*0x14000`,
/// and raises the loaded-count `DAT_8007B649` so `FUN_800542C8` decodes the
/// staged slot without re-seeking), and every further distinct enemy seeks
/// through this site. Round 0 therefore fetches Che via site B and Lu via
/// site A - each site only ever needs its one sibling remapped, which is
/// what lets both routines share the compact single-test shape.
pub const STREAM_HOOK_VA: u32 = 0x8005_451C;
/// The stock instruction at [`STREAM_HOOK_VA`] (`addiu v1,a0,-0x1`) - the
/// displaced conversion the routine replays, and the build fingerprint.
pub const STREAM_HOOK_ORIG: u32 = addiu(V1, A0, 0xFFFF);
/// Where the site-A detour returns (the `sll v0,v1,0x2` of the `*0x14000`).
pub const STREAM_RETURN_VA: u32 = 0x8005_4524;

/// Stream-map detour site B: in the pre-streamer `FUN_80054A6C`, one
/// instruction **before** its `addiu v0,v1,-0x1` conversion. Hooking the
/// conversion itself is impossible: a `j` detour's branch-delay slot
/// executes before the routine, and the conversion's successor is the
/// `sll v1,v0,0x2` that clobbers `v1` - the id register - with stale data.
/// Hooking the preceding `ori a0,a0,0x2800` instead leaves only the
/// harmless conversion in the delay shadow (it recomputes after return),
/// so the routine sees the id intact. Replaced with `j STREAM2_ROUTINE_VA`.
pub const STREAM2_HOOK_VA: u32 = 0x8005_4B70;
/// The stock instruction at [`STREAM2_HOOK_VA`] (`ori a0,a0,0x2800` - the
/// staging-offset materialisation) - replayed in the routine's return delay.
pub const STREAM2_HOOK_ORIG: u32 = ori(A0, A0, 0x2800);
/// The stock instruction in the hook's delay slot (`addiu v0,v1,-0x1`) -
/// runs once pre-routine on the intact id (result discarded) and once after
/// return with the possibly-remapped id. A second build fingerprint.
pub const STREAM2_DELAY_ORIG: u32 = addiu(V0, V1, 0xFFFF);
/// Where the site-B detour returns: the conversion itself, which re-runs
/// with the remapped id and feeds the stock multiply.
pub const STREAM2_RETURN_VA: u32 = 0x8005_4B74;

// --- Magic lockout (battle-action overlay, PROT 0898) ------------------------

/// PROT entry index of the battle-action overlay - the round driver
/// `FUN_801D0748` that every battle (dome legs included) runs.
pub const BATTLE_OVERLAY_PROT_INDEX: usize = 898;
/// Load base VA of the battle-action overlay.
pub const BATTLE_BASE_VA: u32 = 0x801C_E818;
/// The round driver's two Magic-command input arms each reject the
/// selection when `_DAT_8007BAC0 & 0x200` is set - the Master course's
/// magic-lockout bit (`word = 0x321`); Beginner/Expert (`0x101`/`0x111`)
/// lack it and genuinely allow magic. The arena never installs the summon /
/// player-magic sound+art residency, and a live test proved a cast in the
/// Delilas course corrupts the audio state (the koin1 magic-freeze class).
/// Widening each test's mask to `0x300` makes the reject fire on the
/// dome-contest marker bit `0x100` that **every** contest seed carries -
/// so the course (word `0x131`) locks magic through retail's own reject
/// path, with no cave code and no seed-word change. (Retail Beginner /
/// Expert legs lose their latent magic access too - the same corruption
/// waits there, so this is a fix, not a loss.)
pub const MAGIC_REJECT_SITES: [u32; 2] = [0x801D_12E4, 0x801D_1450];
/// The stock instruction at each reject site (`andi v0,v0,0x200`).
pub const MAGIC_REJECT_ORIG: u32 = andi(V0, V0, 0x200);
/// The widened test (`andi v0,v0,0x300`).
pub const MAGIC_REJECT_NEW: u32 = andi(V0, V0, 0x300);

// --- Winnings-display hook (`FUN_801D1184`, arena overlay) -------------------

/// The results screen's own payout-table read: `FUN_801D1184` loads
/// `*(0x801D1860 + course*0x40 + (round-1)*4)` into the winnings-display
/// variable `DAT_801D1AAC` - a second, display-only reader the settlement's
/// reward detour does not cover, so a cleared course *paid* 5000 while the
/// end screen still *said* 0 (the course-3 index reads past the table).
/// The final address `addu` at `0x801D125C` becomes `j` into a small gated
/// override; the displaced table `lw` runs in the delay slot with the
/// unfinished address (a word-aligned low-RAM read - harmless) and the
/// routine recomputes it. On entry `v1` still holds `course << 6`, so the
/// course test costs one immediate.
pub const DISPLAY_HOOK_VA: u32 = 0x801D_125C;
/// The stock instruction at [`DISPLAY_HOOK_VA`] (`addu v0,v0,a0` - the final
/// table-address add the routine replays) - the build fingerprint.
pub const DISPLAY_HOOK_ORIG: u32 = addu(V0, V0, A0);
/// The word after the hook (the table `lw v1,0(v0)` that becomes the delay
/// slot) - pinned as a second fingerprint.
pub const DISPLAY_HOOK_DELAY_ORIG: u32 = lw(V1, V0, 0);
/// Where the display override returns (the `lui` before the
/// `sw v1,0x1AAC`).
pub const DISPLAY_RETURN_VA: u32 = 0x801D_1264;

/// The display override's home: `FUN_800260DC`, the SCUS per-mode camera
/// preset - 16 instructions with **zero references of any form** in any
/// image (the same five-form sweep that cleared the AI block's home;
/// `scripts/ci/port-catalog-ignore.toml` `[unreferenced]`). The Gi arm
/// grew the AI block past sharing `FUN_80035274`'s body with the display
/// override, so the override claims the second unreferenced body.
pub const DISPLAY_ROUTINE_VA: u32 = 0x8002_60DC;
/// First stock word of the overwritten camera preset (`lui v0,0x8008`) -
/// the build fingerprint.
pub const DISPLAY_HOME_ORIG_HEAD: u32 = lui(V0, 0x8008);
/// Second stock word (`sw zero,0x40b8(v0)`) - a second fingerprint pin.
pub const DISPLAY_HOME_ORIG_HEAD2: u32 = sw(ZERO, V0, 0x40B8);
/// Capacity of the overwritten camera-preset body (16 instructions).
pub const DISPLAY_ROUTINE_CAPACITY: usize = 16 * 4;

/// Assemble the winnings-display override (8 code words + 1 data word, over
/// `FUN_800260DC`). Entry: `v0` = `(round-1)*4 + (course<<6)`, `a0` = table
/// base, `v1` = `course << 6`. The trailing zero word is [`GI_FLAGS_VA`],
/// the Gi arm's one-shot flags cell - included in the write because the
/// overwritten body holds code there, not zeros, and the cell must start
/// clear.
pub fn assemble_display_routine() -> Vec<u32> {
    const SKIP: usize = 5;
    let words = vec![
        addiu(T0, ZERO, 3 << 6),                     // 0: course 3, pre-shifted
        bne(V1, T0, (SKIP - (1 + 1)) as i16),        // 1: not course 3 -> stock
        addu(V0, V0, A0),                            // 2: replay addr add (delay)
        j(DISPLAY_RETURN_VA),                        // 3
        addiu(V1, ZERO, COURSE3_CLEAR_COINS as u16), // 4: display 5000 (delay)
        // SKIP (idx 5): stock display value.
        lw(V1, V0, 0),        // 5
        j(DISPLAY_RETURN_VA), // 6
        nop(),                // 7
        0,                    // 8: GI_FLAGS_VA (data - the Gi one-shot flags)
    ];
    debug_assert_eq!(words.len(), 9);
    words
}

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
/// The hub's koin1 menu-selection residue (`0x80084140 + 0x308`). The
/// round-end routing (`0x801CEE44` in the arena overlay) treats the value
/// **4** as "the player chose the quit option": it clears the win latch,
/// sets the ran-away marker `DAT_801D1A74`, and the settlement then ZEROES
/// the winnings counter and skips the payout add. The Delilas enrollment
/// rides the who-menu's 4th slot, so its residue is exactly 4 - every
/// cleared course paid nothing and wiped the balance until the seat
/// routine started clearing this cell at round-0 install.
pub const MENU_RESIDUE_VA: u32 = 0x8008_4448;
/// The on-screen dev error reporter's `PRG ERR%d` arm (reporter at
/// `0x80016444`; the only referent of the `"PRG ERR%d"` string at
/// `0x80010100`): it prints when the **malloc-failure accumulator**
/// `gp+0x510` = `0x8007B828` is non-zero - the same accumulator the malloc
/// wrapper bumps on every failed allocation. The 1v2's effect-alloc bursts
/// feed it directly (each failed transient `0x9C` spawn is tolerated but
/// counted), so the patch silences the print at its gate: the `beqz a1`
/// guarding the print `jal` becomes an unconditional branch. The
/// WORK/READ/CD error arms and the accumulator itself are untouched.
///
/// An earlier build instead NOPed the effect spawner's `ori 0x4000` into
/// `DAT_80083828` - **falsified by a live test** (the text still painted):
/// the reporter never reads that word, and no reference to it exists in any
/// image (address-word scan). Do not re-walk that path.
pub const PRGERR_PRINT_GATE_VA: u32 = 0x8001_64D4;
/// The stock instruction at [`PRGERR_PRINT_GATE_VA`] (`beqz a1, +4` - skip
/// the print when the accumulator is zero) - the build fingerprint.
pub const PRGERR_PRINT_GATE_ORIG: u32 = beq(A1, ZERO, 4);
/// The replacement: `b +4` - skip the print unconditionally.
pub const PRGERR_PRINT_GATE_NEW: u32 = beq(ZERO, ZERO, 4);
/// Round global (`DAT_801D1A94`), sibling of [`COURSE_GLOBAL_VA`].
pub const ROUND_GLOBAL_VA: u32 = 0x801D_1A94;

// --- SCUS routine cave (the preserved rodata gap 0x8007AB38..0x8007AF40) -----
// Placed in the free tail after the flee-EXP routine (0x8007AD00..0x8007AE00),
// so this composes with every other gap feature.

/// Load VA of the seed routine in the preserved SCUS gap.
pub const ROUTINE_VA: u32 = 0x8007_AE00;
/// Load VA of the relocated hub actor template (24 bytes).
pub const TEMPLATE_VA: u32 = 0x8007_AE38;
/// Load VA of the cave roster (2 x 8 bytes).
pub const ROSTER_VA: u32 = 0x8007_AE50;
/// Load VA of the second-seat routine (4-aligned - it is a `j` target).
pub const SEAT_ROUTINE_VA: u32 = 0x8007_AE60;
/// Load VA of the reward routine (4-aligned - it is a `j` target).
pub const REWARD_ROUTINE_VA: u32 = 0x8007_AE8C;
/// Load VA of the site-A stream-map routine (4-aligned - a `j` target).
pub const STREAM_ROUTINE_VA: u32 = 0x8007_AEB0;
/// Load VA of the site-B stream-map routine (4-aligned - a `j` target).
pub const STREAM2_ROUTINE_VA: u32 = 0x8007_AED8;
/// One past the last cave byte used; must stay within the gap end.
pub const CAVE_END_VA: u32 = 0x8007_AF00;
/// End of the usable zero window (exclusive): the SsAPI sound I/O register
/// table begins exactly at `0x8007AF00` and is read every frame (the
/// shiny-seru read-watch pinned it - see `legaia_code_hooks::arenas`), so the cave
/// must end below it even though the bytes scan as zero.
pub const GAP_END_VA: u32 = 0x8007_AF00;

#[cfg(test)]
mod tests;
