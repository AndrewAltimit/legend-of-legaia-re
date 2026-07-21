//! Enemy-ally ("charm") battle feature: a code hook into the **battle setup**
//! that, with a per-battle probability, flags one enemy so it fights on the
//! player's side - an uncontrolled ally, like a guest character, in any
//! **multi-enemy** fight. Single-enemy fights are skipped (see
//! [`SECOND_MONSTER_ID_VA`]): charming the lone enemy of an input-gated tutorial
//! (the Tetsu sparring match) softlocks the scripted fight, and solo bosses are
//! likewise scripted set-pieces.
//!
//! ## Why a code hook (and why "charm", not a 4th party member)
//!
//! Retail battles are hard-wired to **3 party slots + up to 4 monster slots**
//! (`FUN_800513F0`: the party loop is bounded `< 3`, the monster loop `< 4`;
//! party meshes/CLUTs/HUD only exist for slots 0..2). Splicing in a genuine 4th
//! player-side combatant would need rendering, HUD, turn-order and a party-side
//! AI action picker that retail doesn't expose. So instead this rides a mechanic
//! the game **already** implements: the "AI-delegated" flag.
//!
//! Setting an actor's flag bits `+0x16E |= 0x380` makes the per-actor action SM
//! `FUN_801E295C` (battle-action overlay) call the retarget helper `FUN_801E7320`
//! at ActionSeed (state `0x0C`), which **flips that actor's target to the opposite
//! side**. For a *monster*, the side flip means it attacks the *other monsters* -
//! i.e. it fights for the player. The monster AI picker `FUN_801E9FD4` already
//! honours `0x380` (it drops the monster's scripted specials and uses a plain
//! attack), and the retarget is built in - so "an enemy assists you" is just
//! "set `0x380` on one monster at battle setup". No AI-picker or targeting patch
//! is needed; the confuse/charm retarget is stock behaviour.
//!
//! ## The two edits
//!
//! 1. **Setup detour (SCUS, `FUN_800513F0`).** Right after the monster-setup loop
//!    (the battle-actor table at `0x801C9370` and the live enemy count
//!    `ctx[+1]` are populated by then), the two instructions at [`HOOK_VA`]
//!    (`0x80051990`):
//!
//!    ```text
//!    80051990  lui v1,0x8008          ; \ v1 = DAT_8007BD0C (first monster id),
//!    80051994  lbu v1,-0x42f4(v1)     ; /   feeding the next `== 0xB5` check
//!    80051998  ...                    ; <- the detour returns here
//!    ```
//!
//!    are replaced with `j <routine>` + `nop`. The routine rolls the per-battle
//!    chance and, on a hit, OR's `0x380` into the frontmost monster
//!    (actor-table slot 3, `0x801C937C` - always populated, since every battle has
//!    at least one enemy), then replays the two displaced instructions and jumps
//!    back to [`RETURN_VA`]. `FUN_800513F0` saves/restores `ra` on its own frame,
//!    so the routine is free to `jal` the BIOS RNG and clobber the caller-saved
//!    registers; `v0`/`v1` are reloaded by the replay and the code at the join.
//!
//! 2. **Victory-check widen (battle-action overlay 0898).** The monster-wipe gate
//!    in `FUN_801E295C` state `0x5A` counts a monster as "down" if it is dead
//!    (`+0x14C == 0`) **or** non-targetable (`+0x16E & 0x4`); the test instruction
//!    at [`VICTORY_VA`] (`0x801E6638`) is `andi v0,v0,0x4`. A charmed monster is
//!    alive and not `0x4`, so without this edit the player would have to *kill
//!    their own ally* to win. Widening the mask to `0x384` (`andi v0,v0,0x384`)
//!    makes a charmed (`0x380`) monster also count as "down", so victory triggers
//!    once the real enemies fall. (Side effect: a vanilla *confuse*-on-an-enemy,
//!    which also sets `0x380`, likewise stops counting toward "enemies remaining"
//!    while this option is on - a minor, opt-in behaviour change.)
//!
//! ## The routine (lives in preserved rodata padding)
//!
//! Written into the loaded-and-preserved 1028-byte zero gap at `0x8007AB38` that
//! the other code-injection features use, at [`ROUTINE_VA`] = `0x8007ACA0` - the
//! window between the bonus-equipment routine + its (<= 200-id) table
//! (`0x8007AB80`..`0x8007ACA0`) and the flee-EXP routine (`0x8007AD00`), so all
//! the gap features can coexist. On PSX all resident RAM is executable, so a
//! routine placed there runs when jumped to.
//!
//! Three same-size edits, all guarded: the detour + routine in `SCUS_942.54`
//! rodata padding (the detour-site words must match the known US build and the
//! routine region must be all-zero dead space), and the one-word victory mask in
//! the battle-action overlay PROT entry (the original `andi v0,v0,0x4` word is
//! verified before it is widened). A differently-laid-out image is refused, not
//! corrupted. No Sony bytes are embedded: the routine is the randomizer's own
//! code.
//!
//! ## Charm battle softlock - fixed (see [`crate::charm_fix`])
//!
//! The [`VICTORY_VA`] widen has a second-order effect: it desyncs the state-`0x5A`
//! monster-wipe scan (now masking `0x384`) from the initiative scheduler
//! `FUN_801DABA4` (still masking `0x4`). The scheduler keeps picking the living
//! charmed ally, so when the ally's own action kills the last real enemy, the
//! monster-wipe branch fires with a **living monster** as the acting actor - and
//! the victory arm's win-pose staging then indexes the 3-byte party roster
//! `DAT_8007BD10` out of bounds (`0x801E6770`), arming a garbage "ME" archive
//! request. The battle wedges at the victory hand-off. This is the pinned cause
//! of the user-reported charm hard-freeze; the earlier "unbounded reroll in
//! `FUN_801E7320`" theory is falsified (see
//! `docs/subsystems/battle.md` § "Enemy-ally charm at the end-of-action gate").
//!
//! [`crate::charm_fix`] closes it with a **single-word overlay detour** at the
//! victory-arm keep-branch (`0x801E6690`) plus a small guard in the SCUS rodata
//! gap: the acting slot is kept only when it is a living **party** slot, and any
//! other case routes into retail's own valid-slot re-pick. The fix is applied
//! **automatically** by [`crate::apply::inject_enemy_ally`] whenever the charm
//! feature is enabled, so the widen and its softlock fix always ship together.

