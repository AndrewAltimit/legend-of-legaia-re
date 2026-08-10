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
//! - **Slim clones.** `legaia_asset::monster_archive::slim_castables`
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
//! ([`crate::apply`]), which builds them from the user's own disc.

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

// --- AI retime (the bespoke Delilas arm, battle overlay) ---------------------

/// AI-retime hook site: the bespoke Delilas AI arm (`case 0xa2..0xa4` in the
/// monster picker `FUN_801E9FD4`, battle overlay) queues the signature
/// special (`action = formation_id - 0x29`) whenever the shared battle turn
/// counter `ctx[0x28A]` satisfies `% 3 == 2` - attack, attack, special, with
/// both siblings synchronized in the 1v2. The retimed course round wants the
/// special once per **4** turns with the siblings **staggered 2 apart**, so
/// the arm's ctx reload `lw v0,-0x42dc(v0)` (its `lui` already executed, its
/// delay slot is the stock load-delay `nop` - both safe) becomes
/// `j AI_BLOCK_VA`. The block is course-gated: any context whose course word
/// is not exactly [`COURSE3_SEED_WORD`] (ravine duels, Master rounds, the
/// course's own Gi round) resumes the stock arm with every register as
/// retail left it.
pub const AI_HOOK_VA: u32 = 0x801E_B7C4;
/// The stock instruction at [`AI_HOOK_VA`] (`lw v0,-0x42dc(v0)` - the ctx
/// reload the block replays first) - the build fingerprint.
pub const AI_HOOK_ORIG: u32 = lw(V0, V0, 0xBD24);
/// The word before the hook (`lui v0,0x8008`) - pinned so the hook's
/// register assumption (v0 = `0x80080000` on block entry) stays true.
pub const AI_HOOK_PREV_ORIG: u32 = lui(V0, 0x8008);
/// Stock-arm resume point (the `lbu a0,0x28a(v0)` counter load): the block
/// jumps back here for every non-course-3 context.
pub const AI_STOCK_RESUME_VA: u32 = 0x801E_B7CC;
/// The arm's join (the switch `break`): where both the special-queued and
/// no-special exits land.
pub const AI_ARM_JOIN_VA: u32 = 0x801E_BDAC;

/// The AI block's home: `FUN_80035274`, the SCUS item/equipment
/// passive-NAME draw - a real 48-instruction function with **zero
/// references of any form** in any image (five-form address-word scan;
/// `scripts/ci/port-catalog-ignore.toml` `[unreferenced]`). Overwriting an
/// unreferenced body is the only way left into always-resident SCUS: the
/// preserved rodata gap `0x8007AB38..0x8007AF00` is fully allocated across
/// the injection features, and the battle overlay's own image is packed
/// (`static-overlays.toml`: all `.text+.rodata` RAM-matched live).
pub const AI_BLOCK_VA: u32 = 0x8003_5274;
/// First stock word of the overwritten body (`addiu sp,sp,-0x20`) - the
/// build fingerprint.
pub const AI_BLOCK_ORIG_HEAD: u32 = addiu(SP, SP, 0xFFE0);
/// Second stock word (`lui v1,0x8007`) - a second fingerprint pin.
pub const AI_BLOCK_ORIG_HEAD2: u32 = lui(V1, 0x8007);
/// Capacity of the overwritten body (48 instructions).
pub const AI_BLOCK_CAPACITY: usize = 48 * 4;

/// Retimed special period mask (`% 4`) and firing phase: a sibling queues
/// its special when `(counter + offset) & 3 == 3`. With the stock counter
/// starting the first turn at 0, Lu (offset 2) fires on turns 2, 6, 10...
/// and Che (offset 0) on turns 4, 8, 12... - three regular attacks between
/// each sibling's specials, staggered two turns apart.
pub const AI_PERIOD_MASK: u16 = 3;
/// The remainder that queues the special.
pub const AI_SPECIAL_PHASE: u16 = 3;

