//! Bonus equipment drop: a code hook into the battle-end reward routine that,
//! on a low chance, grants **one extra** random piece of equipment — on top of
//! whatever the monster's normal drop slot already gives.
//!
//! ## Why a code hook (not a data edit)
//!
//! A monster record has a single drop slot (`+0x48` item id / `+0x49` chance,
//! see [`crate::monster`]), so a data-only edit can never make a monster drop
//! *two* things — turning a drop into equipment necessarily destroys the normal
//! drop. To make equipment genuinely **additive** we instead patch the
//! executable's reward resolver the same way the starting-bag feature splices a
//! grant into the opening scene: a tiny routine is injected that rolls the
//! game's own RNG and, on success, calls the inventory-add helper for a random
//! equipment id. The normal drop table (vanilla or randomized) is left entirely
//! alone.
//!
//! ## The hook (US build `SCUS_942.54`)
//!
//! The battle-end reward routine `FUN_8004E568` tallies a battle's spoils
//! exactly once (gated on the per-battle state byte `actor+0x6ce == 0`, which it
//! then advances to `1`). Right after it grants the formation's normal drop via
//! `FUN_800421d4(item, 1)` at `0x8004f608`, control joins at `0x8004f610`:
//!
//! ```text
//! 8004f610  lui v0,0x8008          ; \ reload _DAT_8007bac0 (the no-reward flag)
//! 8004f614  lw  v0,-0x4540(v0)     ; / for the next branch
//! 8004f618  nop
//! 8004f61c  bne v0,zero,0x8004f668
//! ```
//!
//! We overwrite the two instructions at `0x8004f610`/`0x8004f614` with
//! `j <routine>` + `nop` (a detour), run our extra-drop routine, replay those
//! two displaced instructions, and `j 0x8004f618` back. The join is reached once
//! per battle, so the extra-drop roll fires once per battle. At the join the
//! only live registers are `$s8`/`$gp`/`$sp` (which the called helpers preserve)
//! and `$v0` (which the displaced `lui`/`lw` reload), so the routine is free to
//! clobber `$v0`/`$v1`/`$a0`/`$a1`/`$t0`/`$ra`.
//!
//! ## The routine (lives in preserved rodata padding)
//!
//! The injected routine + an equipment-id table are written into the 1028-byte
//! zero gap at `0x8007AB38` — the same loaded-and-preserved rodata padding the
//! [`crate::item_name`] string injection uses, but at a non-overlapping offset
//! ([`ROUTINE_VA`], clear of the Seru-Bell string at `0x8007AB40`). On PSX all
//! resident RAM is executable, so a routine placed in that gap runs when jumped
//! to. The routine:
//!
//! 1. `rand() % 100 < chance` — the low-chance gate (default
//!    [`DEFAULT_CHANCE_PCT`]). The roll reuses the battle RNG [`RAND_FN`].
//! 2. `rand() % table_len` indexes the embedded equipment-id table.
//! 3. `FUN_800421d4(id, 1)` ([`ADD_ITEM_FN`]) adds the gear to the bag — the
//!    same helper the normal drop, shop, and minigame rewards use (an unguarded
//!    add, like the minigame completion reward `FUN_801C2748`).
//!
//! The grant is silent (it doesn't push a victory-screen "received" line); the
//! item simply appears in the bag after the battle.
//!
//! Every write is a same-size, in-place `SCUS_942.54` edit. The planner guards
//! on the two detour-site words matching the known US build and on the routine
//! region being all-zero dead space, so a differently-laid-out image is refused
//! rather than corrupted. No Sony bytes are embedded: the routine is the
//! randomizer's own code and the id table comes from the user's disc.

use anyhow::{Result, bail};

use legaia_asset::item_names;

/// Detour site: the first of the two instructions we replace with `j routine`
/// + `nop` (the reward-routine join right after the normal drop grant).
pub const HOOK_VA: u32 = 0x8004_F610;
/// Where the detour returns to (the instruction after the displaced pair).
pub const RETURN_VA: u32 = 0x8004_F618;
/// The two original instructions at [`HOOK_VA`] we displace into the routine and
/// replay: `lui v0,0x8008` then `lw v0,-0x4540(v0)`. Also the recognized-build
/// fingerprint the planner guards on.
pub const DISPLACED: [u32; 2] = [0x3c02_8008, 0x8c42_bac0];

