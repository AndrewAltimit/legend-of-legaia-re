//! **Attack-approach softlock fix**: close the retail "endless camera orbit"
//! park by making the battle engine re-stage a monster's approach animation
//! when it dies mid-approach - the actual defect - instead of waiting forever
//! on a range check that can no longer pass.
//!
//! ## The softlock (pinned; caught live twice, trigger reproduced)
//!
//! The battle-action state machine `FUN_801E295C` (battle-action overlay,
//! PROT entry 898, base VA `0x801CE818`) stages a physical attack through
//! state `0x14`. Out of reach, a **monster** attacker needs the walk chain
//! (`0x15..0x18`), gated on action tag `0x20` - which **180 of the 186
//! monsters don't have** (roster sweep over the PROT 867 archive), so almost
//! every monster melee runs the fallback instead: stage the tag-`1` "Move"
//! clip into `actor+0x1DA` and wait in state `0x19`, the in-range poll,
//! whose arm has **no movement code and no timeout**. Normally the staged
//! clip's playback slides the monster in (~19 units/vsync, measured); but
//! when a **summon's staging round-trip immediately precedes the melee**,
//! the clip dies ~12 vsyncs in (`+0x1DA/+0x1D9` back to `0/0`), nothing
//! re-stages it, and the fight waits forever (reproduced on the first
//! directed attempt; scenario `battle_gaza2_park_0x19_summon_melee`). Full
//! anatomy: `docs/subsystems/battle-action.md`.
//!
//! ## The fix: re-stage on stall, in place (retail's own code does the work)
//!
//! The `0x19` arm spends nine words re-deriving the actor's facing every
//! frame:
//!
//! ```text
//! 801e3568  lh  a0,0x38(s8)      ; \
//! 801e356c  lh  a1,0x34(s8)      ; | bearing(target -> actor)
//! 801e3570  lh  a2,0x38(s3)      ; |   (FUN_80019B28)
//! 801e3574  lh  a3,0x34(s3)      ; |
//! 801e3578  jal 0x80019b28       ; |
//! 801e357c  _nop                 ; /
//! 801e3580  addiu v0,v0,0x800    ; \
//! 801e3584  andi  v0,v0,0xfff    ; | facing = bearing + 0x800
//! 801e3588  sh    v0,0x46(s3)    ; /   -> actor+0x46
//! ```
//!
//! That recompute is redundant during an approach: the target never moves
//! while the acting actor closes in (it idles awaiting the hit), the facing
//! was just computed by state `0x14` at staging, the approach motion runs
//! *along* the facing (so the bearing stays constant), and the strike arm
//! (`0x1E`) re-derives facing itself on entry. The fix replaces exactly
//! those nine words with the guard:
//!
//! ```text
//! 801e3568  lbu v0,0x1da(s3)     ; staged clip index (s3 = acting actor)
//! 801e356c  lui v1,0x8008        ; \
//! 801e3570  lw  v1,-0x42dc(v1)   ; / ctx = *0x8007BD24
//! 801e3574  ori a0,zero,0x14
//! 801e3578  bne v0,zero,+3       ; clip alive -> untouched
//! 801e357c  _nop
//! 801e3580  sb  a0,0x7(v1)       ; dead: state = 0x14 (retail re-stages)
//! 801e3584  nop
//! 801e3588  nop                  ; (branch lands here)
//! ```
//!
//! When the staged clip has died (`+0x1DA == 0`) while the poll is still
//! failing, the state byte is set back to `0x14`: retail's own arm then
//! re-runs the whole approach staging next frame - facing, range check,
//! tag-`1` re-stage - and the monster resumes walking. **No behaviour is
//! invented**: healthy approaches (staged index non-zero) run the identical
//! poll, an in-range poll still enters the strike chain the same frame (the
//! arm's own `0x1E` store runs after ours and wins), and a party attacker
//! whose run clip dies is rescued by the same bounce (state `0x14`'s party
//! branch re-stages anim `1`). The only unrescuable shape would be a monster
//! with no tag-`1` clip at all - the roster sweep finds **zero** such
//! monsters, so the guard needs no fallback arm.
//!
//! ## Register / pipeline safety
//!
//! The guard clobbers `v0`, `v1`, `a0` - all dead at this point: `a0`/`a1`
//! are re-loaded immediately after the window for the range-check call,
//! `v0` is overwritten by that call's return, and `v1` is rebuilt by both
//! downstream paths. `s3` (the acting actor) is the register the arm's own
//! stores use. R3000 load-delay slots are respected (`lbu`/`lw` results are
//! consumed three slots later). No branch in the overlay targets the
//! replaced window (checked over the full dump).
//!
//! One same-size nine-word edit in the raw PROT 898 entry, verified against
//! the stock facing-recompute words (plus the call/branch context around the
//! window) before writing; an unrecognized build is refused, an already-fixed
//! image is a no-op. No Sony bytes are embedded - the stock words are cited
//! from the project's own disassembly reference.

