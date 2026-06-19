//! Run-away EXP reward: a code hook into the battle-action **escape teardown**
//! that banks a small slice of the battle's experience into the party whenever
//! they successfully flee - vanilla awards nothing for running.
//!
//! ## Why a code hook (not a data edit)
//!
//! "Some EXP on a successful escape" is a runtime *behaviour*, not a value in a
//! table: vanilla never even reaches an EXP grant on the flee path. So, like the
//! [`crate::bonus_drop`] equipment hook, this splices a tiny routine into the
//! executable. It runs at exactly the moment a flee is confirmed, sums the
//! formation's listed experience, and adds a fixed percentage of it to each
//! party member's cumulative-XP cell.
//!
//! ## The hook (US build, battle-action overlay = PROT entry 898)
//!
//! The per-actor battle state machine `FUN_801E295C` (battle-action overlay,
//! base VA `0x801CE818`) handles "Run" across states `0x64..0x66`. State `0x66`
//! is the **successful-escape teardown** - reached only when the run roll
//! succeeds (the failed run goes `0x65 -> 0x50` and the battle continues; see
//! `docs/subsystems/battle-action.md`). Its handler begins at VA `0x801E5A10`:
//!
//! ```text
//! 801e5a10  lui   v1,0x801d          ; \ a0 = &DAT_801C9070 (the fade template)
//! 801e5a14  addiu a0,v1,-0x6f90      ; /
//! 801e5a18  clear a1                 ; <- handler continues here (writes the fade
//! 801e5a1c  li    v0,0x2             ;    template + spawns the white-out, then
//! 801e5a20  sh    v0,-0x6f90(v1)     ;    sets the battle-end signal 0xFE)
//! ```
//!
//! We overwrite the two instructions at `0x801E5A10`/`0x801E5A14` with
//! `j <routine>` + `nop` (a detour), run the EXP-grant routine, replay those two
//! displaced instructions, and `j 0x801E5A18` back. State `0x66` advances itself
//! to the terminal `0x67` (no body), so it runs exactly once per escape - the
//! grant fires once per successful flee. The handler clobbers `v0`/`v1`/`a0`/`a1`
//! freely after the join and restores `ra` from its own stack frame, and the
//! party HP was already floored to `>= 1` in state `0x64` (the "escape restores a
//! downed member" mechanism), so at the join every party member is alive and the
//! routine is free to use the caller-saved registers and bank EXP to all slots.
//!
//! ## The routine (lives in preserved rodata padding)
//!
//! Written into the same loaded-and-preserved 1028-byte zero gap at `0x8007AB38`
//! the [`crate::item_name`] / [`crate::bonus_drop`] injections use, but at a
//! non-overlapping offset ([`ROUTINE_VA`] = `0x8007AD00`, clear of the bonus
//! equipment routine + its id table). On PSX all resident RAM is executable, so a
//! routine placed there runs when jumped to. The routine:
//!
//! 1. Sums the formation's experience: it walks the live enemy record-pointer
//!    table at [`ENEMY_TABLE_VA`] (`0x801C9348`) for `actor[+1]` entries
//!    ([`ACTOR_PTR_VA`] = `*0x8007BD24`) and accumulates each record's EXP
//!    halfword (`+0x46`, the same field the victory-spoils routine
//!    `FUN_8004E568` reads).
//! 2. Scales that total to [`pct`](Self::pct) percent (`total * pct / 100`).
//! 3. Adds the scaled amount to **every** party member's cumulative-XP cell. The
//!    party slot -> character-record-id map is at [`SLOT_ID_MAP_VA`]
//!    (`0x8007BD10`), the record array is based at [`RECORD_BASE_VA`]
//!    (`0x80084140`, stride [`RECORD_STRIDE`] = `0x414`), and cumulative XP lives
//!    at [`XP_OFFSET`] (`+0x5C8`) - exactly where `FUN_8004E568` accumulates a
//!    win's EXP and where `FUN_801E9504` reads it to apply levels. Each cell is
//!    clamped to the game's [`XP_CAP`] (`9,999,999`).
//!
//! The grant is **banked**, not applied as an immediate level-up: it only writes
//! the cumulative-XP cell (it does not call the level processor), so the EXP shows
//! in the status screen right away and the character levels up the next time a won
//! battle runs `FUN_801E9504` over the accumulated total. This keeps the routine
//! small and side-effect-free during the escape fade (no stray level-up screen).
//!
//! Two same-size edits: the detour at the escape-teardown hook (PROT entry 898,
//! raw - the overlay maps linearly from base `0x801CE818`) and the routine blob
//! in `SCUS_942.54` rodata padding. The planner guards on the detour-site words
//! matching the known US build and on the routine region being all-zero dead
//! space, so a differently-laid-out image is refused rather than corrupted. No
//! Sony bytes are embedded: the routine is the randomizer's own code.

