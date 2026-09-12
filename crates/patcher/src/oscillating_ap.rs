//! **Oscillating AP costs**: every battle, each Tactical Art is dealt onto one
//! of two sides at random - the **cost** side (retail: pays its AP, full
//! damage) or the **grant** side (admitted at any AP level, *adds* the AP it
//! would have cost, deals a fraction of its damage). The deal is re-rolled per
//! art per battle, so a fight is a mix of both sides and the next fight is a
//! different mix.
//!
//! A community modding knob. It reuses the two facts the arts AP override
//! ([`crate::arts_ap_grant`]) pinned - retail computes an art's AP cost inside
//! the party arts queue-builder `FUN_801EED1C` (PROT 0898) instead of loading
//! it, and the builder identifies the art by its arts-table row `s3 - 0x0B`
//! keyed with the acting slot's party-record id `DAT_8007BD10[slot]` - and adds
//! two things that module has no need for: a per-battle roll, and a damage
//! scale on the grant side.
//!
//! ## What is on the disc after the patch
//!
//! | Piece | Where | What it does |
//! |---|---|---|
//! | roll (S) | SCUS setup hook `0x80051A20`, inside the battle loader `FUN_800513F0` | fills the 16-byte side table with 16 `rand()` draws (one bit per (character, row)) |
//! | guard (A) | 0898 `0x801EF410` | a grant-side art reads as affordable at any Spirit |
//! | debit (C) | 0898 `0x801EF490` | a grant-side art *adds* retail's own computed charge (`a2`, clamped at 100) and skips the spent accrual; the cost side is untouched |
//! | refund (D) | 0898 `0x801EF988` | the end-of-turn `Spirit += spent` refund is clamped at 100 (the arts AP override's routine, verbatim) |
//! | damage (M) | 0898 `0x801EDA10`, inside the arms execution resolver `FUN_801EC3E4` | a grant-side art's strike damage becomes `damage * pct / 100` |
//!
//! `actor[+0x170]` = Spirit/AP; `actor[+0x224]` = spent-AP accumulator.
//!
//! ## The side table
//!
//! Sixteen bytes in the SCUS rodata gap ([`BITS_LEN`]), bit `(char - 1) * 32 +
//! row` set = **grant side this battle**. `char` is the 1-based party-record id
//! (`1` Vahn / `2` Noa / `3` Gala / `4` Terra), `row` the art's arts-table
//! display index (`0` = Miracle Art, then the list order), the same key the
//! arts AP override uses ([`crate::arts_ap_grant::NUM_ROWS`] rows per
//! character, [`crate::arts_ap_grant::ROW_STRIDE`] apart). Rows past the
//! reachable 26 are padding nothing indexes; their random bits are inert.
//!
//! The roll runs from the battle loader's setup site - the one `--shiny-seru`
//! also hooks - after the monster-setup loop, where `ra` is dead (the next
//! retail word is a `jal`) and every caller-saved register is free. It calls
//! retail's own `rand` veneer `FUN_80056798` sixteen times and stores the low
//! byte of each draw; the loop counter lives in a scratch word next to the
//! table because the BIOS clobbers the temporaries. Sixteen extra draws per
//! battle shift the RNG stream, nothing else.
//!
//! ## Identifying the art at strike time
//!
//! The AP sites know the art (the builder's `s3` cursor); the damage kernel
//! does not - it is handed one **action entry** per art and walks that entry's
//! per-strike power bytes with the cursor `actor[+0x1F4]`. The entry pointer is
//! `actor[+0x4C]`, materialised by the SCUS anim commit `FUN_8004AD80` from the
//! staged anim id `q` as `record0[q*4] = bank + 4 + (q - 0x10) * 0xD0 + 0x24`
//! (`0x8004BC80`..`0x8004BC84`; `bank = record0[+0x58]`, the character's
//! `0xD0`-stride art records - the very array the builder walks with
//! `s3 * 0xD0` from `bank + 4`), and the commit snaps the playing id
//! `actor[+0x1D9] = q` (`0x8004BDE0`). So at a strike the executing art's
//! record index is `q - 0x10 = s3`, i.e. **row = `actor[+0x1D9] - 0x1B`**. The
//! damage routine cross-checks that reading before it trusts it: it re-derives
//! `record0[q*4]` and compares it with the entry pointer the kernel was called
//! with (`[sp+0x54]`, the kernel's own spill of `a1`). A plain direction swing
//! (`q` `0x0C..=0x0F`, its entries copied elsewhere by `FUN_800557B8`), a
//! Super Art row past the 26, or any mismatch falls through to retail damage.
//!
//! The site is the word after the 9999 cap (`0x801EDA00..0x801EDA0C`), where
//! `s0 - s1` is the strike's final damage (the kernel adds `s0 - s1` to the
//! defender's tallies at `0x801EDB38` and heals the attacker by it in the
//! drain arm at `0x801EDAF4`). The routine rewrites `s0 = s1 + (s0 - s1) *
//! pct / 100` - exact integer arithmetic, `mflo` kept three words clear of the
//! following `divu` - and replays the two displaced words. `t0..t6` are free
//! there (each is written before its next read in the kernel) and `HI`/`LO`
//! hold nothing (the kernel's next `mflo` follows its own `divu` at
//! `0x801EDB98`).
//!
//! ## Placement
//!
//! The same four verified-dead SCUS regions every hand-assembled feature
//! shares: guard + debit in [`ARENA1_VA`], refund in [`ARENA2_VA`], the roll in
//! [`SLOT6_VA`] (it fills the slot exactly), the damage routine + side table +
//! counter in [`SCUS_GAP_VA`]. So the knob is **mutually exclusive with
//! `--shiny-seru`, `--arts-ap-grant` / `--arts-ap-cost`, `--show-super-arts`,
//! `--super-arts-pack` and `--delilas-challenge`** - enforced in the CLI and
//! the web patcher, and structurally by the all-zero check on every region.
//!
//! ## What it does not do
//!
//! Nothing on screen says which side an art landed on: the pause menu's AP
//! number is a static SCUS byte the roll cannot reach, and the battle UI draws
//! no per-art cost. The gauge moving the other way, and the damage, are the
//! tell. Enemies are untouched (`FUN_801EED1C` is the party builder; monster
//! strikes hit the damage site with a slot `>= 3` and fall through).
//!
//! No Sony bytes are embedded; the routines are the patcher's own code and the
//! fingerprints are single instruction words.

use anyhow::{Result, bail};

