//! Custom items - the Delilas Challenge completion reward.
//!
//! Three brand-new items, granted together on a course-3 full clear:
//!
//! - **Nature's Elixir** (item id `0xB9`, the cut Ra-Seru-egg slot): restores
//!   an ally's HP *and* MP to full, usable in the field menu and in battle.
//!   Retail keeps the two restore classes separate; this is the missing
//!   convenience item.
//! - **Seru Tear** (item id `0x12`, the cut "Ra-Seru Terra $9" weapon slot):
//!   battle-only. Using it converts the committed item action into a real
//!   Ra-Seru summon cast of **the user's own Ra-Seru** (Vahn -> Meta,
//!   Noa -> Terra, Gala -> Ozma) with the MP cost skipped - a free summon,
//!   usable by any character. Caster-matched is a probe-pinned mechanism
//!   constraint: the summons stream the caster's own choreography, and a
//!   mismatched pair parks the battle forever.
//! - **Fury Bloom** (item id `0x1A`, the cut "Ra-Seru Ozma $8" slot):
//!   battle-only. One use sets the Fury Boost action-gauge flag
//!   (`actor+0x1F9`, the retail class-5 write) on **every living party
//!   member** at once, with the retail Fury cue on the user. (An earlier
//!   design, a "Delilas Tear" casting a sibling signature attack, is
//!   structurally impossible player-side: the streamed modules PROT
//!   958..960 stage the *caster's* monster-block entries by raw index,
//!   and a party actor has no monster block - probe-verified park at
//!   battle state `0x70`.)
//!
//! ## Mechanism map
//!
//! Item records and effect descriptors are same-size static-table edits
//! (`0x80074368` / `0x800752C0`). The two effect classes claimed - `0x48`
//! (elixir) and `0x49` (tears) - are jump-table slots that resolve to the
//! default no-op arm in retail (no descriptor and no module literal carries
//! them; the *reachable* unlisted classes 9/10 are the Stone/Curse arms and
//! are left alone). The menu validator's own class table (`0x80014D70`)
//! points both at the existing always-usable arm `0x80040204`.
//!
//! New code lives across six verified-dead SCUS regions (the same
//! unreferenced-function cave discipline as [`crate::delilas_dome`], plus
//! a **cold-boot** verification pass - see [`FURY_ARM_VA`] for the
//! boot-live libapi slot that falsified the seventh):
//!
//! | Region | Home | Contents |
//! |---|---|---|
//! | `0x80025054` | unreferenced actor-template tick | elixir arm |
//! | `0x8003EDAC` | unreferenced CD mode toggle | conversion stage 1 |
//! | `0x8003F210` | unreferenced CD re-seek arm | conversion stage 2 + the three item names |
//! | `0x8004209C` | the unreachable class-14 Point Card arm | Fury Bloom arm + the MP-deduct skip + descriptions |
//! | `0x800352EC` | tail of the dome's AI-block cave (words 30..47) | the reward grant |
//! | `0x80026100` | tail of the dome's display cave (words 9..15) | the free-cast flag |
//!
//! Battle-overlay hooks (PROT 0898, same-size): the action-seed category
//! dispatch at `0x801E2D60` detours through the conversion (a committed
//! Seru Tear item action becomes `+0x1DE=2` magic casting the user's own
//! Ra-Seru summon),
//! and the summon-leg MP deduct at `0x801E4584` honors the one-shot
//! free-cast flag. Arena-overlay hook (PROT 0977): the settle's post-payout
//! `s0` reload at `0x801D114C` detours through the grant, which gives the
//! three items once per winning course-3 settle (the same latch-gated arm
//! that pays the 5000 coins; the retail War God Icon give at `0x801D1140`
//! is the precedent and has already clobbered the caller-saved registers on
//! its own path, so the grant's `jal`s are safe).

use anyhow::{Result, bail};

use crate::delilas_dome::{COURSE_GLOBAL_VA, Write};
use crate::mips::*;

/// Nature's Elixir claims the empty-name cut-egg item slot.
pub const ELIXIR_ITEM_ID: u8 = 0xB9;
/// Honey - the retail all-stats-+4 consumable, and the reward the grant
/// falls back to when the Delilas Challenge ships *without* the custom
/// items (see [`plan_honey_grant`]).
pub const HONEY_ITEM_ID: u8 = 0x65;
/// Seru Tear claims the empty-name cut "Terra $9" weapon slot.
pub const SERU_TEAR_ITEM_ID: u8 = 0x12;
/// Fury Bloom claims the empty-name cut "Ozma $8" weapon slot.
pub const FURY_ITEM_ID: u8 = 0x1A;

/// Effect-descriptor subtypes claimed - three of the nineteen records no
/// kind-2 item references (kind-1 equipment `+1` bytes index the equipment
/// table, never descriptors: Short Sword's `0x23` would otherwise read
/// "usable flute").
pub const ELIXIR_SUB: u8 = 0x34;
pub const SERU_TEAR_SUB: u8 = 0x35;
pub const FURY_SUB: u8 = 0x36;

/// Effect classes claimed. Both resolve to the applier's default arm in
/// retail (bounded dispatch `sltiu 0x84` at `0x80040444`; no descriptor or
/// boss-module literal carries them).
pub const ELIXIR_CLASS: u8 = 0x48;
pub const TEAR_CLASS: u8 = 0x49;

/// Static-table bases (see `docs/formats/item-table.md` /
/// `item-effect-table.md`).
pub const ITEM_TABLE_VA: u32 = 0x8007_4368;
pub const DESC_TABLE_VA: u32 = 0x8007_52C0;
/// The applier `FUN_800402F4`'s 132-entry class jump table.
pub const APPLY_JT_VA: u32 = 0x8001_4FA0;
/// The menu validator `FUN_8003FB10`'s sibling 132-entry class table.
pub const VALID_JT_VA: u32 = 0x8001_4D70;
/// The applier's common exit arm (retail value of `APPLY_JT[0x48]`).
pub const APPLY_DEFAULT_ARM: u32 = 0x8004_21A8;
/// The validator's return-0 default (retail value of `VALID_JT[0x48/0x49]`).
pub const VALID_DEFAULT_ARM: u32 = 0x8004_02E0;
/// The validator's classes-9/10 arm: target mask `gp+0x9a8 = 7`, return 1 -
/// always usable. Both new classes point here.
pub const VALID_ALWAYS_ARM: u32 = 0x8004_0204;