/// Assemble the course-3 AI-retime block (28 words, capacity 48).
///
/// Entry state (from the hooked arm): `v0 = 0x80080000` (the arm's own
/// `lui`), `s4` = actor, `s7` = battle slot; `v1`/`a0`/`a3` are dead (the
/// stock arm overwrites each before reading). The stock-resume path must
/// leave `v0` = the battle ctx pointer, which the replayed `lw` provides.
pub fn assemble_ai_block() -> Vec<u32> {
    const NOSPEC: usize = 24;
    const STOCK: usize = 26;
    let neg_word = -(COURSE3_SEED_WORD as i32) as u32 as u16;
    let words = vec![
        lw(V0, V0, 0xBD24),                      //  0: replay: v0 = battle ctx
        lui(A3, 0x8008),                         //  1
        lw(A0, A3, lo(COURSE_WORD_VA)),          //  2: a0 = course word
        addiu(A3, A3, 0xBD0C),                   //  3: a3 = formation base (delay)
        addiu(A0, A0, neg_word),                 //  4: a0 -= 0x131
        bne(A0, ZERO, (STOCK - (5 + 1)) as i16), //  5: not Che&Lu round -> stock
        nop(),                                   //  6
        lbu(A0, V0, 0x28A),                      //  7: a0 = shared turn counter
        addu(V1, S7, A3),                        //  8
        lbu(V1, V1, 0),                          //  9: v1 = formation id
        nop(),                                   // 10: (load delay)
        andi(A3, V1, 1),                         // 11: Che(163)=1, Lu(164)=0
        xori(A3, A3, 1),                         // 12: Che=0, Lu=1
        sll(A3, A3, 1),                          // 13: Che=0, Lu=2 (stagger)
        addu(A0, A0, A3),                        // 14: counter + offset
        andi(A0, A0, AI_PERIOD_MASK),            // 15: % 4
        addiu(A3, ZERO, AI_SPECIAL_PHASE),       // 16
        bne(A0, A3, (NOSPEC - (17 + 1)) as i16), // 17: wrong phase -> no special
        nop(),                                   // 18
        addiu(V0, ZERO, 2),                      // 19
        sb(V0, S4, 0x1DE),                       // 20: actor+0x1DE = 2
        addiu(V1, V1, 0xFFD7),                   // 21: action = id - 0x29
        j(AI_ARM_JOIN_VA),                       // 22
        sb(V1, S4, 0x1DF),                       // 23: queue special (delay)
        // NOSPEC (idx 24):
        j(AI_ARM_JOIN_VA), // 24
        nop(),             // 25
        // STOCK (idx 26): resume the retail arm, v0 = ctx as it expects.
        j(AI_STOCK_RESUME_VA), // 26
        nop(),                 // 27
    ];
    debug_assert!(words.len() * 4 <= AI_BLOCK_CAPACITY);
    words
}

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

/// The display override's home: the free tail of the AI block's
/// 48-instruction body ([`AI_BLOCK_VA`] + 28 words).
pub const DISPLAY_ROUTINE_VA: u32 = 0x8003_52E4;

/// Assemble the winnings-display override (8 words, in the AI block's tail).
/// Entry: `v0` = `(round-1)*4 + (course<<6)`, `a0` = table base, `v1` =
/// `course << 6`.
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
    ];
    debug_assert_eq!(words.len(), 8);
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

/// One same-size write into a target image: `(file_offset, bytes)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    /// File offset within the target (`SCUS_942.54` or the arena overlay).
    pub off: usize,
    /// Little-endian bytes to write.
    pub bytes: Vec<u8>,
}

/// A planned Delilas-dome injection: the SCUS-cave writes (routines,
/// relocated template, roster), the same-size SCUS stream-map hook, and the
/// arena-overlay writes (seed/seat/reward detours, template repoint,
/// course-3 descriptor). The slim-clone archive slots are planned separately
/// by the apply layer (they need the disc's monster archive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomeInjection {
    /// Writes into `SCUS_942.54` (cave regions + the stream-map hook).
    pub scus: Vec<Write>,
    /// Writes into the arena overlay PROT entry (detours + repoint + descriptor).
    pub overlay: Vec<Write>,
    /// Writes into the battle-action overlay PROT entry (the widened
    /// magic-reject masks).
    pub battle: Vec<Write>,
}

