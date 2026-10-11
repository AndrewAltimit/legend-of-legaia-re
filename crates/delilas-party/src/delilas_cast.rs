//! Delilas party swap: route the signature art into the real enemy cast
//! module, so a swapped hero's special plays the retail boss choreography
//! (camera track, lift, multi-hit damage build-up, face cut-in) instead of
//! the art-side approximation.
//!
//! ## How retail runs a boss special
//!
//! A Delilas signature attack is a capture-class spell (`'c'` at spell-table
//! `+0`): Blazing Slash `0x79` / Megaton Press `0x7A` / Plasma Strike `0x7B`,
//! whose `+1` sub-id pages the per-spell cast module PROT `958`/`959`/`960`
//! into the slot-B overlay window (`0x801F69D8`) at cast time. Battle states
//! `0x28 -> 0x6E..0x71` drive it; state `0x70` re-enters the module's tick
//! every frame ([spell-table.md](../../../docs/formats/spell-table.md),
//! [battle-action.md](../../../docs/subsystems/battle-action.md)).
//!
//! The route is fully data-keyed off the spell id, so a PARTY actor whose
//! action is `category 2 (Magic), +0x1DF = 0x7A` runs the whole spectacle -
//! verified live (PCSX-Redux, `autorun_player_special_cast.lua`): the module
//! pages in, its phase machine advances, the caster plays the staged clips
//! and the retail pillar/impact effects and camera track render around the
//! player character.
//!
//! ## What this module patches
//!
//! 1. **The queue hook** (SCUS arena + one `jal` redirect in PROT 0898):
//!    after the retail Super-Art applier `FUN_801EF9E4` runs over a finished
//!    arts queue, a small stub scans the queue for the slot's signature-art
//!    constant and, when present (and no Super fired - `DAT_801F696C == 0`),
//!    rewrites the action to `+0x1DE = 2` (Magic) with `+0x1DF` = the mapped
//!    sibling's spell id. The attack's chosen target seat (`+0x1DD`) is kept.
//!    The applier has exactly one caller - the queue builder `FUN_801EED1C`'s
//!    `jal` at `0x801EF9AC` - and at that site `a0` = actor slot, `a1` (after
//!    the delay slot) = 0-based character index.
//!
//! 2. **The cast modules** (PROT 958/959/960, link base `0x801F69D8`): four
//!    defect classes separate an enemy cast from a player cast. Three have
//!    edit tables below - the hardcoded `table[0]` damage/HP sites
//!    (retarget to the derived victim), the dead-victim party-wipe arm
//!    (branch to the settle tail), and the finale teardown corpse-model
//!    (neutralise its stream words at the settle tail). The fourth - the
//!    caster staged-clip rows - is fixed on the DATA side by default:
//!    `legaia_party_swap::cast_stage` authors real wind-up/payoff records on the
//!    player rows the module stages (Block re-homed to a spare row, see
//!    [`relocate_block_reaction`]), so the module's retail two-stage step
//!    survives on both caster kinds; the stage-pin edit remains only as
//!    the fallback when that rewrite cannot land. Only a module whose
//!    classes are patched AND probe-verified end-to-end is routed to;
//!    PROT 959 (Megaton Press) is.
//!
//! Verified live for PROT 959: the full choreography - pillar, blackout
//! lift, rock-shatter with the damage counter, smash - runs on a player
//! caster against the chosen target, and the battle continues past the
//! finale.
//!
//! No Sony bytes ship here: every patched word is derived from (and verified
//! against) the user's own disc before writing.

use anyhow::{Context, Result, bail};

use legaia_code_hooks::mips::{self as m, ZERO};
use legaia_disc_patch::disc::DiscPatcher;

mod arena_hook;
mod module_958;
mod module_959;
mod module_960;
mod overlay_sites;
mod stage_caves;

pub use arena_hook::{
    assemble_delilas_arena, assemble_hook, install_delilas_arena, install_strike_morph,
};
pub use module_958::patch_module_958;
pub use module_959::patch_module_959;
pub use module_960::patch_module_960;
pub use overlay_sites::{
    install_cast_hook, install_cast_label_gate, install_chain_admission_tier,
    redirect_applier_call, relocate_block_reaction,
};
pub use stage_caves::install_stage_caves;

/// Battle-action overlay (hosts the queue builder + applier).
const BATTLE_OVERLAY_PROT: usize = 898;
const BATTLE_OVERLAY_BASE: u32 = 0x801C_E818;

/// The applier call site inside `FUN_801EED1C` (its only caller).
const APPLIER_CALL_VA: u32 = 0x801E_F9AC;
/// The retail Super-Art applier the stub still runs first.
const APPLIER_VA: u32 = 0x801E_F9E4;
/// `sw 1 -> 0x801F696C` - the applier's own "a Super fired" flag.
const SUPER_FIRED_VA: u32 = 0x801F_696C;
/// 8-slot battle actor pointer table.
const ACTOR_TABLE_VA: u32 = 0x801C_9370;

/// Per-slot signature route: scan the finished queue for `art_constant`;
/// on a hit the action becomes a Magic cast of `spell_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastRoute {
    /// 0-based character index (0 Vahn / 1 Noa / 2 Gala).
    pub char_index: u8,
    /// The host art's queue constant (bank row + 0x10).
    pub art_constant: u8,
    /// The mapped sibling's capture-class spell id.
    pub spell_id: u8,
}

/// One-word expect/replace edit inside a PROT entry.
#[derive(Debug, Clone, Copy)]
struct WordEdit {
    offset: u64,
    expect: u32,
    replace: u32,
}

/// `move rd, rs` == `addu rd, rs, zero`.
const fn move_(rd: u32, rs: u32) -> u32 {
    m::addu(rd, rs, ZERO)
}

/// PROT 959 (Megaton Press): the five hardcoded `table[0]` damage/HP loads.
/// The tick keeps the true target in `$s4` from prologue to epilogue
/// (written exactly twice in the whole image: the derivation and the
/// register restore), so a plain `move` is safe at every site - each is the
/// `lw; nop; use` shape with no delay-slot dependence on the old value.
const S4: u32 = 20;

/// Verify-then-apply a word-edit set against one PROT entry, with the
/// jewel-fix idempotence contract: all-already-patched is a clean skip
/// (`Ok(false)`), a partial mix or any non-`expect` word refuses before
/// a single byte is written.
fn apply_word_edits(
    p: &mut DiscPatcher,
    prot: usize,
    entry: &[u8],
    edits: &[WordEdit],
) -> Result<bool> {
    let word = |off: u64| -> Result<u32> {
        let off = off as usize;
        Ok(u32::from_le_bytes(
            entry
                .get(off..off + 4)
                .with_context(|| format!("PROT {prot} short at +{off:#x}"))?
                .try_into()?,
        ))
    };
    let already = edits
        .iter()
        .filter(|e| word(e.offset).map(|w| w == e.replace).unwrap_or(false))
        .count();
    if already == edits.len() {
        return Ok(false);
    }
    if already != 0 {
        bail!(
            "PROT {prot} is partially patched ({already}/{} sites) - refusing",
            edits.len()
        );
    }
    for e in edits {
        let w = word(e.offset)?;
        if w != e.expect {
            bail!(
                "PROT {prot} +{:#x}: expected {:#010x}, found {:#010x} - not a known retail image",
                e.offset,
                e.expect,
                w
            );
        }
    }
    for e in edits {
        p.patch_prot_entry(prot, e.offset, &e.replace.to_le_bytes())?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