/// The retail class-0 HP-restore arm the elixir tail-jumps into: with the
/// descriptor tier at 2 its clamp table reads 9999 = a full heal, and its
/// own battle popup / `-amount` mirror accounting runs unchanged.
pub const HP_ARM_VA: u32 = 0x8004_0470;

/// Battle actor pointer table + battle globals.
const ACTOR_TABLE_VA: u32 = 0x801C_9370;
const BCTX_PTR_VA: u32 = 0x8007_BD24;
/// Per-slot party roster char ids (1 = Vahn, 2 = Noa, 3 = Gala).
const ROSTER_VA: u32 = 0x8007_BD10;
const GAME_MODE_VA: u32 = 0x8007_B83C;
/// `FUN_800421D4(item_id, count)` - the give-item routine (bag at
/// `0x80084140+0x1818`, stacks by id, cap 99; preserves `s*` and `a1`).
const GIVE_ITEM_VA: u32 = 0x8004_21D4;
/// `FUN_8004FCC8(cue_id)` - the sound-cue dispatcher (the battle-action
/// cast-audio tail's own play call; cue `0x20C` = the heal-item chime).
const SFX_CUE_VA: u32 = 0x8004_FCC8;
/// `FUN_801E22C8(tint, state_word, slot, group)` - the battle cue-group
/// expander (overlay 0898; callers are battle-gated).
const CUE_EXPANDER_VA: u32 = 0x801E_22C8;

// --- cave homes -------------------------------------------------------------

/// Elixir arm home: the unreferenced actor-template tick (32 words).
pub const ELIXIR_ARM_VA: u32 = 0x8002_5054;
pub const ELIXIR_REGION_CAPACITY: usize = 32 * 4;
pub const ELIXIR_ARM_ORIG_HEAD: u32 = 0x27BD_FFD0; // addiu sp,sp,-0x30
const ELIXIR_ARM_ORIG_WORD1: u32 = 0xAFB0_0028; // sw s0,0x28(sp)

/// Conversion stage 1 home: the unreferenced CD-subsystem mode toggle
/// (21 words).
pub const CONV1_VA: u32 = 0x8003_EDAC;
pub const CONV1_REGION_CAPACITY: usize = 21 * 4;
pub const CONV1_ORIG_HEAD: u32 = 0x27BD_FFE8; // addiu sp,sp,-0x18
const CONV1_ORIG_WORD1: u32 = 0xAFB0_0010; // sw s0,0x10(sp)

/// Conversion stage 2 home: the unreferenced CD-read re-seek arm (32 words).
pub const CONV2_VA: u32 = 0x8003_F210;
pub const CONV2_REGION_CAPACITY: usize = 32 * 4;
pub const CONV2_ORIG_HEAD: u32 = 0x8F82_0988; // lw v0,0x988(gp)
const CONV2_ORIG_WORD1: u32 = 0x27BD_FFE8; // addiu sp,sp,-0x18

/// Fury Bloom + MP-skip home: the class-14 Point Card arm - reachable
/// code with no reachable data (`docs/formats/item-effect-table.md`); 65
/// words ending at the class-0x82 arm `0x800421A0`.
///
/// The MP-skip's first home, the "unreferenced" libapi VBlank-tier slot
/// `FUN_800605C8`, is **boot-live**: the kernel/libapi init invokes it
/// during a cold boot (the game parks at boot mode `0x10` with it
/// overwritten), which no static reference scan or save-state probe can
/// see - every library save state postdates boot. Cold-boot bisect
/// (`autorun_boot_watch.lua`) pinned it; that region must never be
/// claimed as a cave.
pub const FURY_ARM_VA: u32 = 0x8004_209C;
pub const FURY_REGION_CAPACITY: usize = 65 * 4;
pub const FURY_ORIG_HEAD: u32 = 0x3C02_8008; // lui v0,0x8008
const FURY_ORIG_WORD1: u32 = 0x2446_4140; // addiu a2,v0,0x4140
/// The MP-skip packs after the Fury Bloom arm inside the class-14 region.
pub const MPSKIP_VA: u32 = FURY_ARM_VA + 35 * 4;

/// Grant home: words 30..47 of the dome's AI-block cave (`FUN_80035274` -
/// the dome's main block uses words 0..29; the tail keeps its retail bytes).
pub const GRANT_VA: u32 = 0x8003_52EC;
pub const GRANT_REGION_CAPACITY: usize = 18 * 4;
const GRANT_ORIG_HEAD: u32 = 0x3C03_8007; // lui v1,0x8007
const GRANT_ORIG_WORD1: u32 = 0x2463_625C; // addiu v1,v1,0x625c

/// Display-cave tail (`FUN_800260DC` words 9..15; the dome's display
/// routine uses words 0..8): the free-cast flag cell.
pub const DISPLAY_TAIL_VA: u32 = 0x8002_6100;
pub const DISPLAY_TAIL_CAPACITY: usize = 7 * 4;
const DISPLAY_TAIL_ORIG_HEAD: u32 = 0xA422_B790; // sh v0,-0x4870(at)
const DISPLAY_TAIL_ORIG_WORD1: u32 = 0x3C01_8008; // lui at,0x8008
/// One-shot "next MP deduct is free" flag, set by the conversion and
/// consumed by the MP-skip hook. Battle actions execute strictly one at a
/// time, so a single global cell suffices.
pub const FREECAST_FLAG_VA: u32 = DISPLAY_TAIL_VA;

// --- derived string / data addresses ---------------------------------------

pub const SERU_NAME_VA: u32 = CONV2_VA + 21 * 4;
pub const FURY_NAME_VA: u32 = CONV2_VA + 25 * 4;
pub const ELIXIR_NAME_VA: u32 = CONV2_VA + 28 * 4;
pub const ELIXIR_DESC_VA: u32 = FURY_ARM_VA + 45 * 4;
pub const SERU_DESC_VA: u32 = FURY_ARM_VA + 51 * 4;
pub const FURY_DESC_VA: u32 = FURY_ARM_VA + 58 * 4;

pub const ELIXIR_NAME: &[u8] = b"Nature's Elixir\0";
pub const ELIXIR_DESC: &[u8] = b"Restores full HP & MP.\0";
pub const SERU_NAME: &[u8] = b"Ra-Seru Tear\0";
pub const SERU_DESC: &[u8] = b"Sheds your|Ra-Seru summon.\0";
pub const FURY_NAME: &[u8] = b"Fury Bloom\0";
pub const FURY_DESC: &[u8] = b"Party-wide|Fury Boost.\0";