/// Load VA of the injected routine + table, inside the preserved rodata gap at
/// `0x8007AB38` (64 bytes in, clear of the Seru-Bell string at `0x8007AB40`).
pub const ROUTINE_VA: u32 = 0x8007_AB80;
/// End of the preserved zero gap (exclusive). The routine + table must fit below
/// this; the planner additionally checks the region is all-zero.
pub const GAP_END_VA: u32 = 0x8007_AF40;

/// Battle RNG (`FUN_80056798`, the LCG used for drop / tiebreak rolls).
pub const RAND_FN: u32 = 0x8005_6798;
/// Inventory add helper (`FUN_800421d4(id, count)`).
pub const ADD_ITEM_FN: u32 = 0x8004_21D4;

/// Default low chance (percent) for the extra equipment drop, rolled once per
/// battle. Deliberately low: it fires across a whole playthrough of battles.
pub const DEFAULT_CHANCE_PCT: u8 = 5;

/// Upper bound on the embedded id table so the routine + table always fit the
/// preserved gap with margin (the real equipment pool is ~150 ids).
pub const MAX_TABLE_LEN: usize = 200;

// --- MIPS R3000 instruction encoders (little-endian words) ------------------
//
// Only the handful of forms the routine needs. Register numbers are the MIPS
// ABI indices; `imm`/`off` are the raw 16-bit fields (already two's-complement
// for negative offsets).

const ZERO: u32 = 0;
const V0: u32 = 2;
const A0: u32 = 4;
const A1: u32 = 5;
const T0: u32 = 8;