use anyhow::{Result, bail};

use crate::mips::{A0, S3, V0, V1, ZERO, bne, jal, lbu, lui, lw, nop, ori, sb};

/// PROT entry index of the battle-action overlay hosting `FUN_801E295C`.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;

/// Load base VA of the battle-action overlay. A VA inside it maps to
/// PROT-entry file offset `va - OVERLAY_BASE_VA` (the overlay is stored raw).
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// First word of the replaced window: the state-`0x19` arm's facing
/// recompute (`lh a0,0x38(s8)`).
pub const WINDOW_VA: u32 = 0x801E_3568;
/// The bearing helper the stock window calls (`FUN_80019B28`).
pub const BEARING_VA: u32 = 0x8001_9B28;
/// Battle context pointer global (`*0x8007BD24`); state byte at `+0x07`.
pub const CTX_PTR_VA: u32 = 0x8007_BD24;

/// Staged-clip byte offset in the battle actor (`+0x1DA`).
const STAGED_CLIP_OFF: u16 = 0x1DA;
/// State-byte offset in the battle context (`+0x07`).
const STATE_OFF: u16 = 0x07;

/// The nine stock words of the facing recompute
/// (`0x801E3568..=0x801E3588`), from the committed disassembly reference -
/// also the recognized-build fingerprint.
pub const STOCK_WINDOW: [u32; 9] = [
    0x87C4_0038, // lh a0,0x38(s8)
    0x87C5_0034, // lh a1,0x34(s8)
    0x8666_0038, // lh a2,0x38(s3)
    0x8667_0034, // lh a3,0x34(s3)
    0x0C00_66CA, // jal 0x80019b28
    0x0000_0000, // nop (delay)
    0x2442_0800, // addiu v0,v0,0x800
    0x3042_0FFF, // andi v0,v0,0xfff
    0xA662_0046, // sh v0,0x46(s3)
];

/// Assemble the replacement window: bounce a dead approach clip back to
/// state `0x14` so retail's own staging re-runs. Nine words, same size.
pub fn assemble_window() -> Vec<u32> {
    const SKIP: i16 = 3; // idx 4 -> idx 8 (past the state store)
    let words = vec![
        lbu(V0, S3, STAGED_CLIP_OFF), // 0: v0 = staged clip index
        lui(V1, 0x8008),              // 1: \ (fills v0 load delay)
        lw(V1, V1, 0xBD24),           // 2: / v1 = *0x8007BD24 (battle ctx)
        ori(A0, ZERO, 0x14),          // 3: a0 = 0x14 (fills v1 load delay)
        bne(V0, ZERO, SKIP),          // 4: clip alive -> untouched poll
        nop(),                        // 5: (delay)
        sb(A0, V1, STATE_OFF),        // 6: dead: state = 0x14 (retail re-stages)
        nop(),                        // 7: (pad)
        nop(),                        // 8: (pad / branch target)
    ];
    debug_assert_eq!(words.len(), STOCK_WINDOW.len());
    words
}

