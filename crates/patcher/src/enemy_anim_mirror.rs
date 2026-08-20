//! Enemy-side animation mirror for `--delilas-party`: after
//! `apply_delilas_party` re-skins monster blocks 162/163/164 with the
//! mapped heroes' battle models, this pass rewrites the swapped blocks'
//! animation entries with the HERO's own clips
//! (`legaia_asset::party_swap::enemy_anim`), so the Nivora ravine duels
//! fight a Vahn/Noa/Gala who stand, react and strike like themselves
//! instead of posing the hero mesh with Delilas choreography.
//!
//! ## Ordering contract
//!
//! This pass MUST run **after** the party-swap model loop has patched the
//! monster slots (it rewrites the swapped block in place), and its retail
//! inputs MUST be captured **before** any patching: `apply_delilas_party`
//! rewrites the player files (playerize), the readef ME slots (the
//! signature reskin + `--delilas-moves`) and the monster archive itself,
//! so post-patch copies would hand the retarget the wrong data. The
//! [`RetailSources`] struct is that capture; `apply_delilas_party`
//! already reads all three images up front and can pass them through.
//!
//! ## What the cast modules pin
//!
//! The per-spell cast modules (PROT 958/959/960) stage archive entries by
//! **raw index** on the casting monster and their staged indices are
//! compiled MIPS - see `docs/formats/monster-animation.md` § "A special
//! attack can be a chain of entries". The module code is untouched; only
//! the CONTENT of the staged entries changes, every rewritten staged
//! entry keeps at least `MIN_STAGED_FRAMES` keyframes, and the entry
//! count / index space of each block is preserved.

use anyhow::{Context, Result, bail};

use legaia_asset::monster_archive;
use legaia_asset::party_swap::enemy_anim::{
    self, MirrorOptions, PAYOFF_FLOOR_FRAMES, RETAIL_STAGED_FLOOR, StagedPlan,
};

use crate::delilas_party::{PartyMapping, Sibling};
use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

/// The retail images the retarget reads, captured before any patching.
pub struct RetailSources<'a> {
    /// PROT 867 (`monster_data` battle archive), pre-swap.
    pub archive: &'a [u8],
    /// PROT 863/864/865 (`PLAYER1..3`), pre-playerize, slot-indexed
    /// (0 = Vahn, 1 = Noa, 2 = Gala).
    pub players: [&'a [u8]; 3],
    /// PROT 894 (`readef.DAT`), pre-reskin.
    pub readef: &'a [u8],
}

/// The archive entries each sibling's cast module stages by raw index
/// (caster chain in stage order, plus the closing entry where the module
/// stages one), with per-entry keyframe floors. Probe-traced for Che
/// (`10 -> 11`) and Lu (`14 -> 12 -> 13`, close `15`); Gi's chain
/// (`10 -> 11 -> 12`, close `13`) is the module-scan +
/// player-corroborated reading recorded in
/// `docs/formats/monster-animation.md`.
///
/// Floors: the only measured cursor gate is module 0960's (Lu, spell
/// `0x7B`) damage tick, which waits for the CASTER's clip cursor to
/// reach `0x160` sixteenths (keyframe 22) - that binds the payoff stage
/// (entry 13) at [`PAYOFF_FLOOR_FRAMES`]. Module 0959 carries no `slti`
/// cursor test at all, and 0958 is unmeasured; both are held at
/// [`RETAIL_STAGED_FLOOR`] (retail's own smallest staged entry, Gi's
/// 11-frame crouch).
pub fn staged_plan(sibling: Sibling) -> StagedPlan<'static> {
    match sibling {
        Sibling::Gi => StagedPlan {
            chain: &[10, 11, 12],
            chain_floors: &[
                RETAIL_STAGED_FLOOR,
                RETAIL_STAGED_FLOOR,
                RETAIL_STAGED_FLOOR,
            ],
            close: Some(13),
            close_floor: RETAIL_STAGED_FLOOR,
        },
        Sibling::Che => StagedPlan {
            chain: &[10, 11],
            chain_floors: &[RETAIL_STAGED_FLOOR, RETAIL_STAGED_FLOOR],
            close: None,
            close_floor: RETAIL_STAGED_FLOOR,
        },
        Sibling::Lu => StagedPlan {
            chain: &[14, 12, 13],
            chain_floors: &[
                RETAIL_STAGED_FLOOR,
                RETAIL_STAGED_FLOOR,
                PAYOFF_FLOOR_FRAMES,
            ],
            close: Some(15),
            close_floor: RETAIL_STAGED_FLOOR,
        },
    }
}