use crate::arts_ap_grant::{
    C_NATIVE_RET_VA, C_OVERRIDE_RET_VA, HOOK_A_VA, HOOK_A_W0, HOOK_B_VA, HOOK_B_W0, HOOK_C_VA,
    HOOK_C_W0, HOOK_D_VA, HOOK_D_W0, NUM_CHARS, NUM_ROWS, OVERLAY_BASE_VA, RET_A_VA, RET_D_VA,
    assemble_refund, assert_not_in_tables, assert_zero, ov_hook, words_to_bytes,
};
use crate::mips::*;
use crate::shiny_seru::{
    ARENA1_END_VA, ARENA1_VA, ARENA2_END_VA, ARENA2_VA, Edit, HOOK_SETUP_VA, HOOK_SETUP_W0,
    OVERLAY_TABLE_RANGES, RAND_FUNC_VA, SCUS_GAP_END_VA, SCUS_GAP_VA, SCUS_TABLE_RANGES,
    SLOT6_END_VA, SLOT6_VA,
};

/// PROT entry index of the battle-action overlay (0898) hosting the detours.
pub use crate::arts_ap_grant::OVERLAY_PROT_INDEX;

/// Default grant-side damage, percent of the art's retail damage.
pub const DEFAULT_DAMAGE_PCT: u8 = 20;
/// Largest configurable grant-side damage percent (100 = full damage).
pub const MAX_DAMAGE_PCT: u8 = 100;
/// Native Spirit/AP cap the grant clamps at.
pub const AP_CAP: u16 = 100;

/// Side-table length: `NUM_CHARS * ROW_STRIDE` bits.
pub const BITS_LEN: usize = 16;
/// `log2(ROW_STRIDE)` - one character's block of rows is one `sll`.
const ROW_SHIFT: u32 = 5;

// --- Pinned hook sites -------------------------------------------------------

/// M: the damage site. `addiu v1,v0,-0x6c90` (`v0 = 0x801D0000` from the cap's
/// branch-delay `lui`) is the first word after the 9999 cap; the detour
/// replaces it and the following `andi v0,s4,0xff`, both replayed.
pub const HOOK_DMG_VA: u32 = 0x801E_DA10;
pub(crate) const HOOK_DMG_W0: u32 = 0x2443_9370; // addiu v1,v0,-0x6c90
pub(crate) const HOOK_DMG_W1: u32 = 0x3282_00FF; // andi v0,s4,0xff
const RET_DMG_VA: u32 = 0x801E_DA18;

/// The 9999 cap the damage site follows - fingerprinted so a shifted build is
/// refused rather than detoured at the wrong word.
pub(crate) const CAP_FINGERPRINT: [(u32, u32); 4] = [
    (0x801E_DA00, 0x0070_102B), // sltu v0,v1,s0
    (0x801E_DA04, 0x1040_0002), // beq v0,zero,+2 (-> 0x801EDA10)
    (0x801E_DA08, 0x3C02_801D), // lui v0,0x801d
    (0x801E_DA0C, 0x0060_8021), // move s0,v1
];

/// The SCUS anim commit's entry materialisation + playing-id snap the row
/// reading rests on: `addiu v0,v0,-0xcdc` (`= (q-0x10)*0xD0 + 0x24` over
/// `bank+4`), `sw v0,0x0(a0)` (`record0[q*4]`), `sb v0,0x1d9(s1)`.
pub(crate) const COMMIT_FINGERPRINT: [(u32, u32); 3] = [
    (0x8004_BC80, 0x2442_F324), // addiu v0,v0,-0xcdc
    (0x8004_BC84, 0xAC82_0000), // sw v0,0x0(a0)
    (0x8004_BDE0, 0xA222_01D9), // sb v0,0x1d9(s1)
];

/// S: the setup site's second word (`lui v1,0x8008`); the first is
/// [`HOOK_SETUP_W0`].
pub(crate) const HOOK_SETUP_W1: u32 = 0x3C03_8008;

/// Retail's read of the acting slot's party-record id at the builder's head.
const CHAR_READ_VA: u32 = 0x801E_F340;

// --- Runtime addresses the routines index -----------------------------------

/// Battle actor pointer table, indexed by slot.
const ACTOR_TABLE_VA: u32 = 0x801C_9370;
/// Per-party-member `record0` pointer table (the anim / action entry table).
const RECORD0_TABLE_VA: u32 = 0x801C_9360;
/// 1-based party-record id per slot.
const PARTY_ID_TABLE_VA: u32 = 0x8007_BD10;
/// `actor[+0x1D9]`: the playing anim id.
const PLAYING_ID_OFF: u16 = 0x1D9;
/// First art anim id: `q - 0x10` is the art record index, `q - 0x1B` the row.
const ART_ID_BASE: u16 = 0x10;
/// The builder's first art row cursor (`li s3,0xb`).
const ROW_CURSOR_BASE: u16 = 0x0B;
/// The kernel's spill of its `a1` (the action entry) - `sw a1,0x54(sp)`.
const ENTRY_SPILL_OFF: u16 = 0x54;

// --- Routine assemblers ------------------------------------------------------

/// Words that compute the side bit for the builder's current art into `t3`
/// (`0` / `1`), branching to `fail` (a word index) when the row or character is
/// out of range. `t0..t3` scratch. The Spirit load `disp0` rides the first
/// branch's delay slot so `v0` is valid on every path. Shared by the guard and
/// the debit.
fn side_bit_prologue(bits_va: u32, disp0: u32, fail: i32) -> Vec<u32> {
    vec![
        lbu(T1, T6, 0),                                    // 0  t1 = DAT_8007BD10[slot]
        andi(T0, S3, 0xff),                                // 1  load delay: row cursor
        addiu(T0, T0, (-(ROW_CURSOR_BASE as i16)) as u16), // 2 row = s3 - 0xb
        sltiu(T2, T0, NUM_ROWS as u16),                    // 3
        beq(T2, ZERO, (fail - 5) as i16),                  // 4  row out of range
        disp0,                                             // 5  delay: v0 = Spirit
        addiu(T1, T1, 0xFFFF),                             // 6  char index
        sltiu(T2, T1, NUM_CHARS as u16),                   // 7
        beq(T2, ZERO, (fail - 9) as i16),                  // 8  char out of range
        sll(T1, T1, ROW_SHIFT),                            // 9  delay: char * 32
        addu(T0, T0, T1),                                  // 10 bit index
        srl(T2, T0, 3),                                    // 11 byte index
        lui(T3, hi(bits_va)),                              // 12
        addu(T3, T3, T2),                                  // 13
        lbu(T3, T3, lo(bits_va)),                          // 14 side byte
        andi(T2, T0, 7),                                   // 15 load delay: bit
        srlv(T3, T3, T2),                                  // 16
        andi(T3, T3, 1),                                   // 17 t3 = grant side?
    ]
}

