//! **Attack-approach softlock fix**: close the retail "endless camera orbit"
//! park - a monster with a contact attack but **no walk animation** whose
//! target stands beyond its reach polls the range check forever.
//!
//! ## The softlock (pinned; caught live on a Gaza rematch)
//!
//! The battle-action state machine `FUN_801E295C` (battle-action overlay,
//! PROT entry 898, base VA `0x801CE818`) stages a physical attack through
//! state `0x14` (approach setup). When the range check `FUN_8004E2F0` reports
//! the target out of reach, the **monster** sub-path looks up the walk
//! animation - action tag `0x20` - in the acting monster's action table
//! (`record+0x4C`, count `record+0x4A`, records at `DAT_801C9348[seat-3]`)
//! via the tag-scan `FUN_80050E2C`:
//!
//! ```text
//! 801e3260  li  a1,0x20              ; approach-transition tag
//! 801e3268  jal 0x80050e2c           ; scan the action table
//! 801e327c  bne v0,0xff,0x801e32b4   ; found -> state 0x15 (walk start)
//! 801e329c  li  a1,0x1               ; NOT found: fall back to tag 1 (the Move loop)
//! 801e32a4  jal 0x80050e2c
//! 801e32ac  j   0x801e32c4           ; <- the park: state 0x19
//! 801e32b0  _sb v0,0x1da(s3)         ;    (delay: stage that clip)
//! ```
//!
//! State `0x19` re-polls the range check every frame and has **no movement
//! code and no timeout** - its not-in-range edge only bumps the stall counter
//! `ctx+0x6D4` (read solely as an interference-roll modifier, never as a
//! limit). The walking states `0x15..0x18` are unreachable without the tag,
//! so a monster whose action table lacks `0x20` (bosses generally - they
//! never walk) parks the battle the moment it picks a contact attack against
//! a target beyond its size-scaled reach. NB the gate is on the
//! stance-to-moving *transition* clip only: the tag-`1` locomotion ("Move")
//! loop may well exist - it is only ever played inside the walk chain this
//! gate protects, so the parked monster visibly idles/floats in place.
//! Full anatomy:
//! `docs/subsystems/battle-action.md` ("The 0x19 attack-approach park").
//!
//! ## The fix
//!
//! A **single-word** retarget at [`HOOK_VA`]: the not-found path's
//! `j 0x801E32C4` (park in state `0x19`) becomes `j 0x801E3204` - the state
//! `0x14` **in-range continuation**, which enters the strike chain (state
//! `0x1E`) directly. The delay slot (`sb v0,0x1da(s3)`, staging the fallback
//! tag-`1` clip exactly as vanilla does) is left in place and still executes.
//! The landing site rebuilds its registers from scratch (`lui a0,0x8008` -
//! it is the in-range branch's own delay slot), so no register state carries
//! over.
//!
//! Behaviour change is confined to the exact softlocked situation: a monster
//! with **no walk animation**, out of reach, now strikes from where it
//! stands (the hit connects at range; visually the swing covers the
//! distance with the strike lunge only) instead of orbiting forever.
//! Monsters that can walk, all party attacks, and every in-range attack are
//! byte-for-byte untouched - the patched word is on the
//! `seat >= 3 && tag 0x20 absent && out of reach` path only.
//!
//! Same-size in place, verified before write: the hook word must be the
//! stock park jump and its neighbours the documented `jal`/`sb` pair, so a
//! differently-laid-out image is refused rather than corrupted. An
//! already-fixed image is a no-op. No Sony bytes are embedded - the patch
//! word is a `j` encoding of a documented VA.

use anyhow::{Result, bail};

use crate::mips::{S3, V0, j, jal, sb};

/// PROT entry index of the battle-action overlay hosting `FUN_801E295C`.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;

/// Load base VA of the battle-action overlay. A VA inside it maps to
/// PROT-entry file offset `va - OVERLAY_BASE_VA` (the overlay is stored raw).
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// The patched word: state `0x14`'s walk-tag-missing `j 0x801E32C4` (enter
/// the state-`0x19` park).
pub const HOOK_VA: u32 = 0x801E_32AC;
/// Stock jump target: the state-`0x19` staging at `0x801E32C4` (the park).
pub const PARK_TARGET_VA: u32 = 0x801E_32C4;
/// Fixed jump target: the state-`0x14` in-range continuation at `0x801E3204`
/// (state `0x1E`, the strike chain).
pub const STRIKE_TARGET_VA: u32 = 0x801E_3204;

/// Build-fingerprint context: the fallback tag-1 scan `jal FUN_80050E2C`
/// at `HOOK_VA - 8`.
pub const CONTEXT_JAL_VA: u32 = 0x801E_32A4;
/// Tag-scan routine the fingerprint `jal` must target.
pub const TAG_SCAN_VA: u32 = 0x8005_0E2C;