/// The three Ra-Seru summon spell ids (Meta / Terra / Ozma), each `class
/// 0x32` / MP 240 / all-enemies in the spell table. The conversion derives
/// the id as `0x9D + roster char id` - always the caster's own.
pub const SERU_SUMMON_SPELLS: [u8; 3] = [0x9E, 0x9F, 0xA0];

// --- battle-overlay (PROT 0898) hook sites ----------------------------------

/// PROT index of the battle-action overlay.
pub use crate::delilas_dome::BATTLE_OVERLAY_PROT_INDEX;
const BATTLE_BASE_VA: u32 = 0x801C_E818;

/// The action-seed category dispatch's `lbu v1,0x1de(s3)` (state `0xC` of
/// `FUN_801E295C`, followed by a `nop` then the `sltiu/jr` dispatch).
pub const SEED_HOOK_VA: u32 = 0x801E_2D60;
pub const SEED_HOOK_ORIG: u32 = lbu(V1, S3, 0x1de);
const SEED_DELAY_ORIG: u32 = 0; // nop at 0x801E2D64 (left in place)
/// Where the conversion returns to (past the replayed load and the nop).
pub const SEED_RETURN_VA: u32 = 0x801E_2D68;

/// The summon/capture leg's MP deduct (`lhu v0,0x150(s3)` at `0x801E4584`,
/// `sh s0,0x178(s3)` mirror write at `+4`, `subu` at `+8`, exit `j` at
/// `+0xC`). The plain-cast leg's twin at `0x801E3D28` is not on any path a
/// converted tear cast takes and stays stock.
pub const MP_HOOK_VA: u32 = 0x801E_4584;
pub const MP_HOOK_ORIG: u32 = lhu(V0, S3, 0x150);
pub const MP_HOOK_DELAY_ORIG: u32 = sh(S0, S3, 0x178);
/// Stock continuation (the `subu v0,v0,s0`).
const MP_STOCK_RESUME_VA: u32 = 0x801E_458C;
/// The leg's exit tail (the stock `j` target).
const MP_EXIT_VA: u32 = 0x801E_6814;

// --- arena-overlay (PROT 0977) grant hook -----------------------------------

pub use crate::delilas_dome::ARENA_OVERLAY_PROT_INDEX;
const ARENA_BASE_VA: u32 = 0x801C_E818;

/// The settle's post-payout `s0` staging at `0x801D114C` / the `jal
/// 0x800266e0` at `0x801D1150`. Both winning paths flow through here with
/// `s0` already `lui`'d (`0x801D1148`, or the Seru arm's delay slot at
/// `0x801D1138`); the loss / ran-away arms never reach it.
pub const GRANT_HOOK_VA: u32 = 0x801D_114C;
pub const GRANT_HOOK_ORIG: u32 = addiu(S0, S0, 0x56c);
pub const GRANT_HOOK_DELAY_ORIG: u32 = jal(0x8002_66E0);
const GRANT_DISPLACED_JAL: u32 = jal(0x8002_66E0);
/// The `move a0,s0` in the displaced jal's delay slot (replayed).
const GRANT_DISPLACED_DELAY: u32 = addu(A0, S0, ZERO);
const GRANT_RETURN_VA: u32 = 0x801D_1158;

/// Assemble the elixir arm (entered via `APPLY_JT[0x48]`, inside
/// `FUN_800402F4`'s frame: `s3` = target slot, `s6` = tier, the four
/// per-slot pointer arrays at `sp+0x10/0x30/0x50/0x70` = curHP / maxHP /
/// curMP / maxMP). Fills MP to max (with the battle `-amount` mirror write
/// to `actor+0x178`), then tail-jumps into the retail class-0 HP arm, whose
/// tier-2 clamp is a full heal and whose popup / accumulator accounting is
/// the retail one. A dead target skips the MP fill and lets the HP arm's
/// own dead-check no-op the item.
pub fn assemble_elixir_arm() -> Vec<u32> {
    const HP: usize = 27;
    let words = vec![
        andi(V0, S3, 0xff),                   // 0
        sll(V1, V0, 2),                       // 1: v1 = slot*4 (kept)
        addu(V0, SP, V1),                     // 2
        lw(A3, V0, 0x10),                     // 3: &curHP
        lw(A2, V0, 0x50),                     // 4: &curMP
        lhu(T2, A3, 0),                       // 5: curHP
        lw(A0, V0, 0x70),                     // 6: &maxMP
        beq(T2, ZERO, (HP - (7 + 1)) as i16), // 7: dead -> HP arm handles it
        addu(S4, ZERO, ZERO),                 // 8: (delay) s4 = 0
        lhu(T1, A0, 0),                       // 9: maxMP
        lhu(A1, A2, 0),                       // 10: curMP
        addiu(T4, ZERO, 0x15),                // 11: (fills t1 delay)
        subu(T3, T1, A1),                     // 12: mp gained
        sh(T1, A2, 0),                        // 13: curMP = maxMP
        lui(V0, hi(GAME_MODE_VA)),            // 14
        lh(V0, V0, lo(GAME_MODE_VA)),         // 15
        nop(),                                // 16: (v0 delay)
        bne(V0, T4, (HP - (17 + 1)) as i16),  // 17: not battle -> HP arm
        nop(),                                // 18: (delay)
        lui(V0, hi(ACTOR_TABLE_VA)),          // 19
        addiu(V0, V0, lo(ACTOR_TABLE_VA)),    // 20
        addu(V0, V0, V1),                     // 21
        lw(V0, V0, 0),                        // 22: actor
        subu(T3, ZERO, T3),                   // 23: -mp gained (delay filler)
        sh(T3, V0, 0x178),                    // 24: MP mirror (retail MP-arm idiom)
        // The heal cue the cast-audio dispatcher (`FUN_801F3990`) plays for
        // the retail item classes 0..2 - its class jump table stops at 9,
        // so custom classes are silent unless the arm plays it itself.
        jal(SFX_CUE_VA),        // 25
        addiu(A0, ZERO, 0x20C), // 26: (delay) the heal-item cue
        // HP (idx 27): the retail full-heal + popup + exit.
        j(HP_ARM_VA), // 27
        nop(),        // 28: (delay)
    ];
    debug_assert_eq!(words.len(), 29);
    words
}