use anyhow::{Result, bail};

use legaia_asset::item_names;

use crate::mips::*;

/// PROT entry index of the battle-action overlay that hosts the victory check.
/// (Same overlay the flee-EXP / move-power / element-affinity randomizers edit.)
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;

/// Load base VA of the battle-action overlay. A VA inside it maps to PROT-entry
/// file offset `va - OVERLAY_BASE_VA` (the overlay is stored raw).
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// Detour site (SCUS): the first of the two instructions we replace with
/// `j routine` + `nop`, right after the battle-setup monster loop in
/// `FUN_800513F0`.
pub const HOOK_VA: u32 = 0x8005_1990;
/// Where the detour returns to (the instruction after the displaced pair).
pub const RETURN_VA: u32 = 0x8005_1998;
/// The two original instructions at [`HOOK_VA`] we displace into the routine and
/// replay: `lui v1,0x8008` then `lbu v1,-0x42f4(v1)`. Also the recognized-build
/// fingerprint the planner guards on.
pub const DISPLACED: [u32; 2] = [0x3C03_8008, 0x9063_BD0C];

/// Load VA of the injected routine, inside the preserved rodata gap at
/// `0x8007AB38`, in the free window between the bonus-drop routine+table and the
/// flee-EXP routine (so every gap feature can coexist).
pub const ROUTINE_VA: u32 = 0x8007_ACA0;
/// First VA used by the next gap occupant (the flee-EXP routine); the routine
/// must end at or below this.
pub const ROUTINE_REGION_END_VA: u32 = 0x8007_AD00;

/// Second formation slot (`DAT_8007BD0C[1]`). Zero when the fight has a single
/// enemy. Charm **skips single-enemy fights**: charming the lone enemy of an
/// input-gated tutorial (the Tetsu sparring match, monster id 0x4F) softlocks the
/// scripted fight (the tutorial waits for the enemy that is now an ally), and
/// solo story bosses are likewise scripted set-pieces; multi-enemy fights are the
/// random encounters where an uncontrolled ally is the intended, safe effect.
pub const SECOND_MONSTER_ID_VA: u32 = 0x8007_BD0D;