/// A planned approach-softlock fix: one same-size nine-word window rewrite
/// in the battle-action overlay PROT entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproachFix {
    /// File offset of [`WINDOW_VA`] within the overlay PROT entry.
    pub window_off: usize,
    /// Replacement bytes (nine little-endian words).
    pub bytes: Vec<u8>,
}

/// Plan the fix against the battle-action overlay's raw PROT entry bytes.
///
/// Returns `Ok(None)` when the window already holds the guard (idempotent
/// no-op). Fails - rather than corrupts - when the window is neither the
/// stock facing recompute nor the guard, or when the context words around it
/// (the pose call before, the range-check call after) don't match the
/// documented disassembly.
pub fn plan(overlay: &[u8]) -> Result<Option<ApproachFix>> {
    let window_off = (WINDOW_VA - OVERLAY_BASE_VA) as usize;
    let word_at = |off: usize| -> Result<u32> {
        let b = overlay
            .get(off..off + 4)
            .ok_or_else(|| anyhow::anyhow!("overlay entry too short for word at +{off:#x}"))?;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    };

    // Context fingerprint (holds whether or not the window was already
    // patched): the pose call's `_li a1,0x6` delay slot right before the
    // window, and the range-check argument loads + call right after it.
    let context: [(i64, u32); 4] = [
        (-4, 0x2405_0006),          // _li a1,0x6 (0x801E3564)
        (9 * 4, 0x92A4_0002),       // lbu a0,0x2(s5) (0x801E358C)
        (10 * 4, 0x93A5_0020),      // lbu a1,0x20(sp) (0x801E3590)
        (11 * 4, jal(0x8004_E2F0)), // jal FUN_8004E2F0 (0x801E3594)
    ];
    for (delta, want) in context {
        let got = word_at((window_off as i64 + delta) as usize)?;
        if got != want {
            bail!(
                "context word {:#x} = {got:#010x}, expected {want:#010x} \
                 (state-0x19 arm; unrecognized build) - refusing to patch",
                WINDOW_VA as i64 + delta,
            );
        }
    }

    let replacement = assemble_window();
    let bytes: Vec<u8> = replacement.iter().flat_map(|w| w.to_le_bytes()).collect();

    let mut current = [0u32; 9];
    for (i, w) in current.iter_mut().enumerate() {
        *w = word_at(window_off + i * 4)?;
    }
    if current[..] == replacement[..] {
        return Ok(None); // already fixed
    }
    if current[..] != STOCK_WINDOW[..] {
        bail!(
            "approach window {WINDOW_VA:#x} does not hold the stock facing recompute \
             (first divergence at word {}) - unrecognized build, refusing to patch",
            current
                .iter()
                .zip(STOCK_WINDOW.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(0),
        );
    }

    Ok(Some(ApproachFix { window_off, bytes }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(w: u32) -> u32 {
        w >> 26
    }

    #[test]
    fn stock_window_matches_the_documented_disassembly() {
        // Spot-check the hand-encoded stock words against the encoders where
        // helpers exist: the jal and the nop.
        assert_eq!(STOCK_WINDOW[4], jal(BEARING_VA));
        assert_eq!(STOCK_WINDOW[5], nop());
        // lh a0,0x38(s8): opcode 0x21, base s8 (30), rt a0 (4).
        assert_eq!(STOCK_WINDOW[0] >> 26, 0x21);
        assert_eq!((STOCK_WINDOW[0] >> 21) & 0x1F, 30);
        assert_eq!((STOCK_WINDOW[0] >> 16) & 0x1F, 4);
        assert_eq!(STOCK_WINDOW[0] & 0xFFFF, 0x38);
        // sh v0,0x46(s3): opcode 0x29, base s3 (19), rt v0 (2).
        assert_eq!(STOCK_WINDOW[8] >> 26, 0x29);
        assert_eq!((STOCK_WINDOW[8] >> 21) & 0x1F, 19);
        assert_eq!((STOCK_WINDOW[8] >> 16) & 0x1F, 2);
        assert_eq!(STOCK_WINDOW[8] & 0xFFFF, 0x46);
    }

    #[test]
    fn window_offset_is_linear_from_base() {
        assert_eq!((WINDOW_VA - OVERLAY_BASE_VA) as usize, 0x14D50);
    }

    #[test]
    fn guard_is_same_size_and_expected_shape() {
        let g = assemble_window();
        assert_eq!(g.len(), STOCK_WINDOW.len());
        assert_eq!(g[0], lbu(V0, S3, 0x1DA));
        // ctx load: lui 0x8008 + lw -0x42DC = 0x8007BD24.
        assert_eq!(g[1], lui(V1, 0x8008));
        assert_eq!(g[2], lw(V1, V1, 0xBD24));
        // The bounce value and store.
        assert_eq!(g[3], ori(A0, ZERO, 0x14));
        assert_eq!(g[6], sb(A0, V1, 0x07));
        // Tail pads are nops (fall into the unchanged range-check call).
        assert_eq!(g[7], nop());
        assert_eq!(g[8], nop());
    }

    #[test]
    fn guard_branch_skips_the_state_store() {
        let g = assemble_window();
        assert_eq!(op(g[4]), 0x05, "bne");
        let target = 4 + 1 + (g[4] & 0xFFFF) as i16 as i32;
        assert_eq!(target, 8, "alive path lands on the final pad");
    }

    #[test]
    fn guard_respects_r3000_load_delays() {
        let g = assemble_window();
        // lbu v0 at 0 -> first v0 use is the bne at 4.
        assert_eq!(g[0], lbu(V0, S3, 0x1DA));
        assert_eq!(op(g[4]), 0x05);
        // lw v1 at 2 -> first v1 use is the sb at 6.
        assert_eq!(g[2], lw(V1, V1, 0xBD24));
        assert_eq!(g[6], sb(A0, V1, 0x07));
    }

    fn synth_overlay(window: &[u32; 9]) -> Vec<u8> {
        let window_off = (WINDOW_VA - OVERLAY_BASE_VA) as usize;
        let mut ov = vec![0u8; window_off + 0x40];
        // Context: pose-call delay slot before, range-check args + call after.
        ov[window_off - 4..window_off].copy_from_slice(&0x2405_0006u32.to_le_bytes());
        for (i, w) in window.iter().enumerate() {
            let o = window_off + i * 4;
            ov[o..o + 4].copy_from_slice(&w.to_le_bytes());
        }
        for (i, w) in [0x92A4_0002u32, 0x93A5_0020, jal(0x8004_E2F0)]
            .iter()
            .enumerate()
        {
            let o = window_off + (9 + i) * 4;
            ov[o..o + 4].copy_from_slice(&w.to_le_bytes());
        }
        ov
    }

    #[test]
    fn plans_the_window_on_a_stock_image() {
        let fix = plan(&synth_overlay(&STOCK_WINDOW)).unwrap().unwrap();
        assert_eq!(fix.window_off, 0x14D50);
        assert_eq!(fix.bytes.len(), 36);
    }

    #[test]
    fn already_fixed_is_a_no_op() {
        let g: Vec<u32> = assemble_window();
        let mut w = [0u32; 9];
        w.copy_from_slice(&g);
        assert_eq!(plan(&synth_overlay(&w)).unwrap(), None);
    }

    #[test]
    fn refuses_unrecognized_window_or_context() {
        let mut w = STOCK_WINDOW;
        w[3] ^= 0xFF;
        assert!(plan(&synth_overlay(&w)).is_err());
        let mut ov = synth_overlay(&STOCK_WINDOW);
        let off = (WINDOW_VA - OVERLAY_BASE_VA) as usize + 11 * 4; // the jal
        ov[off] ^= 0xFF;
        assert!(plan(&ov).is_err());
    }

    #[test]
    fn refuses_a_truncated_overlay() {
        assert!(plan(&[0u8; 0x100]).is_err());
    }
}