use anyhow::{Result, bail};

use legaia_asset::item_names;

/// PROT entry index of the battle-action overlay that hosts the escape teardown.
/// (Same overlay the move-power / element-affinity randomizers edit.)
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;

/// Load base VA of the battle-action overlay. A VA inside it maps to PROT-entry
/// file offset `va - OVERLAY_BASE_VA` (the overlay is stored raw; see
/// [`legaia_asset::move_power::BATTLE_OVERLAY_BASE`]).
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// Detour site: the first of the two instructions we replace with `j routine` +
/// `nop` (the escape-teardown handler entry, state `0x66`).
pub const HOOK_VA: u32 = 0x801E_5A10;
/// Where the detour returns to (the instruction after the displaced pair).
pub const RETURN_VA: u32 = 0x801E_5A18;
/// The two original instructions at [`HOOK_VA`] we displace into the routine and
/// replay: `lui v1,0x801d` then `addiu a0,v1,-0x6f90`. Also the recognized-build
/// fingerprint the planner guards on.
pub const DISPLACED: [u32; 2] = [0x3C03_801D, 0x2464_9070];

/// Load VA of the injected routine, inside the preserved rodata gap at
/// `0x8007AB38`. Placed past the bonus-equipment routine + its (<= 200-id) table
/// so both hooks can coexist (the planner re-checks the region is all-zero).
pub const ROUTINE_VA: u32 = 0x8007_AD00;
/// End of the preserved zero gap (exclusive); the routine must fit below this.
pub const GAP_END_VA: u32 = 0x8007_AF40;

/// Live battle context pointer (`*0x8007BD24`): `actor[+0]` = party member count,
/// `actor[+1]` = enemy count.
pub const ACTOR_PTR_VA: u32 = 0x8007_BD24;
/// Per-enemy record-pointer table (`0x801C9348`): `actor[+1]` entries of 4 bytes,
/// each a pointer to a monster record whose EXP halfword is at `+0x46`.
pub const ENEMY_TABLE_VA: u32 = 0x801C_9348;
/// EXP halfword offset within a monster record (the victory-spoils field).
pub const ENEMY_EXP_OFFSET: u16 = 0x46;
/// Party slot -> character-record-id map (`0x8007BD10`): byte per slot, a 1-based
/// record id.
pub const SLOT_ID_MAP_VA: u32 = 0x8007_BD10;
/// Base of the live character-record array (`0x80084140`); record `n` is at
/// `RECORD_BASE_VA + (id-1) * RECORD_STRIDE`.
pub const RECORD_BASE_VA: u32 = 0x8008_4140;
/// Stride between live character records.
pub const RECORD_STRIDE: u32 = 0x414;
/// Cumulative-XP cell offset within a record (relative to [`RECORD_BASE_VA`] +
/// `id*stride`). `0x5C8` = the record's experience field (`0x80084708` for id 0).
pub const XP_OFFSET: u16 = 0x5C8;
/// Experience cap the game enforces (`9,999,999`).
pub const XP_CAP: u32 = 0x0098_967F;

/// Default percentage of the formation's experience banked on a successful flee.
pub const DEFAULT_PCT: u8 = 5;

// --- MIPS R3000 instruction encoders (little-endian words) ------------------
//
// Register numbers are the MIPS ABI indices; `imm`/`off` are the raw 16-bit
// fields (already two's-complement for negative offsets).