/// Assemble conversion stage 1 (entered by `j` from [`SEED_HOOK_VA`];
/// `s3` = acting actor). Replays the displaced category load; item actions
/// resolve their descriptor pointer and continue in stage 2 (the two stages
/// live in different caves - a branch cannot span them). Non-item actions
/// return immediately with `v1` = category, the dispatch's contract.
pub fn assemble_conversion_stage1() -> Vec<u32> {
    const OUT: usize = 16;
    let words = vec![
        lbu(V1, S3, 0x1de),                  // 0: category (replay)
        addiu(V0, ZERO, 1),                  // 1
        bne(V1, V0, (OUT - (2 + 1)) as i16), // 2: not an item action
        lbu(A0, S3, 0x1df),                  // 3: (delay) item id
        lui(T2, hi(ITEM_TABLE_VA)),          // 4: (fills the a0 load delay!)
        sll(T0, A0, 3),                      // 5
        sll(T1, A0, 2),                      // 6
        addu(T0, T0, T1),                    // 7: id*12
        addiu(T2, T2, lo(ITEM_TABLE_VA)),    // 8
        addu(T0, T0, T2),                    // 9
        lbu(T1, T0, 1),                      // 10: subtype
        addiu(T2, T2, 0x0F58),               // 11: -> descriptor table base
        sll(T1, T1, 2),                      // 12
        addu(T1, T1, T2),                    // 13: descriptor ptr
        j(CONV2_VA),                         // 14
        lbu(T3, T1, 0),                      // 15: (delay) class
        // OUT (idx 16):
        j(SEED_RETURN_VA), // 16
        nop(),             // 17: (delay)
    ];
    debug_assert_eq!(words.len(), 18);
    debug_assert_eq!(
        DESC_TABLE_VA,
        ITEM_TABLE_VA + 0x0F58,
        "descriptor-table displacement"
    );
    words
}

/// Assemble conversion stage 2: class/tier gates, then the caster-matched
/// summon pick. Only a class-`0x49` tier-0 item (the Seru Tear) converts:
/// the committed action becomes a magic cast (`+0x1DE = 2`) of **the acting
/// character's own Ra-Seru summon** (roster id 1/2/3 -> Meta `0x9E` / Terra
/// `0x9F` / Ozma `0xA0`), and the one-shot free-cast flag arms the MP skip.
/// Everything else falls through with the action untouched (`v1` = category
/// on every path).
///
/// Caster-matched is a *mechanism* constraint, probe-pinned: the big
/// summons stream the caster's own per-character summon choreography, so
/// Gala forced to cast Meta parks the battle forever at the state-`0x36`
/// driver poll, while Gala casting Ozma (and Noa casting Terra) completes
/// and lands damage. The tear therefore always sheds the user's own
/// Ra-Seru - which is also what lets *any* character use it.
pub fn assemble_conversion_stage2() -> Vec<u32> {
    const OUT: usize = 19;
    let words = vec![
        lbu(T4, T1, 1),                        // 0: tier
        addiu(V0, ZERO, TEAR_CLASS as u16),    // 1
        bne(T3, V0, (OUT - (2 + 1)) as i16),   // 2: not a tear
        nop(),                                 // 3: (delay)
        bne(T4, ZERO, (OUT - (4 + 1)) as i16), // 4: Fury Bloom tier stays an item
        nop(),                                 // 5: (delay)
        lui(T0, hi(BCTX_PTR_VA)),              // 6
        lw(T0, T0, lo(BCTX_PTR_VA)),           // 7: battle ctx
        lui(T2, hi(ROSTER_VA)),                // 8: (fills t0 delay)
        lbu(T0, T0, 0x13),                     // 9: acting slot
        addiu(T2, T2, lo(ROSTER_VA)),          // 10
        addu(T2, T2, T0),                      // 11
        lbu(T2, T2, 0),                        // 12: roster char id (1..3)
        addiu(V1, ZERO, 2),                    // 13: (fills t2 delay) new category
        addiu(T6, T2, 0x9D),                   // 14: spell = 0x9D + id
        sb(V1, S3, 0x1de),                     // 15: item -> magic
        sb(T6, S3, 0x1df),                     // 16: item id -> spell id
        lui(T7, hi(FREECAST_FLAG_VA)),         // 17
        sb(V1, T7, lo(FREECAST_FLAG_VA)),      // 18: flag = 2 (nonzero)
        // OUT (idx 19):
        j(SEED_RETURN_VA), // 19
        nop(),             // 20: (delay)
    ];
    debug_assert_eq!(words.len(), 21);
    words
}

/// Assemble the MP-deduct skip (entered by `j` from [`MP_HOOK_VA`]; `s0` =
/// the MP cost, `s3` = the acting actor). Flag clear: replay the stock
/// triple (mirror = cost, `curMP -= cost`) via the stock continuation. Flag
/// set (a converted tear cast): clear it, zero the mirror, and exit past
/// the deduct - the free cast, which also prevents the unconditional
/// deduct's u16 underflow (probe-observed `27 - 240 = 65323`).
pub fn assemble_mp_skip() -> Vec<u32> {
    const STOCK: usize = 8;
    let words = vec![
        lui(T0, hi(FREECAST_FLAG_VA)),           // 0
        lbu(T1, T0, lo(FREECAST_FLAG_VA)),       // 1
        lhu(V0, S3, 0x150),                      // 2: curMP (replay; fills t1 delay)
        beq(T1, ZERO, (STOCK - (3 + 1)) as i16), // 3
        sh(S0, S3, 0x178),                       // 4: (delay) mirror = cost, both paths
        // flag set - the free cast:
        sb(ZERO, T0, lo(FREECAST_FLAG_VA)), // 5: consume the flag
        j(MP_EXIT_VA),                      // 6: skip the deduct
        sh(ZERO, S3, 0x178),                // 7: (delay) no cost shown
        // STOCK (idx 8):
        j(MP_STOCK_RESUME_VA), // 8: stock deduct continues
        nop(),                 // 9: (delay)
    ];
    debug_assert_eq!(words.len(), 10);
    words
}

