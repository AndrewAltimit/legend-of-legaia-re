//! Charm-battle **softlock fix**: the disc-side companion to the enemy-ally
//! ("charm") feature that closes the victory-arm hard-freeze the charm widen
//! makes reachable.
//!
//! ## The softlock (pinned)
//!
//! The charm feature ([`crate::enemy_ally`]) widens the state-`0x5A` monster-wipe
//! down-mask at `0x801E6638` from `andi v0,v0,0x4` to `andi v0,v0,0x384` so a
//! living charmed (`0x380`) ally counts as "down" and the player needn't defeat
//! their own ally to win. That widen desyncs the wipe scan from the initiative
//! scheduler `FUN_801DABA4`, which still gates on the un-widened `0x4`: the
//! scheduler keeps picking the living charmed ally, so when the ally's own action
//! kills the last real enemy, the monster-wipe branch fires with a **living
//! monster** (actor slot `3..6`) as the acting actor.
//!
//! The victory arm then stages the win pose off the acting slot. In retail an
//! alive acting actor at victory is always a party member, so the arm keeps it
//! unconditionally at [`HOOK_VA`] (`0x801E6690`: `lhu a0,0x14c(s3)` /
//! `bne a0,zero,0x801E6728`) and reads the pose slot's character id from the
//! **3-byte** party roster `DAT_8007BD10` at `0x801E6770`. With a living monster
//! acting actor the slot is `>= 3`, so the roster read runs off the end of
//! `DAT_8007BD10[0..2]` into adjacent globals and arms a garbage win-pose "ME"
//! archive request - the battle wedges at the victory hand-off. See
//! `docs/subsystems/battle.md` § "Enemy-ally charm at the end-of-action gate".
//!
//! Note `s5 = ctx + 0x11` (set at `0x801E2994`), so the pose-slot byte the arm
//! reads (`0x2(s5)`) is `ctx[+0x13]` - the **active-actor index** (party `0..2`,
//! monster `3..6`), which is exactly what disambiguates the two cases.
//!
//! ## The fix (mirrors the engine `victory_pose_fixup`)
//!
//! The engine port (`engine-vm::battle_action`) already carries the corrected
//! invariant: keep the acting slot only when it is a **living party slot**;
//! otherwise re-pick a valid party slot. Retail already has the re-pick machinery:
//! the dead-acting-actor path at `0x801E66A4..0x801E6724` rolls `rand % party_count`
//! and rejects any slot that isn't a living, non-`0x404` party member. The only
//! defect is that a *living monster* acting actor skips that re-pick via the
//! unconditional `bne` at [`HOOK_VA`].
//!
//! So the fix widens that keep-condition to "alive **and** a party slot": a
//! **single-word** detour replaces the `bne` at [`HOOK_VA`] with `j <guard>`,
//! leaving the original store `sb v0,-0x42a0(v1)` at `0x801E6694` to run as the
//! jump's delay slot (so the `DAT_8007BD60 &= 0x7F` battle-flag clear still
//! happens on every path). The guard:
//!
//! ```text
//!   beq   a0,zero,to_reroll   ; dead acting actor -> retail re-pick (unchanged)
//!   lbu   v1,0x2(s5)          ; (delay) v1 = ctx[+0x13] = acting slot
//!   nop                       ; load delay
//!   sltiu v1,v1,0x3           ; v1 = (slot < 3) ? party : monster
//!   beq   v1,zero,to_reroll   ; monster slot -> retail re-pick (the charm case)
//!   nop
//!   j     0x801E6728          ; living party slot -> keep (retail Songi + roster read)
//!   nop
//! to_reroll:
//!   j     0x801E6698          ; retail re-pick setup + rejection loop
//!   nop
//! ```
//!
//! The re-pick is retail's own bounded-in-practice loop (a living party member
//! exists - the party-wipe branch already ran), so it cannot spin, and the roster
//! read at `0x801E6770` is guaranteed a valid party slot (`0..2`). Normal
//! (non-charm) battles are unaffected: an alive acting actor there is always a
//! party slot, so the guard takes the `keep` edge exactly as retail did - the
//! victory pose still belongs to the finishing party member.
//!
//! The guard preserves the registers the two landing points need (`a1 =
//! 0x80080000` for the re-pick's `lw v1,-0x42dc(s1)`; `s3`/`s5`) and clobbers only
//! `v1`, which both landings rebuild before use.
//!
//! ## Placement
//!
//! The guard is 10 instructions (40 bytes). It lives in the head of the same
//! preserved 1028-byte rodata gap the other code hooks use, at [`ROUTINE_VA`] =
//! `0x8007AB50` - the unused word-aligned window between the Seru-Bell name string
//! (`0x8007AB40`, <= 16 bytes) and the bonus-equipment routine
//! ([`crate::bonus_drop::ROUTINE_VA`] = `0x8007AB80`). So it composes with every
//! other gap feature (name injection, bonus drop, flee-EXP, shiny Seru) and the
//! charm feature it fixes. The overlay is resident whenever this code runs and
//! `SCUS_942.54` is resident throughout, so a `j` reaches the guard and back.
//!
//! Two same-size edits, both guarded on the known US build: the one-word overlay
//! detour (the `bne` word is verified before it is replaced) and the routine blob
//! (its region must be all-zero dead space, clear of the Seru-Bell string). A
//! differently-laid-out image is refused, not corrupted. No Sony bytes are
//! embedded - the guard is the randomizer's own code.