const ZERO: u32 = 0;
const V0: u32 = 2;
const V1: u32 = 3;
const A0: u32 = 4;
const A1: u32 = 5;
const A2: u32 = 6;
const A3: u32 = 7;
const T0: u32 = 8;
const T1: u32 = 9;
const T2: u32 = 10;

const fn j(target: u32) -> u32 {
    (0x02 << 26) | ((target >> 2) & 0x03ff_ffff)
}
const fn nop() -> u32 {
    0
}
const fn lui(rt: u32, imm: u16) -> u32 {
    (0x0f << 26) | (rt << 16) | imm as u32
}
const fn ori(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0d << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn addiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x09 << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn lbu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x24 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn lhu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x25 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn lw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x23 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn sw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x2b << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn addu(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x21
}
const fn sll(rd: u32, rt: u32, sa: u32) -> u32 {
    (rt << 16) | (rd << 11) | (sa << 6)
}
const fn slt(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x2a
}
const fn sltu(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x2b
}
const fn beq(rs: u32, rt: u32, off: i16) -> u32 {
    (0x04 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
const fn multu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x19
}
const fn divu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x1b
}
const fn mflo(rd: u32) -> u32 {
    (rd << 11) | 0x12
}
/// `move rd, rs` (an `addu rd, rs, zero`).
const fn mv(rd: u32, rs: u32) -> u32 {
    addu(rd, rs, ZERO)
}

/// The low 16 bits of a VA (for `addiu`/`lw`/`sw` offsets off a `lui` high half).
const fn lo(va: u32) -> u16 {
    (va & 0xffff) as u16
}
/// The high 16 bits a `lui` must load so a following signed-`lo` access reaches
/// `va` (the `+0x8000` corrects for the low half's sign extension).
const fn hi(va: u32) -> u16 {
    (va.wrapping_add(0x8000) >> 16) as u16
}

/// Assemble the EXP-grant routine for a `pct` gate. The routine sums the
/// formation EXP, scales it to `pct`%, banks it into every party member's
/// cumulative-XP cell (clamped to [`XP_CAP`]), then replays [`DISPLACED`] and
/// jumps back to [`RETURN_VA`]. 64 instructions, fully self-contained.
pub fn assemble_routine(pct: u8) -> Vec<u32> {
    // Branch targets are computed by instruction index below; keep these in sync
    // with the layout (asserted in the unit tests).
    const SUM_LOOP: usize = 8;
    const AFTER_SUM: usize = 19;
    const GRANT_LOOP: usize = 34;
    const STORE: usize = 56;
    const RET: usize = 60;
    // beq off (words) = target_idx - (branch_idx + 1).
    let sum_exit_off = (AFTER_SUM as i32 - (9 + 1)) as i16; // beq at idx 9
    let grant_exit_off = (RET as i32 - (35 + 1)) as i16; // beq at idx 35
    let clamp_skip_off = (STORE as i32 - (53 + 1)) as i16; // beq at idx 53
    let sum_loop_va = ROUTINE_VA + (SUM_LOOP as u32) * 4;
    let grant_loop_va = ROUTINE_VA + (GRANT_LOOP as u32) * 4;

    let words = vec![
        // --- sum the formation EXP -------------------------------------------
        lui(V0, hi(ACTOR_PTR_VA)),         // 0:  v0 = hi(actor ptr addr)
        lw(A3, V0, lo(ACTOR_PTR_VA)),      // 1:  a3 = *0x8007BD24 (battle ctx)
        nop(),                             // 2:  (load delay)
        lbu(T1, A3, 1),                    // 3:  t1 = enemy count (actor[+1])
        lui(A1, hi(ENEMY_TABLE_VA)),       // 4:  \ a1 = enemy record-ptr table
        addiu(A1, A1, lo(ENEMY_TABLE_VA)), // 5:  /   (0x801C9348)
        mv(A2, ZERO),                      // 6:  a2 = total EXP = 0
        mv(T0, ZERO),                      // 7:  t0 = i = 0
        // SUM_LOOP (idx 8): while (i < enemy_count)
        slt(V0, T0, T1),               // 8
        beq(V0, ZERO, sum_exit_off),   // 9:  -> AFTER_SUM
        nop(),                         // 10: (delay)
        lw(V1, A1, 0),                 // 11: v1 = enemy record ptr
        nop(),                         // 12: (load delay)
        lhu(V0, V1, ENEMY_EXP_OFFSET), // 13: v0 = record EXP (+0x46)
        nop(),                         // 14: (load delay)
        addu(A2, A2, V0),              // 15: total += EXP
        addiu(T0, T0, 1),              // 16: i++
        j(sum_loop_va),                // 17: -> SUM_LOOP
        addiu(A1, A1, 4),              // 18: (delay) next table entry
        // --- scale to pct% (idx 19 = AFTER_SUM) ------------------------------
        addiu(T0, ZERO, pct as u16), // 19: t0 = pct
        multu(A2, T0),               // 20: lo = total * pct
        mflo(A2),                    // 21: a2 = total * pct
        addiu(T0, ZERO, 100),        // 22: t0 = 100
        divu(A2, T0),                // 23: lo = (total*pct) / 100
        mflo(A2),                    // 24: a2 = banked per-member EXP
        // --- bank into every party member ------------------------------------
        lui(V0, hi(ACTOR_PTR_VA)),         // 25: \ a3 = battle ctx (reload)
        lw(A3, V0, lo(ACTOR_PTR_VA)),      // 26: /
        nop(),                             // 27: (load delay)
        lbu(T1, A3, 0),                    // 28: t1 = party member count (actor[+0])
        lui(A0, hi(SLOT_ID_MAP_VA)),       // 29: \ a0 = slot->record-id map
        addiu(A0, A0, lo(SLOT_ID_MAP_VA)), // 30: /   (0x8007BD10)
        lui(A1, hi(RECORD_BASE_VA)),       // 31: \ a1 = record array base
        addiu(A1, A1, lo(RECORD_BASE_VA)), // 32: /   (0x80084140)
        mv(T0, ZERO),                      // 33: i = 0
        // GRANT_LOOP (idx 34): while (i < party_count)
        slt(V0, T0, T1),               // 34
        beq(V0, ZERO, grant_exit_off), // 35: -> RET
        nop(),                         // 36: (delay)
        addu(V0, A0, T0),              // 37: &slotmap[i]
        lbu(V1, V0, 0),                // 38: v1 = record id (1-based)
        nop(),                         // 39: (load delay)
        addiu(V1, V1, 0xFFFF),         // 40: v1 = id - 1
        sll(V0, V1, 6),                // 41: \
        addu(V0, V0, V1),              // 42:  |
        sll(V0, V0, 2),                // 43:  | v0 = (id-1) * 0x414
        addu(V0, V0, V1),              // 44:  |
        sll(V0, V0, 2),                // 45: /
        addu(V0, V0, A1),              // 46: v0 = &record (base + (id-1)*stride)
        lw(V1, V0, XP_OFFSET),         // 47: v1 = cumulative XP (+0x5C8)
        nop(),                         // 48: (load delay)
        addu(V1, V1, A2),              // 49: XP += banked
        lui(T2, hi(XP_CAP)),           // 50: \ t2 = XP cap (0x98967F)
        ori(T2, T2, lo(XP_CAP)),       // 51: /
        sltu(A3, T2, V1),              // 52: a3 = (cap < XP) ? 1 : 0
        beq(A3, ZERO, clamp_skip_off), // 53: -> STORE (no clamp)
        nop(),                         // 54: (delay)
        mv(V1, T2),                    // 55: XP = cap
        // STORE (idx 56)
        sw(V1, V0, XP_OFFSET), // 56: record.XP = v1
        addiu(T0, T0, 1),      // 57: i++
        j(grant_loop_va),      // 58: -> GRANT_LOOP
        nop(),                 // 59: (delay)
        // RET (idx 60): replay displaced instructions, return.
        DISPLACED[0], // 60: lui v1,0x801d
        DISPLACED[1], // 61: addiu a0,v1,-0x6f90
        j(RETURN_VA), // 62: back to the join
        nop(),        // 63: (delay)
    ];
    debug_assert_eq!(words.len(), 64);
    debug_assert_eq!(words[RET], DISPLACED[0]);
    debug_assert_eq!(words[SUM_LOOP], slt(V0, T0, T1));
    debug_assert_eq!(words[GRANT_LOOP], slt(V0, T0, T1));
    let _ = STORE;
    words
}

/// The two detour words written at [`HOOK_VA`]: `j ROUTINE_VA` then `nop`.
pub fn detour_words() -> [u32; 2] {
    [j(ROUTINE_VA), nop()]
}

/// A planned injection: the two same-size writes - the detour at the escape-
/// teardown hook (PROT entry [`BATTLE_ACTION_OVERLAY_PROT_INDEX`]) and the routine
/// blob in `SCUS_942.54` rodata padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleeExpInjection {
    /// File offset of [`HOOK_VA`] within the battle-action overlay PROT entry;
    /// receives [`detour_words`].
    pub overlay_hook_off: usize,
    /// The two detour words to write at the hook.
    pub detour: [u32; 2],
    /// File offset of [`ROUTINE_VA`] within `SCUS_942.54`; receives [`Self::blob`].
    pub routine_off: usize,
    /// Routine bytes (little-endian words).
    pub blob: Vec<u8>,
    /// Percentage of formation EXP banked per member on a flee.
    pub pct: u8,
}