/// The stock word at [`HOOK_VA`]: `j 0x801E32C4` (`0x08078CB1`).
pub const fn park_word() -> u32 {
    j(PARK_TARGET_VA)
}

/// The replacement word: `j 0x801E3204` (`0x08078C81`).
pub const fn strike_word() -> u32 {
    j(STRIKE_TARGET_VA)
}

/// A planned approach-softlock fix: one same-size word write in the
/// battle-action overlay PROT entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApproachFix {
    /// File offset of [`HOOK_VA`] within the overlay PROT entry.
    pub hook_off: usize,
    /// The word written there (`j 0x801E3204`).
    pub word: u32,
}

/// Plan the fix against the battle-action overlay's raw PROT entry bytes.
///
/// Returns `Ok(None)` when the hook already holds the fixed word (idempotent
/// no-op). Fails - rather than corrupts - when the hook word is neither the
/// stock park jump nor the fix, or when the two context words around it
/// (`jal FUN_80050E2C` at `-8`, `sb v0,0x1da(s3)` at `+4`) don't match the
/// documented US-build disassembly.
pub fn plan(overlay: &[u8]) -> Result<Option<ApproachFix>> {
    let hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
    let word_at = |off: usize| -> Result<u32> {
        let bytes = overlay
            .get(off..off + 4)
            .ok_or_else(|| anyhow::anyhow!("overlay entry too short for word at +{off:#x}"))?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    };

    // Context fingerprint first - it must hold whether or not the hook itself
    // was already patched.
    let ctx_jal = word_at(hook_off - 8)?;
    if ctx_jal != jal(TAG_SCAN_VA) {
        bail!(
            "context word {CONTEXT_JAL_VA:#x} = {ctx_jal:#010x}, expected {:#010x} \
             (`jal FUN_80050E2C`; unrecognized build) - refusing to patch",
            jal(TAG_SCAN_VA),
        );
    }
    let ctx_sb = word_at(hook_off + 4)?;
    if ctx_sb != sb(V0, S3, 0x1DA) {
        bail!(
            "context word {:#x} = {ctx_sb:#010x}, expected {:#010x} \
             (`sb v0,0x1da(s3)`; unrecognized build) - refusing to patch",
            HOOK_VA + 4,
            sb(V0, S3, 0x1DA),
        );
    }

    let hook = word_at(hook_off)?;
    if hook == strike_word() {
        return Ok(None); // already fixed
    }
    if hook != park_word() {
        bail!(
            "approach hook {HOOK_VA:#x} = {hook:#010x}, expected {:#010x} \
             (`j 0x801E32C4`; unrecognized build) - refusing to patch",
            park_word(),
        );
    }
    Ok(Some(ApproachFix {
        hook_off,
        word: strike_word(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_match_the_documented_disassembly() {
        // j 0x801E32C4 / j 0x801E3204 / jal 0x80050E2C / sb v0,0x1da(s3).
        assert_eq!(park_word(), 0x08078CB1);
        assert_eq!(strike_word(), 0x08078C81);
        assert_eq!(jal(TAG_SCAN_VA), 0x0C01438B);
        assert_eq!(sb(V0, S3, 0x1DA), 0xA26201DA);
    }

    #[test]
    fn hook_offset_is_linear_from_base() {
        assert_eq!((HOOK_VA - OVERLAY_BASE_VA) as usize, 0x14A94);
    }

    fn synth_overlay(hook: u32) -> Vec<u8> {
        let hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
        let mut v = vec![0u8; hook_off + 8];
        v[hook_off - 8..hook_off - 4].copy_from_slice(&jal(TAG_SCAN_VA).to_le_bytes());
        v[hook_off..hook_off + 4].copy_from_slice(&hook.to_le_bytes());
        v[hook_off + 4..hook_off + 8].copy_from_slice(&sb(V0, S3, 0x1DA).to_le_bytes());
        v
    }

    #[test]
    fn plans_the_single_word_on_a_stock_image() {
        let fix = plan(&synth_overlay(park_word())).unwrap().unwrap();
        assert_eq!(fix.hook_off, 0x14A94);
        assert_eq!(fix.word, strike_word());
    }

    #[test]
    fn already_fixed_is_a_no_op() {
        assert_eq!(plan(&synth_overlay(strike_word())).unwrap(), None);
    }

    #[test]
    fn refuses_an_unrecognized_hook_word() {
        assert!(plan(&synth_overlay(0x1234_5678)).is_err());
    }

    #[test]
    fn refuses_broken_context_words() {
        let mut o = synth_overlay(park_word());
        let hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
        o[hook_off - 8] ^= 0xFF; // corrupt the jal fingerprint
        assert!(plan(&o).is_err());
        let mut o2 = synth_overlay(park_word());
        o2[hook_off + 4] ^= 0xFF; // corrupt the sb fingerprint
        assert!(plan(&o2).is_err());
    }

    #[test]
    fn refuses_a_truncated_overlay() {
        assert!(plan(&[0u8; 0x100]).is_err());
    }
}