/// Assemble the Fury Bloom arm (entered via `APPLY_JT[0x49]` for a tier-1
/// class-`0x49` item, the applier frame; battle-gated - the field bit is
/// clear so the field menu greys it). Sets the Fury Boost gauge flag
/// (`actor+0x1F9 = 1`, exactly the retail class-5 arm's write) on every
/// **living** party member, fires the retail Fury cue group on the acting
/// character, and plays the item chime. The retail arm is single-target;
/// party-wide needs this custom arm because group application lives inside
/// each class arm, not in the dispatcher.
pub fn assemble_fury_arm() -> Vec<u32> {
    const LOOP: usize = 9;
    const NEXT: usize = 18;
    const EXIT: usize = 33;
    let words = vec![
        lui(V0, hi(GAME_MODE_VA)),            // 0
        lh(V0, V0, lo(GAME_MODE_VA)),         // 1
        addiu(V1, ZERO, 0x15),                // 2
        bne(V0, V1, (EXIT - (3 + 1)) as i16), // 3: field -> no-op
        nop(),                                // 4: (delay)
        lui(A1, hi(ACTOR_TABLE_VA)),          // 5
        addiu(A1, A1, lo(ACTOR_TABLE_VA)),    // 6
        addiu(T1, ZERO, 1),                   // 7: the gauge flag value
        addiu(T0, ZERO, 0),                   // 8
        // LOOP (idx 9): every living party slot 0..2.
        sll(V0, T0, 2),                          // 9
        addu(V0, V0, A1),                        // 10
        lw(V0, V0, 0),                           // 11
        nop(),                                   // 12
        lhu(V1, V0, 0x14c),                      // 13
        nop(),                                   // 14
        beq(V1, ZERO, (NEXT - (15 + 1)) as i16), // 15: dead -> skip
        nop(),                                   // 16: (delay)
        sb(T1, V0, 0x1f9),                       // 17: Fury Boost flag
        // NEXT (idx 18):
        addiu(T0, T0, 1),                        // 18
        sltiu(V0, T0, 3),                        // 19
        bne(V0, ZERO, (LOOP as i16) - (20 + 1)), // 20
        nop(),                                   // 21: (delay)
        // The retail class-5 cue (tint 0x004040FF, state 0x100403FF,
        // group 7) on the acting character.
        lui(A2, hi(BCTX_PTR_VA)),    // 22
        lw(A2, A2, lo(BCTX_PTR_VA)), // 23
        lui(A0, 0x40),               // 24: (fills a2 delay)
        lbu(A2, A2, 0x13),           // 25: acting slot
        ori(A0, A0, 0x40ff),         // 26
        lui(A1, 0x1004),             // 27
        ori(A1, A1, 0x3ff),          // 28
        jal(CUE_EXPANDER_VA),        // 29
        addiu(A3, ZERO, 7),          // 30: (delay) group
        // The item chime (see the elixir arm's note).
        jal(SFX_CUE_VA),        // 31
        addiu(A0, ZERO, 0x20C), // 32: (delay)
        // EXIT (idx 33):
        j(APPLY_DEFAULT_ARM), // 33
        nop(),                // 34: (delay)
    ];
    debug_assert_eq!(words.len(), 35);
    words
}

/// Assemble the reward grant (entered by `j` from [`GRANT_HOOK_VA`]; `s0`
/// holds the `lui 0x8007` both winning paths staged). Re-tests course 3
/// from the settle's own course global, gives one of each `items` id, then
/// replays the displaced `addiu` + `jal` pair and rejoins the settle. `ra`
/// is frame-saved by the settle (it `jal`s two words later in retail) and
/// the retail War God give on the fall-through path has already clobbered
/// `v0/v1/a0/a2/a3` here, so no register needs preserving.
pub fn assemble_grant_routine_for(items: &[u8]) -> Vec<u32> {
    assert!(!items.is_empty(), "grant needs at least one item");
    // Head (5 words + branch delay) then 3 words per item (the first item's
    // count-set rides the branch delay), then the 4-word rejoin tail.
    let out = 3 * items.len() + 5;
    let mut words = vec![
        addiu(S0, S0, 0x56c),                // 0: replay the displaced addiu
        lui(T0, hi(COURSE_GLOBAL_VA)),       // 1
        lw(T1, T0, lo(COURSE_GLOBAL_VA)),    // 2
        addiu(T2, ZERO, 3),                  // 3
        bne(T1, T2, (out - (4 + 1)) as i16), // 4: not the Delilas course
        addiu(A1, ZERO, 1),                  // 5: (delay) count = 1
    ];
    for (i, &id) in items.iter().enumerate() {
        if i > 0 {
            words.push(addiu(A1, ZERO, 1)); // count = 1 (clobbered by the give)
        }
        words.push(jal(GIVE_ITEM_VA));
        words.push(addiu(A0, ZERO, id as u16)); // (delay) item id
    }
    // OUT (idx `out`): replay the displaced jal + delay, rejoin.
    words.push(GRANT_DISPLACED_JAL);
    words.push(GRANT_DISPLACED_DELAY); // (delay) move a0,s0
    words.push(j(GRANT_RETURN_VA));
    words.push(nop()); // (delay)
    debug_assert_eq!(words.len(), out + 4);
    words
}

/// The full custom-item reward grant: Nature's Elixir + Seru Tear + Fury
/// Bloom, one of each per winning course-3 settle.
pub fn assemble_grant_routine() -> Vec<u32> {
    assemble_grant_routine_for(&[ELIXIR_ITEM_ID, SERU_TEAR_ITEM_ID, FURY_ITEM_ID])
}

/// Pad a NUL-terminated string to a whole number of words.
fn padded(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
    v
}

/// The 12-byte item record for one custom item.
fn item_record(sub: u8, name_va: u32, desc_va: u32) -> Vec<u8> {
    let mut r = vec![2u8, sub, 0, 0]; // kind 2 (consumable), price 0
    r.extend_from_slice(&name_va.to_le_bytes());
    r.extend_from_slice(&desc_va.to_le_bytes());
    r
}

fn words_to_bytes(w: Vec<u32>) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A planned custom-items injection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomItemsInjection {
    /// Writes into `SCUS_942.54` (records, descriptors, jump-table words,
    /// cave routines + strings).
    pub scus: Vec<Write>,
    /// Writes into the battle-action overlay (PROT 0898): the seed-dispatch
    /// and MP-deduct hooks.
    pub battle: Vec<Write>,
    /// Writes into the arena overlay (PROT 0977): the settle grant hook.
    pub overlay: Vec<Write>,
}

fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    legaia_asset::item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("cannot resolve SCUS VA {va:#x}"))
}

fn read_word(buf: &[u8], off: usize) -> Result<u32> {
    buf.get(off..off + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| anyhow::anyhow!("offset {off:#x} out of range"))
}

fn expect_word(buf: &[u8], off: usize, want: u32, what: &str) -> Result<()> {
    let got = read_word(buf, off)?;
    if got != want {
        bail!("{what}: expected {want:#010x}, found {got:#010x} - refusing to patch");
    }
    Ok(())
}

/// One cave region: `(va, capacity, head fingerprints, payload, label)`.
type CaveRegion = (u32, usize, [u32; 2], Vec<u8>, &'static str);

/// The seven cave regions.
fn cave_payloads() -> Vec<CaveRegion> {
    let elixir = words_to_bytes(assemble_elixir_arm());
    let conv1 = words_to_bytes(assemble_conversion_stage1());
    let mut conv2 = words_to_bytes(assemble_conversion_stage2());
    conv2.extend_from_slice(&padded(SERU_NAME));
    conv2.extend_from_slice(&padded(FURY_NAME));
    conv2.extend_from_slice(&padded(ELIXIR_NAME));
    let mut fury = words_to_bytes(assemble_fury_arm());
    fury.extend_from_slice(&words_to_bytes(assemble_mp_skip()));
    fury.extend_from_slice(&padded(ELIXIR_DESC));
    fury.extend_from_slice(&padded(SERU_DESC));
    fury.extend_from_slice(&padded(FURY_DESC));
    let grant = words_to_bytes(assemble_grant_routine());
    let tail = 0u32.to_le_bytes().to_vec(); // the free-cast flag cell
    vec![
        (
            ELIXIR_ARM_VA,
            ELIXIR_REGION_CAPACITY,
            [ELIXIR_ARM_ORIG_HEAD, ELIXIR_ARM_ORIG_WORD1],
            elixir,
            "elixir arm cave",
        ),
        (
            CONV1_VA,
            CONV1_REGION_CAPACITY,
            [CONV1_ORIG_HEAD, CONV1_ORIG_WORD1],
            conv1,
            "conversion stage-1 cave",
        ),
        (
            CONV2_VA,
            CONV2_REGION_CAPACITY,
            [CONV2_ORIG_HEAD, CONV2_ORIG_WORD1],
            conv2,
            "conversion stage-2 + names cave",
        ),
        (
            FURY_ARM_VA,
            FURY_REGION_CAPACITY,
            [FURY_ORIG_HEAD, FURY_ORIG_WORD1],
            fury,
            "Fury Bloom + MP-skip cave (class-14 arm)",
        ),
        (
            GRANT_VA,
            GRANT_REGION_CAPACITY,
            [GRANT_ORIG_HEAD, GRANT_ORIG_WORD1],
            grant,
            "grant cave (AI-block tail)",
        ),
        (
            DISPLAY_TAIL_VA,
            DISPLAY_TAIL_CAPACITY,
            [DISPLAY_TAIL_ORIG_HEAD, DISPLAY_TAIL_ORIG_WORD1],
            tail,
            "display-cave tail (free-cast flag)",
        ),
    ]
}

impl CustomItemsInjection {
    /// Plan the injection against the three images. Every site is
    /// fingerprinted; an unrecognized build is refused rather than
    /// corrupted. Order-independent of the dome injection except that the
    /// grant only ever fires when the dome's course-3 exists.
    pub fn plan(scus: &[u8], battle: &[u8], overlay: &[u8]) -> Result<Self> {
        let mut scus_writes = Vec::new();

        // --- item records (empty-name slots; fingerprint the whole record) --
        for (id, sub, name_va, desc_va, orig, what) in [
            (
                ELIXIR_ITEM_ID,
                ELIXIR_SUB,
                ELIXIR_NAME_VA,
                ELIXIR_DESC_VA,
                [0x2A02u16, 0x0000, 0xB508, 0x8007, 0x1744, 0x8001],
                "Nature's Elixir item record",
            ),
            (
                SERU_TEAR_ITEM_ID,
                SERU_TEAR_SUB,
                SERU_NAME_VA,
                SERU_DESC_VA,
                [0x1101, 0x0000, 0xB508, 0x8007, 0xB508, 0x8007],
                "Seru Tear item record",
            ),
            (
                FURY_ITEM_ID,
                FURY_SUB,
                FURY_NAME_VA,
                FURY_DESC_VA,
                [0x1901, 0x01F4, 0xB500, 0x8007, 0xB508, 0x8007],
                "Fury Bloom item record",
            ),
        ] {
            let off = scus_off(scus, ITEM_TABLE_VA + id as u32 * 12)?;
            let got = &scus[off..off + 12];
            let want: Vec<u8> = orig.iter().flat_map(|h| h.to_le_bytes()).collect();
            if got != want {
                bail!("{what}: unexpected original bytes - refusing to patch");
            }
            scus_writes.push(Write {
                off,
                bytes: item_record(sub, name_va, desc_va),
            });
        }

        // --- effect descriptors (three of the kind-2-unreferenced spares) ---
        for (sub, bytes, what) in [
            (
                ELIXIR_SUB,
                [ELIXIR_CLASS, 2, 0x86, 0x41],
                "elixir descriptor",
            ),
            (
                SERU_TEAR_SUB,
                [TEAR_CLASS, 0, 0x84, 0x41],
                "Seru Tear descriptor",
            ),
            (
                FURY_SUB,
                [TEAR_CLASS, 1, 0xA4, 0x41],
                "Fury Bloom descriptor",
            ),
        ] {
            let off = scus_off(scus, DESC_TABLE_VA + sub as u32 * 4)?;
            expect_word(scus, off, 0x4189_0000, what)?;
            scus_writes.push(Write {
                off,
                bytes: bytes.to_vec(),
            });
        }

        // --- jump tables ----------------------------------------------------
        let apply_off = scus_off(scus, APPLY_JT_VA + ELIXIR_CLASS as u32 * 4)?;
        expect_word(scus, apply_off, APPLY_DEFAULT_ARM, "applier JT[0x48]")?;
        expect_word(scus, apply_off + 4, APPLY_DEFAULT_ARM, "applier JT[0x49]")?;
        let mut jt = ELIXIR_ARM_VA.to_le_bytes().to_vec();
        jt.extend_from_slice(&FURY_ARM_VA.to_le_bytes());
        scus_writes.push(Write {
            off: apply_off,
            bytes: jt,
        });
        let valid_off = scus_off(scus, VALID_JT_VA + ELIXIR_CLASS as u32 * 4)?;
        expect_word(scus, valid_off, VALID_DEFAULT_ARM, "validator JT[0x48]")?;
        expect_word(scus, valid_off + 4, VALID_DEFAULT_ARM, "validator JT[0x49]")?;
        let mut vt = VALID_ALWAYS_ARM.to_le_bytes().to_vec();
        vt.extend_from_slice(&VALID_ALWAYS_ARM.to_le_bytes());
        scus_writes.push(Write {
            off: valid_off,
            bytes: vt,
        });

        // --- caves ----------------------------------------------------------
        for (va, capacity, heads, payload, what) in cave_payloads() {
            if payload.len() > capacity {
                bail!(
                    "{what}: payload {} bytes exceeds capacity {capacity}",
                    payload.len()
                );
            }
            let off = scus_off(scus, va)?;
            expect_word(scus, off, heads[0], what)?;
            expect_word(scus, off + 4, heads[1], what)?;
            scus_writes.push(Write {
                off,
                bytes: payload,
            });
        }

        // --- battle-overlay hooks (PROT 0898) -------------------------------
        let boff = |va: u32| -> Result<usize> {
            va.checked_sub(BATTLE_BASE_VA)
                .map(|d| d as usize)
                .ok_or_else(|| anyhow::anyhow!("battle VA {va:#x} below base"))
        };
        let mut battle_writes = Vec::new();
        let seed_off = boff(SEED_HOOK_VA)?;
        expect_word(battle, seed_off, SEED_HOOK_ORIG, "seed-dispatch hook")?;
        expect_word(battle, seed_off + 4, SEED_DELAY_ORIG, "seed-dispatch delay")?;
        battle_writes.push(Write {
            off: seed_off,
            bytes: j(CONV1_VA).to_le_bytes().to_vec(),
        });
        let mp_off = boff(MP_HOOK_VA)?;
        expect_word(battle, mp_off, MP_HOOK_ORIG, "MP-deduct hook")?;
        expect_word(battle, mp_off + 4, MP_HOOK_DELAY_ORIG, "MP-deduct delay")?;
        let mut mp = j(MPSKIP_VA).to_le_bytes().to_vec();
        mp.extend_from_slice(&nop().to_le_bytes());
        battle_writes.push(Write {
            off: mp_off,
            bytes: mp,
        });

        // --- arena-overlay grant hook (PROT 0977) ---------------------------
        let goff = GRANT_HOOK_VA
            .checked_sub(ARENA_BASE_VA)
            .map(|d| d as usize)
            .ok_or_else(|| anyhow::anyhow!("grant VA below arena base"))?;
        expect_word(overlay, goff, GRANT_HOOK_ORIG, "grant hook")?;
        expect_word(overlay, goff + 4, GRANT_HOOK_DELAY_ORIG, "grant hook delay")?;
        let mut gh = j(GRANT_VA).to_le_bytes().to_vec();
        gh.extend_from_slice(&nop().to_le_bytes());
        let overlay_writes = vec![Write {
            off: goff,
            bytes: gh,
        }];

        Ok(Self {
            scus: scus_writes,
            battle: battle_writes,
            overlay: overlay_writes,
        })
    }
}

/// Plan the **Honey fallback** grant - the reward set used when the Delilas
/// Challenge is enabled *without* the custom items: the same arena settle
/// hook and grant cave, but a winning course-3 settle awards one retail
/// Honey (id `0x65`) alongside the 5000 coins. Nothing else is touched - no
/// item records, descriptors, jump-table words, or battle-overlay hooks -
/// so the returned plan's `battle` write list is empty.
///
/// Fingerprinted like [`CustomItemsInjection::plan`]: an unrecognized build
/// (or a cave already claimed by the full custom-items set) is refused
/// rather than corrupted.
pub fn plan_honey_grant(scus: &[u8], overlay: &[u8]) -> Result<CustomItemsInjection> {
    let grant = words_to_bytes(assemble_grant_routine_for(&[HONEY_ITEM_ID]));
    debug_assert!(grant.len() <= GRANT_REGION_CAPACITY);
    let off = scus_off(scus, GRANT_VA)?;
    expect_word(scus, off, GRANT_ORIG_HEAD, "grant cave (AI-block tail)")?;
    expect_word(
        scus,
        off + 4,
        GRANT_ORIG_WORD1,
        "grant cave (AI-block tail)",
    )?;
    let scus_writes = vec![Write { off, bytes: grant }];

    let goff = GRANT_HOOK_VA
        .checked_sub(ARENA_BASE_VA)
        .map(|d| d as usize)
        .ok_or_else(|| anyhow::anyhow!("grant VA below arena base"))?;
    expect_word(overlay, goff, GRANT_HOOK_ORIG, "grant hook")?;
    expect_word(overlay, goff + 4, GRANT_HOOK_DELAY_ORIG, "grant hook delay")?;
    let mut gh = j(GRANT_VA).to_le_bytes().to_vec();
    gh.extend_from_slice(&nop().to_le_bytes());
    let overlay_writes = vec![Write {
        off: goff,
        bytes: gh,
    }];

    Ok(CustomItemsInjection {
        scus: scus_writes,
        battle: Vec::new(),
        overlay: overlay_writes,
    })
}

/// The SCUS-side half as `(VA, bytes)` pairs for RAM-installing in an
/// emulator probe (library save states predate the patched disc; the
/// overlay halves stream from the patched `--iso`). Mirrors the `scus`
/// writes [`CustomItemsInjection::plan`] produces.
pub fn probe_ram_writes() -> Vec<(u32, Vec<u8>)> {
    let mut out: Vec<(u32, Vec<u8>)> = Vec::new();
    for (id, sub, name_va, desc_va) in [
        (ELIXIR_ITEM_ID, ELIXIR_SUB, ELIXIR_NAME_VA, ELIXIR_DESC_VA),
        (SERU_TEAR_ITEM_ID, SERU_TEAR_SUB, SERU_NAME_VA, SERU_DESC_VA),
        (FURY_ITEM_ID, FURY_SUB, FURY_NAME_VA, FURY_DESC_VA),
    ] {
        out.push((
            ITEM_TABLE_VA + id as u32 * 12,
            item_record(sub, name_va, desc_va),
        ));
    }
    out.push((
        DESC_TABLE_VA + ELIXIR_SUB as u32 * 4,
        vec![ELIXIR_CLASS, 2, 0x86, 0x41],
    ));
    out.push((
        DESC_TABLE_VA + SERU_TEAR_SUB as u32 * 4,
        vec![TEAR_CLASS, 0, 0x84, 0x41],
    ));
    out.push((
        DESC_TABLE_VA + FURY_SUB as u32 * 4,
        vec![TEAR_CLASS, 1, 0xA4, 0x41],
    ));
    let mut jt = ELIXIR_ARM_VA.to_le_bytes().to_vec();
    jt.extend_from_slice(&FURY_ARM_VA.to_le_bytes());
    out.push((APPLY_JT_VA + ELIXIR_CLASS as u32 * 4, jt));
    let mut vt = VALID_ALWAYS_ARM.to_le_bytes().to_vec();
    vt.extend_from_slice(&VALID_ALWAYS_ARM.to_le_bytes());
    out.push((VALID_JT_VA + ELIXIR_CLASS as u32 * 4, vt));
    for (va, _cap, _heads, payload, _what) in cave_payloads() {
        out.push((va, payload));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes_of(w: Vec<u32>) -> usize {
        w.len() * 4
    }

    #[test]
    fn cave_payloads_fit_their_regions() {
        for (va, capacity, _heads, payload, what) in cave_payloads() {
            assert!(
                payload.len() <= capacity,
                "{what} @ {va:#x}: {} > {capacity}",
                payload.len()
            );
        }
    }

    #[test]
    fn string_addresses_match_the_payload_layout() {
        assert_eq!(
            SERU_NAME_VA,
            CONV2_VA + bytes_of(assemble_conversion_stage2()) as u32
        );
        assert_eq!(FURY_NAME_VA, SERU_NAME_VA + padded(SERU_NAME).len() as u32);
        assert_eq!(
            ELIXIR_NAME_VA,
            FURY_NAME_VA + padded(FURY_NAME).len() as u32
        );
        assert_eq!(
            MPSKIP_VA,
            FURY_ARM_VA + bytes_of(assemble_fury_arm()) as u32
        );
        assert_eq!(
            ELIXIR_DESC_VA,
            MPSKIP_VA + bytes_of(assemble_mp_skip()) as u32
        );
        assert_eq!(
            SERU_DESC_VA,
            ELIXIR_DESC_VA + padded(ELIXIR_DESC).len() as u32
        );
        assert_eq!(FURY_DESC_VA, SERU_DESC_VA + padded(SERU_DESC).len() as u32);
    }

    #[test]
    fn elixir_arm_shape() {
        let w = assemble_elixir_arm();
        assert_eq!(w.len(), 29);
        // Dead-target branch reaches the HP tail-jump (skipping the chime).
        assert_eq!(w[7], beq(T2, ZERO, 19));
        assert_eq!(w[17], bne(V0, T4, 9));
        assert_eq!(w[25], jal(SFX_CUE_VA));
        assert_eq!(w[27], j(HP_ARM_VA));
    }

    #[test]
    fn conversion_shape() {
        let s1 = assemble_conversion_stage1();
        assert_eq!(s1.len(), 18);
        assert_eq!(s1[0], SEED_HOOK_ORIG, "stage 1 must replay the hook insn");
        assert_eq!(s1[14], j(CONV2_VA));
        assert_eq!(s1[16], j(SEED_RETURN_VA));
        let s2 = assemble_conversion_stage2();
        assert_eq!(s2.len(), 21);
        assert_eq!(s2[19], j(SEED_RETURN_VA));
    }

    #[test]
    fn mp_skip_shape() {
        let w = assemble_mp_skip();
        assert_eq!(w.len(), 10);
        assert_eq!(w[2], MP_HOOK_ORIG, "the skip must replay the deduct load");
        assert_eq!(w[4], MP_HOOK_DELAY_ORIG, "mirror write rides the delay");
        assert_eq!(w[6], j(MP_EXIT_VA));
        assert_eq!(w[8], j(MP_STOCK_RESUME_VA));
    }

    #[test]
    fn fury_arm_shape() {
        let w = assemble_fury_arm();
        assert_eq!(w.len(), 35);
        assert_eq!(w[33], j(APPLY_DEFAULT_ARM));
        // The loop's back-branch lands on its own head.
        assert_eq!(w[20], bne(V0, ZERO, -12));
        // The retail class-5 gauge-flag write, per living member.
        assert_eq!(w[17], sb(T1, V0, 0x1f9));
    }

    #[test]
    fn grant_shape() {
        let w = assemble_grant_routine();
        assert_eq!(w.len(), 18);
        assert_eq!(
            w[0], GRANT_HOOK_ORIG,
            "grant must replay the displaced addiu"
        );
        assert_eq!(w[14], GRANT_DISPLACED_JAL);
        assert_eq!(w[16], j(GRANT_RETURN_VA));
    }

    #[test]
    fn honey_grant_shape() {
        let w = assemble_grant_routine_for(&[HONEY_ITEM_ID]);
        assert_eq!(w.len(), 12);
        assert_eq!(w[0], GRANT_HOOK_ORIG, "replay the displaced addiu");
        // The not-course-3 branch lands exactly on the rejoin tail.
        assert_eq!(w[4], bne(T1, T2, 3));
        assert_eq!(w[6], jal(GIVE_ITEM_VA));
        assert_eq!(w[7], addiu(A0, ZERO, HONEY_ITEM_ID as u16));
        assert_eq!(w[8], GRANT_DISPLACED_JAL);
        assert_eq!(w[10], j(GRANT_RETURN_VA));
        assert!(
            w.len() * 4 <= GRANT_REGION_CAPACITY,
            "honey grant must fit the AI-block tail cave"
        );
    }

    #[test]
    fn item_ids_and_subs_are_the_censused_frees() {
        assert_eq!(
            [ELIXIR_ITEM_ID, SERU_TEAR_ITEM_ID, FURY_ITEM_ID],
            [0xB9, 0x12, 0x1A]
        );
        assert_eq!([ELIXIR_SUB, SERU_TEAR_SUB, FURY_SUB], [0x34, 0x35, 0x36]);
    }
}