/// Battle-actor pointer table (`0x801C9370`): slots 0..2 party, 3..6 monsters.
/// Slot 3 (`0x801C937C`) is the frontmost enemy, always present in a battle.
pub const ACTOR_SLOT3_VA: u32 = 0x801C_937C;
/// Per-actor flag halfword offset (`+0x16E`); bit pattern `0x380` = AI-delegated.
pub const FIELD_FLAGS_OFFSET: u16 = 0x16E;
/// The AI-delegated bits that flip an actor's target to the opposite side.
pub const AI_DELEGATE_BITS: u16 = 0x380;
/// BIOS-ish RNG routine the game uses (`rand`); returns a value in `v0`.
pub const RAND_FUNC_VA: u32 = 0x8005_6798;

/// Victory-check test instruction site (battle-action overlay, state `0x5A`
/// monster-wipe loop).
pub const VICTORY_VA: u32 = 0x801E_6638;
/// The stock instruction at [`VICTORY_VA`]: `andi v0,v0,0x4` (the recognized-build
/// fingerprint guarded before widening).
pub const VICTORY_ORIG: u32 = 0x3042_0004;
/// The widened instruction: `andi v0,v0,0x384` (treat `0x380`-charmed monsters,
/// as well as `0x4` non-targetable ones, as "down" for the victory count).
pub const VICTORY_PATCHED: u32 = 0x3042_0384;

/// Default per-battle probability (percent) that an enemy is charmed.
pub const DEFAULT_PCT: u8 = 20;

// MIPS R3000 encoders + register aliases are shared in `crate::mips`.

/// Assemble the charm routine for a `pct`-percent per-battle chance. Rolls the
/// BIOS RNG; on a hit AND only when the fight has a 2nd enemy (so single-enemy
/// tutorial / solo-boss set-pieces are never charmed - see [`SECOND_MONSTER_ID_VA`]),
/// OR's [`AI_DELEGATE_BITS`] into the frontmost monster's `+0x16E`, then replays
/// [`DISPLACED`] and jumps back to [`RETURN_VA`]. 24 instructions, self-contained,
/// fits the 96-byte gap window exactly.
pub fn assemble_routine(pct: u8) -> Vec<u32> {
    const DONE: usize = 20;
    const PCT_BEQ: usize = 6;
    const SOLO_BEQ: usize = 11;
    let pct_skip = (DONE as i32 - (PCT_BEQ as i32 + 1)) as i16;
    let solo_skip = (DONE as i32 - (SOLO_BEQ as i32 + 1)) as i16;

    let words = vec![
        jal(RAND_FUNC_VA),         // 0:  v0 = rand()
        nop(),                     // 1:  (branch delay)
        addiu(T0, ZERO, 100),      // 2:  t0 = 100
        divu(V0, T0),              // 3:  lo/hi = rand / 100
        mfhi(T1),                  // 4:  t1 = rand % 100
        sltiu(T1, T1, pct as u16), // 5:  t1 = (rand%100 < pct) ? 1 : 0
        beq(T1, ZERO, pct_skip),   // 6:  miss -> DONE (no charm)
        nop(),                     // 7:  (branch delay)
        // Single-enemy gate: skip charm unless the formation has a 2nd monster
        // (DAT_8007BD0C[1] != 0). Charming the lone enemy of an input-gated
        // tutorial / solo boss softlocks the scripted fight.
        lui(T0, hi(SECOND_MONSTER_ID_VA)), // 8:  \ t0 = 2nd monster id
        lbu(T0, T0, lo(SECOND_MONSTER_ID_VA)), // 9: /  (DAT_8007BD0C[1])
        nop(),                             // 10: (load delay)
        beq(T0, ZERO, solo_skip),          // 11: single enemy -> DONE (no charm)
        nop(),                             // 12: (branch delay)
        lui(V0, hi(ACTOR_SLOT3_VA)),       // 13: \ t5 = actor-table[3]
        lw(T5, V0, lo(ACTOR_SLOT3_VA)),    // 14: /   (frontmost enemy, 0x801C937C)
        nop(),                             // 15: (load delay)
        lhu(T6, T5, FIELD_FLAGS_OFFSET),   // 16: t6 = actor flags (+0x16E)
        nop(),                             // 17: (load delay)
        ori(T6, T6, AI_DELEGATE_BITS),     // 18: t6 |= 0x380 (AI-delegated)
        sh(T6, T5, FIELD_FLAGS_OFFSET),    // 19: write flags back
        // DONE (idx 20): replay displaced instructions, return.
        DISPLACED[0], // 20: lui v1,0x8008
        DISPLACED[1], // 21: lbu v1,-0x42f4(v1)
        j(RETURN_VA), // 22: back to the join
        nop(),        // 23: (branch delay)
    ];
    debug_assert_eq!(words.len(), 24);
    debug_assert_eq!(words[DONE], DISPLACED[0]);
    words
}