/// (A) Affordability guard. A grant-side art forces `v0 = 0x7FFF` so the
/// stock `slt v0,v0,t7` at the return site reads "affordable"; everything
/// else keeps the real Spirit. `disp = [lhu v0,0x170(a1), mflo t7]`; the
/// routine issues no `mult`/`div`, so the replayed `mflo` still reads retail's
/// computed cost.
pub(crate) fn assemble_guard(bits_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
    const DONE: i32 = 21;
    let mut w = side_bit_prologue(bits_va, disp[0], DONE);
    w.extend([
        beq(T3, ZERO, (DONE - 19) as i16), // 18 cost side -> native
        nop(),                             // 19
        ori(V0, ZERO, 0x7FFF),             // 20 grant side: force affordable
        j(ret),                            // 21 DONE
        disp[1],                           // 22 delay: mflo t7 (replay)
    ]);
    debug_assert_eq!(w.len() as i32, DONE + 2);
    w
}

/// (C) Debit. A grant-side art *adds* retail's own charge (`a2`, the
/// `mflo a2` at `0x801EF478`) to Spirit, clamps at [`AP_CAP`], stores, and
/// returns past the stock debit **and** the `+0x224` accrual (nothing was
/// spent, so nothing is refunded). The cost side returns to the stock
/// `subu v0,v0,a2` with `LO` intact. `disp = [lhu v0,0x170(v1), nop]`.
pub(crate) fn assemble_debit(
    bits_va: u32,
    disp: [u32; 2],
    override_ret: u32,
    native_ret: u32,
) -> Vec<u32> {
    const STORE: i32 = 25;
    const NATIVE: i32 = 28;
    let mut w = side_bit_prologue(bits_va, disp[0], NATIVE);
    w.extend([
        beq(T3, ZERO, (NATIVE - 19) as i16), // 18 cost side -> native
        nop(),                               // 19
        addu(V0, V0, A2),                    // 20 Spirit += retail's charge
        sltiu(T1, V0, AP_CAP + 1),           // 21 <= 100?
        bne(T1, ZERO, (STORE - 23) as i16),  // 22
        nop(),                               // 23
        ori(V0, ZERO, AP_CAP),               // 24 clamp
        sh(V0, V1, 0x170),                   // 25 STORE
        j(override_ret),                     // 26 -> past debit + accrual
        nop(),                               // 27
        j(native_ret),                       // 28 NATIVE -> stock subu
        disp[1],                             // 29 delay: nop (replay)
    ]);
    debug_assert_eq!(w.len() as i32, NATIVE + 2);
    w
}

/// (S) The per-battle roll, detoured from the battle loader's setup site.
/// Sixteen `rand()` calls, the low byte of each stored into the side table;
/// the counter lives at `counter_va` (the BIOS clobbers temporaries). Replays
/// the two displaced `lui`s and returns. Exactly 17 words - the slot's size.
pub(crate) fn assemble_roll(bits_va: u32, counter_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
    const LOOP: i32 = 2;
    let w = vec![
        lui(T0, hi(counter_va)),           // 0
        sw(ZERO, T0, lo(counter_va)),      // 1  count = 0
        jal(RAND_FUNC_VA),                 // 2  LOOP
        nop(),                             // 3
        lui(T0, hi(counter_va)),           // 4
        lw(T1, T0, lo(counter_va)),        // 5  count
        nop(),                             // 6  load delay
        addu(T2, T0, T1),                  // 7
        sb(V0, T2, lo(bits_va)),           // 8  bits[count] = low byte of draw
        addiu(T1, T1, 1),                  // 9
        sltiu(T3, T1, BITS_LEN as u16),    // 10
        bne(T3, ZERO, (LOOP - 12) as i16), // 11
        sw(T1, T0, lo(counter_va)),        // 12 delay: count++
        disp[0],                           // 13 lui v0,0x8008
        disp[1],                           // 14 lui v1,0x8008
        j(ret),                            // 15
        nop(),                             // 16
    ];
    debug_assert_eq!(w.len(), 17);
    w
}

/// (M) Grant-side damage scale, detoured from the word after the 9999 cap.
/// Party attacker only; identifies the art from the playing anim id
/// (`row = actor[+0x1D9] - 0x1B`), cross-checked against the entry pointer the
/// kernel was called with; a set side bit rewrites
/// `s0 = s1 + (s0 - s1) * pct / 100`. Every other case replays the displaced
/// words untouched. `disp = [addiu v1,v0,-0x6c90, andi v0,s4,0xff]`.
pub(crate) fn assemble_damage(bits_va: u32, pct: u8, disp: [u32; 2], ret: u32) -> Vec<u32> {
    const NATIVE: i32 = 44;
    let w = vec![
        andi(T0, S6, 0xff),                 // 0  attacker slot
        sltiu(T1, T0, 3),                   // 1  party?
        beq(T1, ZERO, (NATIVE - 3) as i16), // 2
        sll(T1, T0, 2),                     // 3  delay: slot*4
        lui(T2, hi(ACTOR_TABLE_VA)),        // 4
        addu(T2, T2, T1),                   // 5
        lw(T3, T2, lo(ACTOR_TABLE_VA)),     // 6  actor
        lw(T2, T2, lo(RECORD0_TABLE_VA)),   // 7  record0
        lbu(T4, T3, PLAYING_ID_OFF),        // 8  q = playing anim id
        lw(T5, SP, ENTRY_SPILL_OFF),        // 9  the kernel's entry (a1)
        sll(T6, T4, 2),                     // 10 q*4
        addu(T6, T6, T2),                   // 11
        lw(T6, T6, 0),                      // 12 record0[q*4]
        addiu(T4, T4, (-((ART_ID_BASE + ROW_CURSOR_BASE) as i16)) as u16), // 13 row
        bne(T6, T5, (NATIVE - 15) as i16),  // 14 not the art's entry
        sltiu(T1, T4, NUM_ROWS as u16),     // 15 delay
        beq(T1, ZERO, (NATIVE - 17) as i16), // 16 row out of range
        lui(T2, hi(PARTY_ID_TABLE_VA)),     // 17 delay
        addu(T2, T2, T0),                   // 18
        lbu(T2, T2, lo(PARTY_ID_TABLE_VA)), // 19 DAT_8007BD10[slot]
        nop(),                              // 20 load delay
        addiu(T2, T2, 0xFFFF),              // 21 char index
        sltiu(T1, T2, NUM_CHARS as u16),    // 22
        beq(T1, ZERO, (NATIVE - 24) as i16), // 23
        sll(T2, T2, ROW_SHIFT),             // 24 delay: char*32
        addu(T4, T4, T2),                   // 25 bit index
        srl(T2, T4, 3),                     // 26
        lui(T3, hi(bits_va)),               // 27
        addu(T3, T3, T2),                   // 28
        lbu(T3, T3, lo(bits_va)),           // 29 side byte
        andi(T2, T4, 7),                    // 30 load delay
        srlv(T3, T3, T2),                   // 31
        andi(T3, T3, 1),                    // 32
        beq(T3, ZERO, (NATIVE - 34) as i16), // 33 cost side
        subu(T3, S0, S1),                   // 34 delay: damage
        bltz(T3, (NATIVE - 36) as i16),     // 35 never negative; guard anyway
        ori(T4, ZERO, u16::from(pct)),      // 36 delay
        multu(T3, T4),                      // 37
        mflo(T3),                           // 38 damage * pct
        ori(T4, ZERO, 100),                 // 39
        nop(),                              // 40 (mflo -> div spacing)
        divu(T3, T4),                       // 41
        mflo(T3),                           // 42 / 100
        addu(S0, S1, T3),                   // 43 s0 = s1 + scaled
        lui(V0, 0x801D),                    // 44 NATIVE: v0 as the cap left it
        disp[0],                            // 45 addiu v1,v0,-0x6c90
        j(ret),                             // 46
        disp[1],                            // 47 delay: andi v0,s4,0xff
    ];
    debug_assert_eq!(w.len() as i32, NATIVE + 4);
    w
}