impl FleeExpInjection {
    /// Plan the injection given the `SCUS_942.54` image, the battle-action
    /// overlay's raw PROT entry, and `pct`. Fails (rather than corrupts) if the
    /// build isn't recognized: the detour-site words must match the known US
    /// build, and the routine region must be all-zero dead space within the
    /// preserved gap.
    pub fn plan(scus: &[u8], overlay: &[u8], pct: u8) -> Result<Self> {
        if pct == 0 || pct > 100 {
            bail!("flee-EXP percent {pct} out of range 1..=100");
        }

        // Detour site lives in the overlay, which maps linearly from its base VA.
        let overlay_hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
        let at_hook = [
            read_word(overlay, overlay_hook_off)?,
            read_word(overlay, overlay_hook_off + 4)?,
        ];
        if at_hook != DISPLACED {
            bail!(
                "escape-teardown hook {HOOK_VA:#x} = [{:#010x}, {:#010x}], expected \
                 [{:#010x}, {:#010x}] (unrecognized build) - refusing to patch",
                at_hook[0],
                at_hook[1],
                DISPLACED[0],
                DISPLACED[1],
            );
        }

        let routine = assemble_routine(pct);
        let blob: Vec<u8> = routine.iter().flat_map(|w| w.to_le_bytes()).collect();

        // Routine + a margin must fit the preserved gap and land on all-zero
        // dead space (so we never clobber real rodata or the bonus-drop routine).
        let blob_end_va = ROUTINE_VA + blob.len() as u32;
        if blob_end_va > GAP_END_VA {
            bail!(
                "flee-EXP routine ({} bytes) overruns the preserved gap end {GAP_END_VA:#x}",
                blob.len()
            );
        }
        let routine_off = item_names::file_offset_for_va(scus, ROUTINE_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve routine VA {ROUTINE_VA:#x} in SCUS"))?;
        let region = scus
            .get(routine_off..routine_off + blob.len())
            .ok_or_else(|| anyhow::anyhow!("routine region past end of SCUS"))?;
        if region.iter().any(|&b| b != 0) {
            bail!(
                "flee-EXP routine region {ROUTINE_VA:#x}..+{} is not all-zero dead space \
                 (unrecognized build / collides with another injection) - refusing to patch",
                blob.len()
            );
        }

        Ok(Self {
            overlay_hook_off,
            detour: detour_words(),
            routine_off,
            blob,
            pct,
        })
    }
}

fn read_word(buf: &[u8], off: usize) -> Result<u32> {
    let b = buf
        .get(off..off + 4)
        .ok_or_else(|| anyhow::anyhow!("buffer too short at {off:#x}"))?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let r = assemble_routine(5);
        assert_eq!(r.len(), 64);
        // EXP sum reads the enemy table base and the per-record EXP halfword.
        assert_eq!(r[5], addiu(A1, A1, lo(ENEMY_TABLE_VA)));
        assert_eq!(r[13], lhu(V0, V1, ENEMY_EXP_OFFSET));
        // The pct scale: * pct then / 100.
        assert_eq!(r[19], addiu(T0, ZERO, 5));
        assert_eq!(r[20], multu(A2, T0));
        assert_eq!(r[22], addiu(T0, ZERO, 100));
        assert_eq!(r[23], divu(A2, T0));
        // The grant writes the cumulative-XP cell.
        assert_eq!(r[47], lw(V1, V0, XP_OFFSET));
        assert_eq!(r[56], sw(V1, V0, XP_OFFSET));
        // It returns by replaying the displaced pair then `j RETURN_VA`.
        assert_eq!(r[60], DISPLACED[0]);
        assert_eq!(r[61], DISPLACED[1]);
        assert_eq!(op(r[62]), 0x02);
        assert_eq!((r[62] & 0x03ff_ffff) << 2, RETURN_VA & 0x0fff_ffff);
    }

    #[test]
    fn loop_branches_target_the_right_blocks() {
        let r = assemble_routine(5);
        // sum-loop exit beq (idx 9) -> AFTER_SUM (idx 19): off = 9.
        assert_eq!(op(r[9]), 0x04);
        assert_eq!((r[9] & 0xffff) as i16, 9);
        // grant-loop exit beq (idx 35) -> RET (idx 60): off = 24.
        assert_eq!(op(r[35]), 0x04);
        assert_eq!((r[35] & 0xffff) as i16, 24);
        // clamp-skip beq (idx 53) -> STORE (idx 56): off = 2.
        assert_eq!((r[53] & 0xffff) as i16, 2);
        // back-edges jump to the loop heads.
        assert_eq!(
            (r[17] & 0x03ff_ffff) << 2,
            (ROUTINE_VA + 8 * 4) & 0x0fff_ffff
        );
        assert_eq!(
            (r[58] & 0x03ff_ffff) << 2,
            (ROUTINE_VA + 34 * 4) & 0x0fff_ffff
        );
    }

    #[test]
    fn displaced_pair_matches_the_documented_disassembly() {
        // lui v1,0x801d ; addiu a0,v1,-0x6f90
        assert_eq!(DISPLACED[0], lui(V1, 0x801d));
        assert_eq!(DISPLACED[1], addiu(A0, V1, (-0x6f90i32) as u16));
    }

    #[test]
    fn overlay_hook_offset_is_linear_from_base() {
        assert_eq!((HOOK_VA - OVERLAY_BASE_VA) as usize, 0x171F8);
    }

    #[test]
    fn pct_must_be_in_range() {
        let scus = vec![0u8; 0x100];
        let overlay = vec![0u8; 0x100];
        assert!(FleeExpInjection::plan(&scus, &overlay, 0).is_err());
        assert!(FleeExpInjection::plan(&scus, &overlay, 101).is_err());
    }

    #[test]
    fn plan_rejects_unrecognized_hook() {
        // Overlay big enough to reach the hook offset but holding the wrong bytes.
        let overlay = vec![0u8; (HOOK_VA - OVERLAY_BASE_VA) as usize + 16];
        let scus = vec![0u8; 0x100];
        let err = FleeExpInjection::plan(&scus, &overlay, 5).unwrap_err();
        assert!(err.to_string().contains("escape-teardown hook"));
    }

    #[test]
    fn routine_fits_the_preserved_gap() {
        let blob_len = assemble_routine(5).len() * 4;
        assert!(ROUTINE_VA + blob_len as u32 <= GAP_END_VA);
        // And it must not overlap the bonus-equipment routine + its max table.
        let bonus_end =
            crate::bonus_drop::ROUTINE_VA + 22 * 4 + crate::bonus_drop::MAX_TABLE_LEN as u32;
        assert!(
            ROUTINE_VA >= bonus_end,
            "flee routine overlaps bonus-drop region"
        );
    }
}