impl DomeInjection {
    /// Plan the injection. Fails (rather than corrupts) if the build isn't
    /// recognized: the SCUS cave must be all-zero dead space within the gap,
    /// and each hook site must hold its known stock word.
    pub fn plan(scus: &[u8], overlay: &[u8], battle: &[u8]) -> Result<Self> {
        // --- SCUS cave: routines + template + roster -------------------------
        let words_to_bytes =
            |w: Vec<u32>| -> Vec<u8> { w.iter().flat_map(|w| w.to_le_bytes()).collect() };
        let routine = words_to_bytes(assemble_routine());
        let seat = words_to_bytes(assemble_seat_routine());
        let reward = words_to_bytes(assemble_reward_routine());
        let stream = words_to_bytes(assemble_stream_map_routine());
        let stream2 = words_to_bytes(assemble_stream2_map_routine());
        if TEMPLATE_VA < ROUTINE_VA + routine.len() as u32 {
            bail!("dome seed routine overruns the template slot");
        }
        if SEAT_ROUTINE_VA < ROSTER_VA + roster_bytes().len() as u32 {
            bail!("dome roster overruns the seat-routine slot");
        }
        if REWARD_ROUTINE_VA < SEAT_ROUTINE_VA + seat.len() as u32 {
            bail!("dome seat routine overruns the reward-routine slot");
        }
        if STREAM_ROUTINE_VA < REWARD_ROUTINE_VA + reward.len() as u32 {
            bail!("dome reward routine overruns the stream-routine slot");
        }
        if STREAM2_ROUTINE_VA < STREAM_ROUTINE_VA + stream.len() as u32 {
            bail!("dome site-A stream routine overruns the site-B slot");
        }
        if CAVE_END_VA < STREAM2_ROUTINE_VA + stream2.len() as u32 {
            bail!("dome site-B stream routine overruns the cave end");
        }
        if CAVE_END_VA > GAP_END_VA {
            bail!("dome cave overruns the preserved gap end {GAP_END_VA:#x}");
        }
        let cave: [(u32, Vec<u8>); 7] = [
            (ROUTINE_VA, routine),
            (TEMPLATE_VA, TEMPLATE_BYTES.to_vec()),
            (ROSTER_VA, roster_bytes()),
            (SEAT_ROUTINE_VA, seat),
            (REWARD_ROUTINE_VA, reward),
            (STREAM_ROUTINE_VA, stream),
            (STREAM2_ROUTINE_VA, stream2),
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

        // Stream-map hooks: verify each stock id-1 conversion, replace with
        // a `j` into its cave routine (same-size SCUS code writes; each delay
        // slot is untouched).
        for (hook_va, orig, routine_va, what) in [
            (
                STREAM_HOOK_VA,
                STREAM_HOOK_ORIG,
                STREAM_ROUTINE_VA,
                "stream hook A",
            ),
            (
                STREAM2_HOOK_VA,
                STREAM2_HOOK_ORIG,
                STREAM2_ROUTINE_VA,
                "stream hook B",
            ),
        ] {
            let off = scus_off(scus, hook_va)?;
            expect_word(scus, off, orig, what)?;
            scus_writes.push(Write {
                off,
                bytes: j(routine_va).to_le_bytes().to_vec(),
            });
        }
        // Hook B's delay slot is displaced (replayed by the routine); pin it
        // as a second fingerprint so an unexpected build is refused.
        expect_word(
            scus,
            scus_off(scus, STREAM2_HOOK_VA + 4)?,
            STREAM2_DELAY_ORIG,
            "stream hook B delay slot",
        )?;

        // Silence the dev reporter's `PRG ERR%d` print: under the 1v2's
        // tight heap a transient effect-alloc burst fails (the spawn is
        // skipped, which retail tolerates), but every failure bumps the
        // malloc accumulator the reporter prints from. The `beqz` guarding
        // the print becomes unconditional; the accumulator and the other
        // error arms are untouched.
        let prgerr_off = scus_off(scus, PRGERR_PRINT_GATE_VA)?;
        expect_word(
            scus,
            prgerr_off,
            PRGERR_PRINT_GATE_ORIG,
            "PRG ERR print gate",
        )?;
        scus_writes.push(Write {
            off: prgerr_off,
            bytes: PRGERR_PRINT_GATE_NEW.to_le_bytes().to_vec(),
        });

        // AI-retime block + winnings-display override, contiguous over the
        // unreferenced passive-name draw FUN_80035274 (fingerprint its head;
        // the body is code, not zeros).
        let mut ai_words = assemble_ai_block();
        debug_assert_eq!(
            AI_BLOCK_VA + ai_words.len() as u32 * 4,
            DISPLAY_ROUTINE_VA,
            "display routine sits at the AI block's tail"
        );
        ai_words.extend(assemble_display_routine());
        let ai_block: Vec<u8> = ai_words.iter().flat_map(|w| w.to_le_bytes()).collect();
        if ai_block.len() > AI_BLOCK_CAPACITY {
            bail!("dome AI block overruns the unreferenced body it overwrites");
        }
        let ai_off = scus_off(scus, AI_BLOCK_VA)?;
        expect_word(scus, ai_off, AI_BLOCK_ORIG_HEAD, "AI block home (prologue)")?;
        expect_word(scus, ai_off + 4, AI_BLOCK_ORIG_HEAD2, "AI block home (+4)")?;
        scus_writes.push(Write {
            off: ai_off,
            bytes: ai_block,
        });

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
        expect_word(overlay, seat_off, SEAT_HOOK_ORIG, "seat hook")?;
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

        // Winnings-display detour: the results screen's own table read
        // (`FUN_801D1184`) - without it a cleared course pays 5000 while
        // the end screen says 0.
        let disp_off = overlay_off(DISPLAY_HOOK_VA)?;
        expect_word(overlay, disp_off, DISPLAY_HOOK_ORIG, "display hook")?;
        expect_word(
            overlay,
            disp_off + 4,
            DISPLAY_HOOK_DELAY_ORIG,
            "display hook delay slot",
        )?;
        overlay_writes.push(Write {
            off: disp_off,
            bytes: j(DISPLAY_ROUTINE_VA).to_le_bytes().to_vec(),
        });

        // Magic-reject mask widening in the battle-action overlay: verify
        // each stock `andi v0,v0,0x200`, replace with the `0x300` mask.
        let mut battle_writes = Vec::new();
        for va in MAGIC_REJECT_SITES {
            let off = battle_off(va)?;
            expect_word(battle, off, MAGIC_REJECT_ORIG, "magic-reject mask")?;
            battle_writes.push(Write {
                off,
                bytes: MAGIC_REJECT_NEW.to_le_bytes().to_vec(),
            });
        }

        // AI-retime hook: the bespoke Delilas arm's ctx reload becomes
        // `j AI_BLOCK_VA`. Pin the neighbours the detour depends on: the
        // `lui` before it (v0 = 0x80080000 on block entry) and the stock
        // load-delay `nop` that becomes the hook's delay slot.
        let ai_hook_off = battle_off(AI_HOOK_VA)?;
        expect_word(battle, ai_hook_off - 4, AI_HOOK_PREV_ORIG, "AI hook lui")?;
        expect_word(battle, ai_hook_off, AI_HOOK_ORIG, "AI hook")?;
        expect_word(battle, ai_hook_off + 4, nop(), "AI hook delay slot")?;
        battle_writes.push(Write {
            off: ai_hook_off,
            bytes: j(AI_BLOCK_VA).to_le_bytes().to_vec(),
        });

        Ok(Self {
            scus: scus_writes,
            overlay: overlay_writes,
            battle: battle_writes,
        })
    }
}

/// Resolve a SCUS VA to its file offset within the `SCUS_942.54` image.
fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    legaia_asset::item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("cannot resolve SCUS VA {va:#x}"))
}