// --- The planned injection ---------------------------------------------------

/// A planned oscillating-AP injection: all the same-size writes + where each
/// piece landed (for the oracle to pin the exact bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscillatingApInjection {
    pub edits: Vec<Edit>,
    /// Grant-side damage, percent of retail.
    pub damage_pct: u8,
    pub guard_va: u32,
    pub debit_va: u32,
    pub refund_va: u32,
    pub roll_va: u32,
    pub damage_va: u32,
    /// The 16-byte per-battle side table.
    pub bits_va: u32,
    /// The roll's loop counter word.
    pub counter_va: u32,
}

impl OscillatingApInjection {
    /// Plan all edits for a grant-side damage of `damage_pct` percent. Needs
    /// the `SCUS_942.54` image (setup hook + commit fingerprints + the four
    /// dead regions) and the raw 0898 overlay entry (the four AP sites + the
    /// damage site). Refuses - without touching anything - if the build isn't
    /// the recognized US layout, a region isn't dead, or a routine overruns /
    /// overlaps a live table.
    pub fn plan(scus: &[u8], ov0898: &[u8], damage_pct: u8) -> Result<Self> {
        if damage_pct > MAX_DAMAGE_PCT {
            bail!("oscillating-ap damage percent {damage_pct} exceeds {MAX_DAMAGE_PCT}");
        }

        // --- 0898 fingerprints: the four AP sites (as the arts AP override) --
        let a = ov_hook(ov0898, HOOK_A_VA, HOOK_A_W0)?;
        ov_hook(ov0898, HOOK_B_VA, HOOK_B_W0)?;
        let c = ov_hook(ov0898, HOOK_C_VA, HOOK_C_W0)?;
        let d = ov_hook(ov0898, HOOK_D_VA, HOOK_D_W0)?;
        if a.1[1] != mflo(T7) {
            bail!(
                "0898 site A +4 = {:#010x}, expected mflo t7 (unrecognized build)",
                a.1[1]
            );
        }
        if c.1[1] != nop() || d.1[1] != nop() {
            bail!("0898 site C/D +4 is not the expected nop (unrecognized build)");
        }
        let char_read = read_word(ov0898, (CHAR_READ_VA - OVERLAY_BASE_VA) as usize)?;
        if char_read != lbu(V0, T6, 0) {
            bail!(
                "0898 {CHAR_READ_VA:#x} = {char_read:#010x}, expected lbu v0,0x0(t6) \
                 (the party-record id read the side table keys on) - unrecognized build"
            );
        }
        // --- 0898: the damage site + the 9999 cap in front of it -----------
        let m = ov_hook(ov0898, HOOK_DMG_VA, HOOK_DMG_W0)?;
        if m.1[1] != HOOK_DMG_W1 {
            bail!(
                "0898 damage site +4 = {:#010x}, expected andi v0,s4,0xff (unrecognized build)",
                m.1[1]
            );
        }
        for (va, want) in CAP_FINGERPRINT {
            let got = read_word(ov0898, (va - OVERLAY_BASE_VA) as usize)?;
            if got != want {
                bail!(
                    "0898 {va:#x} = {got:#010x}, expected {want:#010x} (the 9999 damage cap \
                     the damage site follows) - unrecognized build"
                );
            }
        }
        for (va, name) in [
            (HOOK_A_VA, "site-A"),
            (HOOK_C_VA, "site-C"),
            (HOOK_D_VA, "site-D"),
            (HOOK_DMG_VA, "damage site"),
        ] {
            assert_not_in_tables(va, 8, OVERLAY_TABLE_RANGES, name)?;
        }

        // --- SCUS fingerprints: the setup site + the anim commit -----------
        let scus_off = |va: u32| -> Result<usize> {
            legaia_asset::item_names::file_offset_for_va(scus, va)
                .ok_or_else(|| anyhow::anyhow!("can't resolve SCUS VA {va:#x}"))
        };
        let setup_off = scus_off(HOOK_SETUP_VA)?;
        let setup = [read_word(scus, setup_off)?, read_word(scus, setup_off + 4)?];
        if setup != [HOOK_SETUP_W0, HOOK_SETUP_W1] {
            bail!(
                "SCUS setup hook {HOOK_SETUP_VA:#x} = {:#010x} {:#010x}, expected \
                 lui v0,0x8008 / lui v1,0x8008 (unrecognized build)",
                setup[0],
                setup[1]
            );
        }
        for (va, want) in COMMIT_FINGERPRINT {
            let got = read_word(scus, scus_off(va)?)?;
            if got != want {
                bail!(
                    "SCUS {va:#x} = {got:#010x}, expected {want:#010x} (the anim commit's \
                     entry materialisation the row reading rests on) - unrecognized build"
                );
            }
        }

        // --- Layout -----------------------------------------------------------
        let damage_va = SCUS_GAP_VA;
        let damage = assemble_damage(0, damage_pct, m.1, RET_DMG_VA); // sized first
        let bits_va = damage_va + (damage.len() * 4) as u32;
        let counter_va = bits_va + BITS_LEN as u32;
        let gap_end = counter_va + 4;
        let damage = assemble_damage(bits_va, damage_pct, m.1, RET_DMG_VA);

        let guard = assemble_guard(bits_va, a.1, RET_A_VA);
        let debit = assemble_debit(bits_va, c.1, C_OVERRIDE_RET_VA, C_NATIVE_RET_VA);
        let refund = assemble_refund(d.1, RET_D_VA);
        let roll = assemble_roll(bits_va, counter_va, setup, HOOK_SETUP_VA + 8);

        let guard_va = ARENA1_VA;
        let debit_va = guard_va + (guard.len() * 4) as u32;
        let arena1_end = debit_va + (debit.len() * 4) as u32;
        let refund_va = ARENA2_VA;
        let arena2_end = refund_va + (refund.len() * 4) as u32;
        let roll_va = SLOT6_VA;
        let slot6_end = roll_va + (roll.len() * 4) as u32;

        for (va, what) in [
            (guard_va, "guard"),
            (debit_va, "debit"),
            (refund_va, "refund"),
            (roll_va, "roll"),
            (damage_va, "damage"),
            (bits_va, "side table"),
            (counter_va, "counter"),
        ] {
            if va & 3 != 0 {
                bail!("oscillating-ap {what} VA {va:#x} is not 4-byte aligned");
            }
        }
        // Every side-table byte must sit in the page the routines' `lui`
        // addresses (`hi(bits_va)` + a byte index < 16, never crossing the
        // signed-offset boundary).
        if hi(bits_va) != hi(bits_va + BITS_LEN as u32 - 1)
            || hi(counter_va) != hi(bits_va)
            || u32::from(lo(bits_va)) + BITS_LEN as u32 > 0x8000
        {
            bail!("oscillating-ap side table {bits_va:#x} straddles a lui page - refusing");
        }
        if arena1_end > ARENA1_END_VA {
            bail!(
                "oscillating-ap guard + debit ({} B) overrun arena 1 {ARENA1_VA:#x}..{ARENA1_END_VA:#x}",
                arena1_end - ARENA1_VA
            );
        }
        if arena2_end > ARENA2_END_VA {
            bail!(
                "oscillating-ap refund ({} B) overruns arena 2 {ARENA2_VA:#x}..{ARENA2_END_VA:#x}",
                arena2_end - ARENA2_VA
            );
        }
        if slot6_end > SLOT6_END_VA {
            bail!(
                "oscillating-ap roll ({} B) overruns slot 6 {SLOT6_VA:#x}..{SLOT6_END_VA:#x}",
                slot6_end - SLOT6_VA
            );
        }
        if gap_end > SCUS_GAP_END_VA {
            bail!(
                "oscillating-ap damage routine + side table ({} B) overrun the SCUS gap \
                 {SCUS_GAP_VA:#x}..{SCUS_GAP_END_VA:#x}",
                gap_end - SCUS_GAP_VA
            );
        }
        let regions = [
            (ARENA1_VA, arena1_end - ARENA1_VA, "arena1"),
            (ARENA2_VA, arena2_end - ARENA2_VA, "arena2"),
            (SLOT6_VA, slot6_end - SLOT6_VA, "slot6"),
            (SCUS_GAP_VA, gap_end - SCUS_GAP_VA, "gap"),
        ];
        for (va, len, what) in regions {
            assert_not_in_tables(va, len, SCUS_TABLE_RANGES, what)?;
        }
        // Necessary, not sufficient: the regions are also read-watch-verified
        // unreferenced on a live battle - the part a static check can't prove.
        for (va, len, _) in regions {
            assert_zero(scus, scus_off(va)?, len as usize, va)?;
        }

        let detour = |target_va: u32| -> Vec<u8> { words_to_bytes(&[j(target_va), nop()]) };
        let edits = vec![
            // Detours into the 0898 overlay.
            Edit {
                prot_index: Some(OVERLAY_PROT_INDEX),
                file_off: a.0,
                bytes: detour(guard_va),
            },
            Edit {
                prot_index: Some(OVERLAY_PROT_INDEX),
                file_off: c.0,
                bytes: detour(debit_va),
            },
            Edit {
                prot_index: Some(OVERLAY_PROT_INDEX),
                file_off: d.0,
                bytes: detour(refund_va),
            },
            Edit {
                prot_index: Some(OVERLAY_PROT_INDEX),
                file_off: m.0,
                bytes: detour(damage_va),
            },
            // The setup detour in SCUS.
            Edit {
                prot_index: None,
                file_off: setup_off,
                bytes: detour(roll_va),
            },
            // Routines into the dead regions. The side table + counter stay
            // zero on disc (asserted above); the roll fills them per battle.
            Edit {
                prot_index: None,
                file_off: scus_off(guard_va)?,
                bytes: words_to_bytes(&guard),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(debit_va)?,
                bytes: words_to_bytes(&debit),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(refund_va)?,
                bytes: words_to_bytes(&refund),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(roll_va)?,
                bytes: words_to_bytes(&roll),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(damage_va)?,
                bytes: words_to_bytes(&damage),
            },
        ];

        Ok(Self {
            edits,
            damage_pct,
            guard_va,
            debit_va,
            refund_va,
            roll_va,
            damage_va,
            bits_va,
            counter_va,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mips_sim::Cpu;

    const BITS: u32 = SCUS_GAP_VA + 0xC0;
    const CNT: u32 = BITS + 16;

    fn op(w: u32) -> u32 {
        w >> 26
    }
    fn funct(w: u32) -> u32 {
        w & 0x3f
    }
    /// Follow a branch at word `i` and return the word index it lands on.
    fn br(words: &[u32], i: usize) -> i32 {
        i as i32 + 1 + ((words[i] & 0xffff) as i16 as i32)
    }
    /// Registers an instruction reads (a conservative decode of the subset the
    /// routines use).
    fn reads(w: u32) -> Vec<u32> {
        let rs = (w >> 21) & 31;
        let rt = (w >> 16) & 31;
        match op(w) {
            0 => match funct(w) {
                0x00 | 0x02 | 0x03 => vec![rt], // sll/srl/sra
                0x10 | 0x12 => vec![],          // mfhi/mflo
                0x08 => vec![rs],               // jr
                _ => vec![rs, rt],              // 3-register ops, mult/div, srlv
            },
            0x02 | 0x03 | 0x0f => vec![],       // j/jal/lui
            0x01 | 0x06 | 0x07 => vec![rs],     // bltz/bgez/blez/bgtz
            0x04 | 0x05 => vec![rs, rt],        // beq/bne
            0x28 | 0x29 | 0x2b => vec![rs, rt], // sb/sh/sw
            _ => vec![rs],                      // I-type incl. loads
        }
    }
    fn is_load(w: u32) -> bool {
        matches!(op(w), 0x20 | 0x21 | 0x23 | 0x24 | 0x25)
    }
    /// No word reads the register the previous word loaded (R3000 load delay).
    fn assert_no_load_delay_hazard(words: &[u32], what: &str) {
        for i in 0..words.len().saturating_sub(1) {
            if is_load(words[i]) {
                let rt = (words[i] >> 16) & 31;
                assert!(
                    !reads(words[i + 1]).contains(&rt),
                    "{what}: word {} reads ${rt} in the load-delay slot of word {i}",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn fingerprints_match_documented_disassembly() {
        assert_eq!(HOOK_DMG_W0, addiu(V1, V0, 0x9370), "addiu v1,v0,-0x6c90");
        assert_eq!(HOOK_DMG_W1, andi(V0, S4, 0xff));
        assert_eq!(CAP_FINGERPRINT[0].1, sltu(V0, V1, S0));
        assert_eq!(CAP_FINGERPRINT[1].1, beq(V0, ZERO, 2));
        assert_eq!(CAP_FINGERPRINT[2].1, lui(V0, 0x801D));
        assert_eq!(CAP_FINGERPRINT[3].1, addu(S0, V1, ZERO), "move s0,v1");
        assert_eq!(COMMIT_FINGERPRINT[0].1, addiu(V0, V0, (-0xCDC_i16) as u16));
        assert_eq!(COMMIT_FINGERPRINT[1].1, sw(V0, A0, 0));
        assert_eq!(COMMIT_FINGERPRINT[2].1, sb(V0, S1, 0x1D9));
        assert_eq!(HOOK_SETUP_W0, lui(V0, 0x8008));
        assert_eq!(HOOK_SETUP_W1, lui(V1, 0x8008));
        // `(q - 0x10) * 0xD0 + 0x24 - 0x10 * 0xD0` folds to the commit's -0xCDC.
        assert_eq!(0x10 * 0xD0 - 0x24, 0xCDC);
        // The row the damage routine derives: q - 0x10 = s3, s3 - 0x0B = row.
        assert_eq!(ART_ID_BASE + ROW_CURSOR_BASE, 0x1B);
    }

    #[test]
    fn routines_have_no_load_delay_hazards_and_fit() {
        let guard = assemble_guard(BITS, [HOOK_A_W0, mflo(T7)], RET_A_VA);
        let debit = assemble_debit(BITS, [HOOK_C_W0, nop()], C_OVERRIDE_RET_VA, C_NATIVE_RET_VA);
        let roll = assemble_roll(BITS, CNT, [HOOK_SETUP_W0, HOOK_SETUP_W1], HOOK_SETUP_VA + 8);
        let damage = assemble_damage(BITS, 20, [HOOK_DMG_W0, HOOK_DMG_W1], RET_DMG_VA);
        assert_no_load_delay_hazard(&guard, "guard");
        assert_no_load_delay_hazard(&debit, "debit");
        assert_no_load_delay_hazard(&roll, "roll");
        assert_no_load_delay_hazard(&damage, "damage");
        assert_eq!(guard.len(), 23);
        assert_eq!(debit.len(), 30);
        assert_eq!(roll.len(), 17, "the roll fills slot 6 exactly");
        assert_eq!(damage.len(), 48);
        assert!((guard.len() + debit.len()) * 4 <= (ARENA1_END_VA - ARENA1_VA) as usize);
        assert!(roll.len() * 4 == (SLOT6_END_VA - SLOT6_VA) as usize);
        assert!(damage.len() * 4 + BITS_LEN + 4 <= (SCUS_GAP_END_VA - SCUS_GAP_VA) as usize);
        // Neither AP routine touches HI/LO before replaying into a live `mflo`.
        for w in guard.iter().chain(debit.iter()) {
            assert!(!(op(*w) == 0 && matches!(funct(*w), 0x18..=0x1b)));
        }
        // Branch targets land where the comments say.
        assert_eq!(br(&guard, 4), 21);
        assert_eq!(br(&guard, 8), 21);
        assert_eq!(br(&guard, 18), 21);
        assert_eq!(br(&debit, 18), 28);
        assert_eq!(br(&debit, 22), 25);
        assert_eq!(br(&roll, 11), 2);
        for i in [2, 14, 16, 23, 33, 35] {
            assert_eq!(br(&damage, i), 44, "damage word {i} -> NATIVE");
        }
        // `mflo` sits three words clear of the `divu` that follows it.
        assert_eq!(funct(damage[38]), 0x12);
        assert_eq!(funct(damage[41]), 0x1b);
    }

    // --- Simulated executions ----------------------------------------------

    const ACTOR: u32 = 0x8010_0000;
    const RECORD0: u32 = 0x8011_0000;
    const ENTRY: u32 = 0x8011_2000;
    const SP0: u32 = 0x801F_FF00;
    const GUARD: u32 = ARENA1_VA;
    const DEBIT: u32 = ARENA1_VA + 0x80;
    const ROLL: u32 = SLOT6_VA;
    const DMG: u32 = SCUS_GAP_VA;

    fn set_bit(cpu: &mut Cpu, char_idx: u32, row: u32) {
        let idx = char_idx * 32 + row;
        let a = BITS + idx / 8;
        let b = cpu.rd8(a) | (1 << (idx % 8));
        cpu.wr8(a, b);
    }

    /// Guard/debit scene: slot 0 is party-record `char_id`, the builder is on
    /// row cursor `s3`, Spirit is `spirit`, retail's charge is `charge`.
    fn ap_cpu(char_id: u8, s3: u32, spirit: u16, charge: u32) -> Cpu {
        let mut cpu = Cpu::new();
        cpu.load_words(
            GUARD,
            &assemble_guard(BITS, [HOOK_A_W0, mflo(T7)], RET_A_VA),
        );
        cpu.load_words(
            DEBIT,
            &assemble_debit(BITS, [HOOK_C_W0, nop()], C_OVERRIDE_RET_VA, C_NATIVE_RET_VA),
        );
        cpu.wr8(PARTY_ID_TABLE_VA, char_id);
        cpu.wr16(ACTOR + 0x170, spirit);
        cpu.wr8(ACTOR + 0x224, 5);
        cpu.r[T6 as usize] = PARTY_ID_TABLE_VA;
        cpu.r[S3 as usize] = s3;
        cpu.r[A1 as usize] = ACTOR; // site A's actor
        cpu.r[V1 as usize] = ACTOR; // site C's actor
        cpu.r[A2 as usize] = charge;
        cpu.lo = 0x1234; // retail's computed cost, pending in LO at site A
        cpu
    }

    #[test]
    fn guard_admits_only_a_grant_side_art() {
        // Vahn (id 1), row 3 (s3 = 0x0E), cost side: Spirit stays, t7 = LO.
        let mut cpu = ap_cpu(1, 0x0E, 12, 33);
        cpu.pc = GUARD;
        assert_eq!(cpu.run_until(&[RET_A_VA]), RET_A_VA);
        assert_eq!(cpu.r[V0 as usize], 12);
        assert_eq!(cpu.r[T7 as usize], 0x1234);
        // Same art on the grant side: reads as affordable.
        let mut cpu = ap_cpu(1, 0x0E, 12, 33);
        set_bit(&mut cpu, 0, 3);
        cpu.pc = GUARD;
        cpu.run_until(&[RET_A_VA]);
        assert_eq!(cpu.r[V0 as usize], 0x7FFF);
        assert_eq!(cpu.r[T7 as usize], 0x1234);
        // Noa's row 3 bit does not leak onto Vahn's row 3.
        let mut cpu = ap_cpu(1, 0x0E, 12, 33);
        set_bit(&mut cpu, 1, 3);
        cpu.pc = GUARD;
        cpu.run_until(&[RET_A_VA]);
        assert_eq!(cpu.r[V0 as usize], 12);
        // A row past the 26 (a Super Art) and an unknown party id are native.
        for (id, s3) in [(1, 0x0B + 26), (5, 0x0E), (0, 0x0E)] {
            let mut cpu = ap_cpu(id, s3, 12, 33);
            for b in 0..16 {
                cpu.wr8(BITS + b, 0xFF);
            }
            cpu.pc = GUARD;
            cpu.run_until(&[RET_A_VA]);
            assert_eq!(cpu.r[V0 as usize], 12, "id {id} s3 {s3:#x}");
        }
    }

    #[test]
    fn debit_grants_retail_charge_on_the_grant_side_only() {
        // Cost side: the stock debit runs (native return, Spirit loaded).
        let mut cpu = ap_cpu(3, 0x0B, 40, 18);
        cpu.pc = DEBIT;
        assert_eq!(
            cpu.run_until(&[C_NATIVE_RET_VA, C_OVERRIDE_RET_VA]),
            C_NATIVE_RET_VA
        );
        assert_eq!(cpu.r[V0 as usize], 40);
        assert_eq!(
            cpu.rd16(ACTOR + 0x170),
            40,
            "nothing stored on the cost side"
        );
        // Grant side (Gala, row 0): Spirit += 18, accrual untouched, past the
        // stock debit + accrual.
        let mut cpu = ap_cpu(3, 0x0B, 40, 18);
        set_bit(&mut cpu, 2, 0);
        cpu.pc = DEBIT;
        assert_eq!(
            cpu.run_until(&[C_NATIVE_RET_VA, C_OVERRIDE_RET_VA]),
            C_OVERRIDE_RET_VA
        );
        assert_eq!(cpu.rd16(ACTOR + 0x170), 58);
        assert_eq!(cpu.rd8(ACTOR + 0x224), 5, "no spend accrued for a grant");
        // Clamped at 100.
        let mut cpu = ap_cpu(3, 0x0B, 90, 18);
        set_bit(&mut cpu, 2, 0);
        cpu.pc = DEBIT;
        cpu.run_until(&[C_OVERRIDE_RET_VA]);
        assert_eq!(cpu.rd16(ACTOR + 0x170), 100);
        // Admitted at 0 AP too.
        let mut cpu = ap_cpu(3, 0x0B, 0, 18);
        set_bit(&mut cpu, 2, 0);
        cpu.pc = DEBIT;
        cpu.run_until(&[C_OVERRIDE_RET_VA]);
        assert_eq!(cpu.rd16(ACTOR + 0x170), 18);
    }

    /// A fake `rand`: returns a running LCG state from a scratch word, so each
    /// call differs, and clobbers a temporary the way the BIOS does.
    fn install_fake_rand(cpu: &mut Cpu) {
        const SEED: u32 = 0x8012_0000;
        cpu.wr32(SEED, 0x0102_0304);
        cpu.load_words(
            RAND_FUNC_VA,
            &[
                lui(T9, hi(SEED)),
                lw(V0, T9, lo(SEED)),
                nop(),
                addiu(T1, V0, 0x0101), // clobber t1 like a real callee might
                sw(T1, T9, lo(SEED)),
                jr(RA),
                nop(),
            ],
        );
    }

    #[test]
    fn roll_fills_the_side_table_with_sixteen_draws_and_replays() {
        let mut cpu = Cpu::new();
        cpu.load_words(
            ROLL,
            &assemble_roll(BITS, CNT, [HOOK_SETUP_W0, HOOK_SETUP_W1], HOOK_SETUP_VA + 8),
        );
        install_fake_rand(&mut cpu);
        cpu.pc = ROLL;
        assert_eq!(cpu.run_until(&[HOOK_SETUP_VA + 8]), HOOK_SETUP_VA + 8);
        assert_eq!(cpu.rd32(CNT), 16);
        let bytes: Vec<u8> = (0..16).map(|i| cpu.rd8(BITS + i)).collect();
        let want: Vec<u8> = (0..16u32)
            .map(|i| (0x0102_0304u32 + i * 0x101) as u8)
            .collect();
        assert_eq!(bytes, want, "low byte of each successive draw");
        assert_eq!(cpu.r[V0 as usize], 0x8008_0000, "replayed lui v0");
        assert_eq!(cpu.r[V1 as usize], 0x8008_0000, "replayed lui v1");
    }

    /// Damage scene: attacker `slot` (party record `char_id`) playing anim id
    /// `q`, the kernel called with entry `entry`, strike damage `dmg` over base
    /// `base`.
    fn dmg_cpu(slot: u32, char_id: u8, q: u8, entry: u32, base: u32, dmg: u32, pct: u8) -> Cpu {
        let mut cpu = Cpu::new();
        cpu.load_words(
            DMG,
            &assemble_damage(BITS, pct, [HOOK_DMG_W0, HOOK_DMG_W1], RET_DMG_VA),
        );
        cpu.wr32(ACTOR_TABLE_VA + slot * 4, ACTOR);
        cpu.wr32(RECORD0_TABLE_VA + slot * 4, RECORD0);
        cpu.wr8(PARTY_ID_TABLE_VA + slot, char_id);
        cpu.wr8(ACTOR + 0x1D9, q);
        cpu.wr32(RECORD0 + u32::from(q) * 4, ENTRY); // the commit's materialised entry
        cpu.wr32(SP0 + 0x54, entry);
        cpu.r[SP as usize] = SP0;
        cpu.r[S6 as usize] = slot;
        cpu.r[S4 as usize] = 0x1_0005; // defender slot 5 (with junk above the byte)
        cpu.r[S1 as usize] = base;
        cpu.r[S0 as usize] = base + dmg;
        cpu.r[V0 as usize] = 0x801D_0000;
        cpu
    }

    fn run_dmg(cpu: &mut Cpu) -> u32 {
        cpu.pc = DMG;
        assert_eq!(cpu.run_until(&[RET_DMG_VA]), RET_DMG_VA);
        // The replayed words leave v1 / v0 exactly as retail would.
        assert_eq!(cpu.r[V1 as usize], ACTOR_TABLE_VA);
        assert_eq!(cpu.r[V0 as usize], 5);
        cpu.r[S0 as usize] - cpu.r[S1 as usize]
    }

    #[test]
    fn damage_scales_a_grant_side_art_and_nothing_else() {
        // Vahn (slot 0, id 1) executing row 4 (q = 0x1F), 1000 damage at 20%.
        let mut cpu = dmg_cpu(0, 1, 0x1F, ENTRY, 7, 1000, 20);
        assert_eq!(run_dmg(&mut cpu), 1000, "cost side: untouched");
        let mut cpu = dmg_cpu(0, 1, 0x1F, ENTRY, 7, 1000, 20);
        set_bit(&mut cpu, 0, 4);
        assert_eq!(run_dmg(&mut cpu), 200, "grant side: 20%");
        assert_eq!(cpu.r[S1 as usize], 7, "the base is kept");
        // Exact integer arithmetic, floor.
        let mut cpu = dmg_cpu(0, 1, 0x1F, ENTRY, 0, 999, 33);
        set_bit(&mut cpu, 0, 4);
        assert_eq!(run_dmg(&mut cpu), 329);
        // 100% and 0% are exact.
        let mut cpu = dmg_cpu(0, 1, 0x1F, ENTRY, 0, 9999, 100);
        set_bit(&mut cpu, 0, 4);
        assert_eq!(run_dmg(&mut cpu), 9999);
        let mut cpu = dmg_cpu(0, 1, 0x1F, ENTRY, 3, 9999, 0);
        set_bit(&mut cpu, 0, 4);
        assert_eq!(run_dmg(&mut cpu), 0);
        // Another character's bit for the same row does not leak.
        let mut cpu = dmg_cpu(0, 1, 0x1F, ENTRY, 0, 1000, 20);
        set_bit(&mut cpu, 1, 4);
        assert_eq!(run_dmg(&mut cpu), 1000);
        // Gala in slot 2 (id 3), row 0 (q = 0x1B).
        let mut cpu = dmg_cpu(2, 3, 0x1B, ENTRY, 0, 500, 50);
        set_bit(&mut cpu, 2, 0);
        assert_eq!(run_dmg(&mut cpu), 250);
    }

    #[test]
    fn damage_falls_through_on_every_non_art_shape() {
        let all_set = |cpu: &mut Cpu| {
            for b in 0..16 {
                cpu.wr8(BITS + b, 0xFF);
            }
        };
        // A monster attacker (slot 3).
        let mut cpu = dmg_cpu(3, 1, 0x1F, ENTRY, 0, 1000, 20);
        all_set(&mut cpu);
        assert_eq!(run_dmg(&mut cpu), 1000);
        // A plain direction swing (q < 0x1B) - its entry is elsewhere.
        let mut cpu = dmg_cpu(0, 1, 0x0D, ENTRY, 0, 1000, 20);
        all_set(&mut cpu);
        assert_eq!(run_dmg(&mut cpu), 1000);
        // A row past the 26 (a Super Art).
        let mut cpu = dmg_cpu(0, 1, 0x1B + 26, ENTRY, 0, 1000, 20);
        all_set(&mut cpu);
        assert_eq!(run_dmg(&mut cpu), 1000);
        // The playing id's entry is not the entry the kernel was called with.
        let mut cpu = dmg_cpu(0, 1, 0x1F, ENTRY + 0xD0, 0, 1000, 20);
        all_set(&mut cpu);
        assert_eq!(run_dmg(&mut cpu), 1000);
        // An out-of-range party id.
        let mut cpu = dmg_cpu(0, 5, 0x1F, ENTRY, 0, 1000, 20);
        all_set(&mut cpu);
        assert_eq!(run_dmg(&mut cpu), 1000);
        // A negative delta (never produced by the kernel) is left alone.
        let mut cpu = dmg_cpu(0, 1, 0x1F, ENTRY, 100, 0, 20);
        cpu.r[S0 as usize] = 50;
        all_set(&mut cpu);
        cpu.pc = DMG;
        cpu.run_until(&[RET_DMG_VA]);
        assert_eq!(cpu.r[S0 as usize], 50);
    }

    #[test]
    fn plan_lays_pieces_out_in_the_four_regions_and_refuses_a_bad_build() {
        // A synthetic SCUS: a flat image whose VA→offset map is identity off
        // 0x80010000 (as `file_offset_for_va` reads the PS-X EXE header).
        let scus = synthetic_scus();
        let ov = synthetic_overlay();
        let plan = OscillatingApInjection::plan(&scus, &ov, 20).expect("plan");
        assert_eq!(plan.guard_va, ARENA1_VA);
        assert_eq!(plan.refund_va, ARENA2_VA);
        assert_eq!(plan.roll_va, SLOT6_VA);
        assert_eq!(plan.damage_va, SCUS_GAP_VA);
        assert_eq!(plan.bits_va, SCUS_GAP_VA + 48 * 4);
        assert_eq!(plan.counter_va, plan.bits_va + 16);
        assert_eq!(plan.edits.len(), 10);
        assert_eq!(
            plan.edits.iter().filter(|e| e.prot_index.is_some()).count(),
            4
        );
        // Over 100% is refused; a shifted damage site is refused.
        assert!(OscillatingApInjection::plan(&scus, &ov, 101).is_err());
        let mut bad = ov.clone();
        let off = (HOOK_DMG_VA - OVERLAY_BASE_VA) as usize;
        bad[off..off + 4].copy_from_slice(&nop().to_le_bytes());
        assert!(OscillatingApInjection::plan(&scus, &bad, 20).is_err());
        // A dirty slot 6 (another feature's bytes) is refused.
        let mut dirty = scus.clone();
        let off = legaia_asset::item_names::file_offset_for_va(&dirty, SLOT6_VA).unwrap();
        dirty[off] = 1;
        assert!(OscillatingApInjection::plan(&dirty, &ov, 20).is_err());
    }

    /// A PS-X EXE-shaped SCUS image large enough to hold every VA the plan
    /// touches, with the setup site + commit fingerprints in place and the
    /// four regions zero.
    fn synthetic_scus() -> Vec<u8> {
        let base = 0x8001_0000u32;
        let size = 0x7_0000u32;
        let mut img = vec![0u8; 0x800 + size as usize];
        img[..8].copy_from_slice(b"PS-X EXE");
        img[0x18..0x1C].copy_from_slice(&base.to_le_bytes());
        img[0x1C..0x20].copy_from_slice(&size.to_le_bytes());
        let mut put = |va: u32, w: u32| {
            let off = legaia_asset::item_names::file_offset_for_va(&img, va).unwrap();
            img[off..off + 4].copy_from_slice(&w.to_le_bytes());
        };
        put(HOOK_SETUP_VA, HOOK_SETUP_W0);
        put(HOOK_SETUP_VA + 4, HOOK_SETUP_W1);
        for (va, w) in COMMIT_FINGERPRINT {
            put(va, w);
        }
        img
    }

    /// A 0898-shaped overlay with every fingerprinted word in place.
    fn synthetic_overlay() -> Vec<u8> {
        let mut ov = vec![0u8; 0x3_0000];
        let mut put = |va: u32, w: u32| {
            let off = (va - OVERLAY_BASE_VA) as usize;
            ov[off..off + 4].copy_from_slice(&w.to_le_bytes());
        };
        put(HOOK_A_VA, HOOK_A_W0);
        put(HOOK_A_VA + 4, mflo(T7));
        put(HOOK_B_VA, HOOK_B_W0);
        put(HOOK_C_VA, HOOK_C_W0);
        put(HOOK_D_VA, HOOK_D_W0);
        put(CHAR_READ_VA, lbu(V0, T6, 0));
        put(HOOK_DMG_VA, HOOK_DMG_W0);
        put(HOOK_DMG_VA + 4, HOOK_DMG_W1);
        for (va, w) in CAP_FINGERPRINT {
            put(va, w);
        }
        ov
    }
}