/// Compatibility view of [`staged_plan`]: `(chain, close)`.
pub fn staged_entries(sibling: Sibling) -> (&'static [usize], Option<usize>) {
    let p = staged_plan(sibling);
    (p.chain, p.close)
}

/// The budget ladder: rungs are tried in order and the first slot fit
/// wins. Density rungs (exact keyframe halving, duration-invariant) and
/// the compact close come before any content is dropped; the idle and
/// the module-staged special chain are never dropped. A block that
/// misses every rung keeps the sibling's clips (graceful skip) - the
/// bake-parity fix alone already poses the hero mesh correctly.
const LADDER: [MirrorOptions; 7] = [
    MirrorOptions::ALL,
    MirrorOptions {
        halve_non_staged: true,
        ..MirrorOptions::ALL
    },
    MirrorOptions {
        halve_non_staged: true,
        compact_close: true,
        ..MirrorOptions::ALL
    },
    MirrorOptions {
        halve_non_staged: true,
        halve_staged: true,
        compact_close: true,
        ..MirrorOptions::ALL
    },
    MirrorOptions {
        attacks: false,
        halve_non_staged: true,
        halve_staged: true,
        compact_close: true,
        ..MirrorOptions::ALL
    },
    MirrorOptions {
        attacks: false,
        walk: false,
        halve_non_staged: true,
        halve_staged: true,
        compact_close: true,
        ..MirrorOptions::ALL
    },
    MirrorOptions::NONE,
];

/// Rewrite each swapped block's animation entries with the mapped hero's
/// own clips. Idempotent (entry content is a pure function of the retail
/// sources). Errors if a target block does not carry the mapped hero's
/// name - i.e. if the party-swap model loop has not run yet.
pub fn apply_enemy_anim_mirror(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    retail: &RetailSources<'_>,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let current_archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .context("read monster archive")?;
    for (_, rig, slot, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let name = monster_archive::record(&current_archive, id)?
            .map(|r| r.name)
            .ok_or_else(|| anyhow::anyhow!("monster id {id}: empty slot"))?;
        if name != who {
            bail!(
                "monster id {id} is named {name:?}, expected {who:?} - the enemy \
                 anim mirror must run after the party-swap model loop"
            );
        }
        let current_block = monster_archive::decode_block(&current_archive, id)?
            .ok_or_else(|| anyhow::anyhow!("monster id {id}: block does not decode"))?;
        let plan = staged_plan(sibling);

        let mut done = false;
        for (step, opts) in LADDER.iter().enumerate() {
            let mirrored = enemy_anim::mirror_block_animations(
                &current_block,
                retail.archive,
                id,
                retail.players[slot],
                retail.readef,
                slot,
                rig,
                &plan,
                opts,
            )
            .with_context(|| format!("{who} anim mirror for monster {id}"))?;
            match monster_archive::encode_slot(&mirrored.block) {
                Ok(new_slot) => {
                    patcher
                        .patch_monster_slot(id, &new_slot)
                        .with_context(|| format!("write monster {id} slot"))?;
                    notes.push(format!(
                        "{who} (monster {id}): {} entries rewritten with {who}'s own clips{}",
                        mirrored.rewritten.len(),
                        if step > 0 {
                            format!(" (budget ladder step {step}: {opts:?})")
                        } else {
                            String::new()
                        }
                    ));
                    for n in mirrored.notes {
                        notes.push(format!("{who} (monster {id}): {n}"));
                    }
                    done = true;
                    break;
                }
                Err(e) => notes.push(format!(
                    "{who} (monster {id}): ladder step {step} over budget ({e:#})"
                )),
            }
        }
        // Graceful skip: a block that no rung fits keeps the sibling's
        // clips. The phase-38 bake parity alone already poses the hero
        // mesh correctly; a hard error inside apply_delilas_party would
        // cost the whole mod over one block's animations.
        if !done {
            notes.push(format!(
                "{who} (monster {id}): block keeps the sibling's clips (budget)"
            ));
        }
    }
    Ok(notes)
}