use anyhow::{Result, bail};

use legaia_asset::item_names;

use crate::mips::*;

/// PROT entry index of the battle-action overlay that hosts the victory arm.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    crate::enemy_ally::BATTLE_ACTION_OVERLAY_PROT_INDEX;

/// Load base VA of the battle-action overlay. A VA inside it maps to PROT-entry
/// file offset `va - OVERLAY_BASE_VA` (the overlay is stored raw).
pub const OVERLAY_BASE_VA: u32 = crate::enemy_ally::OVERLAY_BASE_VA;

/// Detour site (battle-action overlay): the `bne a0,zero,0x801E6728` keep-branch
/// in the state-`0x5A` victory arm. Replaced with `j ROUTINE_VA`; its delay slot
/// (`0x801E6694`, the `sb v0,-0x42a0(v1)` battle-flag clear) is left in place and
/// runs as the jump's delay slot.
pub const HOOK_VA: u32 = 0x801E_6690;
/// The stock instruction at [`HOOK_VA`]: `bne a0,zero,0x801E6728` (the
/// recognized-build fingerprint guarded before the detour).
pub const HOOK_ORIG: u32 = 0x1480_0025;

/// Retail "keep the acting slot" landing (the Songi formation override +
/// roster-read win-pose staging). The guard jumps here for a living party slot.
pub const KEEP_VA: u32 = 0x801E_6728;
/// Retail "re-pick a valid party slot" landing (the `rand % party_count`
/// rejection loop). The guard jumps here for a dead **or** monster acting actor.
pub const REROLL_VA: u32 = 0x801E_6698;

/// Load VA of the injected guard, in the preserved rodata gap at `0x8007AB38`, in
/// the unused word-aligned window between the Seru-Bell name string
/// (`0x8007AB40`) and the bonus-equipment routine (`0x8007AB80`).
pub const ROUTINE_VA: u32 = 0x8007_AB50;
/// First VA used by the next gap occupant (the bonus-equipment routine); the
/// guard must end at or below this.
pub const ROUTINE_REGION_END_VA: u32 = crate::bonus_drop::ROUTINE_VA;

/// `ctx[+0x13]` reached as `0x2(s5)` (`s5 = ctx + 0x11`) - the active-actor index.
const CTX_SLOT_OFF: u16 = 0x2;
/// Party slots are `0..2`; monster slots are `3..6`. `slot < 3` ⟺ party slot.
const PARTY_SLOT_BOUND: u16 = 3;