const fn j(target: u32) -> u32 {
    (0x02 << 26) | ((target >> 2) & 0x03ff_ffff)
}
const fn jal(target: u32) -> u32 {
    (0x03 << 26) | ((target >> 2) & 0x03ff_ffff)
}
const fn nop() -> u32 {
    0
}
const fn addiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x09 << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn slti(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0a << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn beq(rs: u32, rt: u32, off: i16) -> u32 {
    (0x04 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
const fn lui(rt: u32, imm: u16) -> u32 {
    (0x0f << 26) | (rt << 16) | imm as u32
}
const fn lbu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x24 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn addu(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x21
}
const fn divu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x1b
}
const fn mfhi(rd: u32) -> u32 {
    (rd << 11) | 0x10
}

/// Split a VA into the `lui` high half + signed `lbu`/`lw` low half so
/// `lui rX, hi; lbu rY, lo(rX+...)` addresses `va` (the `+0x8000` corrects for
/// the low half's sign extension).
const fn hi_lo(va: u32) -> (u16, u16) {
    let hi = (va.wrapping_add(0x8000) >> 16) as u16;
    let lo = (va & 0xffff) as u16;
    (hi, lo)
}

/// Assemble the injected routine (22 instructions) for a table at `table_va`
/// with `table_len` entries and a `chance_pct` gate. The routine ends by
/// replaying [`DISPLACED`] and jumping back to [`RETURN_VA`].
pub fn assemble_routine(table_va: u32, table_len: usize, chance_pct: u8) -> Vec<u32> {
    let (tab_hi, tab_lo) = hi_lo(table_va);
    // Lay instructions out so the L_skip branch offset can be computed by index.
    // Index of the join-replay block (`lui v0,0x8008`) the gate skips to:
    const L_SKIP_IDX: usize = 18;
    const BEQ_IDX: usize = 6;
    // beq is PC-relative: offset in words = target - (delay-slot instruction).
    let skip_off = (L_SKIP_IDX as i32 - (BEQ_IDX as i32 + 1)) as i16;

    let words = vec![
        jal(RAND_FN),                      // 0:  rand()
        nop(),                             // 1:  (delay slot)
        addiu(T0, ZERO, 100),              // 2:  t0 = 100
        divu(V0, T0),                      // 3:  v0 / 100
        mfhi(V0),                          // 4:  v0 = rand % 100
        slti(V0, V0, chance_pct as u16),   // 5: v0 = (rand%100 < chance)
        beq(V0, ZERO, skip_off),           // 6:  if !hit -> L_skip
        nop(),                             // 7:  (delay slot)
        jal(RAND_FN),                      // 8:  rand()
        nop(),                             // 9:  (delay slot)
        addiu(T0, ZERO, table_len as u16), // 10: t0 = table_len
        divu(V0, T0),                      // 11: v0 / table_len
        mfhi(V0),                          // 12: v0 = rand % table_len (index)
        lui(A0, tab_hi),                   // 13: a0 = hi(table)
        addu(A0, A0, V0),                  // 14: a0 += index
        lbu(A0, A0, tab_lo),               // 15: a0 = table[index] (equipment id)
        jal(ADD_ITEM_FN),                  // 16: FUN_800421d4(id, ...)
        addiu(A1, ZERO, 1),                // 17: count = 1 (delay slot)
        // L_skip (index 18): replay the two displaced instructions, return.
        DISPLACED[0], // 18: lui v0,0x8008
        DISPLACED[1], // 19: lw v0,-0x4540(v0)
        j(RETURN_VA), // 20: back to the join
        nop(),        // 21: (delay slot)
    ];
    debug_assert_eq!(words.len(), 22);
    debug_assert!(matches!(words[L_SKIP_IDX], w if w == DISPLACED[0]));
    words
}

/// The two detour words written at [`HOOK_VA`]: `j ROUTINE_VA` then `nop`.
pub fn detour_words() -> [u32; 2] {
    [j(ROUTINE_VA), nop()]
}

/// Serialize a routine word list followed by the id table into one contiguous
/// little-endian blob to write at [`ROUTINE_VA`].
fn blob_bytes(routine: &[u32], ids: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(routine.len() * 4 + ids.len());
    for &w in routine {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.extend_from_slice(ids);
    out
}

/// A planned injection: the two same-size writes to `SCUS_942.54` (the detour at
/// the hook site and the routine+table blob in the rodata gap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BonusDropInjection {
    /// File offset of [`HOOK_VA`]; receives [`detour_words`].
    pub hook_off: usize,
    /// File offset of [`ROUTINE_VA`]; receives [`Self::blob`].
    pub blob_off: usize,
    /// Routine + id-table bytes.
    pub blob: Vec<u8>,
    /// VA of the embedded id table (after the routine).
    pub table_va: u32,
    /// Number of equipment ids embedded.
    pub table_len: usize,
    /// The low-chance gate (percent).
    pub chance_pct: u8,
}

impl BonusDropInjection {
    /// Plan the injection for a `SCUS_942.54` image with the given equipment
    /// `ids` and `chance_pct`. Fails (rather than corrupts) if the build isn't
    /// recognized: the detour-site words must match the known US build, and the
    /// routine region must be all-zero dead space within the preserved gap.
    pub fn plan(scus: &[u8], ids: &[u8], chance_pct: u8) -> Result<Self> {
        if ids.is_empty() {
            bail!("equipment id table is empty");
        }
        if chance_pct == 0 || chance_pct > 100 {
            bail!("chance percent {chance_pct} out of range 1..=100");
        }
        let ids: Vec<u8> = ids.iter().copied().take(MAX_TABLE_LEN).collect();

        let hook_off = item_names::file_offset_for_va(scus, HOOK_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve hook VA {HOOK_VA:#x} in SCUS"))?;
        // Guard: the detour site must be the known build's `lui`/`lw` pair.
        let at_hook = [read_word(scus, hook_off)?, read_word(scus, hook_off + 4)?];
        if at_hook != DISPLACED {
            bail!(
                "reward-hook site {HOOK_VA:#x} = [{:#010x}, {:#010x}], expected \
                 [{:#010x}, {:#010x}] (unrecognized build) — refusing to patch",
                at_hook[0],
                at_hook[1],
                DISPLACED[0],
                DISPLACED[1],
            );
        }

        let routine = assemble_routine(table_va(&ids), ids.len(), chance_pct);
        let blob = blob_bytes(&routine, &ids);

        // Guard: the routine + table must fit the preserved gap and land on
        // all-zero dead space (so we never clobber real rodata).
        let blob_end_va = ROUTINE_VA + blob.len() as u32;
        if blob_end_va > GAP_END_VA {
            bail!(
                "routine+table ({} bytes) overruns the preserved gap end {GAP_END_VA:#x}",
                blob.len()
            );
        }
        let blob_off = item_names::file_offset_for_va(scus, ROUTINE_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve routine VA {ROUTINE_VA:#x} in SCUS"))?;
        let region = scus
            .get(blob_off..blob_off + blob.len())
            .ok_or_else(|| anyhow::anyhow!("routine region past end of SCUS"))?;
        if region.iter().any(|&b| b != 0) {
            bail!(
                "routine region {ROUTINE_VA:#x}..+{} is not all-zero dead space \
                 (unrecognized build) — refusing to patch",
                blob.len()
            );
        }

        Ok(Self {
            hook_off,
            blob_off,
            blob,
            table_va: table_va(&ids),
            table_len: ids.len(),
            chance_pct,
        })
    }
}

/// VA the id table lands at: immediately after the 22-instruction routine.
fn table_va(ids: &[u8]) -> u32 {
    let _ = ids; // routine length is fixed (22 instructions)
    ROUTINE_VA + 22 * 4
}

fn read_word(scus: &[u8], off: usize) -> Result<u32> {
    let b = scus
        .get(off..off + 4)
        .ok_or_else(|| anyhow::anyhow!("SCUS too short at {off:#x}"))?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a routine word as enough of an instruction to self-check the
    /// assembler without a disassembler dependency.
    fn op(word: u32) -> u32 {
        word >> 26
    }

    #[test]
    fn detour_jumps_to_the_routine() {
        let [w0, w1] = detour_words();
        assert_eq!(op(w0), 0x02, "first detour word is a `j`");
        assert_eq!((w0 & 0x03ff_ffff) << 2, ROUTINE_VA & 0x0fff_ffff);
        assert_eq!(w1, 0, "delay slot is a nop");
    }

    #[test]
    fn routine_has_the_expected_shape() {
        let ids: Vec<u8> = (1..=64).collect();
        let r = assemble_routine(table_va(&ids), ids.len(), 5);
        assert_eq!(r.len(), 22);
        // Two RNG calls (jal RAND_FN) at the gate and the index roll.
        assert_eq!(r[0], jal(RAND_FN));
        assert_eq!(r[8], jal(RAND_FN));
        // The grant call.
        assert_eq!(r[16], jal(ADD_ITEM_FN));
        // The gate compares rand%100 < chance.
        assert_eq!(r[5], slti(V0, V0, 5));
        // The two displaced instructions are replayed verbatim.
        assert_eq!(r[18], DISPLACED[0]);
        assert_eq!(r[19], DISPLACED[1]);
        // It returns with a `j RETURN_VA`.
        assert_eq!(op(r[20]), 0x02);
        assert_eq!((r[20] & 0x03ff_ffff) << 2, RETURN_VA & 0x0fff_ffff);
    }

    #[test]
    fn gate_branch_skips_to_the_replay_block() {
        let r = assemble_routine(table_va(&[1, 2, 3]), 3, 8);
        // beq at index 6, target index 18 -> offset = 18 - (6+1) = 11 words.
        let off = (r[6] & 0xffff) as i16;
        assert_eq!(off, 11);
        assert_eq!(op(r[6]), 0x04, "index gate is a beq");
    }

    #[test]
    fn hi_lo_round_trips_through_the_sign_correction() {
        // A VA whose low half has the sign bit set needs the +1 hi correction.
        let va = 0x8007_abd8;
        let (hi, lo) = hi_lo(va);
        let recon = ((hi as u32) << 16).wrapping_add(lo as i16 as u32);
        assert_eq!(recon, va);
    }

    #[test]
    fn chance_must_be_in_range() {
        let scus = vec![0u8; 0x100]; // too small / no header, but chance check is first
        assert!(BonusDropInjection::plan(&scus, &[1], 0).is_err());
        assert!(BonusDropInjection::plan(&scus, &[1], 101).is_err());
    }

    #[test]
    fn empty_table_is_rejected() {
        let scus = vec![0u8; 0x100];
        assert!(BonusDropInjection::plan(&scus, &[], 5).is_err());
    }
}