/// The two detour words written at [`HOOK_VA`]: `j ROUTINE_VA` then `nop`.
pub fn detour_words() -> [u32; 2] {
    [j(ROUTINE_VA), nop()]
}

/// A planned injection: the three same-size writes - the setup detour and the
/// routine blob in `SCUS_942.54`, and the one-word victory-mask widen in the
/// battle-action overlay PROT entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyAllyInjection {
    /// File offset of [`HOOK_VA`] within `SCUS_942.54`; receives [`detour_words`].
    pub scus_hook_off: usize,
    /// The two detour words to write at the hook.
    pub detour: [u32; 2],
    /// File offset of [`ROUTINE_VA`] within `SCUS_942.54`; receives [`Self::blob`].
    pub routine_off: usize,
    /// Routine bytes (little-endian words).
    pub blob: Vec<u8>,
    /// File offset of [`VICTORY_VA`] within the battle-action overlay PROT entry.
    pub overlay_victory_off: usize,
    /// The widened victory instruction word ([`VICTORY_PATCHED`]).
    pub victory_word: u32,
    /// Per-battle charm probability (percent).
    pub pct: u8,
}

impl EnemyAllyInjection {
    /// Plan the injection given the `SCUS_942.54` image, the battle-action
    /// overlay's raw PROT entry, and `pct`. Fails (rather than corrupts) if the
    /// build isn't recognized: the SCUS detour-site words and the overlay victory
    /// word must match the known US build, and the routine region must be all-zero
    /// dead space within the preserved gap.
    pub fn plan(scus: &[u8], overlay: &[u8], pct: u8) -> Result<Self> {
        if pct == 0 || pct > 100 {
            bail!("enemy-ally percent {pct} out of range 1..=100");
        }

        // --- SCUS detour site ------------------------------------------------
        let scus_hook_off = item_names::file_offset_for_va(scus, HOOK_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve hook VA {HOOK_VA:#x} in SCUS"))?;
        let at_hook = [
            read_word(scus, scus_hook_off)?,
            read_word(scus, scus_hook_off + 4)?,
        ];
        if at_hook != DISPLACED {
            bail!(
                "battle-setup hook {HOOK_VA:#x} = [{:#010x}, {:#010x}], expected \
                 [{:#010x}, {:#010x}] (unrecognized build) - refusing to patch",
                at_hook[0],
                at_hook[1],
                DISPLACED[0],
                DISPLACED[1],
            );
        }

        // --- routine blob (preserved zero gap) -------------------------------
        let routine = assemble_routine(pct);
        let blob: Vec<u8> = routine.iter().flat_map(|w| w.to_le_bytes()).collect();
        let blob_end_va = ROUTINE_VA + blob.len() as u32;
        if blob_end_va > ROUTINE_REGION_END_VA {
            bail!(
                "enemy-ally routine ({} bytes) overruns its gap window end {ROUTINE_REGION_END_VA:#x}",
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
                "enemy-ally routine region {ROUTINE_VA:#x}..+{} is not all-zero dead space \
                 (unrecognized build / collides with another injection) - refusing to patch",
                blob.len()
            );
        }

        // --- overlay victory mask widen --------------------------------------
        let overlay_victory_off = (VICTORY_VA - OVERLAY_BASE_VA) as usize;
        let at_victory = read_word(overlay, overlay_victory_off)?;
        if at_victory != VICTORY_ORIG {
            bail!(
                "victory check {VICTORY_VA:#x} = {at_victory:#010x}, expected {VICTORY_ORIG:#010x} \
                 (`andi v0,v0,0x4`; unrecognized build) - refusing to patch",
            );
        }

        Ok(Self {
            scus_hook_off,
            detour: detour_words(),
            routine_off,
            blob,
            overlay_victory_off,
            victory_word: VICTORY_PATCHED,
            pct,
        })
    }
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
        let r = assemble_routine(20);
        assert_eq!(r.len(), 24);
        // RNG roll + percent gate.
        assert_eq!(r[0], jal(RAND_FUNC_VA));
        assert_eq!(r[2], addiu(T0, ZERO, 100));
        assert_eq!(r[3], divu(V0, T0));
        assert_eq!(r[4], mfhi(T1));
        assert_eq!(r[5], sltiu(T1, T1, 20));
        // Single-enemy gate: read DAT_8007BD0C[1] (2nd monster id).
        assert_eq!(r[9], lbu(T0, T0, lo(SECOND_MONSTER_ID_VA)));
        assert_eq!(op(r[11]), 0x04, "idx 11 is the single-enemy beq");
        // Charm = OR 0x380 into the frontmost monster's +0x16E.
        assert_eq!(r[13], lui(V0, hi(ACTOR_SLOT3_VA)));
        assert_eq!(r[14], lw(T5, V0, lo(ACTOR_SLOT3_VA)));
        assert_eq!(r[16], lhu(T6, T5, FIELD_FLAGS_OFFSET));
        assert_eq!(r[18], ori(T6, T6, AI_DELEGATE_BITS));
        assert_eq!(r[19], sh(T6, T5, FIELD_FLAGS_OFFSET));
        // Returns by replaying the displaced pair then `j RETURN_VA`.
        assert_eq!(r[20], DISPLACED[0]);
        assert_eq!(r[21], DISPLACED[1]);
        assert_eq!(op(r[22]), 0x02);
        assert_eq!((r[22] & 0x03ff_ffff) << 2, RETURN_VA & 0x0fff_ffff);
    }

    #[test]
    fn gate_branches_target_done() {
        let r = assemble_routine(20);
        // Both the percent-miss beq (idx 6) and the single-enemy beq (idx 11)
        // land on DONE (idx 20 = the DISPLACED replay).
        assert_eq!(op(r[6]), 0x04);
        assert_eq!(6 + 1 + (r[6] & 0xffff) as i16 as i32, 20, "pct beq -> DONE");
        assert_eq!(op(r[11]), 0x04);
        assert_eq!(
            11 + 1 + (r[11] & 0xffff) as i16 as i32,
            20,
            "solo beq -> DONE"
        );
        assert_eq!(r[20], DISPLACED[0]);
    }

    #[test]
    fn displaced_pair_matches_the_documented_disassembly() {
        const V1: u32 = 3;
        // lui v1,0x8008 ; lbu v1,-0x42f4(v1)
        assert_eq!(DISPLACED[0], lui(V1, 0x8008));
        // lbu v1,-0x42f4(v1): opcode 0x24, rs=rt=v1, off=0xBD0C.
        assert_eq!(
            DISPLACED[1],
            (0x24 << 26) | (V1 << 21) | (V1 << 16) | 0xBD0C
        );
    }

    #[test]
    fn victory_words_are_andi_masks() {
        // andi v0,v0,0x4 -> andi v0,v0,0x384 (add the 0x380 charm bits).
        assert_eq!(VICTORY_ORIG, (0x0c << 26) | (V0 << 21) | (V0 << 16) | 0x4);
        assert_eq!(
            VICTORY_PATCHED,
            (0x0c << 26) | (V0 << 21) | (V0 << 16) | 0x384
        );
        assert_eq!(
            VICTORY_PATCHED & VICTORY_ORIG,
            VICTORY_ORIG,
            "keeps the 0x4 bit"
        );
        assert_eq!(VICTORY_PATCHED & 0x384, 0x384);
    }

    #[test]
    fn routine_fits_its_gap_window() {
        let blob_len = assemble_routine(20).len() * 4;
        assert!(ROUTINE_VA + blob_len as u32 <= ROUTINE_REGION_END_VA);
        // And it must start at/after the bonus-equipment routine + its max table.
        let bonus_end =
            crate::bonus_drop::ROUTINE_VA + 22 * 4 + crate::bonus_drop::MAX_TABLE_LEN as u32;
        assert!(
            ROUTINE_VA >= bonus_end,
            "enemy-ally routine overlaps the bonus-drop region"
        );
        // ...and below the flee-EXP routine.
        assert_eq!(ROUTINE_REGION_END_VA, crate::flee_exp::ROUTINE_VA);
    }

    #[test]
    fn pct_must_be_in_range() {
        let scus = vec![0u8; 0x100];
        let overlay = vec![0u8; 0x100];
        assert!(EnemyAllyInjection::plan(&scus, &overlay, 0).is_err());
        assert!(EnemyAllyInjection::plan(&scus, &overlay, 101).is_err());
    }

    #[test]
    fn overlay_victory_offset_is_linear_from_base() {
        assert_eq!((VICTORY_VA - OVERLAY_BASE_VA) as usize, 0x17E20);
    }
}