/// Assemble the victory-arm guard: keep the acting slot only when it is a living
/// **party** slot (`alive && slot < 3`); otherwise fall into retail's own
/// valid-slot re-pick. 10 instructions, self-contained.
pub fn assemble_routine() -> Vec<u32> {
    // to_reroll sits at index 8; the two `beq`s (at indices 0 and 4) target it.
    // A `beq` offset is measured in words from the instruction after the branch.
    const TO_REROLL: i32 = 8;
    let off0 = (TO_REROLL - 1) as i16;
    let off4 = (TO_REROLL - 5) as i16;

    let words = vec![
        beq(A0, ZERO, off0),       // 0: dead acting actor -> re-pick (a0 = liveness)
        lbu(V1, S5, CTX_SLOT_OFF), // 1: (delay) v1 = ctx[+0x13] = acting slot
        nop(),                     // 2: load delay
        sltiu(V1, V1, PARTY_SLOT_BOUND), // 3: v1 = (slot < 3) ? 1 : 0
        beq(V1, ZERO, off4),       // 4: monster slot -> re-pick (the charm case)
        nop(),                     // 5: (branch delay)
        j(KEEP_VA),                // 6: living party slot -> keep acting slot
        nop(),                     // 7: (branch delay)
        // to_reroll (idx 8):
        j(REROLL_VA), // 8: retail re-pick setup + rejection loop
        nop(),        // 9: (branch delay)
    ];
    debug_assert_eq!(words.len(), 10);
    debug_assert_eq!(words[TO_REROLL as usize], j(REROLL_VA));
    words
}

/// The single detour word written at [`HOOK_VA`] in the overlay: `j ROUTINE_VA`.
/// (The delay slot at `0x801E6694` is intentionally left as the original store.)
pub fn detour_word() -> u32 {
    j(ROUTINE_VA)
}

/// A planned charm-softlock fix: two same-size writes - the one-word overlay
/// detour and the guard blob in `SCUS_942.54` rodata padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharmVictoryFix {
    /// File offset of [`HOOK_VA`] within the battle-action overlay PROT entry.
    pub overlay_hook_off: usize,
    /// The detour word written at the hook (`j ROUTINE_VA`).
    pub detour: u32,
    /// File offset of [`ROUTINE_VA`] within `SCUS_942.54`; receives [`Self::blob`].
    pub routine_off: usize,
    /// Guard bytes (little-endian words).
    pub blob: Vec<u8>,
}