/// Resolve a battle-action-overlay VA to its raw PROT-entry file offset.
fn battle_off(va: u32) -> Result<usize> {
    va.checked_sub(BATTLE_BASE_VA)
        .map(|d| d as usize)
        .ok_or_else(|| anyhow::anyhow!("battle-overlay VA {va:#x} below base {BATTLE_BASE_VA:#x}"))
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
        assert!(
            REWARD_ROUTINE_VA + assemble_reward_routine().len() as u32 * 4 <= STREAM_ROUTINE_VA
        );
        assert_eq!(STREAM_ROUTINE_VA % 4, 0, "stream routine A is a j target");
        assert!(
            STREAM_ROUTINE_VA + assemble_stream_map_routine().len() as u32 * 4
                <= STREAM2_ROUTINE_VA
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
        // Course gate: exactly the Che & Lu round word (0x131); everything
        // else (ravine duels, Master rounds, the Gi round 0x132) resumes the
        // stock arm.
        assert_eq!(r[2], lw(A0, A3, lo(COURSE_WORD_VA)));
        assert_eq!(r[4], addiu(A0, A0, 0xFECF));
        assert_eq!(r[5], bne(A0, ZERO, 20)); // idx5 -> STOCK(26)
        assert_eq!(r[26], j(AI_STOCK_RESUME_VA));
        // Retime: (counter + {Che 0, Lu 2}) % 4 == 3 queues the special.
        assert_eq!(r[15], andi(A0, A0, AI_PERIOD_MASK));
        assert_eq!(r[16], addiu(A3, ZERO, AI_SPECIAL_PHASE));
        assert_eq!(r[17], bne(A0, A3, 6)); // idx17 -> NOSPEC(24)
        // Special queue mirrors the stock arm's stores.
        assert_eq!(r[20], sb(V0, S4, 0x1DE));
        assert_eq!(r[21], addiu(V1, V1, 0xFFD7));
        assert_eq!(r[22], j(AI_ARM_JOIN_VA));
        assert_eq!(r[23], sb(V1, S4, 0x1DF));
        assert_eq!(r[24], j(AI_ARM_JOIN_VA));
        // Fingerprint sanity: the hook context words are what the battle
        // overlay really holds (pinned against the dump in plan()).
        assert_eq!(AI_HOOK_PREV_ORIG, lui(V0, 0x8008));
        assert_eq!(AI_BLOCK_ORIG_HEAD, 0x27BD_FFE0);
        assert_eq!(AI_BLOCK_ORIG_HEAD2, 0x3C03_8007);
    }

    #[test]
    fn display_routine_shape() {
        let ai = assemble_ai_block();
        // The display routine starts exactly at the AI block's tail and
        // the pair fits the overwritten 48-instruction body.
        assert_eq!(AI_BLOCK_VA + ai.len() as u32 * 4, DISPLAY_ROUTINE_VA);
        let r = assemble_display_routine();
        assert!((ai.len() + r.len()) * 4 <= AI_BLOCK_CAPACITY);
        // Course test rides the pre-shifted course<<6 still live in v1.
        assert_eq!(r[0], addiu(T0, ZERO, 0xC0));
        assert_eq!(r[1], bne(V1, T0, 3)); // idx1 -> SKIP(5)
        assert_eq!(r[2], DISPLAY_HOOK_ORIG); // replayed address add
        assert_eq!(r[4], addiu(V1, ZERO, COURSE3_CLEAR_COINS as u16));
        assert_eq!(r[5], DISPLAY_HOOK_DELAY_ORIG); // stock table read
        assert_eq!(r[3], j(DISPLAY_RETURN_VA));
        assert_eq!(r[6], j(DISPLAY_RETURN_VA));
        // Stock fingerprints match the arena overlay's real words.
        assert_eq!(DISPLAY_HOOK_ORIG, 0x0044_1021);
        assert_eq!(DISPLAY_HOOK_DELAY_ORIG, 0x8C43_0000);
    }
}