impl CharmVictoryFix {
    /// Plan the fix given `SCUS_942.54` and the battle-action overlay's raw PROT
    /// entry. Fails (rather than corrupts) if the build isn't recognized: the
    /// overlay hook word must be the stock `bne a0,zero,0x801E6728`, and the SCUS
    /// routine region must be all-zero dead space within the preserved gap.
    pub fn plan(scus: &[u8], overlay: &[u8]) -> Result<Self> {
        // --- overlay detour site --------------------------------------------
        let overlay_hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
        let at_hook = read_word(overlay, overlay_hook_off)?;
        if at_hook != HOOK_ORIG {
            bail!(
                "victory keep-branch {HOOK_VA:#x} = {at_hook:#010x}, expected {HOOK_ORIG:#010x} \
                 (`bne a0,zero,0x801E6728`; unrecognized build) - refusing to patch",
            );
        }

        // --- guard blob (preserved zero gap) --------------------------------
        let routine = assemble_routine();
        let blob: Vec<u8> = routine.iter().flat_map(|w| w.to_le_bytes()).collect();
        let blob_end_va = ROUTINE_VA + blob.len() as u32;
        if blob_end_va > ROUTINE_REGION_END_VA {
            bail!(
                "charm-fix guard ({} bytes) overruns its gap window end {ROUTINE_REGION_END_VA:#x}",
                blob.len()
            );
        }
        let routine_off = item_names::file_offset_for_va(scus, ROUTINE_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve guard VA {ROUTINE_VA:#x} in SCUS"))?;
        let region = scus
            .get(routine_off..routine_off + blob.len())
            .ok_or_else(|| anyhow::anyhow!("guard region past end of SCUS"))?;
        if region.iter().any(|&b| b != 0) {
            bail!(
                "charm-fix guard region {ROUTINE_VA:#x}..+{} is not all-zero dead space \
                 (unrecognized build / collides with another injection) - refusing to patch",
                blob.len()
            );
        }

        Ok(Self {
            overlay_hook_off,
            detour: detour_word(),
            routine_off,
            blob,
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
    fn hook_word_matches_the_documented_disassembly() {
        // bne a0,zero,0x801E6728: off = (0x801E6728 - (0x801E6690 + 4)) / 4 = 37.
        let off = ((KEEP_VA as i64 - (HOOK_VA as i64 + 4)) / 4) as i16;
        assert_eq!(off, 37);
        assert_eq!(HOOK_ORIG, bne(A0, ZERO, off));
    }

    #[test]
    fn detour_jumps_to_the_guard() {
        let w = detour_word();
        assert_eq!(op(w), 0x02, "detour is a `j`");
        assert_eq!((w & 0x03ff_ffff) << 2, ROUTINE_VA & 0x0fff_ffff);
    }

    #[test]
    fn guard_has_the_expected_shape() {
        let r = assemble_routine();
        assert_eq!(r.len(), 10);
        // Liveness gate reuses a0 (the acting actor's +0x14C, already loaded at
        // 0x801E6688) - dead -> re-pick.
        assert_eq!(op(r[0]), 0x04, "idx 0 is the liveness beq");
        // Acting slot = ctx[+0x13] read as 0x2(s5).
        assert_eq!(r[1], lbu(V1, S5, CTX_SLOT_OFF));
        assert_eq!(r[3], sltiu(V1, V1, PARTY_SLOT_BOUND));
        assert_eq!(op(r[4]), 0x04, "idx 4 is the party-slot beq");
        // The two decisive jumps.
        assert_eq!(op(r[6]), 0x02);
        assert_eq!((r[6] & 0x03ff_ffff) << 2, KEEP_VA & 0x0fff_ffff);
        assert_eq!(op(r[8]), 0x02);
        assert_eq!((r[8] & 0x03ff_ffff) << 2, REROLL_VA & 0x0fff_ffff);
    }

    #[test]
    fn both_gates_branch_to_the_reroll() {
        let r = assemble_routine();
        // idx 0 (dead) and idx 4 (monster slot) both land on to_reroll (idx 8).
        let target = |src: i32, w: u32| src + 1 + (w & 0xffff) as i16 as i32;
        assert_eq!(target(0, r[0]), 8, "dead beq -> reroll");
        assert_eq!(target(4, r[4]), 8, "monster-slot beq -> reroll");
        assert_eq!(r[8], j(REROLL_VA));
    }

    #[test]
    fn slot_test_discriminates_party_from_monster() {
        // sltiu v1,v1,3: party slots 0/1/2 -> 1 (keep), monster slots 3..6 -> 0
        // (re-pick). The bound is the party-vs-monster boundary in the actor
        // table (party 0..2, monsters 3+).
        assert_eq!(assemble_routine()[3], sltiu(V1, V1, PARTY_SLOT_BOUND));
    }

    #[test]
    fn guard_fits_its_gap_window() {
        // `start`/`end` are `let`-bound so these stay runtime assertions.
        let start = ROUTINE_VA;
        let end = ROUTINE_REGION_END_VA;
        let blob_end = start + assemble_routine().len() as u32 * 4;
        assert!(blob_end <= end, "guard fits below the next gap occupant");
        // ...and starts clear of the Seru-Bell name string (0x8007AB40, <= 16 B).
        assert!(start >= crate::item_name::SERU_BELL_STRING_VA + 0x10);
        // ...and below the bonus-equipment routine (the next gap occupant).
        assert_eq!(end, crate::bonus_drop::ROUTINE_VA);
    }

    #[test]
    fn overlay_hook_offset_is_linear_from_base() {
        assert_eq!((HOOK_VA - OVERLAY_BASE_VA) as usize, 0x17E78);
    }

    #[test]
    fn refuses_an_unrecognized_build() {
        let scus = vec![0u8; 0x100];
        let overlay = vec![0u8; 0x20000]; // hook word is zero, not the stock bne
        assert!(CharmVictoryFix::plan(&scus, &overlay).is_err());
    }
}
